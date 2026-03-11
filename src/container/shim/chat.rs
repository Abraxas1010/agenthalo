use crate::container::shim::make_event;
use crate::container::sidecar::SidecarEvent;
use serde_json::json;

pub fn chat_event(
    seq: u64,
    puf_digest: [u8; 32],
    model: &str,
    prompt: &str,
    completion: &str,
    latency_ms: u64,
) -> SidecarEvent {
    make_event(
        "chat",
        seq,
        puf_digest,
        json!({
            "model": model,
            "prompt": prompt,
            "completion": completion,
            "latency_ms": latency_ms
        }),
    )
}
