pub const PUBLIC_INPUT_SCHEMA_VERSION: u32 = 1;
pub const REQUIRED_PUBLIC_INPUTS: usize = 5;

pub const MERKLE_LO: usize = 0;
pub const MERKLE_HI: usize = 1;
pub const DIGEST_LO: usize = 2;
pub const DIGEST_HI: usize = 3;
pub const EVENT_COUNT: usize = 4;

pub fn build_public_inputs(
    merkle_lo: u128,
    merkle_hi: u128,
    digest_lo: u128,
    digest_hi: u128,
    event_count: u64,
) -> Vec<String> {
    let mut out = vec!["0".to_string(); REQUIRED_PUBLIC_INPUTS];
    out[MERKLE_LO] = merkle_lo.to_string();
    out[MERKLE_HI] = merkle_hi.to_string();
    out[DIGEST_LO] = digest_lo.to_string();
    out[DIGEST_HI] = digest_hi.to_string();
    out[EVENT_COUNT] = event_count.to_string();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_lock_roundtrip() {
        let inputs = build_public_inputs(11, 12, 21, 22, 3);
        assert_eq!(inputs.len(), REQUIRED_PUBLIC_INPUTS);
        assert_eq!(inputs[MERKLE_LO], "11");
        assert_eq!(inputs[MERKLE_HI], "12");
        assert_eq!(inputs[DIGEST_LO], "21");
        assert_eq!(inputs[DIGEST_HI], "22");
        assert_eq!(inputs[EVENT_COUNT], "3");
    }
}
