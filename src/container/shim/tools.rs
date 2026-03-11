use crate::container::shim::make_event;
use crate::container::sidecar::SidecarEvent;
use serde_json::json;

pub fn tool_event(
    seq: u64,
    puf_digest: [u8; 32],
    tool: &str,
    args: serde_json::Value,
    ok: bool,
    duration_ms: u64,
) -> SidecarEvent {
    make_event(
        "tools",
        seq,
        puf_digest,
        json!({
            "tool": tool,
            "args": args,
            "ok": ok,
            "duration_ms": duration_ms
        }),
    )
}
