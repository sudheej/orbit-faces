use orbital_face_host::context::active_window::{ActiveWindowProvider, SystemActiveWindowProvider};
use orbital_face_host::context::ContextItem;
use orbital_face_host::hotkeys::{HotkeyAction, HotkeyProvider, SystemHotkeyProvider};
use orbital_face_host::model_provider::{
    MockModelProvider, ModelChunk, ModelProvider, ModelRequest, OllamaModelProvider,
    RuntimeModelOptions,
};
use orbital_face_host::quick_capture::{
    capture_context_item, SelectionProvider, SystemSelectionProvider,
};
use orbital_face_host::runtime_v0::{
    parse_command, parse_face_message, CompanionState, FaceToRuntimeMessage, RuntimeCommand,
    RuntimeCore, RuntimeToFaceEvent,
};
use orbital_face_host::speech::audio_capture::{AudioCaptureProvider, SystemAudioCapture};
use orbital_face_host::speech::stt::{MockSttProvider, SpeechToTextProvider, WhisperSttProvider};
use orbital_face_host::speech::tts::{
    NoopTtsProvider, PiperTtsProvider, TextToSpeechProvider, WindowsSapiTtsProvider,
};
use orbital_face_host::speech::{spoken_response, SpeechOptions, SpeechRuntimeStatus};
use std::io::{self, BufRead};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tungstenite::{accept, Error as WebSocketError, Message, WebSocket};

const ADDRESS: &str = "127.0.0.1:7373";
const BRIDGE_URL: &str = "ws://127.0.0.1:7373";
const IO_POLL_INTERVAL: Duration = Duration::from_millis(100);

fn main() -> anyhow::Result<()> {
    if std::env::args().any(|argument| matches!(argument.as_str(), "-h" | "--help")) {
        print_help();
        return Ok(());
    }
    let options = RuntimeModelOptions::parse_from(std::env::args().skip(1))?;
    let provider = model_provider_from_options(&options)?;
    let mut speech = build_speech_services(&options.speech)?;
    let bridge = RuntimeBridge::start(ADDRESS)?;
    let terminal = spawn_terminal_reader();
    let mut runtime = RuntimeCore::default();
    runtime.selection_capture_supported = SystemSelectionProvider.supported();
    runtime.active_window_supported = SystemActiveWindowProvider.supported();
    let hotkeys = start_hotkeys(&options, &mut runtime);

    println!("Orbital Runtime v0");
    println!("Bridge listening on {BRIDGE_URL}");
    print_provider_startup(provider.as_ref());
    println!("Type a message and press Enter.");
    println!(
        "Commands: /quit, /status, /model, /clear, /context, /clear-context, \
         /clipboard, /active-window, /watch, /unwatch, /attach-text, /attach-file, \
         /selection, /ask, /ask-selection, /ask-selection-once, /listen, \
         /transcribe-file, /say, /speech-status, /demo, /ping"
    );

    loop {
        while let Some(update) = bridge.try_recv() {
            handle_bridge_update(&bridge, &mut runtime, update);
        }
        if let Some(hotkeys) = &hotkeys {
            while let Ok(action) = hotkeys.try_recv() {
                handle_hotkey_action(
                    &bridge,
                    &mut runtime,
                    provider.as_ref(),
                    &options.system_prompt,
                    &mut speech,
                    action,
                );
            }
        }

        match terminal.try_recv() {
            Ok(line) => {
                if !handle_command(
                    &bridge,
                    &mut runtime,
                    provider.as_ref(),
                    &options.system_prompt,
                    &mut speech,
                    parse_command(&line),
                ) {
                    break;
                }
            }
            Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(25)),
            Err(TryRecvError::Disconnected) => break,
        }
    }

    println!("Orbital Runtime stopped.");
    Ok(())
}

fn model_provider_from_options(
    options: &RuntimeModelOptions,
) -> anyhow::Result<Box<dyn ModelProvider>> {
    match options.provider.as_str() {
        "mock" => Ok(Box::new(MockModelProvider)),
        "ollama" => Ok(Box::new(OllamaModelProvider::new(
            &options.ollama_base_url,
            &options.ollama_model,
        )?)),
        _ => unreachable!("provider name was validated"),
    }
}

struct SpeechServices {
    stt: Box<dyn SpeechToTextProvider>,
    tts: Box<dyn TextToSpeechProvider>,
    audio: SystemAudioCapture,
    options: SpeechOptions,
    status: SpeechRuntimeStatus,
}

fn build_speech_services(options: &SpeechOptions) -> anyhow::Result<SpeechServices> {
    let stt: Box<dyn SpeechToTextProvider> = match options.stt.as_str() {
        "mock" => Box::new(MockSttProvider),
        "whisper" => Box::new(WhisperSttProvider::new(
            options.whisper_bin.clone(),
            options.whisper_model_path.clone(),
        )?),
        _ => unreachable!("STT provider was validated"),
    };
    let tts: Box<dyn TextToSpeechProvider> = match options.tts.as_str() {
        "none" => Box::new(NoopTtsProvider),
        "piper" => Box::new(PiperTtsProvider::new(
            options.piper_bin.clone(),
            options.piper_model.clone(),
            options.piper_config.clone(),
        )?),
        "windows-sapi" => Box::new(WindowsSapiTtsProvider),
        _ => unreachable!("TTS provider was validated"),
    };
    Ok(SpeechServices {
        stt,
        tts,
        audio: SystemAudioCapture,
        options: options.clone(),
        status: SpeechRuntimeStatus::default(),
    })
}

fn print_help() {
    println!(
        "Usage: orbital-runtime [--model mock|ollama] \
         [--ollama-model <name>] [--ollama-base-url <url>] \
         [--system-prompt <text>] [--enable-hotkeys] \
         [--stt mock|whisper] [--whisper-model-path <path>] [--whisper-bin <path>] \
         [--tts none|piper|windows-sapi] [--piper-bin <path>] \
         [--piper-model <path>] [--piper-config <path>] [--speak-responses]"
    );
}

fn print_provider_startup(provider: &dyn ModelProvider) {
    if provider.name() == "ollama" {
        println!("Using Ollama provider");
        println!("Base URL: {}", provider.base_url().unwrap_or("-"));
        println!("Model: {}", provider.model_name());
        let status = provider.status();
        if !status.reachable {
            print_ollama_guidance(provider);
        } else if status.model_available == Some(false) {
            println!("Model may not be pulled yet. Try:");
            println!("ollama pull {}", provider.model_name());
        }
    } else {
        println!("Model provider: mock");
    }
}

fn start_hotkeys(
    options: &RuntimeModelOptions,
    runtime: &mut RuntimeCore,
) -> Option<Receiver<HotkeyAction>> {
    if !options.enable_hotkeys {
        return None;
    }
    match SystemHotkeyProvider.start() {
        Ok(receiver) => {
            runtime.hotkeys_enabled = true;
            println!("Global hotkeys registered:");
            println!("  Ctrl+Alt+O: show runtime status");
            println!("  Ctrl+Alt+S: capture selected text");
            println!("  Ctrl+Alt+A: ask about selected text");
            println!("  Ctrl+Alt+L: listen for 5 seconds");
            Some(receiver)
        }
        Err(error) => {
            runtime.hotkeys_enabled = false;
            eprintln!("Global hotkeys unavailable; continuing without them: {error:#}");
            None
        }
    }
}

fn handle_hotkey_action(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    model: &dyn ModelProvider,
    system_prompt: &str,
    speech: &mut SpeechServices,
    action: HotkeyAction,
) {
    match action {
        HotkeyAction::ShowStatus => {
            println!("Orbital ready for terminal input.");
            print_status(runtime, model, speech);
        }
        HotkeyAction::CaptureSelection => {
            capture_selection(bridge, runtime, &SystemSelectionProvider);
        }
        HotkeyAction::AskSelectionDefault => ask_selection(
            bridge,
            runtime,
            model,
            system_prompt,
            speech,
            "Explain this selected text briefly.".into(),
            true,
        ),
        HotkeyAction::ListenFiveSeconds => {
            listen_and_ask(bridge, runtime, model, system_prompt, speech, 5)
        }
    }
}

fn handle_command(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    provider: &dyn ModelProvider,
    system_prompt: &str,
    speech: &mut SpeechServices,
    command: RuntimeCommand,
) -> bool {
    match command {
        RuntimeCommand::Quit => return false,
        RuntimeCommand::Status => print_status(runtime, provider, speech),
        RuntimeCommand::Model => print_model_status(runtime, provider),
        RuntimeCommand::Clear => {
            runtime.conversation.clear();
            println!("Conversation history cleared.");
        }
        RuntimeCommand::Context => print_context(runtime),
        RuntimeCommand::ClearContext => {
            runtime.context.clear();
            println!("Context cleared.");
            context_success(bridge, runtime, "Context cleared");
        }
        RuntimeCommand::Clipboard => capture_clipboard(bridge, runtime),
        RuntimeCommand::ActiveWindow => capture_active_window(bridge, runtime),
        RuntimeCommand::Watch => start_watch(bridge, runtime),
        RuntimeCommand::Unwatch => {
            runtime.context.stop_watch();
            println!("Stopped watching active-window metadata.");
            context_success(bridge, runtime, "Stopped watching");
        }
        RuntimeCommand::AttachText(text) => attach_text(bridge, runtime, text),
        RuntimeCommand::AttachFile(path) => attach_file(bridge, runtime, &path),
        RuntimeCommand::Selection => {
            capture_selection(bridge, runtime, &SystemSelectionProvider);
        }
        RuntimeCommand::Ask(question) => {
            let question = default_question(question, "What should I help with?");
            run_prompt(
                bridge,
                runtime,
                provider,
                system_prompt,
                speech,
                question,
                None,
            );
        }
        RuntimeCommand::AskSelection(question) => ask_selection(
            bridge,
            runtime,
            provider,
            system_prompt,
            speech,
            question,
            true,
        ),
        RuntimeCommand::AskSelectionOnce(question) => ask_selection(
            bridge,
            runtime,
            provider,
            system_prompt,
            speech,
            question,
            false,
        ),
        RuntimeCommand::Listen(seconds) => {
            listen_and_ask(bridge, runtime, provider, system_prompt, speech, seconds)
        }
        RuntimeCommand::TranscribeFile(path) => transcribe_file(bridge, runtime, speech, &path),
        RuntimeCommand::Say(text) => say_text(bridge, runtime, speech, &text),
        RuntimeCommand::SpeechStatus => print_speech_status(speech),
        RuntimeCommand::Demo => run_demo(bridge, runtime),
        RuntimeCommand::Ping => {
            let id = format!(
                "runtime-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            );
            println!("Sending ping {id}");
            send_event(bridge, runtime, RuntimeToFaceEvent::Ping { id });
        }
        RuntimeCommand::Prompt(input) => run_prompt(
            bridge,
            runtime,
            provider,
            system_prompt,
            speech,
            input,
            None,
        ),
        RuntimeCommand::Empty => {}
        RuntimeCommand::Unknown(command) => {
            eprintln!(
                "Unknown command {command:?}. Use /status, /context, /model, /demo, or /quit."
            );
        }
    }
    true
}

fn run_prompt(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    provider: &dyn ModelProvider,
    system_prompt: &str,
    speech: &mut SpeechServices,
    input: String,
    temporary_context: Option<ContextItem>,
) {
    println!("User: {input}");
    runtime.last_user_input = Some(input.clone());
    if runtime.context.watch_mode() {
        if let Err(error) = runtime.context.refresh_watch() {
            eprintln!("Watch metadata refresh failed: {error:#}");
        }
    }
    send_state(
        bridge,
        runtime,
        CompanionState::Thinking,
        "Thinking...",
        None,
    );

    let prompt_context = temporary_context
        .as_ref()
        .map(|item| runtime.context.prompt_context_with(item))
        .unwrap_or_else(|| runtime.context.prompt_context());
    let context_item_count = prompt_context.items.len();
    let request = ModelRequest {
        user_input: input.clone(),
        prompt_context: (!prompt_context.formatted.is_empty()).then_some(prompt_context.formatted),
        context_item_count,
        system_prompt: Some(system_prompt.to_owned()),
        conversation_id: Some("runtime-session".into()),
        history: runtime.conversation.messages().to_vec(),
        max_tokens: Some(256),
        temperature: Some(0.3),
    };
    let mut accumulated = String::new();
    let mut emitted_length = 0;
    let mut wave_index = 0;
    let result = provider.generate_streaming(request, &mut |chunk: ModelChunk| {
        accumulated.push_str(&chunk.text_delta);
        let should_emit = chunk.done
            || accumulated.len().saturating_sub(emitted_length) >= 12
            || chunk.text_delta.contains(['.', '!', '?', '\n']);
        if should_emit && !accumulated.trim().is_empty() {
            let levels = [0.25, 0.5, 0.8, 0.65, 0.35, 0.9];
            send_state(
                bridge,
                runtime,
                CompanionState::Speaking,
                &compact_caption(&accumulated, 72),
                Some(levels[wave_index % levels.len()]),
            );
            wave_index += 1;
            emitted_length = accumulated.len();
        }
    });

    match result {
        Ok(response) => {
            println!(
                "Orbital [{} / {} / {} ms]: {}",
                response.provider, response.model, response.elapsed_ms, response.text
            );
            runtime.last_response = Some(response.text.clone());
            runtime.last_model_error = None;
            runtime
                .conversation
                .add_exchange(input, response.text.clone());
            if emitted_length == 0 {
                send_state(
                    bridge,
                    runtime,
                    CompanionState::Speaking,
                    &compact_caption(&response.text, 72),
                    Some(0.6),
                );
            }
            if speech.options.speak_responses && speech.tts.enabled() {
                let spoken = spoken_response(&response.text, 500);
                if let Err(error) = speech.tts.speak(&spoken) {
                    let message = format!("{error:#}");
                    eprintln!("TTS warning: {message}");
                    speech.status.last_error = Some(message);
                }
            }
            send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
        }
        Err(error) => {
            eprintln!("Model error: {error:#}");
            runtime.last_model_error = Some(format!("{error:#}"));
            send_state(
                bridge,
                runtime,
                CompanionState::Error,
                "Model unavailable",
                None,
            );
            if provider.name() == "ollama" {
                print_ollama_guidance(provider);
            }
            thread::sleep(Duration::from_millis(900));
            send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
        }
    }
}

fn default_question(question: String, fallback: &str) -> String {
    if question.trim().is_empty() {
        fallback.into()
    } else {
        question
    }
}

fn capture_selection(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    provider: &dyn SelectionProvider,
) -> Option<ContextItem> {
    send_state(
        bridge,
        runtime,
        CompanionState::Thinking,
        "Capturing selection...",
        None,
    );
    match capture_context_item(provider, &mut runtime.context, true) {
        Ok((capture, item)) => {
            if let Some(warning) = capture.warning {
                eprintln!("Selection capture warning: {warning}");
            }
            runtime.last_capture_result = Some(format!(
                "captured {} characters; clipboard restored: {}",
                item.size_chars, capture.clipboard_restored
            ));
            println!(
                "Selection attached: {} characters from {}",
                item.size_chars, item.source
            );
            context_success(bridge, runtime, "Selection attached");
            Some(item)
        }
        Err(error) => {
            runtime.last_capture_result = Some(format!("failed: {error:#}"));
            context_failure(bridge, runtime, "No selection found", error);
            None
        }
    }
}

fn ask_selection(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    model: &dyn ModelProvider,
    system_prompt: &str,
    speech: &mut SpeechServices,
    question: String,
    persist: bool,
) {
    let question = default_question(question, "Explain this selected text briefly.");
    send_state(
        bridge,
        runtime,
        CompanionState::Thinking,
        "Capturing selection...",
        None,
    );
    match capture_context_item(&SystemSelectionProvider, &mut runtime.context, persist) {
        Ok((capture, item)) => {
            if let Some(warning) = capture.warning {
                eprintln!("Selection capture warning: {warning}");
            }
            runtime.last_capture_result = Some(format!(
                "captured {} characters; clipboard restored: {}",
                item.size_chars, capture.clipboard_restored
            ));
            println!(
                "Selection captured: {} characters from {}",
                item.size_chars, item.source
            );
            let temporary = (!persist).then_some(item);
            run_prompt(
                bridge,
                runtime,
                model,
                system_prompt,
                speech,
                question,
                temporary,
            );
        }
        Err(error) => {
            runtime.last_capture_result = Some(format!("failed: {error:#}"));
            context_failure(bridge, runtime, "No selection found", error);
        }
    }
}

fn capture_clipboard(bridge: &RuntimeBridge, runtime: &mut RuntimeCore) {
    send_state(
        bridge,
        runtime,
        CompanionState::Thinking,
        "Reading clipboard...",
        None,
    );
    match runtime.context.capture_clipboard() {
        Ok(item) => {
            println!("Clipboard attached: {} characters", item.size_chars);
            context_success(bridge, runtime, "Clipboard attached");
        }
        Err(error) => context_failure(bridge, runtime, "Clipboard empty or unavailable", error),
    }
}

fn capture_active_window(bridge: &RuntimeBridge, runtime: &mut RuntimeCore) {
    send_state(
        bridge,
        runtime,
        CompanionState::Thinking,
        "Reading active window...",
        None,
    );
    match runtime.context.capture_active_window() {
        Ok(window) => {
            println!("Active window:");
            println!("  title: {}", window.title);
            println!(
                "  process: {}",
                window.process_name.as_deref().unwrap_or("unknown")
            );
            println!(
                "  pid: {}",
                window
                    .process_id
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "unknown".into())
            );
            context_success(bridge, runtime, "Window context attached");
        }
        Err(error) => context_failure(bridge, runtime, "Window info unavailable", error),
    }
}

fn start_watch(bridge: &RuntimeBridge, runtime: &mut RuntimeCore) {
    send_state(
        bridge,
        runtime,
        CompanionState::Thinking,
        "Reading active window...",
        None,
    );
    match runtime.context.start_watch() {
        Ok(window) => {
            let process = window.process_name.as_deref().unwrap_or("unknown");
            println!(
                "Watching active-window metadata: {} ({})",
                window.title, process
            );
            context_success(bridge, runtime, "Watching window metadata");
        }
        Err(error) => context_failure(bridge, runtime, "Watch unavailable", error),
    }
}

fn attach_text(bridge: &RuntimeBridge, runtime: &mut RuntimeCore, text: String) {
    match runtime.context.attach_text(text) {
        Ok(item) => {
            println!("Attached text: {} characters", item.size_chars);
            context_success(bridge, runtime, "Text attached");
        }
        Err(error) => context_failure(bridge, runtime, "Text attachment failed", error),
    }
}

fn attach_file(bridge: &RuntimeBridge, runtime: &mut RuntimeCore, path: &str) {
    send_state(
        bridge,
        runtime,
        CompanionState::Thinking,
        "Reading file...",
        None,
    );
    match runtime.context.attach_file(Path::new(path)) {
        Ok(item) => {
            println!(
                "Attached file: {} ({} characters)",
                item.source, item.size_chars
            );
            context_success(bridge, runtime, "File attached");
        }
        Err(error) => context_failure(bridge, runtime, "File attachment failed", error),
    }
}

fn context_success(bridge: &RuntimeBridge, runtime: &mut RuntimeCore, caption: &str) {
    send_state(bridge, runtime, CompanionState::Happy, caption, None);
    thread::sleep(Duration::from_millis(350));
    send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
}

fn context_failure(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    caption: &str,
    error: anyhow::Error,
) {
    eprintln!("Context error: {error:#}");
    send_state(bridge, runtime, CompanionState::Error, caption, None);
    thread::sleep(Duration::from_millis(700));
    send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
}

fn print_context(runtime: &RuntimeCore) {
    let prompt = runtime.context.prompt_context();
    println!("Context:");
    println!("  items: {}", runtime.context.item_count());
    println!("  source characters: {}", prompt.total_chars);
    println!("  watching: {}", runtime.context.watch_mode());
    match runtime.context.last_context_refresh_at() {
        Some(value) => println!("  last refresh: {value}"),
        None => println!("  last refresh: -"),
    }
    if let Some(window) = runtime.context.active_window() {
        println!("  active title: {}", window.title);
        println!(
            "  active process: {}",
            window.process_name.as_deref().unwrap_or("unknown")
        );
        println!(
            "  active pid: {}",
            window
                .process_id
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "unknown".into())
        );
    }
    for item in runtime.context.items() {
        println!(
            "  - {} [{}] {} chars ({})",
            item.title,
            item.kind.as_str(),
            item.size_chars,
            item.source
        );
    }
}

fn listen_and_ask(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    model: &dyn ModelProvider,
    system_prompt: &str,
    speech: &mut SpeechServices,
    seconds: u64,
) {
    send_state(
        bridge,
        runtime,
        CompanionState::Listening,
        "Listening...",
        None,
    );
    let wav_path = temporary_audio_path("listen");
    let result = if speech.stt.requires_audio_capture() {
        speech
            .audio
            .capture_wav(seconds, &wav_path)
            .and_then(|_| transcribe_path(bridge, runtime, speech, &wav_path))
    } else {
        transcribe_path(bridge, runtime, speech, &wav_path)
    };
    let _ = std::fs::remove_file(&wav_path);

    match result {
        Ok(transcript) => {
            println!("Transcript: {transcript}");
            run_prompt(
                bridge,
                runtime,
                model,
                system_prompt,
                speech,
                transcript,
                None,
            );
        }
        Err(error) => speech_failure(bridge, runtime, speech, error),
    }
}

fn transcribe_file(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    speech: &mut SpeechServices,
    path: &str,
) {
    if path.trim().is_empty() {
        speech_failure(
            bridge,
            runtime,
            speech,
            anyhow::anyhow!("/transcribe-file requires a WAV path"),
        );
        return;
    }
    match transcribe_path(bridge, runtime, speech, Path::new(path)) {
        Ok(transcript) => println!("Transcript: {transcript}"),
        Err(error) => speech_failure(bridge, runtime, speech, error),
    }
}

fn transcribe_path(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    speech: &mut SpeechServices,
    path: &Path,
) -> anyhow::Result<String> {
    send_state(
        bridge,
        runtime,
        CompanionState::Thinking,
        "Transcribing...",
        None,
    );
    let transcription = speech.stt.transcribe_wav(path)?;
    speech.status.last_transcription = Some(transcription.text.clone());
    speech.status.last_error = None;
    println!(
        "STT [{} / {} ms]: {}",
        transcription.provider, transcription.elapsed_ms, transcription.text
    );
    send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
    Ok(transcription.text)
}

fn say_text(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    speech: &mut SpeechServices,
    text: &str,
) {
    if text.trim().is_empty() {
        speech_failure(
            bridge,
            runtime,
            speech,
            anyhow::anyhow!("/say requires text"),
        );
        return;
    }
    if !speech.tts.enabled() {
        println!("TTS is disabled. Start runtime with --tts piper or --tts windows-sapi.");
        return;
    }
    send_state(
        bridge,
        runtime,
        CompanionState::Speaking,
        &compact_caption(text, 72),
        Some(0.6),
    );
    match speech.tts.speak(text) {
        Ok(()) => {
            speech.status.last_error = None;
            send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
        }
        Err(error) => speech_failure(bridge, runtime, speech, error),
    }
}

fn speech_failure(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    speech: &mut SpeechServices,
    error: anyhow::Error,
) {
    let message = format!("{error:#}");
    eprintln!("Speech error: {message}");
    speech.status.last_error = Some(message);
    send_state(
        bridge,
        runtime,
        CompanionState::Error,
        "Speech failed",
        None,
    );
    thread::sleep(Duration::from_millis(700));
    send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
}

fn temporary_audio_path(prefix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "orbital-{prefix}-{}-{}.wav",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ))
}

fn run_demo(bridge: &RuntimeBridge, runtime: &mut RuntimeCore) {
    println!("Running face state demo...");
    let steps = [
        ("idle", "Ready", None),
        ("listening", "Listening...", None),
        ("thinking", "Thinking...", None),
        ("speaking", "Here is what I found", Some(0.7)),
        ("happy", "Done", None),
        ("idle", "Ready", None),
    ];
    for (state, caption, audio_level) in steps {
        runtime.state = match state {
            "listening" => CompanionState::Listening,
            "thinking" => CompanionState::Thinking,
            "speaking" => CompanionState::Speaking,
            _ => CompanionState::Idle,
        };
        send_event(
            bridge,
            runtime,
            RuntimeToFaceEvent::State {
                state: state.into(),
                emotion: None,
                caption: Some(caption.into()),
                audio_level,
            },
        );
        thread::sleep(Duration::from_millis(750));
    }
}

fn compact_caption(text: &str, max_chars: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<_> = compact.chars().collect();
    if chars.len() <= max_chars {
        compact
    } else {
        chars[chars.len() - max_chars..].iter().collect()
    }
}

fn send_state(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    state: CompanionState,
    caption: &str,
    audio_level: Option<f32>,
) {
    let event = runtime.state_event(state, Some(caption.to_owned()), audio_level);
    send_event(bridge, runtime, event);
}

fn send_event(bridge: &RuntimeBridge, runtime: &mut RuntimeCore, event: RuntimeToFaceEvent) {
    if !runtime.face_connected {
        eprintln!("Face disconnected; skipped {} event", event.event_type());
        return;
    }
    if let Err(error) = bridge.send(event) {
        eprintln!("Bridge send failed: {error}");
    }
}

fn handle_bridge_update(bridge: &RuntimeBridge, runtime: &mut RuntimeCore, update: ServerUpdate) {
    match update {
        ServerUpdate::Connected => {
            runtime.bridge_connected();
            println!("Face host WebSocket connected; waiting for face.ready");
        }
        ServerUpdate::Disconnected => {
            runtime.bridge_disconnected();
            println!("Face host disconnected; runtime remains available");
        }
        ServerUpdate::Sent(event_type) => runtime.record_sent(event_type),
        ServerUpdate::Warning(message) => eprintln!("Bridge warning: {message}"),
        ServerUpdate::Received(message) => {
            log_face_message(&message);
            if let FaceToRuntimeMessage::Unknown { event_type } = &message {
                eprintln!("Warning: ignored unknown face event {event_type:?}");
            }
            if let Some(response) = runtime.handle_face_message(message) {
                send_event(bridge, runtime, response);
            }
        }
    }
}

fn log_face_message(message: &FaceToRuntimeMessage) {
    match message {
        FaceToRuntimeMessage::Ready { face, version } => {
            println!("<- face.ready face={face:?} version={version:?}")
        }
        FaceToRuntimeMessage::Clicked { x, y, button, .. } => {
            println!("<- face.clicked x={x:.0} y={y:.0} button={button}")
        }
        FaceToRuntimeMessage::DoubleClicked { x, y, button, .. } => {
            println!("<- face.double_clicked x={x:.0} y={y:.0} button={button}");
            println!("Listening toggled visually; voice input is not implemented.");
        }
        FaceToRuntimeMessage::Dragged { x, y } => println!("<- face.dragged x={x} y={y}"),
        FaceToRuntimeMessage::Action { action } => {
            println!("<- face.action action={action:?}");
            if action == "toggle_listening" {
                println!("Listening toggled visually; voice input is not implemented.");
            }
        }
        FaceToRuntimeMessage::Ping { id } => println!("<- ping id={id:?}"),
        FaceToRuntimeMessage::Pong { id } => println!("<- pong id={id:?} (round trip complete)"),
        FaceToRuntimeMessage::Unknown { event_type } => {
            println!("<- unknown type={event_type:?}")
        }
    }
}

fn print_status(runtime: &RuntimeCore, provider: &dyn ModelProvider, speech: &SpeechServices) {
    println!("Runtime status:");
    println!("  state: {:?}", runtime.state);
    println!("  provider: {}", provider.name());
    println!("  model: {}", provider.model_name());
    println!("  face connected: {}", runtime.face_connected);
    println!("  face: {}", runtime.face_name.as_deref().unwrap_or("-"));
    println!(
        "  face version: {}",
        runtime.face_version.as_deref().unwrap_or("-")
    );
    println!(
        "  last input: {}",
        runtime.last_user_input.as_deref().unwrap_or("-")
    );
    println!(
        "  last response: {}",
        runtime.last_response.as_deref().unwrap_or("-")
    );
    println!(
        "  last received: {}",
        runtime.last_bridge_event_received.as_deref().unwrap_or("-")
    );
    println!(
        "  last sent: {}",
        runtime.last_bridge_event_sent.as_deref().unwrap_or("-")
    );
    println!(
        "  conversation messages: {}",
        runtime.conversation.message_count()
    );
    println!("  context items: {}", runtime.context.item_count());
    println!("  watching: {}", runtime.context.watch_mode());
    if let Some(window) = runtime.context.active_window() {
        println!(
            "  active window: {} ({})",
            window.title,
            window.process_name.as_deref().unwrap_or("unknown")
        );
    }
    println!(
        "  last model error: {}",
        runtime.last_model_error.as_deref().unwrap_or("-")
    );
    print!("{}", quick_capture_status(runtime));
    print!("{}", speech_status_text(speech));
}

fn quick_capture_status(runtime: &RuntimeCore) -> String {
    let mut output = format!(
        "  hotkeys enabled: {}\n  selection capture supported: {}\n  active window supported: {}\n  last capture result: {}\n",
        runtime.hotkeys_enabled,
        runtime.selection_capture_supported,
        runtime.active_window_supported,
        runtime.last_capture_result.as_deref().unwrap_or("-")
    );
    if let Some(window) = runtime.context.active_window() {
        output.push_str(&format!(
            "  last active window: {} ({}) pid={}\n",
            window.title,
            window.process_name.as_deref().unwrap_or("unknown"),
            window
                .process_id
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "unknown".into())
        ));
    }
    output
}

fn print_model_status(runtime: &RuntimeCore, provider: &dyn ModelProvider) {
    let status = provider.status();
    println!("Model status:");
    println!("  provider: {}", provider.name());
    println!("  model: {}", provider.model_name());
    if let Some(base_url) = provider.base_url() {
        println!("  base URL: {base_url}");
    }
    println!("  reachable: {}", status.reachable);
    if let Some(available) = status.model_available {
        println!("  model available: {available}");
    }
    println!("  detail: {}", status.detail);
    println!(
        "  last error: {}",
        runtime.last_model_error.as_deref().unwrap_or("-")
    );
    if provider.name() == "ollama" {
        println!("  pull command: ollama pull {}", provider.model_name());
    }
}

fn print_speech_status(speech: &SpeechServices) {
    print!("{}", speech_status_text(speech));
}

fn speech_status_text(speech: &SpeechServices) -> String {
    format!(
        "Speech status:\n  STT provider: {}\n  TTS provider: {}\n  Whisper model: {}\n  Piper binary: {}\n  Piper model: {}\n  speak responses: {}\n  microphone capture supported: {}\n  last transcription: {}\n  last speech error: {}\n",
        speech.stt.name(),
        speech.tts.name(),
        speech
            .stt
            .model_path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "-".into()),
        speech
            .options
            .piper_bin
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "-".into()),
        speech
            .options
            .piper_model
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "-".into()),
        speech.options.speak_responses,
        speech.audio.supported(),
        speech.status.last_transcription.as_deref().unwrap_or("-"),
        speech.status.last_error.as_deref().unwrap_or("-")
    )
}

fn print_ollama_guidance(provider: &dyn ModelProvider) {
    println!(
        "Ollama is not reachable at {}.",
        provider.base_url().unwrap_or("the configured URL")
    );
    println!("Start Ollama and pull the model:");
    println!("ollama pull {}", provider.model_name());
    println!("Or run with mock mode:");
    println!("cargo run --bin orbital-runtime -- --model mock");
}

fn spawn_terminal_reader() -> Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    rx
}

enum ServerUpdate {
    Connected,
    Disconnected,
    Received(FaceToRuntimeMessage),
    Sent(&'static str),
    Warning(String),
}

struct RuntimeBridge {
    outgoing: Sender<RuntimeToFaceEvent>,
    updates: Receiver<ServerUpdate>,
}

impl RuntimeBridge {
    fn start(address: &str) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(address)?;
        let (outgoing, outgoing_rx) = mpsc::channel();
        let (update_tx, updates) = mpsc::channel();
        thread::spawn(move || run_server(listener, outgoing_rx, update_tx));
        Ok(Self { outgoing, updates })
    }

    fn send(&self, event: RuntimeToFaceEvent) -> Result<(), mpsc::SendError<RuntimeToFaceEvent>> {
        self.outgoing.send(event)
    }

    fn try_recv(&self) -> Option<ServerUpdate> {
        self.updates.try_recv().ok()
    }
}

fn run_server(
    listener: TcpListener,
    outgoing: Receiver<RuntimeToFaceEvent>,
    updates: Sender<ServerUpdate>,
) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => match accept_connection(stream) {
                Ok(mut socket) => {
                    let _ = updates.send(ServerUpdate::Connected);
                    if let Err(error) = serve_connection(&mut socket, &outgoing, &updates) {
                        let _ = updates.send(ServerUpdate::Warning(error.to_string()));
                    }
                    let _ = updates.send(ServerUpdate::Disconnected);
                }
                Err(error) => {
                    let _ = updates.send(ServerUpdate::Warning(error.to_string()));
                }
            },
            Err(error) => {
                let _ = updates.send(ServerUpdate::Warning(error.to_string()));
            }
        }
    }
}

fn accept_connection(stream: TcpStream) -> anyhow::Result<WebSocket<TcpStream>> {
    stream.set_read_timeout(Some(IO_POLL_INTERVAL))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    Ok(accept(stream)?)
}

fn serve_connection(
    socket: &mut WebSocket<TcpStream>,
    outgoing: &Receiver<RuntimeToFaceEvent>,
    updates: &Sender<ServerUpdate>,
) -> anyhow::Result<()> {
    loop {
        while let Ok(event) = outgoing.try_recv() {
            let event_type = event.event_type();
            socket.send(Message::Text(serde_json::to_string(&event)?.into()))?;
            let _ = updates.send(ServerUpdate::Sent(event_type));
        }

        match socket.read() {
            Ok(Message::Text(text)) => match parse_face_message(&text) {
                Ok(message) => {
                    let _ = updates.send(ServerUpdate::Received(message));
                }
                Err(error) => {
                    let _ = updates.send(ServerUpdate::Warning(format!(
                        "ignored invalid face message: {error}"
                    )));
                }
            },
            Ok(Message::Close(_)) => return Ok(()),
            Ok(Message::Ping(payload)) => socket.send(Message::Pong(payload))?,
            Ok(_) => {}
            Err(WebSocketError::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed) => return Ok(()),
            Err(error) => return Err(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{build_speech_services, compact_caption, quick_capture_status, speech_status_text};
    use orbital_face_host::runtime_v0::RuntimeCore;
    use orbital_face_host::speech::SpeechOptions;

    #[test]
    fn compact_caption_keeps_recent_text() {
        assert_eq!(compact_caption("one two three four", 8), "ree four");
    }

    #[test]
    fn status_includes_quick_capture_indicators() {
        let runtime = RuntimeCore::default();
        let status = quick_capture_status(&runtime);
        assert!(status.contains("hotkeys enabled: false"));
        assert!(status.contains("selection capture supported:"));
        assert!(status.contains("active window supported:"));
        assert!(status.contains("last capture result: -"));
    }

    #[test]
    fn speech_status_formats_default_providers() {
        let speech = build_speech_services(&SpeechOptions::default()).unwrap();
        let status = speech_status_text(&speech);
        assert!(status.contains("STT provider: mock"));
        assert!(status.contains("TTS provider: none"));
        assert!(status.contains("speak responses: false"));
    }
}
