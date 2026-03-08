use crate::halo::capability_spec::{CapabilityQuery, CapabilitySpec};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityProviderRecord {
    pub agent_did: String,
    pub capability: CapabilitySpec,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityBeaconRecord {
    pub beacon_id: String,
    pub endpoint: String,
    pub supported_domains: Vec<String>,
    pub indexed_capabilities: HashMap<String, Vec<CapabilityProviderRecord>>,
    pub last_seen: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BeaconProviderMatch {
    pub beacon_id: String,
    pub agent_did: String,
    pub capability_id: String,
    pub support_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BeaconDirectory {
    pub beacons: HashMap<String, CapabilityBeaconRecord>,
}

impl BeaconDirectory {
    pub fn register_beacon(
        &mut self,
        beacon_id: &str,
        endpoint: &str,
        supported_domains: Vec<String>,
        now: u64,
    ) {
        self.beacons.insert(
            beacon_id.to_string(),
            CapabilityBeaconRecord {
                beacon_id: beacon_id.to_string(),
                endpoint: endpoint.to_string(),
                supported_domains,
                indexed_capabilities: HashMap::new(),
                last_seen: now,
            },
        );
    }

    pub fn index_capability(
        &mut self,
        beacon_id: &str,
        agent_did: &str,
        capability: CapabilitySpec,
    ) -> Result<(), String> {
        let beacon = self
            .beacons
            .get_mut(beacon_id)
            .ok_or_else(|| format!("unknown beacon `{beacon_id}`"))?;
        beacon
            .indexed_capabilities
            .entry(capability.domain.path.clone())
            .or_default()
            .push(CapabilityProviderRecord {
                agent_did: agent_did.to_string(),
                capability,
            });
        Ok(())
    }

    pub fn query(
        &self,
        query: &CapabilityQuery,
        now: u64,
        attestation_max_age_secs: u64,
    ) -> Vec<BeaconProviderMatch> {
        let mut matches = Vec::new();
        for beacon in self.beacons.values() {
            for providers in beacon.indexed_capabilities.values() {
                for provider in providers {
                    if provider
                        .capability
                        .satisfies_at(query, now, attestation_max_age_secs)
                    {
                        matches.push(BeaconProviderMatch {
                            beacon_id: beacon.beacon_id.clone(),
                            agent_did: provider.agent_did.clone(),
                            capability_id: provider.capability.capability_id.clone(),
                            support_count: 1,
                        });
                    }
                }
            }
        }
        matches
    }

    pub fn quorum_query(
        &self,
        query: &CapabilityQuery,
        min_beacons: usize,
        now: u64,
        attestation_max_age_secs: u64,
    ) -> Vec<BeaconProviderMatch> {
        let mut counts: HashMap<(String, String), BeaconProviderMatch> = HashMap::new();
        for entry in self.query(query, now, attestation_max_age_secs) {
            let key = (entry.agent_did.clone(), entry.capability_id.clone());
            counts
                .entry(key)
                .and_modify(|record| record.support_count += 1)
                .or_insert(entry);
        }
        let mut out = counts
            .into_values()
            .filter(|entry| entry.support_count >= min_beacons)
            .collect::<Vec<_>>();
        out.sort_by(|a, b| {
            b.support_count
                .cmp(&a.support_count)
                .then(a.agent_did.cmp(&b.agent_did))
        });
        out
    }

    pub fn detect_censoring(
        &self,
        query: &CapabilityQuery,
        now: u64,
        attestation_max_age_secs: u64,
    ) -> Vec<String> {
        let per_beacon = self
            .beacons
            .values()
            .map(|beacon| {
                let count = beacon
                    .indexed_capabilities
                    .values()
                    .flatten()
                    .filter(|provider| {
                        provider
                            .capability
                            .satisfies_at(query, now, attestation_max_age_secs)
                    })
                    .count();
                (beacon.beacon_id.clone(), count)
            })
            .collect::<Vec<_>>();
        let max_count = per_beacon
            .iter()
            .map(|(_, count)| *count)
            .max()
            .unwrap_or(0);
        if max_count == 0 {
            return Vec::new();
        }
        per_beacon
            .into_iter()
            .filter(|(_, count)| *count == 0)
            .map(|(beacon_id, _)| beacon_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::capability_spec::{CapabilityDomain, TypeSpec};

    fn query() -> CapabilityQuery {
        CapabilityQuery {
            domain_prefix: "prove/lean".to_string(),
            required_inputs: vec![TypeSpec::LeanTerm],
            required_outputs: vec![TypeSpec::LeanTerm],
            required_constraints: vec![],
            min_success_rate: None,
            max_latency_p99_ms: None,
            max_cost_microdollars: None,
            min_attestations: None,
            min_onchain_reputation: None,
            count: 1,
            query_timeout_ms: 250,
        }
    }

    fn spec() -> CapabilitySpec {
        CapabilitySpec::new(
            CapabilityDomain::new("prove/lean/algebra", 1),
            vec![TypeSpec::LeanTerm],
            vec![TypeSpec::LeanTerm],
            vec![],
        )
    }

    #[test]
    fn beacon_quorum_prefers_providers_seen_by_multiple_beacons() {
        let mut directory = BeaconDirectory::default();
        directory.register_beacon("b1", "http://beacon-1", vec!["prove/lean".to_string()], 100);
        directory.register_beacon("b2", "http://beacon-2", vec!["prove/lean".to_string()], 100);
        directory
            .index_capability("b1", "did:key:a", spec())
            .expect("index b1");
        directory
            .index_capability("b2", "did:key:a", spec())
            .expect("index b2");
        directory
            .index_capability(
                "b2",
                "did:key:b",
                CapabilitySpec::new(
                    CapabilityDomain::new("translate/coq-to-lean", 1),
                    vec![TypeSpec::CoqTerm],
                    vec![TypeSpec::LeanTerm],
                    vec![],
                ),
            )
            .expect("index translate");

        let matches = directory.quorum_query(&query(), 2, 100, 3600);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].agent_did, "did:key:a");
        assert_eq!(matches[0].support_count, 2);
    }

    #[test]
    fn beacon_directory_flags_zero_result_outliers_as_censoring_candidates() {
        let mut directory = BeaconDirectory::default();
        directory.register_beacon("b1", "http://beacon-1", vec!["prove/lean".to_string()], 100);
        directory.register_beacon("b2", "http://beacon-2", vec!["prove/lean".to_string()], 100);
        directory
            .index_capability("b1", "did:key:a", spec())
            .expect("index b1");
        let censoring = directory.detect_censoring(&query(), 100, 3600);
        assert_eq!(censoring, vec!["b2".to_string()]);
    }
}
