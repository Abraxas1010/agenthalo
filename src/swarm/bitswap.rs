use crate::pcn::adapter::channel_snapshot;
use crate::pcn::schema::SettlementOp;
use crate::pod::acl::GrantStore;
use crate::protocol::NucleusDb;
use crate::swarm::config::SwarmConfig;
use crate::swarm::types::{Chunk, ChunkId};
use async_trait::async_trait;
use libp2p::request_response::{self, OutboundRequestId, ProtocolSupport, ResponseChannel};
use libp2p::{
    futures::AsyncRead, futures::AsyncReadExt, futures::AsyncWrite, futures::AsyncWriteExt,
};
use libp2p::{PeerId, StreamProtocol};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io;

pub const BITSWAP_PROTOCOL: StreamProtocol = StreamProtocol::new("/agenthalo/bitswap/1.0.0");
pub const MAX_BITSWAP_MSG_SIZE: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BitswapMessage {
    Want(Vec<ChunkId>),
    Have(Vec<ChunkId>),
    Block(ChunkId, Vec<u8>),
}

#[derive(Clone, Debug, Default)]
pub struct BitswapCodec;

#[derive(Clone, Debug, Default)]
pub struct BitswapProtocol;

pub type BitswapBehaviour = request_response::Behaviour<BitswapCodec>;
pub type BitswapEvent = request_response::Event<BitswapMessage, BitswapMessage>;
pub type BitswapRequestId = OutboundRequestId;
pub type BitswapChannel = ResponseChannel<BitswapMessage>;

#[derive(Clone, Debug, Default)]
pub struct BitswapRuntime {
    local_chunks: BTreeMap<ChunkId, Chunk>,
    peer_inventory: BTreeMap<String, BTreeSet<ChunkId>>,
    peer_aliases: BTreeMap<String, [u8; 32]>,
    grants: GrantStore,
    require_grants: bool,
    active_transfers: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BitswapStatus {
    pub local_chunks: usize,
    pub peer_count: usize,
    pub active_transfers: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BitswapTransferError {
    MissingChannel,
    InsufficientCredit,
    ApplyFailed,
}

impl BitswapProtocol {
    pub fn protocol() -> StreamProtocol {
        BITSWAP_PROTOCOL
    }

    pub fn behaviour() -> BitswapBehaviour {
        request_response::Behaviour::with_codec(
            BitswapCodec,
            std::iter::once((BITSWAP_PROTOCOL, ProtocolSupport::Full)),
            request_response::Config::default(),
        )
    }
}

#[async_trait]
impl request_response::Codec for BitswapCodec {
    type Protocol = StreamProtocol;
    type Request = BitswapMessage;
    type Response = BitswapMessage;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        read_message(io).await
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        read_message(io).await
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_message(io, &req).await
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        resp: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_message(io, &resp).await
    }
}

async fn read_message<T: AsyncRead + Unpin + Send>(io: &mut T) -> io::Result<BitswapMessage> {
    let mut len_buf = [0u8; 4];
    io.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_BITSWAP_MSG_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bitswap message exceeds size limit",
        ));
    }
    let mut buf = vec![0u8; len];
    io.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

async fn write_message<T: AsyncWrite + Unpin + Send>(
    io: &mut T,
    message: &BitswapMessage,
) -> io::Result<()> {
    let raw = serde_json::to_vec(message)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    if raw.len() > MAX_BITSWAP_MSG_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "bitswap message exceeds size limit",
        ));
    }
    io.write_all(&(raw.len() as u32).to_be_bytes()).await?;
    io.write_all(&raw).await?;
    io.close().await
}

impl BitswapRuntime {
    pub fn register_local_chunks(&mut self, chunks: &[Chunk]) {
        for chunk in chunks {
            self.local_chunks.insert(chunk.id.clone(), chunk.clone());
        }
    }

    pub fn set_grants(&mut self, grants: GrantStore) {
        self.grants = grants;
    }

    pub fn set_require_grants(&mut self, require_grants: bool) {
        self.require_grants = require_grants;
    }

    pub fn map_peer_to_grantee(&mut self, peer_id: &PeerId, grantee_puf: [u8; 32]) {
        self.peer_aliases.insert(peer_id.to_string(), grantee_puf);
    }

    pub fn active_transfers(&self) -> usize {
        self.active_transfers
    }

    pub fn status(&self) -> BitswapStatus {
        BitswapStatus {
            local_chunks: self.local_chunks.len(),
            peer_count: self.peer_inventory.len(),
            active_transfers: self.active_transfers,
        }
    }

    pub fn rarest_first(&self, wanted: &[ChunkId]) -> Vec<ChunkId> {
        let mut scored = wanted
            .iter()
            .map(|chunk_id| {
                let count = self
                    .peer_inventory
                    .values()
                    .filter(|inventory| inventory.contains(chunk_id))
                    .count();
                (count, chunk_id.clone())
            })
            .collect::<Vec<_>>();
        scored.sort_by(|(left_count, left_id), (right_count, right_id)| {
            left_count
                .cmp(right_count)
                .then_with(|| left_id.cmp(right_id))
        });
        scored.into_iter().map(|(_, chunk_id)| chunk_id).collect()
    }

    pub fn handle_request(&mut self, peer_id: &PeerId, message: BitswapMessage) -> BitswapMessage {
        match message {
            BitswapMessage::Want(chunk_ids) => {
                self.active_transfers = self.active_transfers.saturating_add(1);
                let allowed = chunk_ids
                    .into_iter()
                    .filter(|chunk_id| self.peer_can_read(peer_id, chunk_id))
                    .filter(|chunk_id| self.local_chunks.contains_key(chunk_id))
                    .collect::<Vec<_>>();
                self.active_transfers = self.active_transfers.saturating_sub(1);
                BitswapMessage::Have(allowed)
            }
            BitswapMessage::Have(chunk_ids) => {
                let entry = self.peer_inventory.entry(peer_id.to_string()).or_default();
                for chunk_id in chunk_ids {
                    entry.insert(chunk_id);
                }
                BitswapMessage::Have(entry.iter().cloned().collect())
            }
            BitswapMessage::Block(chunk_id, payload) => {
                if payload.is_empty() {
                    if self.peer_can_read(peer_id, &chunk_id) {
                        if let Some(chunk) = self.local_chunks.get(&chunk_id) {
                            return BitswapMessage::Block(chunk_id, chunk.data.clone());
                        }
                    }
                    BitswapMessage::Have(Vec::new())
                } else {
                    let chunk = Chunk::new(0, 1, payload.clone());
                    if chunk.id != chunk_id {
                        return BitswapMessage::Have(Vec::new());
                    }
                    self.local_chunks.insert(chunk_id.clone(), chunk);
                    BitswapMessage::Have(vec![chunk_id])
                }
            }
        }
    }

    fn peer_can_read(&self, peer_id: &PeerId, chunk_id: &ChunkId) -> bool {
        let key = format!("swarm/chunk/{chunk_id}");
        if self.grants.list_all().is_empty() {
            return !self.require_grants;
        }
        self.peer_aliases
            .get(&peer_id.to_string())
            .map(|grantee_puf| self.grants.can_read(grantee_puf, &key))
            .unwrap_or(false)
    }
}

pub fn settle_chunk_transfer(
    db: &mut NucleusDb,
    seeder: &str,
    leecher: &str,
    chunk_count: u64,
    config: &SwarmConfig,
) -> Result<(), BitswapTransferError> {
    let snapshot =
        channel_snapshot(db, seeder, leecher).ok_or(BitswapTransferError::MissingChannel)?;
    let cost = chunk_count.saturating_mul(config.chunk_credit_cost);
    let (participant1, participant2) = (
        snapshot.record.participant1.clone(),
        snapshot.record.participant2.clone(),
    );
    let leecher_is_p1 = leecher == participant1;
    let (new_balance1, new_balance2) = if leecher_is_p1 {
        if snapshot.balance1 < cost {
            return Err(BitswapTransferError::InsufficientCredit);
        }
        (
            snapshot.balance1.saturating_sub(cost),
            snapshot.balance2.saturating_add(cost),
        )
    } else {
        if snapshot.balance2 < cost {
            return Err(BitswapTransferError::InsufficientCredit);
        }
        (
            snapshot.balance1.saturating_add(cost),
            snapshot.balance2.saturating_sub(cost),
        )
    };
    SettlementOp::Update {
        p1: participant1,
        p2: participant2,
        balance1: new_balance1,
        balance2: new_balance2,
    }
    .apply(db)
    .map_err(|_| BitswapTransferError::ApplyFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::default_witness_cfg;
    use crate::pcn::adapter::channel_snapshot;
    use crate::protocol::{NucleusDb, VcBackend};
    use crate::state::State;
    use crate::swarm::chunk_engine::chunk_data;
    use crate::swarm::config::ChunkParams;
    use libp2p::futures::io::Cursor;
    use libp2p::request_response::Codec;
    use libp2p::PeerId;

    fn db() -> NucleusDb {
        NucleusDb::new(
            State::new(vec![]),
            VcBackend::BinaryMerkle,
            default_witness_cfg(),
        )
    }

    #[tokio::test]
    async fn codec_roundtrip() {
        let message = BitswapMessage::Want(vec![ChunkId::from_bytes(b"alpha")]);
        let mut codec = BitswapCodec;
        let mut buffer = Cursor::new(Vec::new());
        codec
            .write_request(&BITSWAP_PROTOCOL, &mut buffer, message.clone())
            .await
            .expect("write");
        buffer.set_position(0);
        let decoded = codec
            .read_request(&BITSWAP_PROTOCOL, &mut buffer)
            .await
            .expect("read");
        assert_eq!(decoded, message);
    }

    #[tokio::test]
    async fn codec_rejects_oversized_message() {
        let mut buffer = Cursor::new((MAX_BITSWAP_MSG_SIZE as u32 + 1).to_be_bytes().to_vec());
        let err = read_message(&mut buffer)
            .await
            .expect_err("must reject oversized frame");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn codec_write_rejects_oversized_message() {
        let chunk_ids = (0u32..70_000)
            .map(|idx| ChunkId::from_bytes(&idx.to_be_bytes()))
            .collect::<Vec<_>>();
        let message = BitswapMessage::Want(chunk_ids);
        let mut codec = BitswapCodec;
        let mut buffer = Cursor::new(Vec::new());
        let err = codec
            .write_request(&BITSWAP_PROTOCOL, &mut buffer, message)
            .await
            .expect_err("must reject oversized write frame");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn want_have_exchange_uses_local_inventory() {
        let chunk = chunk_data(b"hello bitswap", &ChunkParams::default())[0].clone();
        let mut runtime = BitswapRuntime::default();
        runtime.register_local_chunks(&[chunk.clone()]);
        let peer = PeerId::random();
        let response = runtime.handle_request(&peer, BitswapMessage::Want(vec![chunk.id.clone()]));
        assert_eq!(response, BitswapMessage::Have(vec![chunk.id]));
    }

    #[test]
    fn block_transfer_returns_payload() {
        let chunk = chunk_data(b"hello bitswap", &ChunkParams::default())[0].clone();
        let mut runtime = BitswapRuntime::default();
        runtime.register_local_chunks(&[chunk.clone()]);
        let peer = PeerId::random();
        let response =
            runtime.handle_request(&peer, BitswapMessage::Block(chunk.id.clone(), Vec::new()));
        assert_eq!(response, BitswapMessage::Block(chunk.id, chunk.data));
    }

    #[test]
    fn grant_denial_blocks_unmapped_peer() {
        let chunk = chunk_data(b"secure chunk", &ChunkParams::default())[0].clone();
        let mut runtime = BitswapRuntime::default();
        runtime.register_local_chunks(&[chunk.clone()]);
        let mut grants = GrantStore::new();
        grants.create(crate::pod::acl::GrantRequest {
            grantor_puf: [1u8; 32],
            grantee_puf: [2u8; 32],
            key_pattern: format!("swarm/chunk/{}", chunk.id),
            permissions: crate::pod::acl::GrantPermissions::read_only(),
            expires_at: None,
        });
        runtime.set_grants(grants);
        let peer = PeerId::random();
        let response = runtime.handle_request(&peer, BitswapMessage::Want(vec![chunk.id]));
        assert_eq!(response, BitswapMessage::Have(Vec::new()));
    }

    #[test]
    fn grant_required_mode_blocks_when_grants_are_empty() {
        let chunk = chunk_data(b"secure chunk", &ChunkParams::default())[0].clone();
        let mut runtime = BitswapRuntime::default();
        runtime.register_local_chunks(&[chunk.clone()]);
        runtime.set_require_grants(true);
        let peer = PeerId::random();
        let response = runtime.handle_request(&peer, BitswapMessage::Want(vec![chunk.id]));
        assert_eq!(response, BitswapMessage::Have(Vec::new()));
    }

    #[test]
    fn block_receive_rejects_mismatched_chunk_id() {
        let mut runtime = BitswapRuntime::default();
        let peer = PeerId::random();
        let payload = b"forged payload".to_vec();
        let claimed_id = ChunkId::from_bytes(b"claimed id");
        let response =
            runtime.handle_request(&peer, BitswapMessage::Block(claimed_id.clone(), payload));
        assert_eq!(response, BitswapMessage::Have(Vec::new()));
        let lookup = runtime.handle_request(&peer, BitswapMessage::Block(claimed_id, Vec::new()));
        assert_eq!(lookup, BitswapMessage::Have(Vec::new()));
    }

    #[test]
    fn rarest_first_prefers_sparsest_chunk() {
        let a = ChunkId::from_bytes(b"a");
        let b = ChunkId::from_bytes(b"b");
        let mut runtime = BitswapRuntime::default();
        runtime
            .peer_inventory
            .insert("peer-a".to_string(), [a.clone()].into_iter().collect());
        runtime.peer_inventory.insert(
            "peer-b".to_string(),
            [a.clone(), b.clone()].into_iter().collect(),
        );
        let order = runtime.rarest_first(&[a.clone(), b.clone()]);
        assert_eq!(order[0], b);
        assert_eq!(order[1], a);
    }

    #[test]
    fn unknown_chunk_returns_empty_have() {
        let mut runtime = BitswapRuntime::default();
        let peer = PeerId::random();
        let response = runtime.handle_request(
            &peer,
            BitswapMessage::Want(vec![ChunkId::from_bytes(b"none")]),
        );
        assert_eq!(response, BitswapMessage::Have(Vec::new()));
    }

    #[test]
    fn pcn_credit_accounting_moves_balance() {
        let mut db = db();
        SettlementOp::Open {
            p1: "leecher".to_string(),
            p2: "seeder".to_string(),
            capacity: 10,
        }
        .apply(&mut db)
        .expect("open");
        SettlementOp::Update {
            p1: "leecher".to_string(),
            p2: "seeder".to_string(),
            balance1: 6,
            balance2: 4,
        }
        .apply(&mut db)
        .expect("seed balance");
        let cfg = SwarmConfig::default();
        settle_chunk_transfer(&mut db, "seeder", "leecher", 2, &cfg).expect("settle");
        let snapshot = channel_snapshot(&db, "leecher", "seeder").expect("snapshot");
        assert_eq!(snapshot.balance1, 4);
        assert_eq!(snapshot.balance2, 6);
    }

    #[test]
    fn pcn_insufficient_credit_fails_closed() {
        let mut db = db();
        SettlementOp::Open {
            p1: "leecher".to_string(),
            p2: "seeder".to_string(),
            capacity: 4,
        }
        .apply(&mut db)
        .expect("open");
        SettlementOp::Update {
            p1: "leecher".to_string(),
            p2: "seeder".to_string(),
            balance1: 0,
            balance2: 4,
        }
        .apply(&mut db)
        .expect("seed balance");
        let err = settle_chunk_transfer(&mut db, "seeder", "leecher", 1, &SwarmConfig::default())
            .expect_err("must fail");
        assert_eq!(err, BitswapTransferError::InsufficientCredit);
    }
}
