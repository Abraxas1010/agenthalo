use crate::cockpit::pty_manager::SessionEvent;
use crate::cockpit::session::SessionStatus;
use crate::dashboard::DashboardState;
use crate::halo::auth::is_dashboard_authenticated;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::broadcast;

fn auth_required_payload() -> serde_json::Value {
    json!({
        "error": "authentication required: open Setup and sign in with GitHub or Google, then retry",
        "code": "auth_required",
        "setup_route": "#/setup",
        "next_steps": [
            "Open Setup",
            "Select Continue with GitHub or Continue with Google"
        ]
    })
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Path(session_id): Path<String>,
    State(state): State<DashboardState>,
) -> Response {
    if !is_dashboard_authenticated(&state.credentials_path) {
        // Browser WebSocket clients don't expose failed-upgrade bodies directly,
        // but we keep this JSON payload consistent with REST auth errors for
        // scripts and future non-browser clients.
        return (StatusCode::UNAUTHORIZED, Json(auth_required_payload())).into_response();
    }

    let Some(session) = state.pty_manager.get_session(&session_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "session not found"})),
        )
            .into_response();
    };

    ws.on_upgrade(move |socket| async move {
        handle_socket(socket, session).await;
    })
    .into_response()
}

async fn handle_socket(
    socket: WebSocket,
    session: std::sync::Arc<crate::cockpit::pty_manager::PtySession>,
) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut out_rx = session.subscribe_output();

    let _ = ws_tx
        .send(Message::Text(
            status_message_json(&session.status(), Some(&session.id))
                .to_string()
                .into(),
        ))
        .await;

    loop {
        tokio::select! {
            outbound = out_rx.recv() => {
                match outbound {
                    Ok(SessionEvent::Output(bytes)) => {
                        crate::halo::governor_telemetry::record_comms_batch(1);
                        if ws_tx.send(Message::Binary(bytes.into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(SessionEvent::Status(status)) => {
                        crate::halo::governor_telemetry::record_comms_batch(1);
                        if ws_tx.send(Message::Text(status_message_json(&status, Some(&session.id)).to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = ws_rx.next() => {
                match incoming {
                    Some(Ok(Message::Binary(bytes))) => {
                        crate::halo::governor_telemetry::record_comms_batch(1);
                        if let Err(e) = session.write_input(bytes.as_ref()) {
                            let _ = ws_tx.send(Message::Text(status_message_json(
                                &SessionStatus::Error { message: e },
                                Some(&session.id)
                            ).to_string().into())).await;
                            break;
                        }
                    }
                    Some(Ok(Message::Text(text))) => {
                        crate::halo::governor_telemetry::record_comms_batch(1);
                        if let Err(e) = handle_text_frame(&session, &text, &mut ws_tx).await {
                            let _ = ws_tx.send(Message::Text(status_message_json(
                                &SessionStatus::Error { message: e },
                                Some(&session.id)
                            ).to_string().into())).await;
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = ws_tx.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Err(_)) | None => break,
                }
            }
        }
    }

    let _ = ws_tx.close().await;
}

async fn handle_text_frame(
    session: &crate::cockpit::pty_manager::PtySession,
    text: &str,
    ws_tx: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) -> Result<(), String> {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(text);
    if let Ok(parsed) = parsed {
        match parsed.get("type").and_then(|t| t.as_str()) {
            Some("resize") => {
                let cols = parsed.get("cols").and_then(|v| v.as_u64()).unwrap_or(80) as u16;
                let rows = parsed.get("rows").and_then(|v| v.as_u64()).unwrap_or(24) as u16;
                session.resize(cols, rows)?;
                return Ok(());
            }
            Some("ping") => {
                let _ = ws_tx
                    .send(Message::Text(json!({"type":"pong"}).to_string().into()))
                    .await;
                return Ok(());
            }
            _ => {}
        }
    }

    // Treat plain text frames as terminal input (fixes dropped paste/typed input).
    session.write_input(text.as_bytes())
}

fn status_message_json(status: &SessionStatus, session_id: Option<&str>) -> serde_json::Value {
    match status {
        SessionStatus::Starting => {
            json!({"type":"status","state":"starting","session_id":session_id})
        }
        SessionStatus::Active => json!({"type":"status","state":"active","session_id":session_id}),
        SessionStatus::Done { exit_code } => {
            json!({"type":"status","state":"done","session_id":session_id,"exit_code":exit_code})
        }
        SessionStatus::Error { message } => {
            json!({"type":"status","state":"error","session_id":session_id,"message":message})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::auth_required_payload;

    #[test]
    fn ws_auth_payload_has_setup_metadata() {
        let payload = auth_required_payload();
        assert_eq!(payload["code"], "auth_required");
        assert_eq!(payload["setup_route"], "#/setup");
        assert_eq!(
            payload["next_steps"][0].as_str().unwrap_or_default(),
            "Open Setup"
        );
        assert!(payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("GitHub or Google"));
    }
}
