use alloy_primitives::U256;
use serde::Serialize;

use crate::uniswap_v2::V2SwapIntent;

#[derive(Debug, Clone)]
pub struct PaperPolicy {
    pub max_amount_in: U256,
    pub max_path_len: usize,
    pub deadline_grace_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum PaperDecision {
    Follow {
        amount_in: U256,
        proportional_amount_out_min: U256,
    },
    Reject {
        reason: PaperRejectReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperRejectReason {
    MissingTypedIntent,
    ZeroAmountIn,
    ZeroMinimumOutput,
    ScaledMinimumOutputIsZero,
    ArithmeticOverflow,
    PathTooLong,
    Expired,
}

impl PaperPolicy {
    pub fn evaluate(
        &self,
        intent: Option<&V2SwapIntent>,
        observed_unix_seconds: u64,
    ) -> PaperDecision {
        let Some(intent) = intent else {
            return PaperDecision::Reject {
                reason: PaperRejectReason::MissingTypedIntent,
            };
        };
        if intent.amount_in == U256::ZERO {
            return PaperDecision::Reject {
                reason: PaperRejectReason::ZeroAmountIn,
            };
        }
        if intent.amount_out_min == U256::ZERO {
            return PaperDecision::Reject {
                reason: PaperRejectReason::ZeroMinimumOutput,
            };
        }
        if intent.path.len() > self.max_path_len {
            return PaperDecision::Reject {
                reason: PaperRejectReason::PathTooLong,
            };
        }
        let oldest_deadline =
            U256::from(observed_unix_seconds.saturating_sub(self.deadline_grace_seconds));
        if intent.deadline < oldest_deadline {
            return PaperDecision::Reject {
                reason: PaperRejectReason::Expired,
            };
        }
        let amount_in = intent.amount_in.min(self.max_amount_in);
        let Some(numerator) = intent.amount_out_min.checked_mul(amount_in) else {
            return PaperDecision::Reject {
                reason: PaperRejectReason::ArithmeticOverflow,
            };
        };
        let proportional_amount_out_min = numerator / intent.amount_in;
        if proportional_amount_out_min == U256::ZERO {
            return PaperDecision::Reject {
                reason: PaperRejectReason::ScaledMinimumOutputIsZero,
            };
        }
        PaperDecision::Follow {
            amount_in,
            proportional_amount_out_min,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uniswap_v2::{V2SwapIntent, V2SwapKind};
    use alloy_primitives::Address;

    fn intent() -> V2SwapIntent {
        V2SwapIntent {
            kind: V2SwapKind::EthForTokens,
            amount_in: U256::from(100),
            amount_out_min: U256::from(80),
            path: vec![Address::with_last_byte(1), Address::with_last_byte(2)],
            recipient: Address::with_last_byte(3),
            deadline: U256::from(1_100),
        }
    }

    fn policy() -> PaperPolicy {
        PaperPolicy {
            max_amount_in: U256::from(25),
            max_path_len: 3,
            deadline_grace_seconds: 2,
        }
    }

    #[test]
    fn caps_input_and_scales_minimum_output() {
        assert_eq!(
            policy().evaluate(Some(&intent()), 1_000),
            PaperDecision::Follow {
                amount_in: U256::from(25),
                proportional_amount_out_min: U256::from(20),
            }
        );
    }

    #[test]
    fn rejects_zero_minimum_output() {
        let mut value = intent();
        value.amount_out_min = U256::ZERO;
        assert_eq!(
            policy().evaluate(Some(&value), 1_000),
            PaperDecision::Reject {
                reason: PaperRejectReason::ZeroMinimumOutput,
            }
        );
    }

    #[test]
    fn rejects_expired_intent() {
        let mut value = intent();
        value.deadline = U256::from(997);
        assert_eq!(
            policy().evaluate(Some(&value), 1_000),
            PaperDecision::Reject {
                reason: PaperRejectReason::Expired,
            }
        );
    }

    #[test]
    fn rejects_when_scaling_rounds_minimum_to_zero() {
        let mut value = intent();
        value.amount_in = U256::from(1_000);
        value.amount_out_min = U256::from(1);
        assert_eq!(
            policy().evaluate(Some(&value), 1_000),
            PaperDecision::Reject {
                reason: PaperRejectReason::ScaledMinimumOutputIsZero,
            }
        );
    }
}
