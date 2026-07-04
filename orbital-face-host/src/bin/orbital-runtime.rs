use orbital_face_host::context::active_window::{ActiveWindowProvider, SystemActiveWindowProvider};
use orbital_face_host::context::clipboard::SystemClipboardProvider;
use orbital_face_host::context::{ContextItem, ContextKind};
use orbital_face_host::hotkeys::{HotkeyAction, HotkeyProvider, SystemHotkeyProvider};
use orbital_face_host::memory::{
    default_database_path, MemoryConfig, MemoryStore, SqliteMemoryStore, ToolInvocationRecord,
};
use orbital_face_host::model_provider::{
    MockModelProvider, ModelChunk, ModelProvider, ModelRequest, OllamaModelProvider,
    RuntimeModelOptions, ToolPlanningRequest,
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
use orbital_face_host::tools::{
    execute_tool, permission_for, ToolEnvironment, ToolInvocation, ToolPermission, ToolRegistry,
    ToolSource,
};
use serde_json::Value;
use std::collections::HashSet;
use std::io::{self, BufRead};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tungstenite::{accept, Error as WebSocketError, Message, WebSocket};

const ADDRESS: &str = "127.0.0.1:7373";
const BRIDGE_URL: &str = "ws://127.0.0.1:7373";
const IO_POLL_INTERVAL: Duration = Duration::from_millis(100);
static FILLER_VARIANT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

struct PendingTool {
    invocation: ToolInvocation,
    audit_id: i64,
    original_input: Option<String>,
}

struct ToolRuntime {
    memory: SqliteMemoryStore,
    config: MemoryConfig,
    writes_enabled: bool,
    session_id: String,
    registry: ToolRegistry,
    pending: Option<PendingTool>,
    persisted_context_ids: HashSet<String>,
    enable_model_tools: bool,
}

fn main() -> anyhow::Result<()> {
    if std::env::args().any(|argument| matches!(argument.as_str(), "-h" | "--help")) {
        print_help();
        return Ok(());
    }
    let options = RuntimeModelOptions::parse_from(std::env::args().skip(1))?;
    let provider = model_provider_from_options(&options)?;
    let memory_path = options
        .db_path
        .clone()
        .unwrap_or_else(default_database_path);
    let memory = match SqliteMemoryStore::open(&memory_path) {
        Ok(memory) => memory,
        Err(error) => {
            eprintln!(
                "Memory database unavailable at {}: {error:#}",
                memory_path.display()
            );
            eprintln!("Continuing with a temporary in-memory database.");
            SqliteMemoryStore::open(":memory:")?
        }
    };
    let session_id = memory.start_session(provider.name(), provider.model_name())?;
    let mut tools = ToolRuntime {
        memory,
        config: MemoryConfig {
            enabled: true,
            store_session_messages: options.store_session_messages,
            store_context_items: options.store_context_items,
            db_path: options.db_path.clone(),
        },
        writes_enabled: true,
        session_id,
        registry: ToolRegistry::with_builtins(),
        pending: None,
        persisted_context_ids: HashSet::new(),
        enable_model_tools: options.enable_model_tools,
    };
    let mut speech = build_speech_services(&options.speech)?;
    let bridge = RuntimeBridge::start(ADDRESS)?;
    let terminal = spawn_terminal_reader();
    let mut runtime = RuntimeCore::default();
    runtime.selection_capture_supported = SystemSelectionProvider.supported();
    runtime.active_window_supported = SystemActiveWindowProvider.supported();
    let hotkeys = start_hotkeys(&options, &mut runtime);
    let mut auto_listen = false;

    println!("Orbital Runtime v0");
    println!("Bridge listening on {BRIDGE_URL}");
    print_provider_startup(provider.as_ref());
    if provider.name() == "ollama" {
        println!(
            "Model thinking: {}",
            if options.enable_model_thinking {
                "enabled"
            } else {
                "disabled"
            }
        );
    }
    println!("Type a message and press Enter.");
    println!(
        "Commands: /quit, /status, /model, /clear, /context, /clear-context, \
         /clipboard, /active-window, /watch, /unwatch, /attach-text, /attach-file, \
         /selection, /ask, /ask-selection, /ask-selection-once, /listen, \
         /auto-listen, /face, /remember, /memories, /memory-search, /tools, /tool, \
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
                    &mut tools,
                    action,
                );
            }
        }

        match terminal.try_recv() {
            Ok(line) => {
                if tools.pending.is_some() {
                    handle_tool_confirmation(
                        &bridge,
                        &mut runtime,
                        provider.as_ref(),
                        &options.system_prompt,
                        &mut speech,
                        &mut tools,
                        &line,
                    );
                } else {
                    if !handle_command(
                        &bridge,
                        &mut runtime,
                        provider.as_ref(),
                        &options.system_prompt,
                        &mut speech,
                        &mut auto_listen,
                        &mut tools,
                        parse_command(&line),
                    ) {
                        break;
                    }
                }
                persist_new_context(&runtime, &mut tools);
            }
            Err(TryRecvError::Empty) => thread::sleep(Duration::from_millis(25)),
            Err(TryRecvError::Disconnected) => break,
        }

        if auto_listen && tools.pending.is_none() {
            auto_listen_and_ask(
                &bridge,
                &mut runtime,
                provider.as_ref(),
                &options.system_prompt,
                &mut speech,
                &mut tools,
            );
        }
    }

    if let Err(error) = tools.memory.end_session(&tools.session_id) {
        eprintln!("Memory warning: failed to close session: {error:#}");
    }
    println!("Orbital Runtime stopped.");
    Ok(())
}

fn model_provider_from_options(
    options: &RuntimeModelOptions,
) -> anyhow::Result<Box<dyn ModelProvider>> {
    match options.provider.as_str() {
        "mock" => Ok(Box::new(MockModelProvider)),
        "ollama" => Ok(Box::new(
            OllamaModelProvider::new(&options.ollama_base_url, &options.ollama_model)?
                .with_thinking(options.enable_model_thinking),
        )),
        _ => unreachable!("provider name was validated"),
    }
}

struct SpeechServices {
    stt: Box<dyn SpeechToTextProvider>,
    tts: Arc<dyn TextToSpeechProvider>,
    audio: SystemAudioCapture,
    options: SpeechOptions,
    status: SpeechRuntimeStatus,
}

#[derive(Debug)]
struct StreamedSpeechResult {
    label: &'static str,
    synthesis_ms: u128,
    playback_ms: u128,
    audio_finished_ms: u128,
    error: Option<String>,
}

fn spawn_speech_worker(
    tts: Arc<dyn TextToSpeechProvider>,
    turn_started: Instant,
) -> (
    Sender<(&'static str, String)>,
    thread::JoinHandle<Vec<StreamedSpeechResult>>,
) {
    let (sender, receiver) = mpsc::channel::<(&'static str, String)>();
    let handle = thread::spawn(move || {
        let mut results = Vec::new();
        for (label, text) in receiver {
            let speech_started_ms = turn_started.elapsed().as_millis();
            match tts.speak_profiled(&text) {
                Ok(profile) => {
                    let audible_ms = speech_started_ms + profile.synthesis_ms;
                    eprintln!(
                        "Streaming audio: {label}, synthesis={} ms, audible_at={} ms, playback={} ms",
                        profile.synthesis_ms, audible_ms, profile.playback_ms
                    );
                    results.push(StreamedSpeechResult {
                        label,
                        synthesis_ms: profile.synthesis_ms,
                        playback_ms: profile.playback_ms,
                        audio_finished_ms: turn_started.elapsed().as_millis(),
                        error: None,
                    });
                }
                Err(error) => results.push(StreamedSpeechResult {
                    label,
                    synthesis_ms: 0,
                    playback_ms: 0,
                    audio_finished_ms: turn_started.elapsed().as_millis(),
                    error: Some(format!("{error:#}")),
                }),
            }
        }
        results
    });
    (sender, handle)
}

fn take_complete_speech_segments(buffer: &mut String) -> Vec<String> {
    const MIN_SEGMENT_CHARS: usize = 40;
    let mut segments = Vec::new();
    loop {
        let boundary = buffer.char_indices().find_map(|(index, character)| {
            let end = index + character.len_utf8();
            (matches!(character, '.' | '!' | '?' | '\n')
                && buffer[..end].chars().count() >= MIN_SEGMENT_CHARS)
                .then_some(end)
        });
        let Some(end) = boundary else {
            break;
        };
        let segment: String = buffer.drain(..end).collect();
        if !segment.trim().is_empty() {
            segments.push(segment);
        }
    }
    segments
}

fn latency_filler(input: &str, variant: usize) -> &'static str {
    let normalized = input.trim().to_ascii_lowercase();
    let is_question = input.contains('?')
        || ["what", "why", "how", "when", "where", "who", "which"]
            .iter()
            .any(|word| normalized.starts_with(word));
    let is_request = ["please", "can you", "could you", "would you", "will you"]
        .iter()
        .any(|prefix| normalized.starts_with(prefix));

    let choices = if is_question {
        [
            "Let me think about that.",
            "Let's see.",
            "Give me a moment to consider that.",
            "Let me work through that.",
        ]
    } else if is_request {
        [
            "All right, one moment.",
            "Okay, give me a moment.",
            "Let me consider that.",
            "One moment while I think it through.",
        ]
    } else {
        [
            "I'm taking that in.",
            "Let me think for a moment.",
            "One moment.",
            "I'm considering what you said.",
        ]
    };
    choices[variant % choices.len()]
}

fn should_use_latency_fillers(input: &str) -> bool {
    let normalized = input
        .trim()
        .trim_matches(|character: char| character.is_ascii_punctuation())
        .to_ascii_lowercase();
    let word_count = normalized.split_whitespace().count();
    word_count > 2
        && !matches!(
            normalized.as_str(),
            "ok" | "okay" | "yes" | "no" | "sure" | "thanks" | "thank you" | "hello" | "hi"
        )
}

fn waiting_filler(variant: usize) -> &'static str {
    const CHOICES: [&str; 4] = [
        "This is taking me a little longer. I'm still working through it.",
        "There's a bit more to consider here.",
        "I'm still thinking this through.",
        "I'm working through the details now.",
    ];
    CHOICES[variant % CHOICES.len()]
}

fn extended_waiting_filler(variant: usize) -> &'static str {
    const CHOICES: [&str; 4] = [
        "Thanks for your patience. I'm still with you.",
        "Sorry for the wait. I'm still working through this.",
        "This is taking longer than usual. Thanks for waiting.",
        "I haven't forgotten you. I'm still putting the answer together.",
    ];
    CHOICES[variant % CHOICES.len()]
}

fn wait_until_or_answer_queued(
    turn_started: Instant,
    deadline: Duration,
    answer_audio_queued: &AtomicBool,
) -> bool {
    while turn_started.elapsed() < deadline {
        if answer_audio_queued.load(Ordering::Acquire) {
            return false;
        }
        let remaining = deadline.saturating_sub(turn_started.elapsed());
        thread::sleep(remaining.min(Duration::from_millis(50)));
    }
    !answer_audio_queued.load(Ordering::Acquire)
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
    let tts: Arc<dyn TextToSpeechProvider> = match options.tts.as_str() {
        "none" => Arc::new(NoopTtsProvider),
        "piper" => Arc::new(PiperTtsProvider::new(
            options.piper_bin.clone(),
            options.piper_model.clone(),
            options.piper_config.clone(),
        )?),
        "windows-sapi" => Arc::new(WindowsSapiTtsProvider),
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
         [--enable-model-tools] [--enable-model-thinking] [--db-path <path>] \
         [--store-session-messages] \
         [--store-context-items] \
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
    tools: &mut ToolRuntime,
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
            tools,
            "Explain this selected text briefly.".into(),
            true,
        ),
        HotkeyAction::ListenFiveSeconds => {
            listen_and_ask(bridge, runtime, model, system_prompt, speech, tools, 5)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_command(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    provider: &dyn ModelProvider,
    system_prompt: &str,
    speech: &mut SpeechServices,
    auto_listen: &mut bool,
    tools: &mut ToolRuntime,
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
                tools,
                question,
                None,
                true,
            );
        }
        RuntimeCommand::AskSelection(question) => ask_selection(
            bridge,
            runtime,
            provider,
            system_prompt,
            speech,
            tools,
            question,
            true,
        ),
        RuntimeCommand::AskSelectionOnce(question) => ask_selection(
            bridge,
            runtime,
            provider,
            system_prompt,
            speech,
            tools,
            question,
            false,
        ),
        RuntimeCommand::Listen(seconds) => listen_and_ask(
            bridge,
            runtime,
            provider,
            system_prompt,
            speech,
            tools,
            seconds,
        ),
        RuntimeCommand::AutoListen(enabled) => {
            if enabled && (!speech.stt.requires_audio_capture() || !speech.audio.supported()) {
                eprintln!("Auto-listen requires a real STT provider and an available microphone.");
                *auto_listen = false;
            } else {
                *auto_listen = enabled;
                println!(
                    "Auto-listen {}.",
                    if enabled {
                        "started; speak when ready"
                    } else {
                        "stopped"
                    }
                );
            }
        }
        RuntimeCommand::Face(face) => {
            if face.is_empty() {
                eprintln!("Usage: /face <name-or-directory>");
            } else if !runtime.face_connected {
                eprintln!("Face disconnected; cannot switch face.");
            } else {
                println!("Requesting face switch to {face:?}");
                send_event(bridge, runtime, RuntimeToFaceEvent::SwitchFace { face });
            }
        }
        RuntimeCommand::TranscribeFile(path) => transcribe_file(bridge, runtime, speech, &path),
        RuntimeCommand::Say(text) => say_text(bridge, runtime, speech, &text),
        RuntimeCommand::SpeechStatus => print_speech_status(speech),
        RuntimeCommand::Remember(text) => remember_command(tools, &text),
        RuntimeCommand::Memories => list_memories_command(tools),
        RuntimeCommand::MemorySearch(query) => search_memories_command(tools, &query),
        RuntimeCommand::ForgetMemory(id) => forget_memory_command(tools, &id),
        RuntimeCommand::MemoryStatus => memory_status_command(tools),
        RuntimeCommand::MemoryClearSession => clear_memory_session_command(tools),
        RuntimeCommand::MemoryEnabled(enabled) => {
            tools.writes_enabled = enabled;
            println!(
                "Memory writes {} for this runtime session.",
                if enabled { "enabled" } else { "disabled" }
            );
        }
        RuntimeCommand::Tools => list_tools_command(tools),
        RuntimeCommand::ToolInfo(name) => tool_info_command(tools, &name),
        RuntimeCommand::Tool { name, arguments } => {
            manual_tool_command(bridge, runtime, tools, &name, &arguments)
        }
        RuntimeCommand::ToolHistory(limit) => tool_history_command(tools, limit),
        RuntimeCommand::ToolClearSession => clear_tool_session_command(tools),
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
            tools,
            input,
            None,
            true,
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

#[allow(clippy::too_many_arguments)]
fn run_prompt(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    provider: &dyn ModelProvider,
    system_prompt: &str,
    speech: &mut SpeechServices,
    tools: &mut ToolRuntime,
    input: String,
    temporary_context: Option<ContextItem>,
    allow_tool_suggestion: bool,
) {
    let turn_started = Instant::now();
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
    let mut combined_context = prompt_context.formatted;
    let memory_context = persistent_memory_context(tools);
    if !memory_context.is_empty() {
        if !combined_context.is_empty() {
            combined_context.push_str("\n\n");
        }
        combined_context.push_str(&memory_context);
    }
    let confirmed_memory_write = temporary_context
        .as_ref()
        .is_some_and(|item| item.title == "Tool result: memory.remember");
    if allow_tool_suggestion && tools.writes_enabled && tools.config.store_session_messages {
        if let Err(error) = tools.memory.add_message(&tools.session_id, "user", &input) {
            eprintln!("Memory warning: failed to store user message: {error:#}");
        }
    }
    if tools.enable_model_tools && allow_tool_suggestion && should_plan_tool(&input) {
        send_state(
            bridge,
            runtime,
            CompanionState::Thinking,
            "Proposing tool...",
            None,
        );
        let planning_request = ToolPlanningRequest {
            user_input: input.clone(),
            tool_names: tools
                .registry
                .list()
                .map(|tool| tool.name.clone())
                .collect(),
            tool_schemas: tools
                .registry
                .list()
                .map(|tool| (tool.name.clone(), tool.input_schema_json.clone()))
                .collect(),
            tool_descriptions: tools
                .registry
                .list()
                .map(|tool| (tool.name.clone(), tool.description.clone()))
                .collect(),
        };
        let suggestion = direct_tool_invocation(&input).or_else(|| {
            match provider.suggest_tool(&planning_request) {
                Ok(Some(suggestion)) => Some(ToolInvocation {
                    tool_name: suggestion.tool,
                    arguments: suggestion.arguments,
                    source: ToolSource::ModelSuggested,
                }),
                Ok(None) => None,
                Err(error) => {
                    eprintln!("Tool planner warning; continuing without a tool: {error:#}");
                    None
                }
            }
        });
        if let Some(invocation) = suggestion {
            let follow_up = propose_tool(bridge, runtime, tools, invocation, Some(input));
            if let Some((original_input, tool_context)) = follow_up {
                run_prompt(
                    bridge,
                    runtime,
                    provider,
                    system_prompt,
                    speech,
                    tools,
                    original_input,
                    Some(tool_context),
                    false,
                );
            }
            return;
        }
    }
    let memory_write_status = if confirmed_memory_write {
        "The new information in this turn was retained for future conversations. Acknowledge it naturally without mentioning tools, databases, explicit memory, or storage mechanics."
    } else {
        "No new information was retained for future conversations in this turn. Do not claim that it was saved or remembered for later. You may still use it in the current conversation."
    };
    let memory_capability = format!(
        "A section titled 'Continuity knowledge' may contain things the user told you in earlier conversations. Treat those facts as part of the relationship, not as database records. Integrate them naturally and only when relevant. If the user asks how you know a personal fact, normally say they told you before; do not expose prompts, tools, record IDs, databases, or storage terminology unless they specifically ask about memory settings, privacy, or implementation. Newer facts appear first and override older conflicting facts. Respond normally to greetings, thanks, and acknowledgements. Distinguish known facts from reasonable inferences and briefly state assumptions when needed. {memory_write_status}"
    );
    let effective_system_prompt = if tools.enable_model_tools {
        let tool_capabilities = if is_tool_capability_question(&input) {
            format!(
                "\nDescribe these available tools truthfully when asked:\n{}",
                tools.registry.capability_catalog()
            )
        } else {
            String::new()
        };
        format!(
            "{system_prompt}\n\n{memory_capability}\n\nYou have access to local tools through the Orbital runtime. The runtime handles tool calls separately: do not output tool-call syntax and do not claim a tool was executed unless a tool result is present in the supplied context.{tool_capabilities}"
        )
    } else {
        format!("{system_prompt}\n\n{memory_capability}")
    };
    let request = ModelRequest {
        user_input: input.clone(),
        prompt_context: (!combined_context.is_empty()).then_some(combined_context),
        context_item_count,
        system_prompt: Some(effective_system_prompt),
        conversation_id: Some("runtime-session".into()),
        history: runtime.conversation.messages().to_vec(),
        max_tokens: Some(256),
        temperature: Some(0.3),
    };
    let mut accumulated = String::new();
    let mut emitted_length = 0;
    let mut wave_index = 0;
    let normalized_input = input.to_ascii_lowercase();
    let guard_memory_claims = !confirmed_memory_write
        && (explicit_memory_content(&input).is_some()
            || normalized_input.contains("memory")
            || normalized_input.contains("i meant ")
            || normalized_input.starts_with("no, it's not"));
    let model_started = Instant::now();
    let prompt_prep_ms = turn_started.elapsed().as_millis();
    let mut first_token_ms = None;
    let mut speech_buffer = String::new();
    let mut spoken_chars = 0usize;
    let answer_audio_queued = Arc::new(AtomicBool::new(false));
    let (speech_sender, speech_worker, filler_worker) =
        if speech.options.speak_responses && speech.tts.enabled() {
            let (sender, worker) = spawn_speech_worker(Arc::clone(&speech.tts), turn_started);
            let filler = should_use_latency_fillers(&input).then(|| {
                let filler_sender = sender.clone();
                let answer_audio_queued = Arc::clone(&answer_audio_queued);
                let filler_variant = FILLER_VARIANT.fetch_add(1, Ordering::Relaxed);
                let filler_text = latency_filler(&input, filler_variant).to_string();
                let waiting_text = waiting_filler(filler_variant + 1).to_string();
                let extended_text = extended_waiting_filler(filler_variant + 2).to_string();
                let filler_delay_ms = 350 + (filler_variant as u64 % 4) * 100;
                thread::spawn(move || {
                    if wait_until_or_answer_queued(
                        turn_started,
                        Duration::from_millis(filler_delay_ms),
                        &answer_audio_queued,
                    ) {
                        let _ = filler_sender.send(("filler_initial", filler_text));
                    } else {
                        return;
                    }
                    if wait_until_or_answer_queued(
                        turn_started,
                        Duration::from_secs(4),
                        &answer_audio_queued,
                    ) {
                        let _ = filler_sender.send(("filler_waiting", waiting_text));
                    } else {
                        return;
                    }
                    if wait_until_or_answer_queued(
                        turn_started,
                        Duration::from_secs(12),
                        &answer_audio_queued,
                    ) {
                        let _ = filler_sender.send(("filler_extended", extended_text));
                    }
                })
            });
            (Some(sender), Some(worker), filler)
        } else {
            (None, None, None)
        };
    let result = provider.generate_streaming(request, &mut |chunk: ModelChunk| {
        if first_token_ms.is_none() && !chunk.text_delta.is_empty() {
            first_token_ms = Some(model_started.elapsed().as_millis());
        }
        accumulated.push_str(&chunk.text_delta);
        if speech_sender.is_some() && !guard_memory_claims && spoken_chars < 500 {
            speech_buffer.push_str(&chunk.text_delta);
            for segment in take_complete_speech_segments(&mut speech_buffer) {
                let remaining = 500usize.saturating_sub(spoken_chars);
                let spoken = spoken_response(&segment, remaining);
                if !spoken.is_empty() {
                    spoken_chars += spoken.chars().count();
                    answer_audio_queued.store(true, Ordering::Release);
                    if let Some(sender) = &speech_sender {
                        let _ = sender.send(("response", spoken));
                    }
                }
            }
        }
        let should_emit = chunk.done
            || accumulated.len().saturating_sub(emitted_length) >= 12
            || chunk.text_delta.contains(['.', '!', '?', '\n']);
        if should_emit && !guard_memory_claims && !accumulated.trim().is_empty() {
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
        Ok(mut response) => {
            let model_ms = model_started.elapsed().as_millis();
            if response.text.contains("```orbital_tool") {
                eprintln!("Suppressed unexpected tool-control syntax from the assistant response.");
                answer_audio_queued.store(true, Ordering::Release);
                if let Some(filler) = filler_worker {
                    let _ = filler.join();
                }
                drop(speech_sender);
                if let Some(worker) = speech_worker {
                    let _ = worker.join();
                }
                send_state(
                    bridge,
                    runtime,
                    CompanionState::Error,
                    "Invalid tool response",
                    None,
                );
                thread::sleep(Duration::from_millis(350));
                send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
                return;
            }
            if !confirmed_memory_write && claims_memory_write(&response.text) {
                eprintln!(
                    "Suppressed an unconfirmed assistant claim that persistent memory was updated."
                );
                response.text = "Understood. I can use that in this conversation. If you want me to retain it for later, ask me to remember it.".into();
            }
            println!(
                "Orbital [{} / {} / {} ms]: {}",
                response.provider, response.model, response.elapsed_ms, response.text
            );
            runtime.last_response = Some(response.text.clone());
            runtime.last_model_error = None;
            runtime
                .conversation
                .add_exchange(input.clone(), response.text.clone());
            if tools.writes_enabled && tools.config.store_session_messages {
                if let Err(error) =
                    tools
                        .memory
                        .add_message(&tools.session_id, "assistant", &response.text)
                {
                    eprintln!("Memory warning: failed to store assistant message: {error:#}");
                }
            }
            if emitted_length == 0 {
                send_state(
                    bridge,
                    runtime,
                    CompanionState::Speaking,
                    &compact_caption(&response.text, 72),
                    Some(0.6),
                );
            }
            if let Some(sender) = &speech_sender {
                let remaining = 500usize.saturating_sub(spoken_chars);
                let final_speech = if guard_memory_claims {
                    response.text.as_str()
                } else {
                    speech_buffer.as_str()
                };
                let spoken = spoken_response(final_speech, remaining);
                if !spoken.is_empty() {
                    answer_audio_queued.store(true, Ordering::Release);
                    let _ = sender.send(("response", spoken));
                }
            }
            if let Some(filler) = filler_worker {
                let _ = filler.join();
            }
            drop(speech_sender);
            if let Some(worker) = speech_worker {
                let results = worker.join().unwrap_or_default();
                let synthesis_ms: u128 = results.iter().map(|result| result.synthesis_ms).sum();
                let playback_ms: u128 = results.iter().map(|result| result.playback_ms).sum();
                if let Some(error) = results.iter().find_map(|result| result.error.as_ref()) {
                    eprintln!("TTS warning: {error}");
                    speech.status.last_error = Some(error.clone());
                } else {
                    speech.status.last_error = None;
                }
                let meaningful_audio_ms = results
                    .iter()
                    .find(|result| result.label == "response")
                    .map(|result| result.audio_finished_ms.saturating_sub(result.playback_ms));
                eprintln!(
                    "Latency: prompt_prep={} ms, first_token={} ms, model={} ms, first_response_audio={} ms, tts_synthesis={} ms, playback={} ms, end_to_end={} ms",
                    prompt_prep_ms,
                    first_token_ms.map_or_else(|| "n/a".into(), |ms| ms.to_string()),
                    model_ms,
                    meaningful_audio_ms.map_or_else(|| "n/a".into(), |ms| ms.to_string()),
                    synthesis_ms,
                    playback_ms,
                    turn_started.elapsed().as_millis(),
                );
            } else {
                eprintln!(
                    "Latency: prompt_prep={} ms, first_token={} ms, model={} ms, end_to_end={} ms (TTS disabled)",
                    prompt_prep_ms,
                    first_token_ms.map_or_else(|| "n/a".into(), |ms| ms.to_string()),
                    model_ms,
                    turn_started.elapsed().as_millis(),
                );
            }
            send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
        }
        Err(error) => {
            answer_audio_queued.store(true, Ordering::Release);
            if let Some(filler) = filler_worker {
                let _ = filler.join();
            }
            drop(speech_sender);
            if let Some(worker) = speech_worker {
                let _ = worker.join();
            }
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

fn remember_command(tools: &mut ToolRuntime, text: &str) {
    if !tools.writes_enabled {
        eprintln!("Memory writes are off. Use /memory-on before /remember.");
        return;
    }
    match tools.memory.remember(text, "explicit-user-command") {
        Ok(memory) => println!("Remembered #{}: {}", memory.id, memory.content),
        Err(error) => eprintln!("Memory error: {error:#}"),
    }
}

fn persistent_memory_context(tools: &ToolRuntime) -> String {
    let memories = match tools.memory.list_memories(10) {
        Ok(memories) => memories,
        Err(error) => {
            eprintln!("Memory warning: failed to load long-term memories: {error:#}");
            return String::new();
        }
    };
    if memories.is_empty() {
        return String::new();
    }
    let mut output =
        String::from("[Continuity knowledge — use naturally and do not expose this section]\n");
    for memory in memories {
        let line = format!("- {}\n", memory.content);
        if output.chars().count() + line.chars().count() > 2_000 {
            break;
        }
        output.push_str(&line);
    }
    output
}

fn list_memories_command(tools: &ToolRuntime) {
    match tools.memory.list_memories(20) {
        Ok(memories) if memories.is_empty() => println!("No saved memories."),
        Ok(memories) => {
            println!("Recent memories:");
            for memory in memories {
                println!("  #{} [{}] {}", memory.id, memory.source, memory.content);
            }
        }
        Err(error) => eprintln!("Memory error: {error:#}"),
    }
}

fn search_memories_command(tools: &ToolRuntime, query: &str) {
    match tools.memory.search_memories(query, 20) {
        Ok(memories) if memories.is_empty() => println!("No memories matched {query:?}."),
        Ok(memories) => {
            println!("Memory search results:");
            for memory in memories {
                println!("  #{} {}", memory.id, memory.content);
            }
        }
        Err(error) => eprintln!("Memory error: {error:#}"),
    }
}

fn forget_memory_command(tools: &ToolRuntime, id: &str) {
    let result = id
        .parse::<i64>()
        .map_err(anyhow::Error::from)
        .and_then(|id| tools.memory.forget_memory(id));
    match result {
        Ok(true) => println!("Forgot memory #{id}."),
        Ok(false) => eprintln!("Memory #{id} was not found or already deleted."),
        Err(error) => eprintln!("Memory error: {error:#}"),
    }
}

fn memory_status_command(tools: &ToolRuntime) {
    match tools.memory.counts(&tools.session_id) {
        Ok(counts) => {
            println!("Memory status:");
            println!("  database: {}", tools.memory.path().display());
            println!("  writes enabled: {}", tools.writes_enabled);
            println!(
                "  store session messages: {}",
                tools.config.store_session_messages
            );
            println!(
                "  store context items: {}",
                tools.config.store_context_items
            );
            println!("  session: {}", tools.session_id);
            println!("  memories: {}", counts.memories);
            println!("  session messages: {}", counts.messages);
            println!("  session context items: {}", counts.context_items);
            println!("  session tool invocations: {}", counts.tool_invocations);
        }
        Err(error) => eprintln!("Memory error: {error:#}"),
    }
}

fn clear_memory_session_command(tools: &mut ToolRuntime) {
    match tools.memory.clear_session(&tools.session_id) {
        Ok(()) => {
            tools.persisted_context_ids.clear();
            println!("Current session messages, context records, and tool audit cleared.");
        }
        Err(error) => eprintln!("Memory error: {error:#}"),
    }
}

fn persist_new_context(runtime: &RuntimeCore, tools: &mut ToolRuntime) {
    if !tools.writes_enabled || !tools.config.store_context_items {
        return;
    }
    for item in runtime.context.items() {
        if tools.persisted_context_ids.insert(item.id.clone()) {
            if let Err(error) = tools.memory.add_context_item(&tools.session_id, item) {
                tools.persisted_context_ids.remove(&item.id);
                eprintln!("Memory warning: failed to store context item: {error:#}");
            }
        }
    }
}

fn list_tools_command(tools: &ToolRuntime) {
    println!("Available local tools:");
    for tool in tools.registry.list() {
        println!(
            "  {} [{}{}] - {}",
            tool.name,
            tool.risk_level.as_str(),
            if tool.requires_confirmation {
                ", confirmation"
            } else {
                ""
            },
            tool.description
        );
    }
}

fn tool_info_command(tools: &ToolRuntime, name: &str) {
    match tools.registry.get(name) {
        Some(tool) => {
            println!("Tool {}:", tool.name);
            println!("  description: {}", tool.description);
            println!("  risk: {}", tool.risk_level.as_str());
            println!("  confirmation by default: {}", tool.requires_confirmation);
            println!("  local only: {}", tool.local_only);
            println!("  read only: {}", tool.read_only);
            println!("  input schema: {}", tool.input_schema_json);
        }
        None => eprintln!("Unknown tool {name:?}. Use /tools."),
    }
}

fn manual_tool_command(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    tools: &mut ToolRuntime,
    name: &str,
    arguments: &str,
) {
    if name.is_empty() || arguments.is_empty() {
        eprintln!("Usage: /tool <tool_name> <json_args>");
        return;
    }
    match serde_json::from_str::<Value>(arguments) {
        Ok(arguments) => {
            let _ = propose_tool(
                bridge,
                runtime,
                tools,
                ToolInvocation {
                    tool_name: name.into(),
                    arguments,
                    source: ToolSource::Manual,
                },
                None,
            );
        }
        Err(error) => eprintln!("Tool arguments must be valid JSON: {error}"),
    }
}

fn propose_tool(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    tools: &mut ToolRuntime,
    invocation: ToolInvocation,
    original_input: Option<String>,
) -> Option<(String, ContextItem)> {
    let Some(definition) = tools.registry.get(&invocation.tool_name).cloned() else {
        eprintln!("Unknown tool {:?}. Use /tools.", invocation.tool_name);
        return None;
    };
    if let Err(error) = tools
        .registry
        .validate(&invocation.tool_name, &invocation.arguments)
    {
        eprintln!("Tool arguments rejected: {error:#}");
        return None;
    }
    if invocation.tool_name == "memory.remember" && !tools.writes_enabled {
        eprintln!("Memory writes are off. Use /memory-on first.");
        return None;
    }
    let permission = permission_for(&definition, invocation.source);
    let status = if permission == ToolPermission::Execute {
        "approved"
    } else {
        "proposed"
    };
    let record = ToolInvocationRecord {
        session_id: tools.session_id.clone(),
        tool_name: invocation.tool_name.clone(),
        arguments_json: invocation.arguments.to_string(),
        source: match invocation.source {
            ToolSource::Manual => "manual",
            ToolSource::ModelSuggested => "model_suggested",
        }
        .into(),
        status: status.into(),
        risk_level: definition.risk_level.as_str().into(),
        requires_confirmation: permission == ToolPermission::Confirm,
    };
    let audit_id = match tools.memory.begin_tool_invocation(&record) {
        Ok(id) => id,
        Err(error) => {
            eprintln!("Tool audit error; execution blocked: {error:#}");
            return None;
        }
    };
    match permission {
        ToolPermission::Deny => {
            let _ = tools.memory.update_tool_invocation(
                audit_id,
                "denied",
                None,
                Some("tool is not local-only"),
            );
            eprintln!("Tool denied by local-only policy.");
            None
        }
        ToolPermission::Confirm => {
            send_state(
                bridge,
                runtime,
                CompanionState::Thinking,
                "Approval needed",
                None,
            );
            println!(
                "Proposed tool: {} {}",
                invocation.tool_name, invocation.arguments
            );
            println!(
                "Tool `{}` requests approval. Approve? y/N",
                invocation.tool_name
            );
            tools.pending = Some(PendingTool {
                invocation,
                audit_id,
                original_input,
            });
            None
        }
        ToolPermission::Execute => {
            execute_audited_tool(bridge, runtime, tools, invocation, audit_id, original_input)
        }
    }
}

fn execute_audited_tool(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    tools: &mut ToolRuntime,
    invocation: ToolInvocation,
    audit_id: i64,
    original_input: Option<String>,
) -> Option<(String, ContextItem)> {
    send_state(
        bridge,
        runtime,
        CompanionState::Thinking,
        "Running tool...",
        None,
    );
    let environment = ToolEnvironment {
        context: &runtime.context,
        memory: &tools.memory,
        active_window: &SystemActiveWindowProvider,
        clipboard: &SystemClipboardProvider,
    };
    match execute_tool(&invocation, &environment) {
        Ok(output) => {
            let result_json = output.result.to_string();
            if let Err(error) =
                tools
                    .memory
                    .update_tool_invocation(audit_id, "executed", Some(&result_json), None)
            {
                eprintln!("Tool audit completion warning: {error:#}");
            }
            println!("Tool result: {}", leading_preview(&result_json, 1_000));
            if let Some((state, caption)) = output.face_state {
                send_event(
                    bridge,
                    runtime,
                    RuntimeToFaceEvent::State {
                        state,
                        emotion: None,
                        caption: Some(caption),
                        audio_level: None,
                    },
                );
            } else {
                send_state(
                    bridge,
                    runtime,
                    CompanionState::Happy,
                    "Tool complete",
                    None,
                );
                thread::sleep(Duration::from_millis(250));
                send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
            }
            original_input.map(|input| {
                let context = ContextItem::new(
                    format!("tool-result-{audit_id}"),
                    ContextKind::AttachedText,
                    format!("Tool result: {}", invocation.tool_name),
                    result_json,
                    "tool kernel",
                );
                (input, context)
            })
        }
        Err(error) => {
            let message = format!("{error:#}");
            let _ = tools
                .memory
                .update_tool_invocation(audit_id, "failed", None, Some(&message));
            eprintln!("Tool failed: {message}");
            send_state(bridge, runtime, CompanionState::Error, "Tool failed", None);
            thread::sleep(Duration::from_millis(500));
            send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
            None
        }
    }
}

fn handle_tool_confirmation(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    provider: &dyn ModelProvider,
    system_prompt: &str,
    speech: &mut SpeechServices,
    tools: &mut ToolRuntime,
    answer: &str,
) {
    let pending = tools.pending.take().expect("pending tool was checked");
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        let _ = tools
            .memory
            .update_tool_invocation(pending.audit_id, "denied", None, None);
        println!("Tool denied.");
        send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
        return;
    }
    if let Err(error) =
        tools
            .memory
            .update_tool_invocation(pending.audit_id, "approved", None, None)
    {
        eprintln!("Tool audit error; execution blocked: {error:#}");
        return;
    }
    if let Some((original_input, tool_context)) = execute_audited_tool(
        bridge,
        runtime,
        tools,
        pending.invocation,
        pending.audit_id,
        pending.original_input,
    ) {
        run_prompt(
            bridge,
            runtime,
            provider,
            system_prompt,
            speech,
            tools,
            original_input,
            Some(tool_context),
            false,
        );
    }
}

fn tool_history_command(tools: &ToolRuntime, limit: usize) {
    match tools
        .memory
        .recent_tool_invocations(&tools.session_id, limit)
    {
        Ok(entries) if entries.is_empty() => println!("No tool invocations in this session."),
        Ok(entries) => {
            println!("Recent tool invocations:");
            for entry in entries {
                println!(
                    "  #{} {} [{} / {} / {}] {}",
                    entry.id,
                    entry.tool_name,
                    entry.source,
                    entry.risk_level,
                    entry.status,
                    entry.arguments_json
                );
            }
        }
        Err(error) => eprintln!("Tool history error: {error:#}"),
    }
}

fn clear_tool_session_command(tools: &ToolRuntime) {
    match tools.memory.clear_tool_session(&tools.session_id) {
        Ok(count) => println!("Cleared {count} tool audit entries for this session."),
        Err(error) => eprintln!("Tool history error: {error:#}"),
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

#[allow(clippy::too_many_arguments)]
fn ask_selection(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    model: &dyn ModelProvider,
    system_prompt: &str,
    speech: &mut SpeechServices,
    tools: &mut ToolRuntime,
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
                tools,
                question,
                temporary,
                true,
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
    tools: &mut ToolRuntime,
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
        Ok(transcript) if is_non_speech_transcript(&transcript) => {
            println!("Ignored non-speech transcript: {transcript}");
            send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
        }
        Ok(transcript) => {
            println!("Transcript: {transcript}");
            run_prompt(
                bridge,
                runtime,
                model,
                system_prompt,
                speech,
                tools,
                transcript,
                None,
                true,
            );
        }
        Err(error) => speech_failure(bridge, runtime, speech, error),
    }
}

fn auto_listen_and_ask(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    model: &dyn ModelProvider,
    system_prompt: &str,
    speech: &mut SpeechServices,
    tools: &mut ToolRuntime,
) {
    send_state(
        bridge,
        runtime,
        CompanionState::Listening,
        "Listening for speech...",
        None,
    );
    let wav_path = temporary_audio_path("auto-listen");
    let result = speech
        .audio
        .capture_until_pause(20, &wav_path)
        .and_then(|_| transcribe_path(bridge, runtime, speech, &wav_path));
    let _ = std::fs::remove_file(&wav_path);

    match result {
        Ok(transcript) if is_non_speech_transcript(&transcript) => {
            println!("Ignored non-speech transcript: {transcript}");
            send_state(bridge, runtime, CompanionState::Idle, "Ready", None);
        }
        Ok(transcript) => {
            println!("Transcript: {transcript}");
            run_prompt(
                bridge,
                runtime,
                model,
                system_prompt,
                speech,
                tools,
                transcript,
                None,
                true,
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
    let spoken = spoken_response(text, 500);
    match speech.tts.speak_profiled(&spoken) {
        Ok(profile) => {
            eprintln!(
                "Latency: tts_synthesis={} ms, playback={} ms, tts_total={} ms",
                profile.synthesis_ms, profile.playback_ms, profile.total_ms
            );
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

fn leading_preview(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

fn is_non_speech_transcript(text: &str) -> bool {
    let text = text.trim();
    text.is_empty()
        || (text.starts_with('[') && text.ends_with(']'))
        || (text.starts_with('(') && text.ends_with(')'))
}

fn explicit_memory_content(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let lowercase = trimmed.to_ascii_lowercase();
    let markers = [
        "i would like you to remember ",
        "i'd like you to remember ",
        "i want you to remember ",
        "can you please remember ",
        "could you please remember ",
        "can you remember ",
        "could you remember ",
        "would you remember ",
        "please remember ",
        "to remember ",
        "remember that ",
        "remember ",
    ];
    markers.into_iter().find_map(|marker| {
        let index = lowercase.find(marker)?;
        if marker == "remember " && index != 0 {
            return None;
        }
        let content = trimmed.get(index + marker.len()..)?.trim();
        (!content.is_empty()).then(|| content.to_owned())
    })
}

fn natural_family_memory_content(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let lowercase = trimmed.to_ascii_lowercase();
    if let Some(index) = lowercase.rfind("do you remember") {
        let fact = trimmed[..index]
            .trim()
            .trim_start_matches(|character: char| character == ',' || character.is_whitespace())
            .trim_end_matches(|character: char| {
                character == '.' || character == ',' || character.is_whitespace()
            });
        let fact = fact
            .strip_prefix("But ")
            .or_else(|| fact.strip_prefix("but "))
            .unwrap_or(fact);
        if !fact.is_empty() {
            return Some(canonical_family_fact(&format!("{fact}.")));
        }
    }
    let starts_as_question = [
        "what ", "why ", "how ", "when ", "where ", "who ", "is ", "are ", "do ", "does ", "did ",
        "can ", "could ", "would ", "should ",
    ]
    .into_iter()
    .any(|prefix| lowercase.starts_with(prefix));
    if starts_as_question || trimmed.ends_with('?') {
        return None;
    }
    let family_fact = [
        "daughter's name is ",
        "son's name is ",
        " has a sister ",
        " has a brother ",
        " has one daughter",
        " has two daughters",
        " has one son",
        " has two sons",
    ]
    .into_iter()
    .any(|marker| lowercase.contains(marker));
    family_fact.then(|| canonical_family_fact(trimmed))
}

fn canonical_family_fact(fact: &str) -> String {
    let lowercase = fact.to_ascii_lowercase();
    for (marker, prefix) in [
        (
            "second daughter's name is ",
            "User has at least two daughters. User's second daughter's name is ",
        ),
        ("my daughter's name is ", "User's daughter's name is "),
        ("my son's name is ", "User's son's name is "),
    ] {
        if let Some(index) = lowercase.find(marker) {
            let raw_name = fact[index + marker.len()..]
                .trim()
                .trim_end_matches(['.', '?', '!']);
            let raw_name_lowercase = raw_name.to_ascii_lowercase();
            if let Some(spelled_index) = raw_name_lowercase.find(" spelled ") {
                let spelling = raw_name[spelled_index + " spelled ".len()..].trim();
                let letters = spelling
                    .chars()
                    .filter(char::is_ascii_alphabetic)
                    .collect::<String>();
                if !letters.is_empty() {
                    let mut characters = letters.to_ascii_lowercase().chars().collect::<Vec<_>>();
                    characters[0] = characters[0].to_ascii_uppercase();
                    let normalized_name = characters.into_iter().collect::<String>();
                    return format!("{prefix}{normalized_name} (spelled {spelling}).");
                }
            }
            return format!("{prefix}{raw_name}.");
        }
    }
    fact.to_owned()
}

fn claims_memory_write(response: &str) -> bool {
    let response = response.trim().to_ascii_lowercase();
    response.starts_with("remembered")
        || response.contains("i have remembered")
        || response.contains("i've remembered")
        || response.contains("i will now remember")
        || response.contains("i have updated my memory")
        || response.contains("i've updated my memory")
        || response.contains("i have updated the memory")
        || response.contains("i've updated the memory")
        || response.contains("updated the explicit user memories")
        || response.contains("registered the explicit memories")
        || response.contains("processed the explicit memories")
        || response.contains("saved to persistent memory")
}

fn should_plan_tool(input: &str) -> bool {
    let input = input.trim().to_ascii_lowercase();
    if input.is_empty()
        || input.contains("can you do tool")
        || input.contains("what tools")
        || input.contains("ask you to store")
        || input.contains("going to ask you")
    {
        return false;
    }
    input == "time"
        || input.contains("what time")
        || input.contains("current time")
        || input.contains("time now")
        || input.contains("clipboard")
        || input.contains("active window")
        || input.contains("window metadata")
        || input.contains("search memory")
        || input.contains("find in memory")
        || input.contains("list memories")
        || input.contains("show memories")
        || input.contains("what do you remember")
        || input.contains("words in your memory")
        || (input.contains("tell me") && input.contains("memory"))
        || input.contains("my name is ")
        || input.contains("my name as ")
        || input.contains("remember my name")
        || input.contains("store my name")
        || input.contains("call me ")
        || input.contains("forget my name")
        || input.contains("forget memory")
        || input.contains("delete memory")
        || explicit_memory_content(&input).is_some()
        || natural_family_memory_content(&input).is_some()
        || ((input.contains("store ") || input.contains("save ")) && input.contains("memory"))
        || input.contains("list context")
        || input.contains("show context")
        || input.contains("read file")
        || input.contains("open file")
        || input.contains("set face")
}

fn is_tool_capability_question(input: &str) -> bool {
    let input = input.to_ascii_lowercase();
    input.contains("what tools")
        || input.contains("which tools")
        || input.contains("tools do you")
        || input.contains("access to tools")
        || input.contains("tool access")
}

fn direct_tool_invocation(input: &str) -> Option<ToolInvocation> {
    let trimmed = input.trim();
    let lowercase = trimmed.to_ascii_lowercase();
    let name_value = ["my name is ", "my name as ", "call me "]
        .into_iter()
        .find_map(|marker| {
            lowercase.find(marker).and_then(|index| {
                let value = trimmed[index + marker.len()..]
                    .trim()
                    .trim_end_matches(['.', '?', '!', ',']);
                (!value.is_empty()).then_some(value)
            })
        });
    let wants_name_deletion = lowercase.contains("forget my name")
        || lowercase.contains("delete my name")
        || lowercase.contains("erase my name")
        || (lowercase.contains("remove my name") && !lowercase.contains("remove our my name"));
    let (tool_name, arguments) = if let Some(name) = name_value {
        if wants_name_deletion {
            ("memory.forget", serde_json::json!({"query":name}))
        } else {
            (
                "memory.remember",
                serde_json::json!({"content":format!("User's name is {name}.")}),
            )
        }
    } else if wants_name_deletion {
        (
            "memory.forget",
            serde_json::json!({"query":"User's name is"}),
        )
    } else if lowercase == "time"
        || lowercase.contains("what time")
        || lowercase.contains("current time")
        || lowercase.contains("time now")
    {
        ("time.now", serde_json::json!({}))
    } else if lowercase.contains("clipboard") {
        ("clipboard.read", serde_json::json!({}))
    } else if lowercase.contains("active window") || lowercase.contains("window metadata") {
        ("active_window.get", serde_json::json!({}))
    } else if lowercase.contains("list memories")
        || lowercase.contains("show memories")
        || lowercase.contains("what do you remember")
        || lowercase.contains("words in your memory")
        || (lowercase.contains("tell me") && lowercase.contains("memory"))
    {
        ("memory.list", serde_json::json!({}))
    } else if let Some(index) = lowercase.find("search memory for ") {
        let query = trimmed[index + "search memory for ".len()..]
            .trim()
            .trim_end_matches(['.', '?']);
        if query.is_empty() {
            return None;
        }
        ("memory.search", serde_json::json!({"query":query}))
    } else if let Some(index) = lowercase.find("find in memory ") {
        let query = trimmed[index + "find in memory ".len()..]
            .trim()
            .trim_end_matches(['.', '?']);
        if query.is_empty() {
            return None;
        }
        ("memory.search", serde_json::json!({"query":query}))
    } else if let Some(content) =
        explicit_memory_content(trimmed).or_else(|| natural_family_memory_content(trimmed))
    {
        ("memory.remember", serde_json::json!({"content":content}))
    } else if let Some(index) = lowercase.find("forget memory about ") {
        let query = trimmed[index + "forget memory about ".len()..]
            .trim()
            .trim_end_matches(['.', '?']);
        if query.is_empty() {
            return None;
        }
        ("memory.forget", serde_json::json!({"query":query}))
    } else {
        return None;
    };
    Some(ToolInvocation {
        tool_name: tool_name.into(),
        arguments,
        source: ToolSource::ModelSuggested,
    })
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
            println!(
                "Double-click toggles visual state only; use /listen or /auto-listen for voice."
            );
        }
        FaceToRuntimeMessage::Dragged { x, y } => println!("<- face.dragged x={x} y={y}"),
        FaceToRuntimeMessage::Action { action } => {
            println!("<- face.action action={action:?}");
            if action == "toggle_listening" {
                println!(
                    "This action toggles visual state only; use /listen or /auto-listen for voice."
                );
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
    use super::{
        build_speech_services, claims_memory_write, compact_caption, direct_tool_invocation,
        extended_waiting_filler, is_non_speech_transcript, is_tool_capability_question,
        latency_filler, leading_preview, persistent_memory_context, quick_capture_status,
        remember_command, should_plan_tool, should_use_latency_fillers, speech_status_text,
        take_complete_speech_segments, wait_until_or_answer_queued, waiting_filler, ToolRuntime,
    };
    use orbital_face_host::memory::{MemoryConfig, MemoryStore, SqliteMemoryStore};
    use orbital_face_host::runtime_v0::RuntimeCore;
    use orbital_face_host::speech::SpeechOptions;
    use orbital_face_host::tools::ToolRegistry;
    use std::collections::HashSet;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    #[test]
    fn compact_caption_keeps_recent_text() {
        assert_eq!(compact_caption("one two three four", 8), "ree four");
        assert_eq!(leading_preview("abcdef", 3), "abc...");
    }

    #[test]
    fn streamed_speech_waits_for_complete_meaningful_segments() {
        let mut buffer = "Short intro. This sentence is still arriving".to_string();
        assert!(take_complete_speech_segments(&mut buffer).is_empty());

        buffer.push_str(" and is now complete.");
        assert_eq!(
            take_complete_speech_segments(&mut buffer),
            vec!["Short intro. This sentence is still arriving and is now complete."]
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn latency_fillers_rotate_and_match_the_utterance_type() {
        assert_eq!(
            latency_filler("How does this work?", 0),
            "Let me think about that."
        );
        assert_eq!(latency_filler("How does this work?", 1), "Let's see.");
        assert_eq!(
            latency_filler("Please explain this", 2),
            "Let me consider that."
        );
        assert_eq!(
            latency_filler("Today was difficult", 3),
            "I'm considering what you said."
        );
        assert_ne!(waiting_filler(0), waiting_filler(1));
        assert_ne!(extended_waiting_filler(0), extended_waiting_filler(1));
    }

    #[test]
    fn trivial_turns_do_not_trigger_latency_fillers() {
        for input in ["Hi", "okay", "thanks", "yes", "Why?"] {
            assert!(
                !should_use_latency_fillers(input),
                "unexpected filler for {input}"
            );
        }
        assert!(should_use_latency_fillers(
            "How can I reduce response latency?"
        ));
    }

    #[test]
    fn filler_schedule_stops_when_response_audio_is_queued() {
        let queued = AtomicBool::new(true);
        assert!(!wait_until_or_answer_queued(
            Instant::now(),
            Duration::from_secs(1),
            &queued
        ));
    }

    #[test]
    fn whisper_non_speech_markers_are_not_prompts() {
        assert!(is_non_speech_transcript("[BLANK_AUDIO]"));
        assert!(is_non_speech_transcript("(whooshing)"));
        assert!(is_non_speech_transcript("(singing in foreign language)"));
        assert!(!is_non_speech_transcript("What time is it?"));
    }

    #[test]
    fn tool_planning_runs_only_for_actionable_requests() {
        assert!(should_plan_tool("What is the time now?"));
        assert!(should_plan_tool("Read from the clipboard"));
        assert!(should_plan_tool("Remember heliotrope"));
        assert!(should_plan_tool("I want you to remember the word cat."));
        assert!(should_plan_tool(
            "I would like you to remember the word car."
        ));
        assert!(should_plan_tool(
            "Okay, can you remember the word tat and call?"
        ));
        assert!(should_plan_tool(
            "Okay, second daughter's name is TestChildB spelled T-E-S-T-C-H-I-L-D-B."
        ));
        assert!(should_plan_tool(
            "But the user has two daughters. Do you remember?"
        ));
        assert!(should_plan_tool("Search memory for Orbital"));
        assert!(should_plan_tool("What do you remember about me?"));
        assert!(should_plan_tool("Now tell me the words in your memory."));
        assert!(!should_plan_tool("Can you do tool calling?"));
        assert!(!should_plan_tool(
            "Return the word that I ask you to store in the memory"
        ));
        assert!(!should_plan_tool(
            "Why don't you remember the cat in the car?"
        ));
        assert!(!should_plan_tool(
            "How many daughters do I have and what are their names?"
        ));
    }

    #[test]
    fn explicit_natural_language_tools_are_routed_deterministically() {
        let remember =
            direct_tool_invocation("Remember that the second verification word is cerulean.")
                .unwrap();
        assert_eq!(remember.tool_name, "memory.remember");
        assert_eq!(
            remember.arguments["content"],
            "the second verification word is cerulean."
        );
        assert_eq!(
            direct_tool_invocation("What is the time now?")
                .unwrap()
                .tool_name,
            "time.now"
        );
        assert_eq!(
            direct_tool_invocation("Read the clipboard")
                .unwrap()
                .tool_name,
            "clipboard.read"
        );
        let noisy_name =
            direct_tool_invocation("I want you to remove our my name, my name is TestUser.")
                .unwrap();
        assert_eq!(noisy_name.tool_name, "memory.remember");
        assert_eq!(noisy_name.arguments["content"], "User's name is TestUser.");
        let remember_name =
            direct_tool_invocation("I want you to remember my name as TestUser.").unwrap();
        assert_eq!(remember_name.tool_name, "memory.remember");
        assert_eq!(
            remember_name.arguments["content"],
            "User's name is TestUser."
        );
        for (request, expected) in [
            ("I want you to remember the word cat.", "the word cat."),
            (
                "Okay, can you remember the word tat and call?",
                "the word tat and call?",
            ),
            (
                "I would like you to remember the word car.",
                "the word car.",
            ),
            (
                "I would like a cue to remember the word called.",
                "the word called.",
            ),
        ] {
            let invocation = direct_tool_invocation(request).unwrap();
            assert_eq!(invocation.tool_name, "memory.remember");
            assert_eq!(invocation.arguments["content"], expected);
        }
        assert_eq!(
            direct_tool_invocation("Now tell me the words in your memory.")
                .unwrap()
                .tool_name,
            "memory.list"
        );
        for (request, expected) in [
            (
                "My daughter's name is TestChildA.",
                "User's daughter's name is TestChildA.",
            ),
            (
                "Okay, second daughter's name is TestChildB spelled T-E-S-T-C-H-I-L-D-B.",
                "User has at least two daughters. User's second daughter's name is Testchildb (spelled T-E-S-T-C-H-I-L-D-B).",
            ),
            (
                "But the user has two daughters. Do you remember?",
                "the user has two daughters.",
            ),
            (
                "TestChildA, TestUser's daughter, has a sister named TestChildB.",
                "TestChildA, TestUser's daughter, has a sister named TestChildB.",
            ),
        ] {
            let invocation = direct_tool_invocation(request).unwrap();
            assert_eq!(invocation.tool_name, "memory.remember");
            assert_eq!(invocation.arguments["content"], expected);
        }
        assert!(
            direct_tool_invocation("How many daughters do I have and what are their names?")
                .is_none()
        );
        assert!(direct_tool_invocation("Why don't you remember the cat in the car?").is_none());
        let forget_name = direct_tool_invocation("Forget my name, my name is TestUser.").unwrap();
        assert_eq!(forget_name.tool_name, "memory.forget");
        assert_eq!(forget_name.arguments["query"], "TestUser");
        assert!(is_tool_capability_question(
            "What tools do you have available?"
        ));
        assert!(!is_tool_capability_question("What are you?"));
    }

    #[test]
    fn detects_false_memory_write_claims() {
        assert!(claims_memory_write(
            "I have remembered the word cat and the word call."
        ));
        assert!(claims_memory_write(
            "I have updated my memory: TestChildA has a sibling named TestChildB."
        ));
        assert!(claims_memory_write("Remembered. I will keep that in mind."));
        assert!(claims_memory_write(
            "I have now registered the explicit memories."
        ));
        assert!(!claims_memory_write(
            "I can use that correction in this conversation."
        ));
        assert!(!claims_memory_write(
            "Your saved memories include TestUser."
        ));
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

    #[test]
    fn memory_off_blocks_explicit_memory_writes() {
        let memory = SqliteMemoryStore::open(":memory:").unwrap();
        let session_id = memory.start_session("mock", "test").unwrap();
        let mut tools = ToolRuntime {
            memory,
            config: MemoryConfig::default(),
            writes_enabled: false,
            session_id,
            registry: ToolRegistry::with_builtins(),
            pending: None,
            persisted_context_ids: HashSet::new(),
            enable_model_tools: false,
        };
        remember_command(&mut tools, "should not be stored");
        assert_eq!(tools.memory.counts(&tools.session_id).unwrap().memories, 0);
    }

    #[test]
    fn continuity_knowledge_is_available_without_storage_metadata() {
        let memory = SqliteMemoryStore::open(":memory:").unwrap();
        let old_session = memory.start_session("mock", "test").unwrap();
        memory
            .remember("User's project codename is Orbital", "test")
            .unwrap();
        memory.end_session(&old_session).unwrap();
        let session_id = memory.start_session("mock", "test").unwrap();
        let tools = ToolRuntime {
            memory,
            config: MemoryConfig::default(),
            writes_enabled: true,
            session_id,
            registry: ToolRegistry::with_builtins(),
            pending: None,
            persisted_context_ids: HashSet::new(),
            enable_model_tools: false,
        };
        let context = persistent_memory_context(&tools);
        assert!(context.contains("project codename is Orbital"));
        assert!(context.contains("Continuity knowledge"));
        assert!(!context.contains("Explicit User Memories"));
        assert!(!context.contains("#1"));
    }
}
