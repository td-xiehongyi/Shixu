//! OneBot 11 forward WebSocket, event-only, literal loopback, text arrays only.
//! No API/send surface. Successful protocol authentication is not Windows G2.
use super::{adapter::NativeQQAdapter, transport::*};
use shixu_core::{
    contracts::{AppResult, UtcMillis, error::AppError, notification::*, vault::SecretBytes},
    notifications::{MessageStore, source::QQAdapter},
    runtime::workers::ReceivePort,
};
use std::{
    net::{Shutdown, TcpStream},
    time::{Duration, Instant},
};
use tungstenite::{
    Error, HandshakeError, Message, WebSocket, client::IntoClientRequest, http::HeaderValue,
    protocol::WebSocketConfig,
};
use zeroize::Zeroizing;
const FRAME_LIMIT: usize = 1024 * 1024;
const CONNECT_LIMIT: Duration = Duration::from_secs(1);
const POLL_LIMIT: Duration = Duration::from_millis(100);
#[derive(Default)]
pub struct OneBotTextTransport {
    socket: Option<WebSocket<TcpStream>>,
    config: Option<SourceConfig>,
    pending: Option<Delivery>,
}
fn wire_error(error: Error) -> AppError {
    match error {
        Error::Http(response) if matches!(response.status().as_u16(), 401 | 403) => {
            AppError::AuthFailed
        }
        Error::Capacity(_) => AppError::InvalidInput,
        Error::Protocol(_) | Error::Utf8(_) | Error::AttackAttempt => AppError::ParseFailed,
        _ => AppError::Disconnected,
    }
}
impl OneBotTextTransport {
    fn read_event(&mut self, deadline: Instant) -> AppResult<Option<serde_json::Value>> {
        let socket = self.socket.as_mut().ok_or(AppError::Disconnected)?;
        loop {
            if Instant::now() >= deadline {
                return Ok(None);
            }
            match socket.read() {
                Ok(Message::Text(text)) => {
                    return serde_json::from_str(&text)
                        .map(Some)
                        .map_err(|_| AppError::ParseFailed);
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                    // tungstenite queues the protocol pong; flush without business data.
                    match socket.flush() {
                        Ok(()) => (),
                        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                        Err(e) => return Err(wire_error(e)),
                    }
                }
                Ok(Message::Close(_)) => {
                    let _ = socket.flush();
                    return Err(AppError::Disconnected);
                }
                Ok(_) => return Err(AppError::Unsupported),
                Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(e) => return Err(wire_error(e)),
            }
        }
    }
    fn decode(&self, event: serde_json::Value) -> AppResult<Option<Delivery>> {
        let c = self.config.as_ref().ok_or(AppError::Disconnected)?;
        let integer = |key: &str| event[key].as_i64().ok_or(AppError::InvalidInput);
        if integer("self_id")?.to_string() != c.account_id {
            return Err(AppError::AuthFailed);
        }
        match event["post_type"].as_str() {
            Some("meta_event")
                if matches!(
                    event["meta_event_type"].as_str(),
                    Some("heartbeat" | "lifecycle")
                ) =>
            {
                return Ok(None);
            }
            Some("message") => (),
            Some("notice" | "request") => return Ok(None),
            _ => return Err(AppError::InvalidInput),
        }
        match event["message_type"].as_str() {
            Some("private") => return Ok(None),
            Some("group") => (),
            _ => return Err(AppError::InvalidInput),
        }
        let group_id = integer("group_id")?.to_string();
        // Before inspecting/storing message content, reject groups outside the scope.
        if !c.allowed_group_ids.contains(&group_id) {
            return Ok(None);
        }
        let native_id = integer("message_id")?;
        i32::try_from(native_id).map_err(|_| AppError::InvalidInput)?;
        let sent_at = integer("time")?
            .checked_mul(1000)
            .ok_or(AppError::InvalidInput)?;
        let sender_id = integer("user_id")?.to_string();
        let segments = event["message"].as_array().ok_or(AppError::Unsupported)?;
        if segments.is_empty() {
            return Err(AppError::InvalidInput);
        }
        let mut text = String::new();
        for segment in segments {
            if segment["type"] != "text" {
                return Err(AppError::Unsupported);
            }
            text.push_str(
                segment["data"]["text"]
                    .as_str()
                    .ok_or(AppError::InvalidInput)?,
            );
        }
        let cursor = native_id.to_string();
        Ok(Some(Delivery {
            cursor: cursor.clone(),
            message: MessageEnvelope {
                message_key: MessageKey::from_uuid(uuid::Uuid::nil()),
                source_id: c.source_id,
                account_id: c.account_id.clone(),
                group_id,
                native_message_id: cursor,
                sent_at,
                received_at: sent_at,
                sender_id,
                text,
                reply_to: None,
                revision: 1,
                revoked: false,
                processing_state: ProcessingState::Persisted,
                parts: vec![],
            },
        }))
    }
}
impl ReceiveTransport for OneBotTextTransport {
    fn connect(
        &mut self,
        endpoint: &LoopbackEndpoint,
        config: &SourceConfig,
        token: SecretBytes,
    ) -> AppResult<Vec<SourceCapability>> {
        self.disconnect()?;
        if config.adapter_type != "onebot11-text"
            || config
                .account_id
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .map(|id| id.to_string())
                .as_deref()
                != Some(&config.account_id)
            || token.expose().is_empty()
            || token.expose().len() > 4096
            || !token.expose().iter().all(|b| b.is_ascii_graphic())
        {
            return Err(AppError::InvalidInput);
        }
        let mut request = format!("ws://{}/event", endpoint.address())
            .into_client_request()
            .map_err(|_| AppError::InvalidInput)?;
        let mut bearer = Zeroizing::new(b"Bearer ".to_vec());
        bearer.extend_from_slice(token.expose());
        let mut header = HeaderValue::from_bytes(&bearer).map_err(|_| AppError::InvalidInput)?;
        header.set_sensitive(true);
        request.headers_mut().insert("Authorization", header);
        let stream = TcpStream::connect_timeout(&endpoint.address(), CONNECT_LIMIT)
            .map_err(|_| AppError::Disconnected)?;
        stream
            .set_nonblocking(true)
            .map_err(|_| AppError::Disconnected)?;
        let limits = WebSocketConfig::default()
            .max_message_size(Some(FRAME_LIMIT))
            .max_frame_size(Some(FRAME_LIMIT))
            .max_write_buffer_size(FRAME_LIMIT);
        let deadline = Instant::now() + CONNECT_LIMIT;
        let mut handshake = tungstenite::client::client_with_config(request, stream, Some(limits));
        let socket = loop {
            match handshake {
                Ok((socket, _)) => break socket,
                Err(HandshakeError::Failure(e)) => return Err(wire_error(e)),
                Err(HandshakeError::Interrupted(mid)) => {
                    if Instant::now() >= deadline {
                        return Err(AppError::Disconnected);
                    }
                    std::thread::sleep(Duration::from_millis(2));
                    handshake = mid.handshake();
                }
            }
        };
        self.socket = Some(socket);
        self.config = Some(config.clone());
        let result = self
            .read_event(Instant::now() + CONNECT_LIMIT)
            .and_then(|e| self.decode(e.ok_or(AppError::Disconnected)?));
        match result {
            Ok(delivery) => self.pending = delivery,
            Err(e) => {
                let _ = self.disconnect();
                return Err(e);
            }
        }
        Ok(vec![SourceCapability::LiveMessages])
    }
    fn disconnect(&mut self) -> AppResult<()> {
        if let Some(mut socket) = self.socket.take() {
            let _ = socket.close(None);
            let _ = socket.flush();
            let _ = socket.get_mut().shutdown(Shutdown::Both);
        }
        self.config = None;
        self.pending = None;
        Ok(())
    }
    fn next(&mut self) -> AppResult<Option<Delivery>> {
        if self.pending.is_some() {
            return Ok(self.pending.take());
        }
        match self.read_event(Instant::now() + POLL_LIMIT)? {
            Some(event) => self.decode(event),
            None => Ok(None),
        }
    }
    fn consumed_unsupported_delivery(&self) -> bool {
        self.socket.is_some()
    }
    fn backfill(&mut self, _: &str, _: &str) -> AppResult<BackfillBatch> {
        Err(AppError::Unsupported)
    }
}
impl Drop for OneBotTextTransport {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}
/// Trusted native assembly. The caller supplies an independently protected token
/// in memory and the same database/configuration used by Supervisor. Never IPC.
pub fn connected_onebot_receiver(
    endpoint: LoopbackEndpoint,
    config: SourceConfig,
    token: SecretBytes,
    store: MessageStore,
    now: fn() -> UtcMillis,
) -> AppResult<Box<dyn ReceivePort>> {
    let mut adapter = NativeQQAdapter::new(OneBotTextTransport::default(), endpoint, store, now);
    adapter.connect(config, token)?;
    Ok(Box::new(adapter))
}
