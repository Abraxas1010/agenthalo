//! Range-proof guest program stub.
//!
//! Intended behavior:
//! - private input: value
//! - public input: (min, max)
//! - assert min <= value <= max
//! - commit a value commitment into the public journal

pub fn description() -> &'static str {
    "range proof guest stub (compile with RISC Zero toolchain)"
}
