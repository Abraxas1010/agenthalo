use crate::container::shim::make_event;
use crate::container::sidecar::SidecarEvent;
use serde_json::json;

pub fn raw_event(seq: u64, puf_digest: [u8; 32], stream: &str, payload: &str) -> SidecarEvent {
    make_event(
        "everything",
        seq,
        puf_digest,
        json!({
            "stream": stream,
            "payload": payload
        }),
    )
}
