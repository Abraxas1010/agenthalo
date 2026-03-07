use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Runtime uncertainty frameworks used by agents/tools.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UncertaintyKind {
    Probability,
    CertaintyFactor,
    Possibility,
    Binary,
}

pub trait UncertaintyTranslator {
    fn to_probability(&self, value: f64) -> f64;
    fn map_probability(&self, probability: f64) -> f64;
    fn is_balanced(&self) -> bool;
}

impl UncertaintyTranslator for UncertaintyKind {
    fn to_probability(&self, value: f64) -> f64 {
        match self {
            Self::Probability => value.clamp(0.0, 1.0),
            Self::CertaintyFactor => {
                if value <= 0.0 {
                    0.0
                } else {
                    value / (1.0 + value)
                }
            }
            Self::Possibility => value.clamp(0.0, 1.0),
            Self::Binary => {
                if value >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    fn map_probability(&self, probability: f64) -> f64 {
        let p = probability.clamp(0.0, 1.0);
        match self {
            Self::Probability => p,
            Self::CertaintyFactor => {
                if p >= 1.0 {
                    f64::INFINITY
                } else if p <= 0.0 {
                    0.0
                } else {
                    p / (1.0 - p)
                }
            }
            Self::Possibility => p,
            Self::Binary => {
                if p >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    fn is_balanced(&self) -> bool {
        matches!(self, Self::Probability | Self::Possibility)
    }
}

pub fn translate_uncertainty(from: UncertaintyKind, to: UncertaintyKind, value: f64) -> f64 {
    let prob = from.to_probability(value);
    to.map_probability(prob)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probability_cf_roundtrip_is_lossless() {
        let p = 0.75;
        let cf = translate_uncertainty(
            UncertaintyKind::Probability,
            UncertaintyKind::CertaintyFactor,
            p,
        );
        let recovered = translate_uncertainty(
            UncertaintyKind::CertaintyFactor,
            UncertaintyKind::Probability,
            cf,
        );
        assert!((recovered - p).abs() < 1e-10);
    }

    #[test]
    fn binary_translation_snaps_to_extremes() {
        assert_eq!(
            translate_uncertainty(UncertaintyKind::Probability, UncertaintyKind::Binary, 0.6,),
            1.0
        );
        assert_eq!(
            translate_uncertainty(UncertaintyKind::Probability, UncertaintyKind::Binary, 0.4,),
            0.0
        );
    }
}
