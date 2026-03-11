use crate::container::launcher::{Channel, MonitorConfig};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildConfig {
    pub image: String,
    pub channels: Vec<Channel>,
    pub agent_id: String,
    pub release: bool,
    pub output_dir: PathBuf,
}

fn dockerfile_text(release: bool) -> String {
    let base = if release {
        "gcr.io/distroless/static-debian12"
    } else {
        "alpine:3.19"
    };
    format!(
        "FROM {base}\n\
         COPY nucleusdb-sidecar /usr/local/bin/nucleusdb-sidecar\n\
         COPY nucleusdb-agent.json /etc/nucleusdb/config.json\n\
         ENTRYPOINT [\"/usr/local/bin/nucleusdb-sidecar\"]\n"
    )
}

pub fn build_container_image(cfg: &BuildConfig) -> Result<PathBuf, String> {
    std::fs::create_dir_all(&cfg.output_dir).map_err(|e| {
        format!(
            "failed to create output dir {}: {e}",
            cfg.output_dir.display()
        )
    })?;
    let monitor = MonitorConfig {
        channels: cfg.channels.clone(),
        agent_id: cfg.agent_id.clone(),
        max_nesting_depth: 3,
    };
    let config_path = cfg.output_dir.join("nucleusdb-agent.json");
    let dockerfile_path = cfg.output_dir.join("Dockerfile");
    std::fs::write(
        &config_path,
        serde_json::to_vec_pretty(&monitor).map_err(|e| format!("config encoding failed: {e}"))?,
    )
    .map_err(|e| format!("failed to write {}: {e}", config_path.display()))?;
    std::fs::write(&dockerfile_path, dockerfile_text(cfg.release))
        .map_err(|e| format!("failed to write {}: {e}", dockerfile_path.display()))?;

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let labels = vec![
        format!("nucleusdb.channels={}", monitor.channels_csv()),
        format!("nucleusdb.built_at_unix={stamp}"),
    ];
    let mut cmd = Command::new("docker");
    cmd.arg("build")
        .arg("-t")
        .arg(&cfg.image)
        .arg("-f")
        .arg(&dockerfile_path);
    for label in labels {
        cmd.arg("--label").arg(label);
    }
    cmd.arg(&cfg.output_dir);
    let out = cmd.output();
    match out {
        Ok(result) if result.status.success() => Ok(cfg.output_dir.clone()),
        Ok(result) => Err(format!(
            "docker build failed: {}",
            String::from_utf8_lossy(&result.stderr)
        )),
        Err(e) => Err(format!("failed to run docker build: {e}")),
    }
}

pub fn parse_channel_list(raw: &str) -> Result<Vec<Channel>, String> {
    let mut out = Vec::new();
    for item in raw.split(',') {
        let v = item.trim().to_ascii_lowercase();
        if v.is_empty() {
            continue;
        }
        let ch = match v.as_str() {
            "everything" | "all" => Channel::Everything,
            "chat" => Channel::Chat,
            "payments" => Channel::Payments,
            "tools" => Channel::Tools,
            "state" => Channel::State,
            _ => return Err(format!("unknown channel '{item}'")),
        };
        out.push(ch);
    }
    if out.is_empty() {
        return Err("no channels selected".to_string());
    }
    Ok(out)
}

pub fn default_build_dir(db_root: &Path) -> PathBuf {
    db_root.join(".nucleusdb_container")
}
