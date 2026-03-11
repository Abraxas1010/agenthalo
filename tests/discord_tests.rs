use nucleusdb::discord::config::DiscordConfig;
use nucleusdb::discord::recorder::{seal_record, DiscordRecorder};
use nucleusdb::discord::schema::DiscordMessageRecord;
use nucleusdb::protocol::NucleusDb;
use nucleusdb::test_support::lock_env;
use tempfile::TempDir;

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: Option<&str>) -> Self {
        let previous = std::env::var(key).ok();
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var(self.key, value);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn sample_record(message_id: &str, content: &str) -> DiscordMessageRecord {
    let mut record = DiscordMessageRecord {
        message_id: message_id.to_string(),
        channel_id: "123".to_string(),
        channel_name: "general".to_string(),
        guild_id: Some("999".to_string()),
        guild_name: Some("guild".to_string()),
        author_id: "777".to_string(),
        author_name: "tester".to_string(),
        author_discriminator: Some("0420".to_string()),
        author_bot: false,
        content: content.to_string(),
        timestamp: "2026-03-10T00:00:00Z".to_string(),
        edited_timestamp: None,
        attachments: vec![],
        embeds_count: 0,
        mentions: vec!["alice".to_string()],
        reference_message_id: None,
        thread_id: None,
        reactions: vec!["👍:1".to_string()],
        recorded_at: "2026-03-10T00:00:00Z".to_string(),
        record_seal: String::new(),
    };
    record.record_seal = seal_record(&record).expect("seal record");
    record
}

#[test]
fn discord_config_from_env_parses_fields() {
    let _guard = lock_env();
    let _home = EnvVarGuard::set("NUCLEUSDB_HOME", Some("/tmp/nucleusdb-test-home"));
    let _token = EnvVarGuard::set("NUCLEUSDB_DISCORD_TOKEN", Some("token-123"));
    let _channels = EnvVarGuard::set("NUCLEUSDB_DISCORD_CHANNELS", Some("1,2,3"));
    let _batch_size = EnvVarGuard::set("NUCLEUSDB_DISCORD_BATCH_SIZE", Some("25"));
    let _batch_timeout = EnvVarGuard::set("NUCLEUSDB_DISCORD_BATCH_TIMEOUT_SECS", Some("9"));
    let _record_bots = EnvVarGuard::set("NUCLEUSDB_DISCORD_RECORD_BOTS", Some("true"));
    let _record_edits = EnvVarGuard::set("NUCLEUSDB_DISCORD_RECORD_EDITS", Some("false"));
    let _record_deletes = EnvVarGuard::set("NUCLEUSDB_DISCORD_RECORD_DELETES", Some("false"));

    let config = DiscordConfig::from_env(true).expect("discord config");
    assert_eq!(config.token, "token-123");
    assert_eq!(config.channels, Some(vec![1, 2, 3]));
    assert_eq!(config.batch_size, 25);
    assert_eq!(config.batch_timeout_secs, 9);
    assert!(config.record_bots);
    assert!(!config.record_edits);
    assert!(!config.record_deletes);
}

#[test]
fn recorder_round_trip_search_verify_and_integrity() {
    let _guard = lock_env();
    let temp = TempDir::new().expect("tempdir");
    let db_path = temp.path().join("discord.ndb");
    let _home = EnvVarGuard::set("NUCLEUSDB_HOME", temp.path().to_str());
    let recorder = DiscordRecorder::new(&db_path);

    let first = sample_record("1001", "hello discord");
    let second = sample_record("1002", "searchable content");
    recorder
        .record_messages(&[first.clone(), second.clone()])
        .expect("record messages");

    let found = recorder
        .search("searchable", Some("123"), 10)
        .expect("search");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].message_id, "1002");

    let verify = recorder
        .verify_message("123", "1001")
        .expect("verify message")
        .expect("message exists");
    assert!(verify.0);

    let channels = recorder.recorded_channels().expect("recorded channels");
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].records, 2);

    let (append_only, seal_count) = recorder.integrity_summary().expect("integrity summary");
    assert!(append_only);
    assert!(seal_count > 0);
}

#[test]
fn seal_matches_serialized_record() {
    let record = sample_record("2001", "sealed payload");
    let recomputed = seal_record(&DiscordMessageRecord {
        record_seal: String::new(),
        ..record.clone()
    })
    .expect("recompute seal");
    assert_eq!(record.record_seal, recomputed);
}

#[test]
fn batched_edits_and_deletes_persist_entries() {
    let _guard = lock_env();
    let temp = TempDir::new().expect("tempdir");
    let db_path = temp.path().join("discord.ndb");
    let _home = EnvVarGuard::set("NUCLEUSDB_HOME", temp.path().to_str());
    let recorder = DiscordRecorder::new(&db_path);

    let edited = sample_record("3001", "edited payload");
    recorder.record_edits(&[edited]).expect("record edits");
    recorder
        .record_deletes(&[(
            serenity::all::ChannelId::new(123),
            serenity::all::MessageId::new(3001),
        )])
        .expect("record deletes");

    let db = NucleusDb::load_persistent(&db_path, nucleusdb::cli::default_witness_cfg())
        .expect("load db");
    let mut saw_edit = false;
    let mut saw_delete = false;
    for (key, _) in db.keymap.all_keys() {
        saw_edit |= key.starts_with("edit:123:3001:");
        saw_delete |= key.starts_with("del:123:3001:");
    }
    assert!(saw_edit, "expected edit entry");
    assert!(saw_delete, "expected delete entry");
}
