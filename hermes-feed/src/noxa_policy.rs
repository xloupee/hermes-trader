use alloy_primitives::U256;
use serde::Serialize;

const MAX_TX_MULTIPLIER_PERCENT: u64 = 110;
const PERCENT_DENOMINATOR: u64 = 100;

/// Inputs needed to decide whether one NOXA pool buy is restriction-compliant.
///
/// Every block value is an Ethereum L1 block height. Robinhood's L2 block
/// number must never be substituted for these fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoxaPolicyInput {
    pub launch_l1_block: u64,
    pub restrictions_end_l1_block: u64,
    pub current_l1_block: u64,
    pub recipient_balance_before: U256,
    pub expected_bought_output: U256,
    pub origin_bought_before: U256,
    pub max_wallet_limit: U256,
    pub max_tx_limit: U256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "decision")]
pub enum NoxaPolicyDecision {
    /// Restrictions are active and all restricted-window checks passed.
    Restricted {
        resulting_recipient_balance: U256,
        cumulative_origin_bought: U256,
        cumulative_origin_limit: U256,
    },
    /// The restriction window has ended, so wallet and transaction limits do
    /// not apply.
    Unrestricted,
    Reject {
        reason: NoxaRejectReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum NoxaRejectReason {
    /// The observed L1 height predates the launch. Treat this as inconsistent
    /// state rather than speculating about eligibility.
    BeforeLaunchBlock {
        current_l1_block: u64,
        launch_l1_block: u64,
    },
    /// Public pool buys are always forbidden in the launch L1 block.
    LaunchBlockBuyBlocked {
        launch_l1_block: u64,
    },
    RecipientBalanceOverflow,
    MaxWalletExceeded {
        resulting_balance: U256,
        limit: U256,
    },
    MaxTransactionCapOverflow,
    CumulativeOriginBoughtOverflow,
    MaxTransactionExceeded {
        cumulative_bought: U256,
        limit: U256,
    },
}

/// Evaluate NOXA's launch and restricted-window rules without performing any
/// network access or transaction construction.
pub fn evaluate_noxa_policy(input: NoxaPolicyInput) -> NoxaPolicyDecision {
    if input.current_l1_block < input.launch_l1_block {
        return NoxaPolicyDecision::Reject {
            reason: NoxaRejectReason::BeforeLaunchBlock {
                current_l1_block: input.current_l1_block,
                launch_l1_block: input.launch_l1_block,
            },
        };
    }
    if input.current_l1_block == input.launch_l1_block {
        return NoxaPolicyDecision::Reject {
            reason: NoxaRejectReason::LaunchBlockBuyBlocked {
                launch_l1_block: input.launch_l1_block,
            },
        };
    }
    if input.current_l1_block > input.restrictions_end_l1_block {
        return NoxaPolicyDecision::Unrestricted;
    }

    let Some(resulting_recipient_balance) = input
        .recipient_balance_before
        .checked_add(input.expected_bought_output)
    else {
        return NoxaPolicyDecision::Reject {
            reason: NoxaRejectReason::RecipientBalanceOverflow,
        };
    };
    if resulting_recipient_balance > input.max_wallet_limit {
        return NoxaPolicyDecision::Reject {
            reason: NoxaRejectReason::MaxWalletExceeded {
                resulting_balance: resulting_recipient_balance,
                limit: input.max_wallet_limit,
            },
        };
    }

    let Some(scaled_max_tx) = input
        .max_tx_limit
        .checked_mul(U256::from(MAX_TX_MULTIPLIER_PERCENT))
    else {
        return NoxaPolicyDecision::Reject {
            reason: NoxaRejectReason::MaxTransactionCapOverflow,
        };
    };
    let cumulative_origin_limit = scaled_max_tx / U256::from(PERCENT_DENOMINATOR);
    let Some(cumulative_origin_bought) = input
        .origin_bought_before
        .checked_add(input.expected_bought_output)
    else {
        return NoxaPolicyDecision::Reject {
            reason: NoxaRejectReason::CumulativeOriginBoughtOverflow,
        };
    };
    if cumulative_origin_bought > cumulative_origin_limit {
        return NoxaPolicyDecision::Reject {
            reason: NoxaRejectReason::MaxTransactionExceeded {
                cumulative_bought: cumulative_origin_bought,
                limit: cumulative_origin_limit,
            },
        };
    }

    NoxaPolicyDecision::Restricted {
        resulting_recipient_balance,
        cumulative_origin_bought,
        cumulative_origin_limit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(current_l1_block: u64) -> NoxaPolicyInput {
        NoxaPolicyInput {
            launch_l1_block: 100,
            restrictions_end_l1_block: 110,
            current_l1_block,
            recipient_balance_before: U256::from(10),
            expected_bought_output: U256::from(20),
            origin_bought_before: U256::from(30),
            max_wallet_limit: U256::from(100),
            max_tx_limit: U256::from(100),
        }
    }

    #[test]
    fn fails_closed_before_launch_l1_block() {
        assert_eq!(
            evaluate_noxa_policy(input(99)),
            NoxaPolicyDecision::Reject {
                reason: NoxaRejectReason::BeforeLaunchBlock {
                    current_l1_block: 99,
                    launch_l1_block: 100,
                },
            }
        );
    }

    #[test]
    fn blocks_the_launch_l1_block() {
        assert_eq!(
            evaluate_noxa_policy(input(100)),
            NoxaPolicyDecision::Reject {
                reason: NoxaRejectReason::LaunchBlockBuyBlocked {
                    launch_l1_block: 100,
                },
            }
        );
    }

    #[test]
    fn restriction_window_is_inclusive_at_both_eligible_boundaries() {
        let expected = NoxaPolicyDecision::Restricted {
            resulting_recipient_balance: U256::from(30),
            cumulative_origin_bought: U256::from(50),
            cumulative_origin_limit: U256::from(110),
        };
        assert_eq!(evaluate_noxa_policy(input(101)), expected);
        assert_eq!(evaluate_noxa_policy(input(110)), expected);
    }

    #[test]
    fn becomes_unrestricted_only_after_restrictions_end() {
        let mut value = input(111);
        // Unrestricted evaluation must not accidentally apply restricted
        // arithmetic or limits.
        value.recipient_balance_before = U256::MAX;
        value.expected_bought_output = U256::MAX;
        value.origin_bought_before = U256::MAX;
        value.max_wallet_limit = U256::ZERO;
        value.max_tx_limit = U256::MAX;
        assert_eq!(
            evaluate_noxa_policy(value),
            NoxaPolicyDecision::Unrestricted
        );
    }

    #[test]
    fn an_empty_restriction_window_is_unrestricted_after_launch() {
        let mut value = input(101);
        value.restrictions_end_l1_block = value.launch_l1_block;
        assert_eq!(
            evaluate_noxa_policy(value),
            NoxaPolicyDecision::Unrestricted
        );
    }

    #[test]
    fn permits_resulting_wallet_balance_exactly_at_limit() {
        let mut value = input(101);
        value.recipient_balance_before = U256::from(80);
        assert!(matches!(
            evaluate_noxa_policy(value),
            NoxaPolicyDecision::Restricted {
                resulting_recipient_balance,
                ..
            } if resulting_recipient_balance == U256::from(100)
        ));
    }

    #[test]
    fn rejects_resulting_wallet_balance_above_limit() {
        let mut value = input(101);
        value.recipient_balance_before = U256::from(81);
        assert_eq!(
            evaluate_noxa_policy(value),
            NoxaPolicyDecision::Reject {
                reason: NoxaRejectReason::MaxWalletExceeded {
                    resulting_balance: U256::from(101),
                    limit: U256::from(100),
                },
            }
        );
    }

    #[test]
    fn rejects_recipient_balance_overflow() {
        let mut value = input(101);
        value.recipient_balance_before = U256::MAX;
        value.max_wallet_limit = U256::MAX;
        assert_eq!(
            evaluate_noxa_policy(value),
            NoxaPolicyDecision::Reject {
                reason: NoxaRejectReason::RecipientBalanceOverflow,
            }
        );
    }

    #[test]
    fn floors_the_one_hundred_ten_percent_transaction_cap() {
        let mut value = input(101);
        value.max_tx_limit = U256::from(101);
        value.origin_bought_before = U256::from(91);
        assert_eq!(
            evaluate_noxa_policy(value),
            NoxaPolicyDecision::Restricted {
                resulting_recipient_balance: U256::from(30),
                cumulative_origin_bought: U256::from(111),
                cumulative_origin_limit: U256::from(111),
            }
        );

        value.origin_bought_before = U256::from(92);
        assert_eq!(
            evaluate_noxa_policy(value),
            NoxaPolicyDecision::Reject {
                reason: NoxaRejectReason::MaxTransactionExceeded {
                    cumulative_bought: U256::from(112),
                    limit: U256::from(111),
                },
            }
        );
    }

    #[test]
    fn rejects_max_transaction_cap_overflow() {
        let mut value = input(101);
        value.max_tx_limit = U256::MAX;
        assert_eq!(
            evaluate_noxa_policy(value),
            NoxaPolicyDecision::Reject {
                reason: NoxaRejectReason::MaxTransactionCapOverflow,
            }
        );
    }

    #[test]
    fn rejects_cumulative_origin_bought_overflow() {
        let mut value = input(101);
        value.origin_bought_before = U256::MAX;
        value.max_tx_limit = U256::MAX / U256::from(MAX_TX_MULTIPLIER_PERCENT);
        assert_eq!(
            evaluate_noxa_policy(value),
            NoxaPolicyDecision::Reject {
                reason: NoxaRejectReason::CumulativeOriginBoughtOverflow,
            }
        );
    }
}
