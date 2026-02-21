use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub values: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delta {
    pub writes: Vec<(usize, u64)>,
}

impl State {
    pub fn new(values: Vec<u64>) -> Self {
        Self { values }
    }
}

impl Delta {
    pub fn new(writes: Vec<(usize, u64)>) -> Self {
        Self { writes }
    }
}

pub fn apply(prev: &State, delta: &Delta) -> State {
    let mut next = prev.values.clone();
    for (idx, value) in &delta.writes {
        if *idx >= next.len() {
            next.resize(*idx + 1, 0);
        }
        next[*idx] = *value;
    }
    State { values: next }
}
