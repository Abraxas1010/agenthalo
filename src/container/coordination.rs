use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_MESH_REGISTRY_VOLUME: &str = "mesh";
const LOCAL_MESH_AUTH_TOKEN_PATH: &str = "mesh/auth_token";
const SHARED_BIND_DIR_MODE: u32 = 0o1777;
const PRIVATE_DIR_MODE: u32 = 0o700;
const PRIVATE_LOCK_FILE_MODE: u32 = 0o600;

pub fn mesh_auth_token() -> Option<String> {
    std::env::var("NUCLEUSDB_MESH_AUTH_TOKEN")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            std::env::var("AGENTHALO_MCP_SECRET")
                .ok()
                .filter(|v| !v.trim().is_empty())
        })
        .or_else(|| local_mesh_auth_token().ok())
}

pub fn registry_volume_is_named(path: &Path) -> bool {
    !path.is_absolute()
}

pub fn resolve_registry_dir(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        crate::halo::config::halo_dir().join(path)
    }
}

pub fn prepare_bind_mount_dir(path: &Path, context: &str) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|e| format!("create {context} {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ =
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(SHARED_BIND_DIR_MODE));
    }
    Ok(())
}

pub fn prepare_named_volume(volume: &Path, image: &str, context: &str) -> Result<(), String> {
    let _ = image;
    let target = resolve_registry_dir(volume);
    prepare_bind_mount_dir(&target, context)
}

fn local_mesh_auth_token() -> Result<String, String> {
    let path = crate::halo::config::halo_dir().join(LOCAL_MESH_AUTH_TOKEN_PATH);
    if let Ok(existing) = read_nonempty_token(&path) {
        return Ok(existing);
    }

    if let Some(parent) = path.parent() {
        prepare_private_dir(parent, "mesh auth dir")?;
    }

    let lock = lock_path(&path);
    let _guard = acquire_pid_lock(
        &lock,
        Duration::from_secs(2),
        Duration::from_millis(20),
        "mesh auth token",
    )?;

    if let Ok(existing) = read_nonempty_token(&path) {
        return Ok(existing);
    }

    let token = uuid::Uuid::new_v4().simple().to_string();
    write_private_file(&path, token.as_bytes(), "mesh auth token")?;
    Ok(token)
}

fn read_nonempty_token(path: &Path) -> Result<String, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read mesh auth token {}: {e}", path.display()))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(format!("mesh auth token {} is empty", path.display()));
    }
    Ok(trimmed.to_string())
}

fn write_private_file(path: &Path, data: &[u8], context: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        prepare_private_dir(parent, context)?;
    }
    std::fs::write(path, data).map_err(|e| format!("write {context} {}: {e}", path.display()))?;
    set_private_file_permissions(path);
    Ok(())
}

fn lock_path(path: &Path) -> PathBuf {
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    PathBuf::from(lock)
}

fn prepare_private_dir(path: &Path, context: &str) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|e| format!("create {context} {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(PRIVATE_DIR_MODE));
    }
    Ok(())
}

pub fn acquire_pid_lock(
    lock_path: &Path,
    timeout: Duration,
    retry: Duration,
    context: &str,
) -> Result<File, String> {
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create {context} dir {}: {e}", parent.display()))?;
    }
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(lock_path)
        {
            Ok(mut file) => {
                set_private_file_permissions(lock_path);
                let _ = writeln!(file, "pid={}", std::process::id());
                let _ = file.flush();
                return Ok(file);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                if stale_lock_path(lock_path, retry) {
                    let _ = std::fs::remove_file(lock_path);
                    continue;
                }
                if std::time::Instant::now() >= deadline {
                    return Err(format!(
                        "timed out acquiring {context} lock {}",
                        lock_path.display()
                    ));
                }
                std::thread::sleep(retry);
            }
            Err(err) => {
                return Err(format!(
                    "open {context} lock {}: {err}",
                    lock_path.display()
                ));
            }
        }
    }
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(
        path,
        std::fs::Permissions::from_mode(PRIVATE_LOCK_FILE_MODE),
    );
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) {}

fn stale_lock_path(lock_path: &Path, retry: Duration) -> bool {
    let raw = match std::fs::read_to_string(lock_path) {
        Ok(raw) => raw,
        Err(_) => return stale_unparseable_lock(lock_path, retry),
    };
    let pid = raw
        .lines()
        .find_map(|line| line.strip_prefix("pid="))
        .and_then(|value| value.trim().parse::<u32>().ok());
    match pid {
        Some(pid) => !pid_is_alive(pid),
        None => stale_unparseable_lock(lock_path, retry),
    }
}

fn stale_unparseable_lock(lock_path: &Path, retry: Duration) -> bool {
    let modified = match std::fs::metadata(lock_path).and_then(|meta| meta.modified()) {
        Ok(modified) => modified,
        Err(_) => return false,
    };
    match modified.elapsed() {
        Ok(elapsed) => elapsed > retry.saturating_mul(4),
        Err(_) => false,
    }
}

#[cfg(unix)]
fn pid_is_alive(pid: u32) -> bool {
    let rc = unsafe { libc::kill(pid as i32, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn pid_is_alive(_pid: u32) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{lock_env, EnvVarGuard};
    use std::io::Write;

    #[test]
    fn mesh_auth_token_prefers_explicit_mesh_secret() {
        let _guard = lock_env();
        let _mesh = EnvVarGuard::set("NUCLEUSDB_MESH_AUTH_TOKEN", Some("mesh-secret"));
        let _mcp = EnvVarGuard::set("AGENTHALO_MCP_SECRET", Some("mcp-secret"));
        assert_eq!(mesh_auth_token().as_deref(), Some("mesh-secret"));
    }

    #[test]
    fn mesh_auth_token_generates_local_secret_when_env_missing() {
        let _guard = lock_env();
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set(
            "AGENTHALO_HOME",
            Some(home.path().to_str().expect("utf8 home")),
        );
        let _mesh = EnvVarGuard::set("NUCLEUSDB_MESH_AUTH_TOKEN", None);
        let _mcp = EnvVarGuard::set("AGENTHALO_MCP_SECRET", None);

        let first = mesh_auth_token().expect("generated token");
        let second = mesh_auth_token().expect("stable token");
        assert_eq!(first, second);
        assert_eq!(first.len(), 32);

        let saved = std::fs::read_to_string(home.path().join(LOCAL_MESH_AUTH_TOKEN_PATH))
            .expect("saved token");
        assert_eq!(saved.trim(), first);
    }

    #[test]
    fn acquire_pid_lock_reclaims_dead_pid_lock() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("registry.lock");
        {
            let mut file = File::create(&path).expect("stale lock");
            writeln!(file, "pid=999999").expect("write pid");
        }
        let file = acquire_pid_lock(
            &path,
            Duration::from_millis(50),
            Duration::from_millis(5),
            "test registry",
        )
        .expect("acquire reclaimed lock");
        drop(file);
        let raw = std::fs::read_to_string(&path).expect("read lock");
        assert!(raw.contains("pid="));
        assert!(!raw.contains("999999"));
    }
}
