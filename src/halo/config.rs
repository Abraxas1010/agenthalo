use std::path::PathBuf;

pub fn halo_dir() -> PathBuf {
    if let Ok(p) = std::env::var("AGENTHALO_HOME") {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agenthalo")
}

pub fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("AGENTHALO_DB_PATH") {
        return PathBuf::from(p);
    }
    halo_dir().join("traces.ndb")
}

pub fn credentials_path() -> PathBuf {
    halo_dir().join("credentials.json")
}

pub fn pricing_path() -> PathBuf {
    halo_dir().join("pricing.json")
}

pub fn ensure_halo_dir() -> Result<(), String> {
    std::fs::create_dir_all(halo_dir()).map_err(|e| format!("create halo dir: {e}"))
}
