use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContainerBackend {
    Podman,
    Docker,
}

impl ContainerBackend {
    pub fn detect() -> Self {
        if let Some(engine) = std::env::var("NUCLEUSDB_CONTAINER_ENGINE")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
        {
            match engine.as_str() {
                "podman" => return Self::Podman,
                "docker" => return Self::Docker,
                _ => {}
            }
        }
        if binary_in_path("podman") {
            Self::Podman
        } else {
            Self::Docker
        }
    }

    pub fn binary(self) -> &'static str {
        match self {
            Self::Podman => "podman",
            Self::Docker => "docker",
        }
    }

    pub fn command(self) -> Command {
        Command::new(self.binary())
    }

    pub fn default_socket(self) -> String {
        match self {
            Self::Podman => {
                let xdg = std::env::var("XDG_RUNTIME_DIR")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| format!("/run/user/{}", current_uid_fallback()));
                format!("unix://{xdg}/podman/podman.sock")
            }
            Self::Docker => "unix:///var/run/docker.sock".to_string(),
        }
    }
}

impl std::fmt::Display for ContainerBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.binary())
    }
}

fn current_uid_fallback() -> u32 {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() as u32 }
    }
    #[cfg(not(unix))]
    {
        1000
    }
}

fn binary_in_path(name: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| binary_exists_in_dir(&dir, name))
}

fn binary_exists_in_dir(dir: &PathBuf, name: &str) -> bool {
    let candidate = dir.join(name);
    if !candidate.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = match candidate.metadata() {
            Ok(meta) => meta.permissions().mode(),
            Err(_) => return false,
        };
        mode & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{lock_env, EnvVarGuard};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn write_fake_binary(dir: &std::path::Path, name: &str) {
        let path = dir.join(name);
        fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write fake binary");
        let mut perms = fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("chmod fake binary");
    }

    #[test]
    fn detect_prefers_env_override() {
        let _guard = lock_env();
        let _engine = EnvVarGuard::set("NUCLEUSDB_CONTAINER_ENGINE", Some("docker"));
        let _path = EnvVarGuard::set("PATH", Some(""));
        assert_eq!(ContainerBackend::detect(), ContainerBackend::Docker);
    }

    #[test]
    fn detect_prefers_podman_when_available() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        write_fake_binary(dir.path(), "podman");
        let _engine = EnvVarGuard::set("NUCLEUSDB_CONTAINER_ENGINE", None);
        let _path = EnvVarGuard::set("PATH", Some(dir.path().to_str().expect("utf8 path")));
        assert_eq!(ContainerBackend::detect(), ContainerBackend::Podman);
    }

    #[test]
    fn default_socket_matches_engine() {
        let _guard = lock_env();
        let _xdg = EnvVarGuard::set("XDG_RUNTIME_DIR", Some("/tmp/runtime-test"));
        assert_eq!(
            ContainerBackend::Podman.default_socket(),
            "unix:///tmp/runtime-test/podman/podman.sock"
        );
        assert_eq!(
            ContainerBackend::Docker.default_socket(),
            "unix:///var/run/docker.sock"
        );
    }
}
