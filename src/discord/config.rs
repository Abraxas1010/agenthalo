#[derive(Clone, Debug)]
pub struct DiscordConfig {
    pub token: String,
    pub db_path: String,
    pub channels: Option<Vec<u64>>,
    pub batch_size: usize,
    pub batch_timeout_secs: u64,
    pub record_bots: bool,
    pub record_edits: bool,
    pub record_deletes: bool,
    pub dry_run: bool,
}

impl DiscordConfig {
    pub fn from_env(dry_run: bool) -> Result<Self, String> {
        let token = std::env::var("NUCLEUSDB_DISCORD_TOKEN").unwrap_or_default();
        if token.trim().is_empty() && !dry_run {
            return Err("NUCLEUSDB_DISCORD_TOKEN is required".to_string());
        }
        let channels = std::env::var("NUCLEUSDB_DISCORD_CHANNELS")
            .ok()
            .and_then(|raw| {
                let raw = raw.trim().to_string();
                if raw.is_empty() || raw.eq_ignore_ascii_case("all") {
                    None
                } else {
                    Some(
                        raw.split(',')
                            .filter_map(|s| s.trim().parse::<u64>().ok())
                            .collect(),
                    )
                }
            });
        Ok(Self {
            token,
            db_path: crate::config::discord_db_path().display().to_string(),
            channels,
            batch_size: std::env::var("NUCLEUSDB_DISCORD_BATCH_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
            batch_timeout_secs: std::env::var("NUCLEUSDB_DISCORD_BATCH_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            record_bots: std::env::var("NUCLEUSDB_DISCORD_RECORD_BOTS")
                .ok()
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            record_edits: std::env::var("NUCLEUSDB_DISCORD_RECORD_EDITS")
                .ok()
                .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
                .unwrap_or(true),
            record_deletes: std::env::var("NUCLEUSDB_DISCORD_RECORD_DELETES")
                .ok()
                .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"))
                .unwrap_or(true),
            dry_run,
        })
    }

    pub fn should_record_channel(&self, channel_id: u64) -> bool {
        self.channels
            .as_ref()
            .map(|ids| ids.contains(&channel_id))
            .unwrap_or(true)
    }
}
