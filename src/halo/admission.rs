//! Shared AETHER admission policy for manager and container control surfaces.
//!
//! This is an engineering gate built on top of the formally-ported governor and
//! Betti/Chebyshev components. It does not claim a formal end-to-end proof for
//! launch policy; it centralizes runtime checks so CLI, dashboard, and MCP
//! manager paths make the same decision from the same evidence.

use crate::cockpit::deploy::DeployTopologyStatus;
use crate::halo::governor_registry::{GovernorRegistry, GovernorSnapshot};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionMode {
    Warn,
    Block,
    Force,
}

impl AdmissionMode {
    pub fn parse(raw: Option<&str>) -> Result<Self, String> {
        match raw
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("warn")
            .to_ascii_lowercase()
            .as_str()
        {
            "warn" => Ok(Self::Warn),
            "block" => Ok(Self::Block),
            "force" => Ok(Self::Force),
            other => Err(format!(
                "invalid admission mode `{other}`; expected warn, block, or force"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Warn => "warn",
            Self::Block => "block",
            Self::Force => "force",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AdmissionIssue {
    pub code: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AdmissionReport {
    pub mode: String,
    pub allowed: bool,
    pub forced: bool,
    pub issues: Vec<AdmissionIssue>,
    pub governors: Vec<GovernorSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topology: Option<DeployTopologyStatus>,
}

pub fn evaluate_launch_admission(
    mode: AdmissionMode,
    registry: Option<&GovernorRegistry>,
    topology: Option<&DeployTopologyStatus>,
) -> AdmissionReport {
    let mut issues = Vec::new();
    let governors = registry
        .map(|registry| {
            ["gov-compute", "gov-pty"]
                .iter()
                .filter_map(|instance_id| registry.snapshot_one(instance_id).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    for snapshot in &governors {
        if snapshot.gain_violated {
            issues.push(AdmissionIssue {
                code: "governor_gain_violated".to_string(),
                severity: "block".to_string(),
                message: format!(
                    "{} is outside the AETHER gain regime; launch control is not trustworthy until it is reset or reconfigured.",
                    snapshot.instance_id
                ),
                instance_id: Some(snapshot.instance_id.clone()),
            });
        } else if snapshot.oscillating {
            issues.push(AdmissionIssue {
                code: "governor_oscillating".to_string(),
                severity: "warn".to_string(),
                message: format!(
                    "{} has shown Lyapunov non-descent in multi-step engineering mode; new manager load may amplify oscillation.",
                    snapshot.instance_id
                ),
                instance_id: Some(snapshot.instance_id.clone()),
            });
        } else if !snapshot.stable {
            issues.push(AdmissionIssue {
                code: "governor_unstable".to_string(),
                severity: "warn".to_string(),
                message: format!(
                    "{} is still being observed and has not reached a clean stable state.",
                    snapshot.instance_id
                ),
                instance_id: Some(snapshot.instance_id.clone()),
            });
        }
    }

    if let Some(topology) = topology {
        if topology.structural_change_flagged {
            issues.push(AdmissionIssue {
                code: "topology_structural_change".to_string(),
                severity: "block".to_string(),
                message: topology.warning.clone().unwrap_or_else(|| {
                    "binary topology changed beyond the Betti overlap bound; keep SHA-256 as the primary authenticator and require operator review before launch.".to_string()
                }),
                instance_id: None,
            });
        } else if topology.hash_changed {
            issues.push(AdmissionIssue {
                code: "topology_hash_changed".to_string(),
                severity: "warn".to_string(),
                message: topology.warning.clone().unwrap_or_else(|| {
                    "binary hash changed; Betti overlap stayed within the loose bound, but manager policy should treat this as a review signal rather than a proof of equivalence.".to_string()
                }),
                instance_id: None,
            });
        }
    }

    let has_blocker = issues.iter().any(|issue| issue.severity == "block");
    let has_warning = !issues.is_empty();
    let forced = mode == AdmissionMode::Force && has_warning;
    let allowed = match mode {
        AdmissionMode::Warn => true,
        AdmissionMode::Block => {
            !has_blocker && !issues.iter().any(|issue| issue.severity == "warn")
        }
        AdmissionMode::Force => true,
    };

    AdmissionReport {
        mode: mode.as_str().to_string(),
        allowed,
        forced,
        issues,
        governors,
        topology: topology.cloned(),
    }
}
