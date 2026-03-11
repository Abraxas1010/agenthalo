use crate::container::shim::make_event;
use crate::container::sidecar::SidecarEvent;
use serde_json::json;

pub fn payment_event(
    seq: u64,
    puf_digest: [u8; 32],
    amount: u64,
    counterparty: &str,
    direction: &str,
    tx_hash: &str,
) -> SidecarEvent {
    make_event(
        "payments",
        seq,
        puf_digest,
        json!({
            "amount": amount,
            "counterparty": counterparty,
            "direction": direction,
            "tx_hash": tx_hash
        }),
    )
}
