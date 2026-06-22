use serde_json::{json, Value};
use std::io;
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};
use tungstenite::{accept, Error as WebSocketError, Message, WebSocket};

const ADDRESS: &str = "127.0.0.1:7373";
const STEP_DELAY: Duration = Duration::from_millis(1500);

fn main() -> anyhow::Result<()> {
    let listener = TcpListener::bind(ADDRESS)?;
    println!("Orbital runtime mock listening on ws://{ADDRESS}");

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                println!("Face host connected");
                if let Err(error) = serve_connection(stream) {
                    eprintln!("Face host disconnected: {error}");
                }
                println!("Waiting for face host to reconnect...");
            }
            Err(error) => eprintln!("Bridge accept failed: {error}"),
        }
    }
    Ok(())
}

fn serve_connection(stream: TcpStream) -> anyhow::Result<()> {
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    stream.set_write_timeout(Some(Duration::from_secs(1)))?;
    let mut socket = accept(stream)?;
    let script = scripted_events();
    let mut next_event = 0;
    let mut next_send = Instant::now();
    let mut ping_sent = false;

    loop {
        if next_event < script.len() && Instant::now() >= next_send {
            send_json(&mut socket, &script[next_event])?;
            next_event += 1;
            next_send = Instant::now() + STEP_DELAY;
        } else if next_event == script.len() && !ping_sent && Instant::now() >= next_send {
            send_json(&mut socket, &json!({"type":"ping","id":"mock-bridge-v0"}))?;
            ping_sent = true;
        }

        match socket.read() {
            Ok(Message::Text(text)) => match serde_json::from_str::<Value>(&text) {
                Ok(value) => println!("<- {}", serde_json::to_string(&value)?),
                Err(error) => eprintln!("<- invalid JSON ({error}): {text}"),
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

fn send_json(socket: &mut WebSocket<TcpStream>, value: &Value) -> anyhow::Result<()> {
    let text = serde_json::to_string(value)?;
    println!("-> {text}");
    socket.send(Message::Text(text.into()))?;
    Ok(())
}

fn scripted_events() -> Vec<Value> {
    vec![
        json!({"type":"state","state":"idle","caption":"Ready"}),
        json!({"type":"state","state":"listening","caption":"Listening..."}),
        json!({"type":"state","state":"thinking","caption":"Thinking..."}),
        json!({
            "type":"state",
            "state":"speaking",
            "caption":"Here is what I found",
            "audio_level":0.7
        }),
        json!({"type":"state","state":"happy","caption":"Done"}),
        json!({"type":"state","state":"idle","caption":"Ready"}),
    ]
}
