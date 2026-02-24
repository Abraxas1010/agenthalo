use crate::cli::default_witness_cfg;
use crate::halo::config;
use crate::halo::pricing;
use crate::halo::pricing::ModelPricing;
use crate::halo::schema::*;
use crate::persistence::{
    default_wal_path, init_wal, load_snapshot, persist_snapshot_and_sync_wal,
};
use crate::protocol::{NucleusDb, VcBackend};
use crate::state::{Delta, State};
use crate::witness::{WitnessConfig, WitnessSignatureAlgorithm};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct CostBucket {
    pub label: String,
    pub sessions: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Clone, Debug)]
pub struct PaidCostBucket {
    pub label: String,
    pub operations: u64,
    pub credits_spent: u64,
    pub usd_spent: f64,
}

pub struct TraceWriter {
    session_id: String,
    db: NucleusDb,
    db_path: PathBuf,
    wal_path: PathBuf,
    seq: u64,
    summary: SessionSummary,
    session_meta: Option<SessionMetadata>,
    pricing: HashMap<String, ModelPricing>,
}

impl TraceWriter {
    pub fn new(db_path: &Path) -> Result<Self, String> {
        let db_path = db_path.to_path_buf();
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create db parent {}: {e}", parent.display()))?;
        }

        let witness_cfg = halo_witness_cfg();
        let db = if db_path.exists() {
            load_snapshot(&db_path, witness_cfg)
                .map_err(|e| format!("load snapshot {}: {e:?}", db_path.display()))?
        } else {
            NucleusDb::new(
                State::new(vec![]),
                VcBackend::BinaryMerkle,
                halo_witness_cfg(),
            )
        };

        let wal_path = default_wal_path(&db_path);
        init_wal(&wal_path, &db).map_err(|e| format!("init WAL {}: {e:?}", wal_path.display()))?;

        let pricing = pricing::load_or_default(&config::pricing_path())?;

        Ok(Self {
            session_id: format!("sess-{}", now_unix_secs()),
            db,
            db_path,
            wal_path,
            seq: 0,
            summary: SessionSummary::default(),
            session_meta: None,
            pricing,
        })
    }

    pub fn start_session(&mut self, meta: SessionMetadata) -> Result<(), String> {
        self.session_id = meta.session_id.clone();
        self.seq = 0;
        self.summary = SessionSummary::default();
        self.summary.model = meta.model.clone();

        let mut writes = Vec::new();
        let base = session_base_key(&meta.session_id);
        let raw =
            serde_json::to_vec(&meta).map_err(|e| format!("serialize session metadata: {e}"))?;
        append_blob_writes(&base, &raw, &mut writes);

        writes.push((
            format!(
                "{IDX_AGENT_PREFIX}{}:{}:{}",
                meta.agent, meta.started_at, meta.session_id
            ),
            1,
        ));
        let date = day_label(meta.started_at);
        writes.push((
            format!(
                "{IDX_DATE_PREFIX}{date}:{}:{}",
                meta.started_at, meta.session_id
            ),
            1,
        ));
        if let Some(model) = &meta.model {
            writes.push((
                format!(
                    "{IDX_MODEL_PREFIX}{model}:{}:{}",
                    meta.started_at, meta.session_id
                ),
                1,
            ));
        }

        self.commit_writes(writes)?;
        self.session_meta = Some(meta);
        Ok(())
    }

    pub fn write_event(&mut self, mut event: TraceEvent) -> Result<(), String> {
        if self.session_meta.is_none() {
            return Err("session not started".to_string());
        }

        self.seq += 1;
        event.seq = self.seq;
        if event.timestamp == 0 {
            event.timestamp = now_unix_secs();
        }
        if event.content_hash.trim().is_empty() {
            event.content_hash = event_content_hash(&event.content)?;
        }

        self.summary.total_input_tokens += event.input_tokens.unwrap_or(0);
        self.summary.total_output_tokens += event.output_tokens.unwrap_or(0);
        self.summary.total_cache_read_tokens += event.cache_read_tokens.unwrap_or(0);
        self.summary.event_count += 1;

        match event.event_type {
            EventType::ToolCall => self.summary.tool_calls += 1,
            EventType::McpToolCall => self.summary.mcp_tool_calls += 1,
            EventType::FileChange => {
                let op = event
                    .content
                    .get("op")
                    .and_then(|v| v.as_str())
                    .unwrap_or("modify");
                if op.eq_ignore_ascii_case("create") {
                    self.summary.files_created += 1;
                    self.summary.files_modified += 1;
                } else if op.eq_ignore_ascii_case("read") {
                    self.summary.files_read += 1;
                    // Reads are not modifications — don't increment files_modified.
                } else {
                    // modify, edit, write, delete, etc.
                    self.summary.files_modified += 1;
                }
            }
            EventType::BashCommand => self.summary.bash_commands += 1,
            EventType::Error => self.summary.errors += 1,
            EventType::SubagentSpawn => self.summary.subagents_spawned += 1,
            _ => {}
        }

        let mut writes = Vec::new();
        let base = event_base_key(&self.session_id, event.seq);
        let raw = serde_json::to_vec(&event).map_err(|e| format!("serialize trace event: {e}"))?;
        append_blob_writes(&base, &raw, &mut writes);
        self.commit_writes(writes)?;

        Ok(())
    }

    pub fn end_session(&mut self, status: SessionStatus) -> Result<SessionSummary, String> {
        let mut meta = self
            .session_meta
            .clone()
            .ok_or_else(|| "session not started".to_string())?;
        let ended = now_unix_secs();
        meta.ended_at = Some(ended);
        meta.status = status;

        let duration = ended.saturating_sub(meta.started_at);
        self.summary.duration_secs = duration;
        self.summary.model = meta.model.clone();

        let model_name = meta.model.clone().unwrap_or_default();
        self.summary.estimated_cost_usd = pricing::calculate_cost(
            &model_name,
            self.summary.total_input_tokens,
            self.summary.total_output_tokens,
            self.summary.total_cache_read_tokens,
            &self.pricing,
        );

        let mut writes = Vec::new();
        let meta_base = session_base_key(&meta.session_id);
        let meta_raw =
            serde_json::to_vec(&meta).map_err(|e| format!("serialize final metadata: {e}"))?;
        append_blob_writes(&meta_base, &meta_raw, &mut writes);

        let summary_base = summary_base_key(&meta.session_id);
        let summary_raw =
            serde_json::to_vec(&self.summary).map_err(|e| format!("serialize summary: {e}"))?;
        append_blob_writes(&summary_base, &summary_raw, &mut writes);

        // Aggregate daily/monthly counters in fixed-point (1e4 USD).
        let day = day_label(ended);
        let month = month_label(ended);
        self.append_cost_aggregate_writes(&mut writes, &format!("{COSTS_DAILY_PREFIX}{day}"))?;
        self.append_cost_aggregate_writes(&mut writes, &format!("{COSTS_MONTHLY_PREFIX}{month}"))?;

        self.commit_writes(writes)?;
        persist_snapshot_and_sync_wal(&self.db_path, &self.wal_path, &self.db)
            .map_err(|e| format!("persist trace DB {}: {e:?}", self.db_path.display()))?;

        Ok(self.summary.clone())
    }

    pub fn record_paid_operation(&mut self, op: PaidOperation) -> Result<(), String> {
        let mut writes = Vec::new();
        let base = format!("{PAID_OPS_PREFIX}{}", op.operation_id);
        let raw = serde_json::to_vec(&op).map_err(|e| format!("serialize paid operation: {e}"))?;
        append_blob_writes(&base, &raw, &mut writes);

        let day = day_label(op.timestamp);
        writes.push((
            format!("{PAID_DAILY_PREFIX}{day}:{}", op.operation_id),
            op.credits_spent,
        ));

        self.commit_writes(writes)?;
        persist_snapshot_and_sync_wal(&self.db_path, &self.wal_path, &self.db)
            .map_err(|e| format!("persist trace DB {}: {e:?}", self.db_path.display()))?;
        Ok(())
    }

    fn append_cost_aggregate_writes(
        &self,
        writes: &mut Vec<(String, u64)>,
        prefix: &str,
    ) -> Result<(), String> {
        let next_sessions = self
            .read_value_by_key(&format!("{prefix}:sessions"))
            .unwrap_or(0)
            .saturating_add(1);
        let next_input = self
            .read_value_by_key(&format!("{prefix}:input_tokens"))
            .unwrap_or(0)
            .saturating_add(self.summary.total_input_tokens);
        let next_output = self
            .read_value_by_key(&format!("{prefix}:output_tokens"))
            .unwrap_or(0)
            .saturating_add(self.summary.total_output_tokens);
        let next_cache = self
            .read_value_by_key(&format!("{prefix}:cache_tokens"))
            .unwrap_or(0)
            .saturating_add(self.summary.total_cache_read_tokens);

        let cost_x10000 = (self.summary.estimated_cost_usd * 10_000.0).round() as u64;
        let next_cost = self
            .read_value_by_key(&format!("{prefix}:cost_x10000"))
            .unwrap_or(0)
            .saturating_add(cost_x10000);

        writes.push((format!("{prefix}:sessions"), next_sessions));
        writes.push((format!("{prefix}:input_tokens"), next_input));
        writes.push((format!("{prefix}:output_tokens"), next_output));
        writes.push((format!("{prefix}:cache_tokens"), next_cache));
        writes.push((format!("{prefix}:cost_x10000"), next_cost));
        Ok(())
    }

    fn commit_writes(&mut self, writes: Vec<(String, u64)>) -> Result<(), String> {
        if writes.is_empty() {
            return Ok(());
        }
        let delta = writes
            .into_iter()
            .map(|(key, value)| {
                let idx = self.db.keymap.get_or_create(&key);
                (idx, value)
            })
            .collect();

        self.db
            .commit(Delta::new(delta), &[])
            .map_err(|e| format!("commit trace data: {e:?}"))?;
        Ok(())
    }

    fn read_value_by_key(&self, key: &str) -> Option<u64> {
        let idx = self.db.keymap.get(key)?;
        self.db.state.values.get(idx).copied()
    }
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn list_sessions(db_path: &Path) -> Result<Vec<SessionMetadata>, String> {
    let db = load_db(db_path)?;
    let mut out = Vec::new();

    for (key, _) in db.keymap.all_keys() {
        if !key.starts_with(SESSION_PREFIX) || !key.ends_with(":len") {
            continue;
        }
        let base = key.trim_end_matches(":len");
        let blob = read_blob(&db, base)?;
        if let Some(bytes) = blob {
            if let Ok(meta) = serde_json::from_slice::<SessionMetadata>(&bytes) {
                out.push(meta);
            }
        }
    }

    out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(out)
}

pub fn session_summary(db_path: &Path, session_id: &str) -> Result<Option<SessionSummary>, String> {
    let db = load_db(db_path)?;
    let base = summary_base_key(session_id);
    let Some(bytes) = read_blob(&db, &base)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| format!("parse session summary {session_id}: {e}"))
}

pub fn session_events(db_path: &Path, session_id: &str) -> Result<Vec<TraceEvent>, String> {
    let db = load_db(db_path)?;
    let prefix = format!("{EVENT_PREFIX}{session_id}:");
    let mut seqs = Vec::new();

    for (key, _idx) in db.keymap.all_keys() {
        if key.starts_with(&prefix) && key.ends_with(":len") {
            let raw = key.trim_end_matches(":len");
            if let Some((_, seq_part)) = raw.rsplit_once(':') {
                if let Ok(seq) = seq_part.parse::<u64>() {
                    seqs.push(seq);
                }
            }
        }
    }

    seqs.sort_unstable();
    seqs.dedup();

    let mut out = Vec::new();
    for seq in seqs {
        let base = event_base_key(session_id, seq);
        if let Some(bytes) = read_blob(&db, &base)? {
            if let Ok(ev) = serde_json::from_slice::<TraceEvent>(&bytes) {
                out.push(ev);
            }
        }
    }

    Ok(out)
}

pub fn cost_buckets(db_path: &Path, monthly: bool) -> Result<Vec<CostBucket>, String> {
    let db = load_db(db_path)?;
    let prefix = if monthly {
        COSTS_MONTHLY_PREFIX
    } else {
        COSTS_DAILY_PREFIX
    };

    let mut labels = std::collections::BTreeSet::new();
    for (key, _idx) in db.keymap.all_keys() {
        if let Some(rest) = key.strip_prefix(prefix) {
            if let Some((label, _suffix)) = rest.split_once(':') {
                labels.insert(label.to_string());
            }
        }
    }

    let mut out = Vec::new();
    for label in labels {
        let root = format!("{prefix}{label}");
        let sessions = read_value_by_key_from_db(&db, &format!("{root}:sessions")).unwrap_or(0);
        if sessions == 0 {
            continue;
        }
        let input = read_value_by_key_from_db(&db, &format!("{root}:input_tokens")).unwrap_or(0);
        let output = read_value_by_key_from_db(&db, &format!("{root}:output_tokens")).unwrap_or(0);
        let cache = read_value_by_key_from_db(&db, &format!("{root}:cache_tokens")).unwrap_or(0);
        let cost_x10000 =
            read_value_by_key_from_db(&db, &format!("{root}:cost_x10000")).unwrap_or(0);

        out.push(CostBucket {
            label,
            sessions,
            input_tokens: input,
            output_tokens: output,
            cache_tokens: cache,
            cost_usd: (cost_x10000 as f64) / 10_000.0,
        });
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    Ok(out)
}

pub fn paid_operations(db_path: &Path) -> Result<Vec<PaidOperation>, String> {
    let db = load_db(db_path)?;
    let mut out = Vec::new();
    for (key, _) in db.keymap.all_keys() {
        if !key.starts_with(PAID_OPS_PREFIX) || !key.ends_with(":len") {
            continue;
        }
        let base = key.trim_end_matches(":len");
        if let Some(bytes) = read_blob(&db, base)? {
            if let Ok(op) = serde_json::from_slice::<PaidOperation>(&bytes) {
                out.push(op);
            }
        }
    }
    out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(out)
}

pub fn paid_cost_buckets(db_path: &Path, monthly: bool) -> Result<Vec<PaidCostBucket>, String> {
    let ops = paid_operations(db_path)?;
    let mut by_label: BTreeMap<String, (u64, u64, f64)> = BTreeMap::new();
    for op in ops {
        if !op.success {
            continue;
        }
        let label = if monthly {
            month_label(op.timestamp)
        } else {
            day_label(op.timestamp)
        };
        let entry = by_label.entry(label).or_insert((0, 0, 0.0));
        entry.0 += 1;
        entry.1 = entry.1.saturating_add(op.credits_spent);
        entry.2 += op.usd_equivalent;
    }

    Ok(by_label
        .into_iter()
        .map(
            |(label, (operations, credits_spent, usd_spent))| PaidCostBucket {
                label,
                operations,
                credits_spent,
                usd_spent,
            },
        )
        .collect())
}

pub fn paid_breakdown_by_operation_type(
    db_path: &Path,
) -> Result<Vec<(String, u64, u64, f64)>, String> {
    let ops = paid_operations(db_path)?;
    let mut by_type: BTreeMap<String, (u64, u64, f64)> = BTreeMap::new();
    for op in ops {
        if !op.success {
            continue;
        }
        let entry = by_type
            .entry(op.operation_type.clone())
            .or_insert((0, 0, 0.0));
        entry.0 += 1;
        entry.1 = entry.1.saturating_add(op.credits_spent);
        entry.2 += op.usd_equivalent;
    }
    Ok(by_type
        .into_iter()
        .map(|(kind, (count, credits, usd))| (kind, count, credits, usd))
        .collect())
}

pub fn record_paid_operation_for_halo(
    operation_type: &str,
    credits_spent: u64,
    session_id: Option<String>,
    result_digest: Option<String>,
    success: bool,
    error: Option<String>,
) -> Result<(), String> {
    let db_path = config::db_path();
    let mut writer = TraceWriter::new(&db_path)?;
    writer.record_paid_operation(PaidOperation {
        operation_id: uuid::Uuid::new_v4().to_string(),
        timestamp: now_unix_secs(),
        operation_type: operation_type.to_string(),
        credits_spent,
        usd_equivalent: (credits_spent as f64) * 0.01,
        session_id,
        result_digest,
        success,
        error,
    })
}

fn load_db(db_path: &Path) -> Result<NucleusDb, String> {
    if !db_path.exists() {
        return Ok(NucleusDb::new(
            State::new(vec![]),
            VcBackend::BinaryMerkle,
            halo_witness_cfg(),
        ));
    }
    load_snapshot(db_path, halo_witness_cfg())
        .map_err(|e| format!("load snapshot {}: {e:?}", db_path.display()))
}

fn halo_witness_cfg() -> WitnessConfig {
    let mut cfg = default_witness_cfg();
    cfg.signing_algorithm = WitnessSignatureAlgorithm::MlDsa65;
    cfg
}

fn append_blob_writes(base_key: &str, payload: &[u8], writes: &mut Vec<(String, u64)>) {
    writes.push((format!("{base_key}:len"), payload.len() as u64));
    let chunks = bytes_to_u64_chunks(payload);
    for (idx, chunk) in chunks.iter().enumerate() {
        writes.push((format!("{base_key}:chunk:{idx}"), *chunk));
    }
}

fn read_blob(db: &NucleusDb, base_key: &str) -> Result<Option<Vec<u8>>, String> {
    let len_key = format!("{base_key}:len");
    let Some(len) = read_value_by_key_from_db(db, &len_key) else {
        return Ok(None);
    };
    let len = len as usize;
    let chunk_count = len.div_ceil(8);

    let mut chunks = Vec::with_capacity(chunk_count);
    for idx in 0..chunk_count {
        let key = format!("{base_key}:chunk:{idx}");
        let chunk = read_value_by_key_from_db(db, &key)
            .ok_or_else(|| format!("missing chunk key {key}"))?;
        chunks.push(chunk);
    }

    let mut bytes = u64_chunks_to_bytes(&chunks);
    bytes.truncate(len);
    Ok(Some(bytes))
}

fn read_value_by_key_from_db(db: &NucleusDb, key: &str) -> Option<u64> {
    let idx = db.keymap.get(key)?;
    db.state.values.get(idx).copied()
}

fn bytes_to_u64_chunks(bytes: &[u8]) -> Vec<u64> {
    let mut out = Vec::with_capacity(bytes.len().div_ceil(8));
    for chunk in bytes.chunks(8) {
        let mut arr = [0u8; 8];
        arr[..chunk.len()].copy_from_slice(chunk);
        out.push(u64::from_le_bytes(arr));
    }
    out
}

fn u64_chunks_to_bytes(chunks: &[u64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunks.len() * 8);
    for chunk in chunks {
        out.extend_from_slice(&chunk.to_le_bytes());
    }
    out
}

fn event_content_hash(content: &serde_json::Value) -> Result<String, String> {
    let bytes =
        serde_json::to_vec(content).map_err(|e| format!("serialize content hash input: {e}"))?;
    let digest = Sha256::digest(&bytes);
    Ok(hex_encode(digest.as_slice()))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn session_base_key(session_id: &str) -> String {
    format!("{SESSION_PREFIX}{session_id}")
}

fn event_base_key(session_id: &str, seq: u64) -> String {
    format!("{EVENT_PREFIX}{session_id}:{seq}")
}

fn summary_base_key(session_id: &str) -> String {
    format!("{SUMMARY_PREFIX}{session_id}")
}

fn day_label(ts: u64) -> String {
    if let Some(dt) = chrono::DateTime::from_timestamp(ts as i64, 0) {
        dt.format("%Y-%m-%d").to_string()
    } else {
        "1970-01-01".to_string()
    }
}

fn month_label(ts: u64) -> String {
    if let Some(dt) = chrono::DateTime::from_timestamp(ts as i64, 0) {
        dt.format("%Y-%m").to_string()
    } else {
        "1970-01".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::schema::PaidOperation;

    fn temp_db_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agenthalo_paid_{tag}_{}_{}.ndb",
            std::process::id(),
            now_unix_secs()
        ))
    }

    #[test]
    fn paid_operation_recorded() {
        let db_path = temp_db_path("record");
        let mut writer = TraceWriter::new(&db_path).expect("writer");
        writer
            .record_paid_operation(PaidOperation {
                operation_id: "op-1".to_string(),
                timestamp: now_unix_secs(),
                operation_type: "attest".to_string(),
                credits_spent: 10,
                usd_equivalent: 0.10,
                session_id: Some("sess-1".to_string()),
                result_digest: Some("abcd".to_string()),
                success: true,
                error: None,
            })
            .expect("record paid op");

        let ops = paid_operations(&db_path).expect("read paid ops");
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].operation_type, "attest");
        assert_eq!(ops[0].credits_spent, 10);
    }

    #[test]
    fn daily_aggregation() {
        let db_path = temp_db_path("daily");
        let base_ts = now_unix_secs();
        let mut writer = TraceWriter::new(&db_path).expect("writer");

        for (idx, credits) in [10u64, 20u64].iter().enumerate() {
            writer
                .record_paid_operation(PaidOperation {
                    operation_id: format!("op-{idx}"),
                    timestamp: base_ts,
                    operation_type: "audit_small".to_string(),
                    credits_spent: *credits,
                    usd_equivalent: (*credits as f64) * 0.01,
                    session_id: None,
                    result_digest: None,
                    success: true,
                    error: None,
                })
                .expect("record paid op");
        }

        let buckets = paid_cost_buckets(&db_path, false).expect("daily buckets");
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0].operations, 2);
        assert_eq!(buckets[0].credits_spent, 30);
    }
}
