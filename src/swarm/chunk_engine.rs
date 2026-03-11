use crate::swarm::config::ChunkParams;
use crate::swarm::types::Chunk;

pub fn chunk_data(data: &[u8], params: &ChunkParams) -> Vec<Chunk> {
    if data.is_empty() {
        return vec![Chunk::new(0, 1, Vec::new())];
    }
    let chunk_size = params.chunk_size_bytes.max(1);
    let total_chunks = data.len().div_ceil(chunk_size) as u32;
    data.chunks(chunk_size)
        .enumerate()
        .map(|(idx, piece)| Chunk::new(idx as u32, total_chunks, piece.to_vec()))
        .collect()
}

pub fn verify_chunk(chunk: &Chunk) -> bool {
    chunk.id == crate::swarm::types::ChunkId::from_bytes(&chunk.data)
        && chunk.size == chunk.data.len()
}

pub fn reassemble_chunks(chunks: &[Chunk]) -> Result<Vec<u8>, String> {
    if chunks.is_empty() {
        return Ok(Vec::new());
    }
    let total_chunks = chunks[0].total_chunks;
    let mut ordered = chunks.to_vec();
    ordered.sort_by_key(|chunk| chunk.index);
    if ordered.len() != total_chunks as usize {
        return Err("missing chunks for reassembly".to_string());
    }
    for (expected, chunk) in ordered.iter().enumerate() {
        if !verify_chunk(chunk) {
            return Err(format!("chunk {} failed verification", chunk.index));
        }
        if chunk.index != expected as u32 {
            return Err("chunk ordering is incomplete".to_string());
        }
        if chunk.total_chunks != total_chunks {
            return Err("chunk set disagrees on total_chunks".to_string());
        }
    }
    let mut data = Vec::with_capacity(ordered.iter().map(|chunk| chunk.size).sum());
    for chunk in ordered {
        data.extend_from_slice(&chunk.data);
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::config::ChunkParams;

    #[test]
    fn chunk_roundtrip_small_payload() {
        let chunks = chunk_data(
            b"hello world",
            &ChunkParams {
                chunk_size_bytes: 4,
            },
        );
        let data = reassemble_chunks(&chunks).expect("reassemble");
        assert_eq!(data, b"hello world");
    }

    #[test]
    fn chunk_roundtrip_large_payload() {
        let data = vec![0x42u8; 900_000];
        let chunks = chunk_data(&data, &ChunkParams::default());
        assert!(chunks.len() > 1);
        let rebuilt = reassemble_chunks(&chunks).expect("reassemble");
        assert_eq!(rebuilt, data);
    }

    #[test]
    fn verify_detects_tamper() {
        let mut chunk = Chunk::new(0, 1, b"hello".to_vec());
        chunk.data[0] = b'j';
        assert!(!verify_chunk(&chunk));
    }

    #[test]
    fn reassemble_rejects_missing_chunk() {
        let mut chunks = chunk_data(
            b"abcdefgh",
            &ChunkParams {
                chunk_size_bytes: 2,
            },
        );
        chunks.pop();
        assert!(reassemble_chunks(&chunks).is_err());
    }

    #[test]
    fn chunking_is_deterministic() {
        let params = ChunkParams {
            chunk_size_bytes: 3,
        };
        let left = chunk_data(b"deterministic", &params);
        let right = chunk_data(b"deterministic", &params);
        assert_eq!(left, right);
    }
}
