use serde::Deserialize;
use std::io::{self, BufRead};
use std::sync::mpsc::{self, Receiver};
use std::thread;

#[derive(Debug, Copy, Clone, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceState {
    Idle,
    Listening,
    Thinking,
    Speaking,
}

impl FaceState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Listening => "listening",
            Self::Thinking => "thinking",
            Self::Speaking => "speaking",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum StdinEvent {
    State { state: FaceState },
    AlwaysOnTop { always_on_top: bool },
}

#[derive(Debug)]
pub enum FaceEvent {
    StateChanged(FaceState),
    AlwaysOnTopChanged(bool),
}

fn parse_stdin_event(line: &str) -> serde_json::Result<FaceEvent> {
    let event = serde_json::from_str::<StdinEvent>(line)?;
    Ok(match event {
        StdinEvent::State { state } => FaceEvent::StateChanged(state),
        StdinEvent::AlwaysOnTop { always_on_top } => FaceEvent::AlwaysOnTopChanged(always_on_top),
    })
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
    use super::{parse_stdin_event, FaceEvent, FaceState};

    #[test]
    fn parses_supported_state_event() {
        let event = parse_stdin_event(r#"{"state":"thinking"}"#).unwrap();

        assert!(matches!(
            event,
            FaceEvent::StateChanged(FaceState::Thinking)
        ));
    }

    #[test]
    fn rejects_unknown_state() {
        assert!(parse_stdin_event(r#"{"state":"sleeping"}"#).is_err());
    }

    #[test]
    fn parses_always_on_top_event() {
        let event = parse_stdin_event(r#"{"always_on_top":true}"#).unwrap();

        assert!(matches!(event, FaceEvent::AlwaysOnTopChanged(true)));
    }
}
