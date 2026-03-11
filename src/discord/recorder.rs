use crate::discord::schema::{
    DiscordAttachment, DiscordBotStatus, DiscordChannelStatus, DiscordMessageRecord,
};
use crate::typed_value::TypedValue;
use chrono::Utc;
use serenity::all::{ChannelId, Message, MessageId};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct DiscordRecorder {
    pub db_path: std::path::PathBuf,
}

impl DiscordRecorder {
    pub fn new(db_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    pub fn ensure_db(&self) -> Result<(), String> {
        let path = &self.db_path;
        let wal_path = crate::persistence::default_wal_path(path);
        if path.exists() {
            return Ok(());
        }
        let mut db = crate::protocol::NucleusDb::new(
            crate::state::State::new(vec![]),
            crate::protocol::VcBackend::BinaryMerkle,
            crate::cli::default_witness_cfg(),
        );
        db.set_append_only();
        db.save_persistent(path)
            .map_err(|e| format!("save snapshot {}: {e:?}", path.display()))?;
        crate::persistence::init_wal(&wal_path, &db)
            .map_err(|e| format!("init WAL {}: {e:?}", wal_path.display()))?;
        Ok(())
    }

    pub fn record_messages(&self, records: &[DiscordMessageRecord]) -> Result<(), String> {
        let entries = records
            .iter()
            .map(|record| {
                let key = format!("msg:{}:{}", record.channel_id, record.message_id);
                let payload =
                    serde_json::to_value(record).map_err(|e| format!("serialize message: {e}"))?;
                Ok((key, payload))
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.record_json_entries(&entries)
    }

    pub fn record_edit(&self, record: &DiscordMessageRecord) -> Result<(), String> {
        self.record_edits(std::slice::from_ref(record))
    }

    pub fn record_delete(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
    ) -> Result<(), String> {
        self.record_deletes(&[(channel_id, message_id)])
    }

    pub fn record_edits(&self, records: &[DiscordMessageRecord]) -> Result<(), String> {
        let entries = records
            .iter()
            .map(|record| {
                let key = format!(
                    "edit:{}:{}:{}",
                    record.channel_id,
                    record.message_id,
                    Utc::now().timestamp()
                );
                let payload = serde_json::to_value(record)
                    .map_err(|e| format!("serialize edit payload: {e}"))?;
                Ok((key, payload))
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.record_json_entries(&entries)
    }

    pub fn record_deletes(&self, records: &[(ChannelId, MessageId)]) -> Result<(), String> {
        let entries = records
            .iter()
            .map(|(channel_id, message_id)| {
                let payload = serde_json::json!({
                    "channel_id": channel_id.to_string(),
                    "message_id": message_id.to_string(),
                    "deleted_at": Utc::now().to_rfc3339(),
                });
                Ok((
                    format!(
                        "del:{}:{}:{}",
                        channel_id,
                        message_id,
                        Utc::now().timestamp()
                    ),
                    payload,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.record_json_entries(&entries)
    }

    pub fn recorded_channels(&self) -> Result<Vec<DiscordChannelStatus>, String> {
        let db = self.load_db()?;
        let mut channels: BTreeMap<String, DiscordChannelStatus> = BTreeMap::new();
        for (key, _) in db.keymap.all_keys() {
            if !key.starts_with("msg:") {
                continue;
            }
            let Some(TypedValue::Json(doc)) = db.get_typed(key) else {
                continue;
            };
            let record: DiscordMessageRecord = serde_json::from_value(doc)
                .map_err(|e| format!("parse message record {key}: {e}"))?;
            let entry = channels
                .entry(record.channel_id.clone())
                .or_insert_with(|| DiscordChannelStatus {
                    channel_id: record.channel_id.clone(),
                    channel_name: record.channel_name.clone(),
                    records: 0,
                    last_message_id: None,
                });
            entry.channel_name = record.channel_name.clone();
            entry.records += 1;
            if entry
                .last_message_id
                .as_ref()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
                < record.message_id.parse::<u64>().unwrap_or(0)
            {
                entry.last_message_id = Some(record.message_id.clone());
            }
        }
        Ok(channels.into_values().collect())
    }

    pub fn last_recorded_message_id(
        &self,
        channel_id: ChannelId,
    ) -> Result<Option<MessageId>, String> {
        let db = self.load_db()?;
        let prefix = format!("msg:{}:", channel_id);
        let mut best = None;
        for (key, _) in db.keymap.all_keys() {
            if !key.starts_with(&prefix) {
                continue;
            }
            if let Some(raw) = key.rsplit(':').next() {
                if let Ok(value) = raw.parse::<u64>() {
                    if best.map(|cur| value > cur).unwrap_or(true) {
                        best = Some(value);
                    }
                }
            }
        }
        Ok(best.map(MessageId::new))
    }

    pub fn search(
        &self,
        query: &str,
        channel_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<DiscordMessageRecord>, String> {
        let mut rows = Vec::new();
        let query = query.to_ascii_lowercase();
        let db = self.load_db()?;
        for (key, _) in db.keymap.all_keys() {
            if !key.starts_with("msg:") {
                continue;
            }
            if let Some(cid) = channel_id {
                if !key.starts_with(&format!("msg:{cid}:")) {
                    continue;
                }
            }
            let Some(TypedValue::Json(doc)) = db.get_typed(key) else {
                continue;
            };
            let record: DiscordMessageRecord = serde_json::from_value(doc)
                .map_err(|e| format!("parse message record {key}: {e}"))?;
            let haystack = format!(
                "{} {} {} {}",
                record.author_name,
                record.channel_name,
                record.content,
                record.mentions.join(" ")
            )
            .to_ascii_lowercase();
            if haystack.contains(&query) {
                rows.push(record);
                if rows.len() >= limit {
                    break;
                }
            }
        }
        Ok(rows)
    }

    pub fn recent(
        &self,
        channel_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<DiscordMessageRecord>, String> {
        let db = self.load_db()?;
        let mut rows = Vec::new();
        for (key, _) in db.keymap.all_keys() {
            if !key.starts_with("msg:") {
                continue;
            }
            if let Some(cid) = channel_id {
                if !key.starts_with(&format!("msg:{cid}:")) {
                    continue;
                }
            }
            let Some(TypedValue::Json(doc)) = db.get_typed(key) else {
                continue;
            };
            let record: DiscordMessageRecord = serde_json::from_value(doc)
                .map_err(|e| format!("parse message record {key}: {e}"))?;
            rows.push(record);
        }
        rows.sort_by_key(|record| record.message_id.parse::<u64>().unwrap_or(0));
        rows.reverse();
        rows.truncate(limit);
        Ok(rows)
    }

    pub fn export_channel(&self, channel_id: &str) -> Result<Vec<DiscordMessageRecord>, String> {
        let db = self.load_db()?;
        let prefix = format!("msg:{channel_id}:");
        let mut rows = Vec::new();
        for (key, _) in db.keymap.all_keys() {
            if !key.starts_with(&prefix) {
                continue;
            }
            let Some(TypedValue::Json(doc)) = db.get_typed(key) else {
                continue;
            };
            let record: DiscordMessageRecord = serde_json::from_value(doc)
                .map_err(|e| format!("parse message record {key}: {e}"))?;
            rows.push(record);
        }
        rows.sort_by_key(|record| record.message_id.parse::<u64>().unwrap_or(0));
        Ok(rows)
    }

    pub fn verify_message(
        &self,
        channel_id: &str,
        message_id: &str,
    ) -> Result<Option<(bool, u64)>, String> {
        let db = self.load_db()?;
        let key = format!("msg:{channel_id}:{message_id}");
        let Some(idx) = db.keymap.get(&key) else {
            return Ok(None);
        };
        let Some((value, proof, root)) = db.query(idx) else {
            return Ok(None);
        };
        Ok(Some((db.verify_query(idx, value, &proof, root), value)))
    }

    pub fn integrity_summary(&self) -> Result<(bool, usize), String> {
        let db = self.load_db()?;
        Ok((
            matches!(db.write_mode(), crate::immutable::WriteMode::AppendOnly),
            db.monotone_seals().len(),
        ))
    }

    pub fn rebuild_status(&self, connected: bool, guilds: usize) -> Result<(), String> {
        let channels = self.recorded_channels()?;
        let pending_messages = crate::discord::status::load_status()
            .unwrap_or_default()
            .pending_messages;
        crate::discord::status::save_status(&DiscordBotStatus {
            connected,
            guilds,
            channels,
            last_commit_at: Some(Utc::now().to_rfc3339()),
            pending_messages,
        })
    }

    pub fn update_status<F>(&self, updater: F) -> Result<(), String>
    where
        F: FnOnce(&mut DiscordBotStatus),
    {
        let mut status = crate::discord::status::load_status().unwrap_or_default();
        updater(&mut status);
        crate::discord::status::save_status(&status)
    }

    fn refresh_status_preserving_flags(&self) -> Result<(), String> {
        let existing = crate::discord::status::load_status().unwrap_or_default();
        let channels = self.recorded_channels()?;
        crate::discord::status::save_status(&DiscordBotStatus {
            connected: existing.connected,
            guilds: existing.guilds,
            channels,
            last_commit_at: Some(Utc::now().to_rfc3339()),
            pending_messages: existing.pending_messages,
        })
    }

    fn record_json_entries(&self, entries: &[(String, serde_json::Value)]) -> Result<(), String> {
        if entries.is_empty() {
            return Ok(());
        }
        self.ensure_db()?;
        let mut db = self.load_db()?;
        if !matches!(db.write_mode(), crate::immutable::WriteMode::AppendOnly) {
            db.set_append_only();
        }
        let mut writes = Vec::with_capacity(entries.len());
        for (key, payload) in entries {
            let (idx, cell) = db
                .put_typed(key, TypedValue::Json(payload.clone()))
                .map_err(|e| format!("put typed: {e}"))?;
            writes.push((idx, cell));
        }
        db.commit(crate::state::Delta::new(writes), &[])
            .map_err(|e| format!("commit failed: {e:?}"))?;
        self.persist_db(&db)?;
        self.refresh_status_preserving_flags()?;
        Ok(())
    }

    fn load_db(&self) -> Result<crate::protocol::NucleusDb, String> {
        self.ensure_db()?;
        crate::protocol::NucleusDb::load_persistent(
            &self.db_path,
            crate::cli::default_witness_cfg(),
        )
        .map_err(|e| format!("load snapshot {}: {e:?}", self.db_path.display()))
    }

    fn persist_db(&self, db: &crate::protocol::NucleusDb) -> Result<(), String> {
        let wal_path = crate::persistence::default_wal_path(&self.db_path);
        crate::persistence::persist_snapshot_and_sync_wal(&self.db_path, &wal_path, db)
            .map_err(|e| format!("persist: {e:?}"))
    }
}

pub fn seal_record(record: &DiscordMessageRecord) -> Result<String, String> {
    let mut canonical_record = record.clone();
    canonical_record.record_seal.clear();
    let canonical = serde_json::to_vec(&canonical_record)
        .map_err(|e| format!("serialize discord record: {e}"))?;
    Ok(crate::util::hex_encode(&Sha256::digest(&canonical)))
}

pub fn from_message(
    message: &Message,
    channel_name: String,
    guild_name: Option<String>,
) -> DiscordMessageRecord {
    let mut record = DiscordMessageRecord {
        message_id: message.id.to_string(),
        channel_id: message.channel_id.to_string(),
        channel_name,
        guild_id: message.guild_id.map(|g| g.to_string()),
        guild_name,
        author_id: message.author.id.to_string(),
        author_name: message.author.name.clone(),
        author_discriminator: message.author.discriminator.map(|d| d.to_string()),
        author_bot: message.author.bot,
        content: message.content.clone(),
        timestamp: message.timestamp.to_string(),
        edited_timestamp: message.edited_timestamp.map(|t| t.to_string()),
        attachments: message
            .attachments
            .iter()
            .map(|a| DiscordAttachment {
                id: a.id.to_string(),
                filename: a.filename.clone(),
                size: u64::from(a.size),
                url: a.url.clone(),
                content_type: a.content_type.clone(),
            })
            .collect(),
        embeds_count: message.embeds.len(),
        mentions: message.mentions.iter().map(|m| m.name.clone()).collect(),
        reference_message_id: message
            .referenced_message
            .as_ref()
            .map(|m| m.id.to_string()),
        thread_id: message.thread.as_ref().map(|t| t.id.to_string()),
        reactions: message
            .reactions
            .iter()
            .map(|r| format!("{}:{}", r.reaction_type, r.count))
            .collect(),
        recorded_at: Utc::now().to_rfc3339(),
        record_seal: String::new(),
    };
    record.record_seal = seal_record(&record).expect("discord message record seal");
    record
}
