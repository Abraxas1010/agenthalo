pub mod chat;
pub mod everything;
pub mod payments;
pub mod state;
pub mod tools;

use crate::container::sidecar::SidecarEvent;
use serde_json::json;

pub fn make_event(
    channel: &str,
    seq: u64,
    puf_digest: [u8; 32],
    payload: serde_json::Value,
) -> SidecarEvent {
    SidecarEvent {
        channel: channel.to_string(),
        seq,
        timestamp: crate::util::now_unix_secs(),
        puf_digest,
        payload,
    }
}

pub fn heartbeat(seq: u64, puf_digest: [u8; 32]) -> SidecarEvent {
    make_event(
        "state",
        seq,
        puf_digest,
        json!({
            "kind": "heartbeat",
        }),
    )
}
