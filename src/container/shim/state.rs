use crate::container::shim::make_event;
use crate::container::sidecar::SidecarEvent;
use serde_json::json;

pub fn state_event(
    seq: u64,
    puf_digest: [u8; 32],
    key: &str,
    value: u64,
    op: &str,
) -> SidecarEvent {
    make_event(
        "state",
        seq,
        puf_digest,
        json!({
            "op": op,
            "key": key,
            "value": value
        }),
    )
}
