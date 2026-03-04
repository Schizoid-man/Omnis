/// WebSocket client mirroring engine/services/websocket.ts
///
/// - Connects to ws://{base_url}/chat/ws/{chat_id}?token=&device_id=
/// - Sends ping every 25 s
/// - Exponential back-off reconnect on disconnect
/// - Incoming frames forwarded to an mpsc channel consumed by the chat screen
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::{
    sync::mpsc,
    time::{interval, sleep},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::types::WsFrame;

pub enum WsEvent {
    Frame(WsFrame),
    Connected,
    Disconnected,
}

/// Spawn a background task that maintains the WebSocket connection.
/// Returns a sender for control messages and a receiver for incoming events.
pub fn connect(
    base_url: String,
    chat_id: i64,
    token: String,
    device_id: String,
) -> mpsc::UnboundedReceiver<WsEvent> {
    let (tx, rx) = mpsc::unbounded_channel::<WsEvent>();
    tokio::spawn(ws_task(base_url, chat_id, token, device_id, tx));
    rx
}

async fn ws_task(
    base_url: String,
    chat_id: i64,
    token: String,
    device_id: String,
    tx: mpsc::UnboundedSender<WsEvent>,
) {
    let mut backoff_secs: u64 = 1;

    loop {
        // Convert http(s) → ws(s)
        let ws_base = base_url
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1);
        let url = format!(
            "{}/chat/ws/{}?token={}&device_id={}",
            ws_base.trim_end_matches('/'),
            chat_id,
            token,
            device_id
        );

        match connect_async(&url).await {
            Ok((ws_stream, _)) => {
                backoff_secs = 1; // reset back-off on successful connect
                let _ = tx.send(WsEvent::Connected);
                let (mut sink, mut stream) = ws_stream.split();

                // Ping task — every 25 s
                let tx_ping = tx.clone();
                let ping_handle = tokio::spawn(async move {
                    let mut ticker = interval(Duration::from_secs(25));
                    ticker.tick().await; // skip the first immediate tick
                    loop {
                        ticker.tick().await;
                        let frame = serde_json::json!({"type": "ping"}).to_string();
                        if sink.send(Message::Text(frame)).await.is_err() {
                            break;
                        }
                    }
                });

                // Read loop
                while let Some(msg) = stream.next().await {
                    match msg {
                        Ok(Message::Text(text)) => {
                            if let Ok(frame) = serde_json::from_str::<WsFrame>(&text) {
                                if tx.send(WsEvent::Frame(frame)).is_err() {
                                    return; // receiver dropped — stop
                                }
                            }
                        }
                        Ok(Message::Close(_)) | Err(_) => break,
                        _ => {}
                    }
                }

                ping_handle.abort();
                let _ = tx.send(WsEvent::Disconnected);
            }
            Err(_) => {
                let _ = tx.send(WsEvent::Disconnected);
            }
        }

        // Exponential back-off: 1, 2, 4, 8, 16, 30 s cap
        sleep(Duration::from_secs(backoff_secs)).await;
        backoff_secs = (backoff_secs * 2).min(30);
    }
}
