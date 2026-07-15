use std::collections::HashSet;

use alloy_primitives::{Address, B256, U256};
use serde::Serialize;
use thiserror::Error;

use crate::noxa_abi::V3ExactInputIntent;
use crate::robinhood::{CHAIN_ID, NOXA_POOL_FEE, UNISWAP_V3_SWAP_ROUTER_02, WETH};

#[derive(Debug, Clone)]
pub struct WatchedWalletCopyPolicy {
    watched_wallets: HashSet<Address>,
    allowed_tokens: HashSet<Address>,
    follower_entry_amount: U256,
    max_leader_entry_amount: U256,
    max_triggers: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedCopySwap {
    pub tx_hash: B256,
    pub chain_id: Option<u64>,
    pub from: Address,
    pub to: Address,
    pub value: U256,
    pub intent: V3ExactInputIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyPosition {
    pub token: Address,
    pub token_amount: U256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "side")]
pub enum CopyDecision {
    Entry {
        leader: Address,
        token: Address,
        follower_amount_in: U256,
        follower_minimum_out: U256,
    },
    Exit {
        leader: Address,
        token: Address,
        follower_amount_in: U256,
        follower_minimum_out: U256,
    },
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyRejectReason {
    #[error("copy trigger limit reached")]
    TriggerLimit,
    #[error("transaction is not on the pinned Robinhood mainnet chain")]
    WrongChain,
    #[error("transaction does not call the canonical SwapRouter02")]
    WrongRouter,
    #[error("transaction signer is not watched")]
    UnwatchedWallet,
    #[error("router recipient must be the watched signer")]
    RedirectedRecipient,
    #[error("copy transaction must use pre-wrapped tokens and zero native value")]
    NonZeroValue,
    #[error("copy transaction is not a pinned-fee single-hop WETH pair")]
    UnsupportedPair,
    #[error("copy transaction uses a non-zero price limit")]
    PriceLimit,
    #[error("leader amounts and limit price must be non-zero")]
    ZeroAmount,
    #[error("token is not explicitly allowlisted")]
    TokenNotAllowed,
    #[error("leader entry exceeds its configured cap")]
    LeaderEntryCap,
    #[error("entry ignored while a follower position is already open")]
    PositionAlreadyOpen,
    #[error("exit ignored because the follower has no matching position")]
    PositionMissing,
    #[error("scaled follower limit price is invalid or overflows")]
    LimitPrice,
}

impl WatchedWalletCopyPolicy {
    pub fn new(
        watched_wallets: HashSet<Address>,
        allowed_tokens: HashSet<Address>,
        follower_entry_amount: U256,
        max_leader_entry_amount: U256,
        max_triggers: u64,
    ) -> Result<Self, CopyRejectReason> {
        if watched_wallets.is_empty()
            || follower_entry_amount == U256::ZERO
            || max_leader_entry_amount == U256::ZERO
            || max_triggers == 0
        {
            return Err(CopyRejectReason::ZeroAmount);
        }
        if allowed_tokens.contains(&Address::ZERO) || allowed_tokens.contains(&WETH) {
            return Err(CopyRejectReason::TokenNotAllowed);
        }
        Ok(Self {
            watched_wallets,
            allowed_tokens,
            follower_entry_amount,
            max_leader_entry_amount,
            max_triggers,
        })
    }

    pub fn watched_wallets(&self) -> &HashSet<Address> {
        &self.watched_wallets
    }

    pub fn allowed_tokens(&self) -> &HashSet<Address> {
        &self.allowed_tokens
    }

    pub fn evaluate(
        &self,
        observed: &ObservedCopySwap,
        follower_position: Option<CopyPosition>,
        admitted_triggers: u64,
    ) -> Result<CopyDecision, CopyRejectReason> {
        self.evaluate_inner(observed, follower_position, admitted_triggers, false, true)
    }

    /// Evaluate a candidate whose token/pool identity was independently
    /// validated by the runtime registry. This is the path used for tokens
    /// learned from verified NOXA launch receipts or asynchronous RPC proof.
    pub fn evaluate_validated(
        &self,
        observed: &ObservedCopySwap,
        follower_position: Option<CopyPosition>,
        admitted_triggers: u64,
    ) -> Result<CopyDecision, CopyRejectReason> {
        self.evaluate_inner(observed, follower_position, admitted_triggers, true, true)
    }

    /// Evaluate an independently validated candidate without a preselected
    /// leader. This is intended only for paper discovery; signed runtimes must
    /// continue to use `evaluate_validated` and its explicit wallet allowlist.
    pub fn evaluate_validated_discovered(
        &self,
        observed: &ObservedCopySwap,
        follower_position: Option<CopyPosition>,
        admitted_triggers: u64,
    ) -> Result<CopyDecision, CopyRejectReason> {
        self.evaluate_inner(observed, follower_position, admitted_triggers, true, false)
    }

    fn evaluate_inner(
        &self,
        observed: &ObservedCopySwap,
        follower_position: Option<CopyPosition>,
        admitted_triggers: u64,
        independently_validated_token: bool,
        require_watched_wallet: bool,
    ) -> Result<CopyDecision, CopyRejectReason> {
        if admitted_triggers >= self.max_triggers {
            return Err(CopyRejectReason::TriggerLimit);
        }
        if observed.chain_id != Some(CHAIN_ID) {
            return Err(CopyRejectReason::WrongChain);
        }
        if observed.to != UNISWAP_V3_SWAP_ROUTER_02 {
            return Err(CopyRejectReason::WrongRouter);
        }
        if require_watched_wallet && !self.watched_wallets.contains(&observed.from) {
            return Err(CopyRejectReason::UnwatchedWallet);
        }
        if observed.intent.recipient != observed.from {
            return Err(CopyRejectReason::RedirectedRecipient);
        }
        if observed.value != U256::ZERO {
            return Err(CopyRejectReason::NonZeroValue);
        }
        if observed.intent.fee != NOXA_POOL_FEE
            || !is_weth_pair(observed.intent.token_in, observed.intent.token_out)
        {
            return Err(CopyRejectReason::UnsupportedPair);
        }
        if observed.intent.sqrt_price_limit_x96 != U256::ZERO {
            return Err(CopyRejectReason::PriceLimit);
        }
        if observed.intent.amount_in == U256::ZERO
            || observed.intent.amount_out_minimum == U256::ZERO
        {
            return Err(CopyRejectReason::ZeroAmount);
        }

        let token = if observed.intent.token_in == WETH {
            observed.intent.token_out
        } else {
            observed.intent.token_in
        };
        if !independently_validated_token && !self.allowed_tokens.contains(&token) {
            return Err(CopyRejectReason::TokenNotAllowed);
        }

        if observed.intent.token_in == WETH {
            if follower_position.is_some() {
                return Err(CopyRejectReason::PositionAlreadyOpen);
            }
            if observed.intent.amount_in > self.max_leader_entry_amount {
                return Err(CopyRejectReason::LeaderEntryCap);
            }
            let follower_minimum_out = scale_limit_price(
                observed.intent.amount_out_minimum,
                self.follower_entry_amount,
                observed.intent.amount_in,
            )?;
            Ok(CopyDecision::Entry {
                leader: observed.from,
                token,
                follower_amount_in: self.follower_entry_amount,
                follower_minimum_out,
            })
        } else {
            let position = follower_position
                .filter(|position| position.token == token && position.token_amount > U256::ZERO)
                .ok_or(CopyRejectReason::PositionMissing)?;
            let follower_minimum_out = scale_limit_price(
                observed.intent.amount_out_minimum,
                position.token_amount,
                observed.intent.amount_in,
            )?;
            Ok(CopyDecision::Exit {
                leader: observed.from,
                token,
                follower_amount_in: position.token_amount,
                follower_minimum_out,
            })
        }
    }
}

fn is_weth_pair(token_in: Address, token_out: Address) -> bool {
    token_in != Address::ZERO
        && token_out != Address::ZERO
        && token_in != token_out
        && ((token_in == WETH) ^ (token_out == WETH))
}

fn scale_limit_price(
    leader_minimum_out: U256,
    follower_amount_in: U256,
    leader_amount_in: U256,
) -> Result<U256, CopyRejectReason> {
    let scaled = leader_minimum_out
        .checked_mul(follower_amount_in)
        .and_then(|value| value.checked_div(leader_amount_in))
        .ok_or(CopyRejectReason::LimitPrice)?;
    if scaled == U256::ZERO {
        return Err(CopyRejectReason::LimitPrice);
    }
    Ok(scaled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leader() -> Address {
        Address::with_last_byte(1)
    }

    fn token() -> Address {
        Address::with_last_byte(2)
    }

    fn policy() -> WatchedWalletCopyPolicy {
        WatchedWalletCopyPolicy::new(
            HashSet::from([leader()]),
            HashSet::from([token()]),
            U256::from(100),
            U256::from(1_000),
            2,
        )
        .unwrap()
    }

    fn observed(
        token_in: Address,
        token_out: Address,
        amount_in: u64,
        minimum_out: u64,
    ) -> ObservedCopySwap {
        ObservedCopySwap {
            tx_hash: B256::with_last_byte(9),
            chain_id: Some(CHAIN_ID),
            from: leader(),
            to: UNISWAP_V3_SWAP_ROUTER_02,
            value: U256::ZERO,
            intent: V3ExactInputIntent {
                token_in,
                token_out,
                fee: NOXA_POOL_FEE,
                recipient: leader(),
                amount_in: U256::from(amount_in),
                amount_out_minimum: U256::from(minimum_out),
                sqrt_price_limit_x96: U256::ZERO,
            },
        }
    }

    #[test]
    fn scales_entry_without_copying_the_leaders_size() {
        let decision = policy()
            .evaluate(&observed(WETH, token(), 200, 500), None, 0)
            .unwrap();
        assert_eq!(
            decision,
            CopyDecision::Entry {
                leader: leader(),
                token: token(),
                follower_amount_in: U256::from(100),
                follower_minimum_out: U256::from(250),
            }
        );
    }

    #[test]
    fn paper_discovery_can_admit_an_unlisted_leader_without_weakening_normal_policy() {
        let mut candidate = observed(WETH, token(), 200, 500);
        candidate.from = Address::with_last_byte(99);
        candidate.intent.recipient = candidate.from;
        assert_eq!(
            policy().evaluate_validated(&candidate, None, 0),
            Err(CopyRejectReason::UnwatchedWallet)
        );
        assert!(matches!(
            policy().evaluate_validated_discovered(&candidate, None, 0),
            Ok(CopyDecision::Entry { leader, .. }) if leader == candidate.from
        ));
    }

    #[test]
    fn exits_the_full_follower_position_at_the_leaders_limit_price() {
        let decision = policy()
            .evaluate(
                &observed(token(), WETH, 400, 160),
                Some(CopyPosition {
                    token: token(),
                    token_amount: U256::from(250),
                }),
                1,
            )
            .unwrap();
        assert_eq!(
            decision,
            CopyDecision::Exit {
                leader: leader(),
                token: token(),
                follower_amount_in: U256::from(250),
                follower_minimum_out: U256::from(100),
            }
        );
    }

    #[test]
    fn rejects_unwatched_redirected_unlisted_and_busy_entries() {
        let mut unwatched = observed(WETH, token(), 100, 100);
        unwatched.from = Address::with_last_byte(8);
        unwatched.intent.recipient = unwatched.from;
        assert_eq!(
            policy().evaluate(&unwatched, None, 0),
            Err(CopyRejectReason::UnwatchedWallet)
        );

        let mut redirected = observed(WETH, token(), 100, 100);
        redirected.intent.recipient = Address::with_last_byte(7);
        assert_eq!(
            policy().evaluate(&redirected, None, 0),
            Err(CopyRejectReason::RedirectedRecipient)
        );

        let unlisted = observed(WETH, Address::with_last_byte(6), 100, 100);
        assert_eq!(
            policy().evaluate(&unlisted, None, 0),
            Err(CopyRejectReason::TokenNotAllowed)
        );

        assert_eq!(
            policy().evaluate(
                &observed(WETH, token(), 100, 100),
                Some(CopyPosition {
                    token: token(),
                    token_amount: U256::from(1),
                }),
                0,
            ),
            Err(CopyRejectReason::PositionAlreadyOpen)
        );
    }

    #[test]
    fn rejects_missing_positions_unsafe_limits_and_trigger_overflow() {
        assert_eq!(
            policy().evaluate(&observed(token(), WETH, 100, 100), None, 0),
            Err(CopyRejectReason::PositionMissing)
        );
        assert_eq!(
            policy().evaluate(&observed(WETH, token(), 1_001, 100), None, 0),
            Err(CopyRejectReason::LeaderEntryCap)
        );
        assert_eq!(
            policy().evaluate(&observed(WETH, token(), 100, 100), None, 2),
            Err(CopyRejectReason::TriggerLimit)
        );
        assert_eq!(
            policy().evaluate(&observed(WETH, token(), 1_000, 1), None, 0),
            Err(CopyRejectReason::LimitPrice)
        );
    }

    #[test]
    fn independently_validated_tokens_do_not_need_a_static_allowlist() {
        let policy = WatchedWalletCopyPolicy::new(
            HashSet::from([leader()]),
            HashSet::new(),
            U256::from(100),
            U256::from(1_000),
            2,
        )
        .unwrap();
        let candidate = observed(WETH, token(), 200, 500);
        assert_eq!(
            policy.evaluate(&candidate, None, 0),
            Err(CopyRejectReason::TokenNotAllowed)
        );
        assert!(policy.evaluate_validated(&candidate, None, 0).is_ok());
    }
}
