use crate::pcn::schema::{ChannelRecord, ChannelStatus, SettlementOp};
use crate::protocol::{CommitError, NucleusDb};
use crate::state::Delta;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, PartialEq, Eq)]
pub enum PcnError {
    ChannelAlreadyExists,
    ChannelMissing,
    ChannelNotOpen,
    InvalidConservation {
        left: u64,
        right: u64,
        capacity: u64,
    },
    Commit(CommitError),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChannelOpView {
    pub seq: u64,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChannelSnapshot {
    pub record: ChannelRecord,
    pub balance1: u64,
    pub balance2: u64,
    pub ops: Vec<ChannelOpView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PcnComplianceWitness {
    pub feasibility_root: [u8; 32],
    pub replay_seq: u64,
    pub channel_count: u64,
    pub valid_channels: u64,
    pub invalid_channels: u64,
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn canonical_pair(p1: &str, p2: &str) -> (String, String) {
    if p1 <= p2 {
        (p1.to_string(), p2.to_string())
    } else {
        (p2.to_string(), p1.to_string())
    }
}

fn channel_base(p1: &str, p2: &str) -> String {
    let (a, b) = canonical_pair(p1, p2);
    format!("channel:{a}:{b}")
}

fn status_code(s: &ChannelStatus) -> u64 {
    match s {
        ChannelStatus::Open => 1,
        ChannelStatus::Closing => 2,
        ChannelStatus::Closed => 3,
    }
}

fn status_from_code(v: u64) -> ChannelStatus {
    match v {
        1 => ChannelStatus::Open,
        2 => ChannelStatus::Closing,
        3 => ChannelStatus::Closed,
        _ => ChannelStatus::Open,
    }
}

fn op_code(op: &SettlementOp) -> u64 {
    match op {
        SettlementOp::Open { .. } => 1,
        SettlementOp::Close { .. } => 2,
        SettlementOp::Splice { .. } => 3,
        SettlementOp::Update { .. } => 4,
    }
}

fn op_name(code: u64) -> String {
    match code {
        1 => "open".to_string(),
        2 => "close".to_string(),
        3 => "splice".to_string(),
        4 => "update".to_string(),
        _ => "unknown".to_string(),
    }
}

fn compliance_leaf(
    channel_base: &str,
    seq: u64,
    capacity: u64,
    balance1: u64,
    balance2: u64,
    status: &ChannelStatus,
    feasible: bool,
) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([0x00]); // RFC6962-style leaf prefix.
    h.update(b"nucleusdb.pcn.compliance.v1|");
    h.update(channel_base.as_bytes());
    h.update([0u8]);
    h.update(seq.to_be_bytes());
    h.update(capacity.to_be_bytes());
    h.update(balance1.to_be_bytes());
    h.update(balance2.to_be_bytes());
    h.update(status_code(status).to_be_bytes());
    h.update([u8::from(feasible)]);
    h.finalize().into()
}

fn compliance_merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    if leaves.len() == 1 {
        return leaves[0];
    }
    let mut layer = leaves.to_vec();
    while layer.len() > 1 {
        let mut next = Vec::with_capacity(layer.len().div_ceil(2));
        let mut i = 0;
        while i + 1 < layer.len() {
            let mut h = Sha256::new();
            h.update([0x01]); // RFC6962-style interior prefix.
            h.update(layer[i]);
            h.update(layer[i + 1]);
            next.push(h.finalize().into());
            i += 2;
        }
        if i < layer.len() {
            next.push(layer[i]);
        }
        layer = next;
    }
    layer[0]
}

fn value_at(db: &NucleusDb, key: &str) -> Option<u64> {
    let idx = db.keymap.get(key)?;
    db.state.values.get(idx).copied()
}

fn put_key(writes: &mut Vec<(usize, u64)>, db: &mut NucleusDb, key: String, value: u64) {
    let idx = db.keymap.get_or_create(&key);
    writes.push((idx, value));
}

// Sentinel digest for immutable existence anchoring.
// This is not a cryptographic state commitment for full record reconstruction.
fn record_sentinel(record: &ChannelRecord) -> u64 {
    let mut h = Sha256::new();
    h.update(b"nucleusdb.pcn.channel_record.v1|");
    h.update(record.participant1.as_bytes());
    h.update([0u8]);
    h.update(record.participant2.as_bytes());
    h.update(record.capacity.to_le_bytes());
    h.update(status_code(&record.status).to_le_bytes());
    h.update(record.created_at.to_le_bytes());
    h.update(record.last_seq.to_le_bytes());
    let full: [u8; 32] = h.finalize().into();
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&full[..8]);
    u64::from_le_bytes(buf)
}

fn parse_seq_from_key(prefix: &str, key: &str) -> Option<u64> {
    if !key.starts_with(prefix) {
        return None;
    }
    key[prefix.len()..].parse::<u64>().ok()
}

fn latest_seq(db: &NucleusDb, base: &str) -> u64 {
    let prefix = format!("{base}:state:");
    let mut best = 0u64;
    for (key, _) in db.keymap.all_keys() {
        if let Some(raw) = parse_seq_from_key(&prefix, key) {
            let seq = key
                .strip_prefix(&format!("{base}:state:{raw}:"))
                .map(|_| raw)
                .unwrap_or(0);
            if seq > best {
                best = seq;
            }
        }
    }
    if best > 0 {
        return best;
    }
    // Fallback parser for keys like `channel:x:y:state:<seq>:b1`.
    let prefix = format!("{base}:state:");
    for (key, _) in db.keymap.all_keys() {
        if !key.starts_with(&prefix) {
            continue;
        }
        let rest = &key[prefix.len()..];
        if let Some((seq_str, _)) = rest.split_once(':') {
            if let Ok(seq) = seq_str.parse::<u64>() {
                best = best.max(seq);
            }
        }
    }
    best
}

fn capacity_for(db: &NucleusDb, base: &str) -> Option<u64> {
    value_at(db, &format!("{base}:meta:capacity"))
}

fn exists_channel(db: &NucleusDb, base: &str) -> bool {
    value_at(db, base).unwrap_or(0) != 0
}

fn apply_writes(db: &mut NucleusDb, writes: Vec<(usize, u64)>) -> Result<(), PcnError> {
    db.commit(Delta::new(writes), &[])
        .map_err(PcnError::Commit)?;
    Ok(())
}

impl SettlementOp {
    pub fn apply(&self, db: &mut NucleusDb) -> Result<(), PcnError> {
        match self {
            SettlementOp::Open { p1, p2, capacity } => {
                let base = channel_base(p1, p2);
                if exists_channel(db, &base) {
                    return Err(PcnError::ChannelAlreadyExists);
                }
                db.set_append_only();
                let created_at = now_unix_secs();
                let record = ChannelRecord {
                    participant1: canonical_pair(p1, p2).0,
                    participant2: canonical_pair(p1, p2).1,
                    capacity: *capacity,
                    status: ChannelStatus::Open,
                    created_at,
                    last_seq: 1,
                };
                let mut writes = Vec::new();
                put_key(&mut writes, db, base.clone(), record_sentinel(&record));
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:meta:capacity"),
                    record.capacity,
                );
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:meta:created_at"),
                    record.created_at,
                );
                put_key(&mut writes, db, format!("{base}:ops:1:kind"), op_code(self));
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:state:1:status"),
                    status_code(&ChannelStatus::Open),
                );
                put_key(&mut writes, db, format!("{base}:state:1:b1"), *capacity);
                put_key(&mut writes, db, format!("{base}:state:1:b2"), 0);
                apply_writes(db, writes)
            }
            SettlementOp::Update {
                p1,
                p2,
                balance1,
                balance2,
            } => {
                let base = channel_base(p1, p2);
                if !exists_channel(db, &base) {
                    return Err(PcnError::ChannelMissing);
                }
                let cap = capacity_for(db, &base).ok_or(PcnError::ChannelMissing)?;
                if balance1.saturating_add(*balance2) != cap {
                    return Err(PcnError::InvalidConservation {
                        left: *balance1,
                        right: *balance2,
                        capacity: cap,
                    });
                }
                let cur_seq = latest_seq(db, &base);
                if cur_seq == 0 {
                    return Err(PcnError::ChannelMissing);
                }
                let status = status_from_code(
                    value_at(db, &format!("{base}:state:{cur_seq}:status")).unwrap_or(1),
                );
                if status != ChannelStatus::Open {
                    return Err(PcnError::ChannelNotOpen);
                }
                let next_seq = cur_seq + 1;
                let mut writes = Vec::new();
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:ops:{next_seq}:kind"),
                    op_code(self),
                );
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:state:{next_seq}:status"),
                    status_code(&ChannelStatus::Open),
                );
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:state:{next_seq}:b1"),
                    *balance1,
                );
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:state:{next_seq}:b2"),
                    *balance2,
                );
                apply_writes(db, writes)
            }
            SettlementOp::Close { p1, p2 } => {
                let base = channel_base(p1, p2);
                if !exists_channel(db, &base) {
                    return Err(PcnError::ChannelMissing);
                }
                let cur_seq = latest_seq(db, &base);
                if cur_seq == 0 {
                    return Err(PcnError::ChannelMissing);
                }
                let b1 = value_at(db, &format!("{base}:state:{cur_seq}:b1")).unwrap_or(0);
                let b2 = value_at(db, &format!("{base}:state:{cur_seq}:b2")).unwrap_or(0);
                let next_seq = cur_seq + 1;
                let mut writes = Vec::new();
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:ops:{next_seq}:kind"),
                    op_code(self),
                );
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:state:{next_seq}:status"),
                    status_code(&ChannelStatus::Closed),
                );
                put_key(&mut writes, db, format!("{base}:state:{next_seq}:b1"), b1);
                put_key(&mut writes, db, format!("{base}:state:{next_seq}:b2"), b2);
                apply_writes(db, writes)
            }
            SettlementOp::Splice {
                p1,
                p2,
                new_capacity,
            } => {
                let base = channel_base(p1, p2);
                if !exists_channel(db, &base) {
                    return Err(PcnError::ChannelMissing);
                }
                let cur_seq = latest_seq(db, &base);
                let b1 = value_at(db, &format!("{base}:state:{cur_seq}:b1")).unwrap_or(0);
                let next_seq = cur_seq + 1;
                let new_b1 = b1.min(*new_capacity);
                let new_b2 = new_capacity.saturating_sub(new_b1);
                let mut writes = Vec::new();
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:ops:{next_seq}:kind"),
                    op_code(self),
                );
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:state:{next_seq}:status"),
                    status_code(&ChannelStatus::Open),
                );
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:state:{next_seq}:capacity"),
                    *new_capacity,
                );
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:state:{next_seq}:b1"),
                    new_b1,
                );
                put_key(
                    &mut writes,
                    db,
                    format!("{base}:state:{next_seq}:b2"),
                    new_b2,
                );
                apply_writes(db, writes)
            }
        }
    }
}

pub fn channel_snapshot(db: &NucleusDb, p1: &str, p2: &str) -> Option<ChannelSnapshot> {
    let base = channel_base(p1, p2);
    if !exists_channel(db, &base) {
        return None;
    }
    let capacity = capacity_for(db, &base)?;
    let created_at = value_at(db, &format!("{base}:meta:created_at")).unwrap_or(0);
    let seq = latest_seq(db, &base);
    if seq == 0 {
        return None;
    }
    let status = status_from_code(value_at(db, &format!("{base}:state:{seq}:status")).unwrap_or(1));
    let balance1 = value_at(db, &format!("{base}:state:{seq}:b1")).unwrap_or(0);
    let balance2 = value_at(db, &format!("{base}:state:{seq}:b2")).unwrap_or(0);
    let (a, b) = canonical_pair(p1, p2);
    let mut op_index = BTreeMap::<u64, String>::new();
    let op_prefix = format!("{base}:ops:");
    for (key, idx) in db.keymap.all_keys() {
        if !key.starts_with(&op_prefix) || !key.ends_with(":kind") {
            continue;
        }
        let rest = &key[op_prefix.len()..];
        let Some((seq_str, _)) = rest.split_once(':') else {
            continue;
        };
        let Ok(op_seq) = seq_str.parse::<u64>() else {
            continue;
        };
        let kind_code = db.state.values.get(idx).copied().unwrap_or(0);
        op_index.insert(op_seq, op_name(kind_code));
    }
    let ops = op_index
        .into_iter()
        .map(|(seq, kind)| ChannelOpView { seq, kind })
        .collect::<Vec<_>>();
    Some(ChannelSnapshot {
        record: ChannelRecord {
            participant1: a,
            participant2: b,
            capacity,
            status,
            created_at,
            last_seq: seq,
        },
        balance1,
        balance2,
        ops,
    })
}

pub fn compliance_witness(db: &NucleusDb) -> PcnComplianceWitness {
    let mut channel_bases = Vec::<String>::new();
    for (key, _) in db.keymap.all_keys() {
        if !key.starts_with("channel:") || !key.ends_with(":meta:capacity") {
            continue;
        }
        let base = key.trim_end_matches(":meta:capacity");
        if exists_channel(db, base) {
            channel_bases.push(base.to_string());
        }
    }
    channel_bases.sort();
    channel_bases.dedup();

    let mut replay_seq = 0u64;
    let mut valid = 0u64;
    let mut invalid = 0u64;
    let mut leaves = Vec::<[u8; 32]>::new();

    for base in &channel_bases {
        let seq = latest_seq(db, base);
        if seq == 0 {
            continue;
        }
        replay_seq = replay_seq.max(seq);

        let cap = value_at(db, &format!("{base}:meta:capacity")).unwrap_or(0);
        let status =
            status_from_code(value_at(db, &format!("{base}:state:{seq}:status")).unwrap_or(1));
        let b1 = value_at(db, &format!("{base}:state:{seq}:b1")).unwrap_or(0);
        let b2 = value_at(db, &format!("{base}:state:{seq}:b2")).unwrap_or(0);
        let feasible = b1.saturating_add(b2) == cap;
        if feasible {
            valid += 1;
        } else {
            invalid += 1;
        }
        leaves.push(compliance_leaf(base, seq, cap, b1, b2, &status, feasible));
    }

    PcnComplianceWitness {
        feasibility_root: compliance_merkle_root(&leaves),
        replay_seq,
        channel_count: channel_bases.len() as u64,
        valid_channels: valid,
        invalid_channels: invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::VcBackend;
    use crate::state::{Delta, State};
    use crate::witness::WitnessConfig;

    fn mk_db() -> NucleusDb {
        let cfg =
            WitnessConfig::with_generated_keys(2, vec!["w1".into(), "w2".into(), "w3".into()]);
        NucleusDb::new(State::new(vec![]), VcBackend::BinaryMerkle, cfg)
    }

    #[test]
    fn pcn_adapter_roundtrip_open_update_close() {
        let mut db = mk_db();
        SettlementOp::Open {
            p1: "a".into(),
            p2: "b".into(),
            capacity: 100,
        }
        .apply(&mut db)
        .expect("open");
        SettlementOp::Update {
            p1: "a".into(),
            p2: "b".into(),
            balance1: 70,
            balance2: 30,
        }
        .apply(&mut db)
        .expect("update");
        SettlementOp::Close {
            p1: "a".into(),
            p2: "b".into(),
        }
        .apply(&mut db)
        .expect("close");
        let snap = channel_snapshot(&db, "a", "b").expect("snapshot");
        assert_eq!(snap.record.last_seq, 3);
        assert_eq!(snap.record.status, ChannelStatus::Closed);
        assert_eq!(snap.balance1 + snap.balance2, snap.record.capacity);
        assert_eq!(snap.ops.len(), 3);
    }

    #[test]
    fn pcn_conservation_rejected() {
        let mut db = mk_db();
        SettlementOp::Open {
            p1: "a".into(),
            p2: "b".into(),
            capacity: 10,
        }
        .apply(&mut db)
        .expect("open");
        let err = SettlementOp::Update {
            p1: "a".into(),
            p2: "b".into(),
            balance1: 8,
            balance2: 1,
        }
        .apply(&mut db)
        .expect_err("must fail");
        match err {
            PcnError::InvalidConservation { .. } => {}
            other => panic!("unexpected err: {other:?}"),
        }
    }

    #[test]
    fn pcn_compliance_witness_tracks_replay_seq() {
        let mut db = mk_db();
        SettlementOp::Open {
            p1: "a".into(),
            p2: "b".into(),
            capacity: 100,
        }
        .apply(&mut db)
        .expect("open");
        SettlementOp::Update {
            p1: "a".into(),
            p2: "b".into(),
            balance1: 60,
            balance2: 40,
        }
        .apply(&mut db)
        .expect("update");
        SettlementOp::Close {
            p1: "a".into(),
            p2: "b".into(),
        }
        .apply(&mut db)
        .expect("close");

        let witness = compliance_witness(&db);
        assert_eq!(witness.replay_seq, 3);
        assert_eq!(witness.channel_count, 1);
        assert_eq!(witness.valid_channels, 1);
        assert_eq!(witness.invalid_channels, 0);
        assert_ne!(witness.feasibility_root, [0u8; 32]);
    }

    #[test]
    fn pcn_compliance_witness_flags_invalid_channel_state() {
        let mut db = mk_db();
        SettlementOp::Open {
            p1: "a".into(),
            p2: "b".into(),
            capacity: 10,
        }
        .apply(&mut db)
        .expect("open");

        let base = "channel:a:b";
        let mut writes = Vec::new();
        put_key(&mut writes, &mut db, format!("{base}:ops:2:kind"), 4);
        put_key(&mut writes, &mut db, format!("{base}:state:2:status"), 1);
        put_key(&mut writes, &mut db, format!("{base}:state:2:b1"), 11);
        put_key(&mut writes, &mut db, format!("{base}:state:2:b2"), 0);
        db.commit(Delta::new(writes), &[])
            .expect("commit tampered state");

        let witness = compliance_witness(&db);
        assert_eq!(witness.replay_seq, 2);
        assert_eq!(witness.channel_count, 1);
        assert_eq!(witness.valid_channels, 0);
        assert_eq!(witness.invalid_channels, 1);
    }
}
