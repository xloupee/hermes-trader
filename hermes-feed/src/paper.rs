use alloy_primitives::U256;
use serde::Serialize;

use crate::uniswap_v2::V2SwapIntent;
use crate::v2_simulator::{OrderedCopyQuote, ReserveBook};

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
    ReserveQuoteFailed,
    QuotedOutputBelowMinimum,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum ReservePaperDecision {
    Follow {
        proportional_amount_out_min: U256,
        quote: OrderedCopyQuote,
    },
    Reject {
        reason: PaperRejectReason,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        quoted_amount_out: Option<U256>,
        #[serde(skip_serializing_if = "Option::is_none")]
        minimum_amount_out: Option<U256>,
    },
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

    pub fn evaluate_with_reserves(
        &self,
        intent: Option<&V2SwapIntent>,
        observed_unix_seconds: u64,
        reserves: &ReserveBook,
        minimum_snapshot_block: u64,
    ) -> ReservePaperDecision {
        let policy = self.evaluate(intent, observed_unix_seconds);
        let PaperDecision::Follow {
            amount_in,
            proportional_amount_out_min,
        } = policy
        else {
            let PaperDecision::Reject { reason } = policy else {
                unreachable!("paper decision variants are exhaustive")
            };
            return ReservePaperDecision::Reject {
                reason,
                detail: None,
                quoted_amount_out: None,
                minimum_amount_out: None,
            };
        };
        let intent = intent.expect("follow requires a typed intent");
        let quote = match reserves.simulate_leader_then_follower(
            &intent.path,
            intent.amount_in,
            amount_in,
            minimum_snapshot_block,
        ) {
            Ok(quote) => quote,
            Err(error) => {
                return ReservePaperDecision::Reject {
                    reason: PaperRejectReason::ReserveQuoteFailed,
                    detail: Some(error.to_string()),
                    quoted_amount_out: None,
                    minimum_amount_out: Some(proportional_amount_out_min),
                };
            }
        };
        if quote.follower_amount_out < proportional_amount_out_min {
            return ReservePaperDecision::Reject {
                reason: PaperRejectReason::QuotedOutputBelowMinimum,
                detail: None,
                quoted_amount_out: Some(quote.follower_amount_out),
                minimum_amount_out: Some(proportional_amount_out_min),
            };
        }
        ReservePaperDecision::Follow {
            proportional_amount_out_min,
            quote,
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

    #[test]
    fn reserve_policy_quotes_follower_after_leader() {
        use crate::v2_simulator::PairSnapshot;

        let value = intent();
        let reserves = ReserveBook::from_snapshots([PairSnapshot {
            pair: Address::with_last_byte(9),
            token0: value.path[0],
            token1: value.path[1],
            reserve0: U256::from(1_000),
            reserve1: U256::from(1_000),
            block_number: 50,
        }])
        .unwrap();
        let decision = policy().evaluate_with_reserves(Some(&value), 1_000, &reserves, 50);
        let ReservePaperDecision::Follow { quote, .. } = decision else {
            panic!("expected reserve-aware follow");
        };
        assert_eq!(quote.leader_amount_out, U256::from(90));
        assert_eq!(quote.follower_amount_out, U256::from(20));
    }

    #[test]
    fn reserve_policy_rejects_quote_below_scaled_minimum() {
        use crate::v2_simulator::PairSnapshot;

        let mut value = intent();
        value.amount_out_min = U256::from(100);
        let reserves = ReserveBook::from_snapshots([PairSnapshot {
            pair: Address::with_last_byte(9),
            token0: value.path[0],
            token1: value.path[1],
            reserve0: U256::from(1_000),
            reserve1: U256::from(1_000),
            block_number: 50,
        }])
        .unwrap();
        assert!(matches!(
            policy().evaluate_with_reserves(Some(&value), 1_000, &reserves, 50),
            ReservePaperDecision::Reject {
                reason: PaperRejectReason::QuotedOutputBelowMinimum,
                ..
            }
        ));
    }
}
