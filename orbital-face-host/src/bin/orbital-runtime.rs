use orbital_face_host::model_provider::{
    MockModelProvider, ModelChunk, ModelProvider, ModelRequest, OllamaModelProvider,
    RuntimeModelOptions,
};
use orbital_face_host::runtime_v0::{
    parse_command, parse_face_message, CompanionState, FaceToRuntimeMessage, RuntimeCommand,
    RuntimeCore, RuntimeToFaceEvent,
};
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
    let bridge = RuntimeBridge::start(ADDRESS)?;
    let terminal = spawn_terminal_reader();
    let mut runtime = RuntimeCore::default();

    println!("Orbital Runtime v0");
    println!("Bridge listening on {BRIDGE_URL}");
    print_provider_startup(provider.as_ref());
    println!("Type a message and press Enter.");
    println!(
        "Commands: /quit, /status, /model, /clear, /context, /clear-context, \
         /clipboard, /active-window, /watch, /unwatch, /attach-text, /attach-file, /demo, /ping"
    );

    loop {
        while let Some(update) = bridge.try_recv() {
            handle_bridge_update(&bridge, &mut runtime, update);
        }

        match terminal.try_recv() {
            Ok(line) => {
                if !handle_command(
                    &bridge,
                    &mut runtime,
                    provider.as_ref(),
                    &options.system_prompt,
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

fn print_help() {
    println!(
        "Usage: orbital-runtime [--model mock|ollama] \
         [--ollama-model <name>] [--ollama-base-url <url>] \
         [--system-prompt <text>]"
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

fn handle_command(
    bridge: &RuntimeBridge,
    runtime: &mut RuntimeCore,
    provider: &dyn ModelProvider,
    system_prompt: &str,
    command: RuntimeCommand,
) -> bool {
    match command {
        RuntimeCommand::Quit => return false,
        RuntimeCommand::Status => print_status(runtime, provider),
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
        RuntimeCommand::Prompt(input) => {
            run_prompt(bridge, runtime, provider, system_prompt, input)
        }
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
    input: String,
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

    let prompt_context = runtime.context.prompt_context();
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
            println!("  process: {}", window.process);
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
            println!(
                "Watching active-window metadata: {} ({})",
                window.title, window.process
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
        println!("  active process: {}", window.process);
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

fn print_status(runtime: &RuntimeCore, provider: &dyn ModelProvider) {
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
        println!("  active window: {} ({})", window.title, window.process);
    }
    println!(
        "  last model error: {}",
        runtime.last_model_error.as_deref().unwrap_or("-")
    );
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
    use super::compact_caption;

    #[test]
    fn compact_caption_keeps_recent_text() {
        assert_eq!(compact_caption("one two three four", 8), "ree four");
    }
}
