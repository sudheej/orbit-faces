use crate::context::ContextManager;
use crate::model_provider::ModelMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompanionState {
    Idle,
    Listening,
    Thinking,
    Speaking,
    Happy,
    Error,
    Disconnected,
}

impl CompanionState {
    pub fn protocol_name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Listening => "listening",
            Self::Thinking => "thinking",
            Self::Speaking => "speaking",
            Self::Happy => "happy",
            Self::Error => "error",
            Self::Disconnected => "disconnected",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCommand {
    Quit,
    Status,
    Demo,
    Ping,
    Model,
    Clear,
    Context,
    ClearContext,
    Clipboard,
    ActiveWindow,
    Watch,
    Unwatch,
    AttachText(String),
    AttachFile(String),
    Prompt(String),
    Empty,
    Unknown(String),
}

pub fn parse_command(line: &str) -> RuntimeCommand {
    let input = line.trim();
    if let Some(text) = input.strip_prefix("/attach-text ") {
        return RuntimeCommand::AttachText(text.trim().to_owned());
    }
    if let Some(path) = input.strip_prefix("/attach-file ") {
        return RuntimeCommand::AttachFile(path.trim().to_owned());
    }
    match input {
        "" => RuntimeCommand::Empty,
        "/quit" => RuntimeCommand::Quit,
        "/status" => RuntimeCommand::Status,
        "/demo" => RuntimeCommand::Demo,
        "/ping" => RuntimeCommand::Ping,
        "/model" => RuntimeCommand::Model,
        "/clear" => RuntimeCommand::Clear,
        "/context" => RuntimeCommand::Context,
        "/clear-context" => RuntimeCommand::ClearContext,
        "/clipboard" => RuntimeCommand::Clipboard,
        "/active-window" => RuntimeCommand::ActiveWindow,
        "/watch" => RuntimeCommand::Watch,
        "/unwatch" => RuntimeCommand::Unwatch,
        "/attach-text" => RuntimeCommand::AttachText(String::new()),
        "/attach-file" => RuntimeCommand::AttachFile(String::new()),
        command if command.starts_with('/') => RuntimeCommand::Unknown(command.to_owned()),
        prompt => RuntimeCommand::Prompt(prompt.to_owned()),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum RuntimeToFaceEvent {
    #[serde(rename = "state")]
    State {
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        emotion: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_level: Option<f32>,
    },
    #[serde(rename = "ping")]
    Ping { id: String },
    #[serde(rename = "pong")]
    Pong { id: String },
}

impl RuntimeToFaceEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::State { .. } => "state",
            Self::Ping { .. } => "ping",
            Self::Pong { .. } => "pong",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FaceToRuntimeMessage {
    Ready { face: String, version: String },
    Clicked { x: f32, y: f32, button: String },
    DoubleClicked { x: f32, y: f32, button: String },
    Dragged { x: i32, y: i32 },
    Action { action: String },
    Ping { id: String },
    Pong { id: String },
    Unknown { event_type: String },
}

#[derive(Debug, Deserialize)]
struct RawFaceMessage {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    face: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    x: Option<Value>,
    #[serde(default)]
    y: Option<Value>,
    #[serde(default)]
    button: Option<String>,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(flatten)]
    _unknown: std::collections::HashMap<String, Value>,
}

pub fn parse_face_message(text: &str) -> serde_json::Result<FaceToRuntimeMessage> {
    let raw = serde_json::from_str::<RawFaceMessage>(text)?;
    Ok(match raw.event_type.as_str() {
        "face.ready" => FaceToRuntimeMessage::Ready {
            face: required(raw.face, "face.ready is missing face")?,
            version: required(raw.version, "face.ready is missing version")?,
        },
        "face.clicked" => FaceToRuntimeMessage::Clicked {
            x: value_f32(raw.x, "face.clicked is missing x")?,
            y: value_f32(raw.y, "face.clicked is missing y")?,
            button: required(raw.button, "face.clicked is missing button")?,
        },
        "face.double_clicked" => FaceToRuntimeMessage::DoubleClicked {
            x: value_f32(raw.x, "face.double_clicked is missing x")?,
            y: value_f32(raw.y, "face.double_clicked is missing y")?,
            button: required(raw.button, "face.double_clicked is missing button")?,
        },
        "face.dragged" => FaceToRuntimeMessage::Dragged {
            x: value_i32(raw.x, "face.dragged is missing x")?,
            y: value_i32(raw.y, "face.dragged is missing y")?,
        },
        "face.action" => FaceToRuntimeMessage::Action {
            action: required(raw.action, "face.action is missing action")?,
        },
        "ping" => FaceToRuntimeMessage::Ping {
            id: required(raw.id, "ping is missing id")?,
        },
        "pong" => FaceToRuntimeMessage::Pong {
            id: required(raw.id, "pong is missing id")?,
        },
        _ => FaceToRuntimeMessage::Unknown {
            event_type: raw.event_type,
        },
    })
}

fn required<T>(value: Option<T>, message: &str) -> serde_json::Result<T> {
    value.ok_or_else(|| invalid_message(message))
}

fn value_f32(value: Option<Value>, message: &str) -> serde_json::Result<f32> {
    required(value.and_then(|value| value.as_f64()), message).map(|value| value as f32)
}

fn value_i32(value: Option<Value>, message: &str) -> serde_json::Result<i32> {
    required(value.and_then(|value| value.as_i64()), message).map(|value| value as i32)
}

fn invalid_message(message: &str) -> serde_json::Error {
    serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        message,
    ))
}

#[derive(Debug)]
pub struct RuntimeCore {
    pub state: CompanionState,
    pub face_connected: bool,
    pub face_name: Option<String>,
    pub face_version: Option<String>,
    pub last_user_input: Option<String>,
    pub last_response: Option<String>,
    pub last_bridge_event_received: Option<String>,
    pub last_bridge_event_sent: Option<String>,
    pub conversation: ConversationHistory,
    pub context: ContextManager,
    pub last_model_error: Option<String>,
    suppress_next_toggle_action: bool,
}

impl Default for RuntimeCore {
    fn default() -> Self {
        Self {
            state: CompanionState::Disconnected,
            face_connected: false,
            face_name: None,
            face_version: None,
            last_user_input: None,
            last_response: None,
            last_bridge_event_received: None,
            last_bridge_event_sent: None,
            conversation: ConversationHistory::new(6),
            context: ContextManager::default(),
            last_model_error: None,
            suppress_next_toggle_action: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConversationHistory {
    messages: Vec<ModelMessage>,
    max_exchanges: usize,
}

impl ConversationHistory {
    pub fn new(max_exchanges: usize) -> Self {
        Self {
            messages: Vec::new(),
            max_exchanges,
        }
    }

    pub fn messages(&self) -> &[ModelMessage] {
        &self.messages
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    pub fn add_exchange(&mut self, user: impl Into<String>, assistant: impl Into<String>) {
        self.messages.push(ModelMessage {
            role: "user".into(),
            content: user.into(),
        });
        self.messages.push(ModelMessage {
            role: "assistant".into(),
            content: assistant.into(),
        });
        let max_messages = self.max_exchanges * 2;
        if self.messages.len() > max_messages {
            self.messages.drain(0..self.messages.len() - max_messages);
        }
    }
}

impl RuntimeCore {
    pub fn bridge_connected(&mut self) {
        self.face_connected = true;
        self.state = CompanionState::Idle;
    }

    pub fn bridge_disconnected(&mut self) {
        self.face_connected = false;
        self.state = CompanionState::Disconnected;
        self.face_name = None;
        self.face_version = None;
    }

    pub fn state_event(
        &mut self,
        state: CompanionState,
        caption: impl Into<Option<String>>,
        audio_level: Option<f32>,
    ) -> RuntimeToFaceEvent {
        self.state = state;
        RuntimeToFaceEvent::State {
            state: state.protocol_name().into(),
            emotion: None,
            caption: caption.into(),
            audio_level,
        }
    }

    pub fn record_sent(&mut self, event_type: impl Into<String>) {
        self.last_bridge_event_sent = Some(event_type.into());
    }

    pub fn handle_face_message(
        &mut self,
        message: FaceToRuntimeMessage,
    ) -> Option<RuntimeToFaceEvent> {
        self.last_bridge_event_received = Some(match &message {
            FaceToRuntimeMessage::Unknown { event_type } => event_type.clone(),
            _ => face_event_type(&message).into(),
        });
        match message {
            FaceToRuntimeMessage::Ready { face, version } => {
                self.face_connected = true;
                self.face_name = Some(face);
                self.face_version = Some(version);
                Some(self.state_event(CompanionState::Idle, Some("Ready".into()), None))
            }
            FaceToRuntimeMessage::DoubleClicked { .. } => {
                self.suppress_next_toggle_action = true;
                Some(self.toggle_listening())
            }
            FaceToRuntimeMessage::Action { action } if action == "toggle_listening" => {
                if self.suppress_next_toggle_action {
                    self.suppress_next_toggle_action = false;
                    None
                } else {
                    Some(self.toggle_listening())
                }
            }
            FaceToRuntimeMessage::Ping { id } => Some(RuntimeToFaceEvent::Pong { id }),
            FaceToRuntimeMessage::Clicked { .. }
            | FaceToRuntimeMessage::Dragged { .. }
            | FaceToRuntimeMessage::Action { .. }
            | FaceToRuntimeMessage::Pong { .. }
            | FaceToRuntimeMessage::Unknown { .. } => None,
        }
    }

    fn toggle_listening(&mut self) -> RuntimeToFaceEvent {
        if self.state == CompanionState::Listening {
            self.state_event(CompanionState::Idle, Some("Ready".into()), None)
        } else {
            self.state_event(
                CompanionState::Listening,
                Some("Listening mode (voice not implemented)".into()),
                None,
            )
        }
    }
}

pub fn face_event_type(message: &FaceToRuntimeMessage) -> &'static str {
    match message {
        FaceToRuntimeMessage::Ready { .. } => "face.ready",
        FaceToRuntimeMessage::Clicked { .. } => "face.clicked",
        FaceToRuntimeMessage::DoubleClicked { .. } => "face.double_clicked",
        FaceToRuntimeMessage::Dragged { .. } => "face.dragged",
        FaceToRuntimeMessage::Action { .. } => "face.action",
        FaceToRuntimeMessage::Ping { .. } => "ping",
        FaceToRuntimeMessage::Pong { .. } => "pong",
        FaceToRuntimeMessage::Unknown { .. } => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_commands() {
        assert_eq!(parse_command("/status"), RuntimeCommand::Status);
        assert_eq!(
            parse_command("explain this error"),
            RuntimeCommand::Prompt("explain this error".into())
        );
        assert_eq!(
            parse_command("/missing"),
            RuntimeCommand::Unknown("/missing".into())
        );
    }

    #[test]
    fn face_ready_updates_runtime_and_generates_idle() {
        let mut runtime = RuntimeCore::default();
        let event = runtime
            .handle_face_message(FaceToRuntimeMessage::Ready {
                face: "Basic Orb".into(),
                version: "0.1".into(),
            })
            .unwrap();

        assert!(runtime.face_connected);
        assert_eq!(runtime.face_name.as_deref(), Some("Basic Orb"));
        assert_eq!(runtime.state, CompanionState::Idle);
        assert!(matches!(
            event,
            RuntimeToFaceEvent::State { ref state, .. } if state == "idle"
        ));
    }

    #[test]
    fn ping_generates_matching_pong() {
        let mut runtime = RuntimeCore::default();
        assert_eq!(
            runtime.handle_face_message(FaceToRuntimeMessage::Ping { id: "p1".into() }),
            Some(RuntimeToFaceEvent::Pong { id: "p1".into() })
        );
    }

    #[test]
    fn state_transitions_generate_protocol_events() {
        let mut runtime = RuntimeCore::default();
        let event = runtime.state_event(CompanionState::Thinking, Some("Thinking...".into()), None);
        assert_eq!(runtime.state, CompanionState::Thinking);
        assert_eq!(serde_json::to_value(event).unwrap()["state"], "thinking");
    }

    #[test]
    fn unknown_face_event_is_non_fatal() {
        let message = parse_face_message(r#"{"type":"face.future","value":1}"#).unwrap();
        assert_eq!(
            message,
            FaceToRuntimeMessage::Unknown {
                event_type: "face.future".into()
            }
        );
        assert!(RuntimeCore::default()
            .handle_face_message(message)
            .is_none());
    }

    #[test]
    fn disconnected_is_an_explicit_state() {
        let mut runtime = RuntimeCore::default();
        runtime.bridge_connected();
        runtime.bridge_disconnected();
        assert_eq!(runtime.state, CompanionState::Disconnected);
        assert!(!runtime.face_connected);
    }

    #[test]
    fn double_click_and_followup_action_toggle_only_once() {
        let mut runtime = RuntimeCore {
            state: CompanionState::Idle,
            ..RuntimeCore::default()
        };
        runtime.handle_face_message(FaceToRuntimeMessage::DoubleClicked {
            x: 10.0,
            y: 20.0,
            button: "left".into(),
        });
        assert_eq!(runtime.state, CompanionState::Listening);

        let duplicate = runtime.handle_face_message(FaceToRuntimeMessage::Action {
            action: "toggle_listening".into(),
        });
        assert!(duplicate.is_none());
        assert_eq!(runtime.state, CompanionState::Listening);
    }

    #[test]
    fn parses_model_and_clear_commands() {
        assert_eq!(parse_command("/model"), RuntimeCommand::Model);
        assert_eq!(parse_command("/clear"), RuntimeCommand::Clear);
    }

    #[test]
    fn parses_context_and_attachment_commands() {
        assert_eq!(parse_command("/context"), RuntimeCommand::Context);
        assert_eq!(
            parse_command("/attach-text Build failed with missing class"),
            RuntimeCommand::AttachText("Build failed with missing class".into())
        );
        assert_eq!(
            parse_command("/attach-file ./README.md"),
            RuntimeCommand::AttachFile("./README.md".into())
        );
        assert_eq!(
            parse_command("/clear-context"),
            RuntimeCommand::ClearContext
        );
    }

    #[test]
    fn conversation_history_trims_old_exchanges() {
        let mut history = ConversationHistory::new(2);
        history.add_exchange("u1", "a1");
        history.add_exchange("u2", "a2");
        history.add_exchange("u3", "a3");
        assert_eq!(history.message_count(), 4);
        assert_eq!(history.messages()[0].content, "u2");
        assert_eq!(history.messages()[3].content, "a3");
    }

    #[test]
    fn conversation_history_can_be_cleared() {
        let mut history = ConversationHistory::new(6);
        history.add_exchange("hello", "hi");
        history.clear();
        assert_eq!(history.message_count(), 0);
    }

    #[test]
    fn runtime_error_state_generation_is_short_and_explicit() {
        let mut runtime = RuntimeCore::default();
        let event = runtime.state_event(
            CompanionState::Error,
            Some("Model unavailable".into()),
            None,
        );
        assert_eq!(runtime.state, CompanionState::Error);
        assert!(matches!(
            event,
            RuntimeToFaceEvent::State { ref state, .. } if state == "error"
        ));
    }
}
