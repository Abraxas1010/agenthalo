use crate::state::State;

pub fn materialize(state: &State) -> Vec<u64> {
    state.values.clone()
}
