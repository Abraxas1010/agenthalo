use crate::discord::schema::DiscordBotStatus;

pub fn load_status() -> Result<DiscordBotStatus, String> {
    let path = crate::config::discord_status_path();
    if !path.exists() {
        return Ok(DiscordBotStatus::default());
    }
    let raw =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse {}: {e}", path.display()))
}

pub fn save_status(status: &DiscordBotStatus) -> Result<(), String> {
    crate::config::ensure_nucleusdb_dir()?;
    let path = crate::config::discord_status_path();
    let raw = serde_json::to_string_pretty(status).map_err(|e| format!("serialize status: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("write {}: {e}", path.display()))
}
