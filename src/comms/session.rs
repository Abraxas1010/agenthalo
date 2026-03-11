//! DIDComm session management.
//!
//! A session represents an authenticated, encrypted communication channel
//! between two agents. Sessions are established via a handshake and maintain
//! state for request/response correlation and capability caching.

use crate::comms::didcomm::MessageType;
use crate::halo::did::DIDDocument;
use crate::pod::capability::CapabilityToken;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct PeerSession {
    pub peer_did: String,
    pub peer_document: DIDDocument,
    pub established_at: u64,
    pub last_activity: u64,
    pub message_count: u64,
    pub granted_capabilities: Vec<CapabilityToken>,
    pub received_capabilities: Vec<CapabilityToken>,
    /// Pending request IDs awaiting response.
    pub pending_requests: BTreeMap<String, PendingRequest>,
}

#[derive(Clone, Debug)]
pub struct PendingRequest {
    pub message_id: String,
    pub sent_at: u64,
    pub message_type: MessageType,
    pub timeout_secs: u64,
}

#[derive(Clone, Debug, Default)]
pub struct SessionManager {
    pub sessions: BTreeMap<String, PeerSession>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Establish a new session with a peer. Returns a reference to the session.
    pub fn establish(&mut self, peer_did: String, peer_document: DIDDocument) -> &PeerSession {
        let now = crate::pod::now_unix();
        self.sessions
            .entry(peer_did.clone())
            .and_modify(|s| {
                s.peer_document = peer_document.clone();
                s.last_activity = now;
            })
            .or_insert_with(|| PeerSession {
                peer_did,
                peer_document,
                established_at: now,
                last_activity: now,
                message_count: 0,
                granted_capabilities: Vec::new(),
                received_capabilities: Vec::new(),
                pending_requests: BTreeMap::new(),
            })
    }

    /// Get session for a peer DID, if established.
    pub fn get(&self, peer_did: &str) -> Option<&PeerSession> {
        self.sessions.get(peer_did)
    }

    /// Get mutable session reference.
    pub fn get_mut(&mut self, peer_did: &str) -> Option<&mut PeerSession> {
        self.sessions.get_mut(peer_did)
    }

    /// Check whether a session exists for the given peer.
    pub fn has_session(&self, peer_did: &str) -> bool {
        self.sessions.contains_key(peer_did)
    }

    /// Record outgoing request for correlation.
    pub fn track_request(
        &mut self,
        peer_did: &str,
        message_id: &str,
        message_type: MessageType,
        timeout_secs: u64,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(peer_did)
            .ok_or_else(|| format!("no session established with {peer_did}"))?;
        let now = crate::pod::now_unix();
        session.pending_requests.insert(
            message_id.to_string(),
            PendingRequest {
                message_id: message_id.to_string(),
                sent_at: now,
                message_type,
                timeout_secs,
            },
        );
        session.last_activity = now;
        session.message_count += 1;
        Ok(())
    }

    /// Complete a pending request with a response. Returns the request info.
    pub fn complete_request(
        &mut self,
        peer_did: &str,
        thread_id: &str,
    ) -> Result<PendingRequest, String> {
        let session = self
            .sessions
            .get_mut(peer_did)
            .ok_or_else(|| format!("no session established with {peer_did}"))?;
        session
            .pending_requests
            .remove(thread_id)
            .ok_or_else(|| format!("no pending request with id {thread_id}"))
    }

    /// Store a granted capability in the session.
    pub fn add_granted_capability(&mut self, peer_did: &str, token: CapabilityToken) {
        if let Some(session) = self.sessions.get_mut(peer_did) {
            session.granted_capabilities.push(token);
        }
    }

    /// Store a received capability in the session.
    pub fn add_received_capability(&mut self, peer_did: &str, token: CapabilityToken) {
        if let Some(session) = self.sessions.get_mut(peer_did) {
            session.received_capabilities.push(token);
        }
    }

    /// Clean up expired sessions and timed-out requests. Returns count of removed sessions.
    pub fn cleanup(&mut self, now: u64, session_timeout_secs: u64) -> usize {
        // First prune timed-out requests within live sessions.
        for session in self.sessions.values_mut() {
            session
                .pending_requests
                .retain(|_, req| now.saturating_sub(req.sent_at) < req.timeout_secs);
        }
        // Then remove sessions that have been idle too long.
        let before = self.sessions.len();
        self.sessions
            .retain(|_, session| now.saturating_sub(session.last_activity) < session_timeout_secs);
        before - self.sessions.len()
    }

    /// List all active session peer DIDs.
    pub fn active_peers(&self) -> Vec<&str> {
        self.sessions.keys().map(|k| k.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_identity(byte: u8) -> crate::halo::did::DIDIdentity {
        let seed = [byte; 64];
        crate::halo::did::did_from_genesis_seed(&seed).unwrap()
    }

    #[test]
    fn establish_and_get_session() {
        let mut mgr = SessionManager::new();
        let bob = test_identity(0xB1);
        mgr.establish(bob.did.clone(), bob.did_document.clone());
        assert!(mgr.has_session(&bob.did));
        assert!(!mgr.has_session("did:key:nonexistent"));
        let session = mgr.get(&bob.did).unwrap();
        assert_eq!(session.peer_did, bob.did);
        assert_eq!(session.message_count, 0);
    }

    #[test]
    fn track_and_complete_request() {
        let mut mgr = SessionManager::new();
        let bob = test_identity(0xB2);
        mgr.establish(bob.did.clone(), bob.did_document.clone());
        mgr.track_request(&bob.did, "msg-1", MessageType::McpToolCall, 60)
            .unwrap();
        assert_eq!(mgr.get(&bob.did).unwrap().pending_requests.len(), 1);
        assert_eq!(mgr.get(&bob.did).unwrap().message_count, 1);

        let completed = mgr.complete_request(&bob.did, "msg-1").unwrap();
        assert_eq!(completed.message_id, "msg-1");
        assert_eq!(completed.message_type, MessageType::McpToolCall);
        assert_eq!(mgr.get(&bob.did).unwrap().pending_requests.len(), 0);
    }

    #[test]
    fn track_request_fails_without_session() {
        let mut mgr = SessionManager::new();
        let result = mgr.track_request("did:key:unknown", "msg-1", MessageType::Heartbeat, 30);
        assert!(result.is_err());
    }

    #[test]
    fn complete_request_fails_for_missing_id() {
        let mut mgr = SessionManager::new();
        let bob = test_identity(0xB3);
        mgr.establish(bob.did.clone(), bob.did_document.clone());
        mgr.track_request(&bob.did, "msg-1", MessageType::McpToolCall, 60)
            .unwrap();
        let result = mgr.complete_request(&bob.did, "msg-999");
        assert!(result.is_err());
    }

    #[test]
    fn cleanup_removes_expired_sessions() {
        let mut mgr = SessionManager::new();
        let bob = test_identity(0xB4);
        mgr.establish(bob.did.clone(), bob.did_document.clone());
        // Manually set last_activity to the past.
        mgr.get_mut(&bob.did).unwrap().last_activity = 1000;
        // now=2000, timeout=500 → session at t=1000 is 1000s old > 500s timeout.
        let removed = mgr.cleanup(2000, 500);
        assert_eq!(removed, 1);
        assert!(!mgr.has_session(&bob.did));
    }

    #[test]
    fn cleanup_keeps_active_sessions() {
        let mut mgr = SessionManager::new();
        let bob = test_identity(0xB5);
        mgr.establish(bob.did.clone(), bob.did_document.clone());
        // Session just established (last_activity ≈ now).
        let now = crate::pod::now_unix();
        let removed = mgr.cleanup(now, 3600);
        assert_eq!(removed, 0);
        assert!(mgr.has_session(&bob.did));
    }

    #[test]
    fn cleanup_prunes_timed_out_requests() {
        let mut mgr = SessionManager::new();
        let bob = test_identity(0xB6);
        mgr.establish(bob.did.clone(), bob.did_document.clone());
        // Insert a request with a short timeout.
        mgr.track_request(&bob.did, "msg-old", MessageType::McpToolCall, 10)
            .unwrap();
        // Backdate the request.
        mgr.get_mut(&bob.did)
            .unwrap()
            .pending_requests
            .get_mut("msg-old")
            .unwrap()
            .sent_at = 1000;
        // Cleanup at now=2000 → request is 1000s old > 10s timeout → pruned.
        mgr.cleanup(2000, 99999);
        assert_eq!(mgr.get(&bob.did).unwrap().pending_requests.len(), 0);
    }

    #[test]
    fn active_peers_lists_sessions() {
        let mut mgr = SessionManager::new();
        let alice = test_identity(0xB7);
        let bob = test_identity(0xB8);
        mgr.establish(alice.did.clone(), alice.did_document.clone());
        mgr.establish(bob.did.clone(), bob.did_document.clone());
        let peers = mgr.active_peers();
        assert_eq!(peers.len(), 2);
    }

    #[test]
    fn re_establish_updates_document() {
        let mut mgr = SessionManager::new();
        let bob = test_identity(0xB9);
        mgr.establish(bob.did.clone(), bob.did_document.clone());
        let first_time = mgr.get(&bob.did).unwrap().established_at;
        // Re-establish with same DID — updates document and last_activity, keeps established_at.
        mgr.establish(bob.did.clone(), bob.did_document.clone());
        assert_eq!(mgr.get(&bob.did).unwrap().established_at, first_time);
    }
}
