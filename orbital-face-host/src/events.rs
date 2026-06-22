use serde::Deserialize;
use serde_json::Value;
use std::io::{self, BufRead};
use std::sync::mpsc::{self, Receiver};
use std::thread;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct StateEvent {
    pub state: String,
    #[serde(default)]
    pub emotion: Option<String>,
    #[serde(default)]
    pub caption: Option<String>,
    #[serde(default)]
    pub audio_level: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct RawEvent {
    #[serde(rename = "type", default)]
    event_type: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    emotion: Option<String>,
    #[serde(default)]
    caption: Option<String>,
    #[serde(default)]
    audio_level: Option<f32>,
    #[serde(default)]
    always_on_top: Option<bool>,
    #[serde(default)]
    debug: Option<bool>,
    #[serde(default)]
    debug_overlay: Option<bool>,
    #[serde(default)]
    id: Option<String>,
    #[serde(flatten)]
    _unknown: std::collections::HashMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FaceEvent {
    State(StateEvent),
    Config {
        always_on_top: Option<bool>,
        debug_overlay: Option<bool>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeMessage {
    Event {
        event_type: &'static str,
        event: FaceEvent,
    },
    Ping {
        id: String,
    },
    Unknown {
        event_type: String,
    },
}

pub fn parse_runtime_message(line: &str) -> serde_json::Result<RuntimeMessage> {
    let event = serde_json::from_str::<RawEvent>(line)?;
    match event.event_type.as_deref() {
        Some("state") | None if event.state.is_some() => Ok(RuntimeMessage::Event {
            event_type: "state",
            event: FaceEvent::State(StateEvent {
                state: event.state.unwrap(),
                emotion: event.emotion,
                caption: event.caption,
                audio_level: event.audio_level,
            }),
        }),
        Some("config") => {
            if event.always_on_top.is_some() || event.debug_overlay.is_some() {
                Ok(RuntimeMessage::Event {
                    event_type: "config",
                    event: FaceEvent::Config {
                        always_on_top: event.always_on_top,
                        debug_overlay: event.debug_overlay,
                    },
                })
            } else {
                Err(invalid_message(
                    "config event requires always_on_top or debug_overlay",
                ))
            }
        }
        Some("ping") => Ok(RuntimeMessage::Ping {
            id: event
                .id
                .ok_or_else(|| invalid_message("ping event is missing id"))?,
        }),
        None if event.always_on_top.is_some() => Ok(RuntimeMessage::Event {
            event_type: "config",
            event: FaceEvent::Config {
                always_on_top: event.always_on_top,
                debug_overlay: None,
            },
        }),
        None if event.debug.is_some() => Ok(RuntimeMessage::Event {
            event_type: "config",
            event: FaceEvent::Config {
                always_on_top: None,
                debug_overlay: event.debug,
            },
        }),
        Some(event_type) => Ok(RuntimeMessage::Unknown {
            event_type: event_type.to_owned(),
        }),
        None => Err(invalid_message("message has no supported fields")),
    }
}

fn parse_stdin_event(line: &str) -> serde_json::Result<FaceEvent> {
    match parse_runtime_message(line)? {
        RuntimeMessage::Event { event, .. } => Ok(event),
        RuntimeMessage::Ping { .. } => {
            Err(invalid_message("ping is only supported in bridge mode"))
        }
        RuntimeMessage::Unknown { event_type } => Err(invalid_message(&format!(
            "unsupported event type {event_type:?}"
        ))),
    }
}

fn invalid_message(message: &str) -> serde_json::Error {
    serde_json::Error::io(io::Error::new(io::ErrorKind::InvalidInput, message))
}

pub fn spawn_stdin_reader() -> Receiver<FaceEvent> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else {
                break;
            };
            if line.trim().is_empty() {
                continue;
            }

            match parse_stdin_event(&line) {
                Ok(event) => {
                    if tx.send(event).is_err() {
                        break;
                    }
                }
                Err(err) => eprintln!("ignored invalid stdin event: {err}"),
            }
        }
    });

    rx
}

#[cfg(test)]
mod tests {
    use super::{parse_runtime_message, parse_stdin_event, FaceEvent, RuntimeMessage};

    #[test]
    fn parses_supported_state_event() {
        let event = parse_stdin_event(r#"{"state":"thinking"}"#).unwrap();

        assert!(matches!(
            event,
            FaceEvent::State(ref state) if state.state == "thinking"
        ));
    }

    #[test]
    fn parses_typed_state_event_and_ignores_unknown_fields() {
        let event = parse_stdin_event(
            r#"{"type":"state","state":"speaking","caption":"Hi","audio_level":0.8,"extra":true}"#,
        )
        .unwrap();

        assert!(matches!(
            event,
            FaceEvent::State(ref state)
                if state.state == "speaking"
                    && state.caption.as_deref() == Some("Hi")
                    && state.audio_level == Some(0.8)
        ));
    }

    #[test]
    fn unknown_state_is_parseable_for_manifest_validation() {
        let event = parse_stdin_event(r#"{"state":"custom"}"#).unwrap();
        assert!(matches!(
            event,
            FaceEvent::State(ref state) if state.state == "custom"
        ));
    }

    #[test]
    fn parses_always_on_top_event() {
        let event = parse_stdin_event(r#"{"always_on_top":true}"#).unwrap();

        assert!(matches!(
            event,
            FaceEvent::Config {
                always_on_top: Some(true),
                ..
            }
        ));
    }

    #[test]
    fn parses_bridge_config_event() {
        let message = parse_runtime_message(
            r#"{"type":"config","always_on_top":true,"debug_overlay":false}"#,
        )
        .unwrap();

        assert!(matches!(
            message,
            RuntimeMessage::Event {
                event: FaceEvent::Config {
                    always_on_top: Some(true),
                    debug_overlay: Some(false),
                },
                ..
            }
        ));
    }

    #[test]
    fn parses_ping_event() {
        let message = parse_runtime_message(r#"{"type":"ping","id":"abc123"}"#).unwrap();
        assert_eq!(
            message,
            RuntimeMessage::Ping {
                id: "abc123".into()
            }
        );
    }

    #[test]
    fn unknown_message_is_non_fatal() {
        let message = parse_runtime_message(r#"{"type":"future.event","value":1}"#).unwrap();
        assert_eq!(
            message,
            RuntimeMessage::Unknown {
                event_type: "future.event".into()
            }
        );
    }
}
