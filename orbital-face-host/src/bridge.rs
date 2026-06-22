use crate::events::{parse_runtime_message, RuntimeMessage};
use serde::Serialize;
use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;
use tungstenite::{client, Error as WebSocketError, Message, WebSocket};

const RETRY_DELAY: Duration = Duration::from_secs(2);
const IO_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub enum BridgeUpdate {
    Connected,
    Disconnected,
    Received(RuntimeMessage),
    Sent(&'static str),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum FaceToRuntimeEvent {
    #[serde(rename = "face.ready")]
    Ready { face: String, version: String },
    #[serde(rename = "face.clicked")]
    Clicked { x: f32, y: f32, button: String },
    #[serde(rename = "face.double_clicked")]
    DoubleClicked { x: f32, y: f32, button: String },
    #[serde(rename = "face.dragged")]
    Dragged { x: i32, y: i32 },
    #[serde(rename = "face.action")]
    Action { action: String },
    #[serde(rename = "pong")]
    Pong { id: String },
}

impl FaceToRuntimeEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Ready { .. } => "face.ready",
            Self::Clicked { .. } => "face.clicked",
            Self::DoubleClicked { .. } => "face.double_clicked",
            Self::Dragged { .. } => "face.dragged",
            Self::Action { .. } => "face.action",
            Self::Pong { .. } => "pong",
        }
    }
}

pub struct BridgeHandle {
    updates: Receiver<BridgeUpdate>,
    outgoing: Sender<FaceToRuntimeEvent>,
}

impl BridgeHandle {
    pub fn connect(url: String, ready: FaceToRuntimeEvent) -> Self {
        let (update_tx, updates) = mpsc::channel();
        let (outgoing, outgoing_rx) = mpsc::channel();
        thread::spawn(move || run_client(url, ready, outgoing_rx, update_tx));
        Self { updates, outgoing }
    }

    pub fn try_recv(&self) -> Result<BridgeUpdate, TryRecvError> {
        self.updates.try_recv()
    }

    pub fn send(&self, event: FaceToRuntimeEvent) {
        let _ = self.outgoing.send(event);
    }
}

fn run_client(
    url: String,
    ready: FaceToRuntimeEvent,
    outgoing: Receiver<FaceToRuntimeEvent>,
    updates: Sender<BridgeUpdate>,
) {
    loop {
        match connect_local(&url) {
            Ok(mut socket) => {
                eprintln!("bridge connected: {url}");
                let _ = updates.send(BridgeUpdate::Connected);
                if write_event(&mut socket, &ready, &updates).is_err()
                    || run_connection(&mut socket, &outgoing, &updates).is_err()
                {
                    eprintln!("bridge disconnected: {url}");
                }
                let _ = updates.send(BridgeUpdate::Disconnected);
            }
            Err(error) => {
                eprintln!("bridge connection failed ({url}): {error}");
                let _ = updates.send(BridgeUpdate::Disconnected);
            }
        }
        thread::sleep(RETRY_DELAY);
    }
}

fn connect_local(url: &str) -> anyhow::Result<WebSocket<TcpStream>> {
    let address = url
        .strip_prefix("ws://")
        .ok_or_else(|| anyhow::anyhow!("only unencrypted ws:// bridge URLs are supported"))?
        .split('/')
        .next()
        .filter(|address| !address.is_empty())
        .ok_or_else(|| anyhow::anyhow!("bridge URL is missing host and port"))?;
    let socket_address = address
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow::anyhow!("bridge address did not resolve"))?;
    let stream = TcpStream::connect_timeout(&socket_address, Duration::from_secs(1))?;
    stream.set_read_timeout(Some(IO_POLL_INTERVAL))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let (socket, _) = client(url, stream)?;
    Ok(socket)
}

fn run_connection(
    socket: &mut WebSocket<TcpStream>,
    outgoing: &Receiver<FaceToRuntimeEvent>,
    updates: &Sender<BridgeUpdate>,
) -> anyhow::Result<()> {
    loop {
        while let Ok(event) = outgoing.try_recv() {
            write_event(socket, &event, updates)?;
        }

        match socket.read() {
            Ok(Message::Text(text)) => match parse_runtime_message(&text) {
                Ok(message) => {
                    let pong = match &message {
                        RuntimeMessage::Ping { id } => {
                            Some(FaceToRuntimeEvent::Pong { id: id.clone() })
                        }
                        _ => None,
                    };
                    let _ = updates.send(BridgeUpdate::Received(message));
                    if let Some(pong) = pong {
                        write_event(socket, &pong, updates)?;
                    }
                }
                Err(error) => eprintln!("warning: ignored invalid bridge message: {error}"),
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

fn write_event(
    socket: &mut WebSocket<TcpStream>,
    event: &FaceToRuntimeEvent,
    updates: &Sender<BridgeUpdate>,
) -> anyhow::Result<()> {
    socket.send(Message::Text(serde_json::to_string(event)?.into()))?;
    let _ = updates.send(BridgeUpdate::Sent(event.event_type()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::FaceToRuntimeEvent;

    #[test]
    fn serializes_face_to_runtime_events() {
        let event = FaceToRuntimeEvent::Clicked {
            x: 120.0,
            y: 96.0,
            button: "left".into(),
        };
        let json = serde_json::to_value(event).unwrap();
        assert_eq!(json["type"], "face.clicked");
        assert_eq!(json["x"], 120.0);
        assert_eq!(json["button"], "left");
    }
}
