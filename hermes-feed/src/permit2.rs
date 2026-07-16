//! Pure Permit2 follower planning over startup-verified, warm contract pins.
//!
//! This module deliberately has no transport or persistence dependencies. A
//! caller may verify runtime identities during startup and retain the resulting
//! pins in memory. Candidate-time planning only validates that warm state and
//! constructs follower-owned constraints; it never accepts a leader permit,
//! route, signature, deadline, nonce, or minimum output.

use alloy_primitives::{Address, B256, U256};
use thiserror::Error;

pub const ROBINHOOD_CHAIN_ID: u64 = 4_663;
pub const MAX_PERMIT2_EXPIRATION: u64 = (1_u64 << 48) - 1;
pub const MAX_PERMIT2_NONCE: u64 = (1_u64 << 48) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeContractPin {
    pub address: Address,
    pub runtime_code_hash: B256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedRuntimeContract {
    pub chain_id: u64,
    pub address: Address,
    pub runtime_code_hash: B256,
}

/// A contract identity whose chain, address, and runtime hash matched an exact
/// startup pin. Its fields are intentionally private so candidate-time code
/// cannot manufacture a verified pin accidentally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedRuntimeContractPin {
    address: Address,
    runtime_code_hash: B256,
}

impl VerifiedRuntimeContractPin {
    pub fn verify(
        expected: RuntimeContractPin,
        observed: ObservedRuntimeContract,
    ) -> Result<Self, Permit2PinError> {
        validate_pin(expected)?;
        if observed.chain_id != ROBINHOOD_CHAIN_ID {
            return Err(Permit2PinError::WrongChain {
                expected: ROBINHOOD_CHAIN_ID,
                actual: observed.chain_id,
            });
        }
        if observed.address != expected.address {
            return Err(Permit2PinError::AddressMismatch);
        }
        if observed.runtime_code_hash != expected.runtime_code_hash {
            return Err(Permit2PinError::RuntimeCodeHashMismatch);
        }
        Ok(Self {
            address: expected.address,
            runtime_code_hash: expected.runtime_code_hash,
        })
    }

    pub const fn address(self) -> Address {
        self.address
    }

    pub const fn runtime_code_hash(self) -> B256 {
        self.runtime_code_hash
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum Permit2PinError {
    #[error("Permit2 planning is restricted to chain {expected}, got {actual}")]
    WrongChain { expected: u64, actual: u64 },
    #[error("contract pin address is zero")]
    ZeroAddress,
    #[error("contract runtime code hash is zero")]
    ZeroRuntimeCodeHash,
    #[error("observed contract address does not match the startup pin")]
    AddressMismatch,
    #[error("observed runtime code hash does not match the startup pin")]
    RuntimeCodeHashMismatch,
}

fn validate_pin(pin: RuntimeContractPin) -> Result<(), Permit2PinError> {
    if pin.address == Address::ZERO {
        return Err(Permit2PinError::ZeroAddress);
    }
    if pin.runtime_code_hash == B256::ZERO {
        return Err(Permit2PinError::ZeroRuntimeCodeHash);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permit2WarmState {
    pub chain_id: u64,
    pub permit2: Option<VerifiedRuntimeContractPin>,
    pub router: Option<VerifiedRuntimeContractPin>,
}

impl Default for Permit2WarmState {
    fn default() -> Self {
        Self {
            chain_id: ROBINHOOD_CHAIN_ID,
            permit2: None,
            router: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FollowerPermit2Request {
    pub owner: Address,
    pub token: Address,
    pub spender: Address,
    pub recipient: Address,
    pub amount: U256,
    pub min_receive: U256,
    pub deadline: u64,
    pub nonce: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permit2FollowerPlan {
    pub chain_id: u64,
    pub permit2: Address,
    pub router: Address,
    pub owner: Address,
    pub token: Address,
    pub spender: Address,
    pub recipient: Address,
    pub amount: U256,
    pub min_receive: U256,
    pub deadline: u64,
    pub nonce: u64,
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum Permit2PlanError {
    #[error("Permit2 planning is restricted to chain {expected}, got {actual}")]
    WrongChain { expected: u64, actual: u64 },
    #[error("Permit2 execution is disabled because its verified runtime pin is unavailable")]
    MissingPermit2Pin,
    #[error("Permit2 execution is disabled because the verified router runtime pin is unavailable")]
    MissingRouterPin,
    #[error("owner is zero")]
    ZeroOwner,
    #[error("token is zero")]
    ZeroToken,
    #[error("spender is zero")]
    ZeroSpender,
    #[error("recipient is zero")]
    ZeroRecipient,
    #[error("amount is zero")]
    ZeroAmount,
    #[error("minimum receive is zero")]
    ZeroMinReceive,
    #[error("amount exceeds Permit2 uint160 capacity")]
    AmountOverflow,
    #[error("deadline exceeds Permit2 uint48 capacity")]
    DeadlineOverflow,
    #[error("nonce exceeds Permit2 uint48 capacity")]
    NonceOverflow,
    #[error("permit deadline is expired")]
    Expired,
    #[error("follower-selected spender is not the pinned router")]
    WrongSpender,
}

impl Permit2WarmState {
    /// Builds a fresh follower plan from warm pins and follower policy only.
    ///
    /// There is intentionally no leader-call input to this API. In particular,
    /// a leader's permit, route, amount, minimum output, deadline, nonce, and
    /// recipient cannot be inherited by construction.
    pub fn plan(
        &self,
        request: FollowerPermit2Request,
        now_unix_seconds: u64,
    ) -> Result<Permit2FollowerPlan, Permit2PlanError> {
        if self.chain_id != ROBINHOOD_CHAIN_ID {
            return Err(Permit2PlanError::WrongChain {
                expected: ROBINHOOD_CHAIN_ID,
                actual: self.chain_id,
            });
        }
        let permit2 = self.permit2.ok_or(Permit2PlanError::MissingPermit2Pin)?;
        let router = self.router.ok_or(Permit2PlanError::MissingRouterPin)?;

        if request.owner == Address::ZERO {
            return Err(Permit2PlanError::ZeroOwner);
        }
        if request.token == Address::ZERO {
            return Err(Permit2PlanError::ZeroToken);
        }
        if request.spender == Address::ZERO {
            return Err(Permit2PlanError::ZeroSpender);
        }
        if request.recipient == Address::ZERO {
            return Err(Permit2PlanError::ZeroRecipient);
        }
        if request.amount == U256::ZERO {
            return Err(Permit2PlanError::ZeroAmount);
        }
        if request.min_receive == U256::ZERO {
            return Err(Permit2PlanError::ZeroMinReceive);
        }
        if request.amount > max_uint160() {
            return Err(Permit2PlanError::AmountOverflow);
        }
        if request.deadline > MAX_PERMIT2_EXPIRATION {
            return Err(Permit2PlanError::DeadlineOverflow);
        }
        if request.nonce > MAX_PERMIT2_NONCE {
            return Err(Permit2PlanError::NonceOverflow);
        }
        if request.deadline <= now_unix_seconds {
            return Err(Permit2PlanError::Expired);
        }
        if request.spender != router.address() {
            return Err(Permit2PlanError::WrongSpender);
        }

        Ok(Permit2FollowerPlan {
            chain_id: self.chain_id,
            permit2: permit2.address(),
            router: router.address(),
            owner: request.owner,
            token: request.token,
            spender: request.spender,
            recipient: request.recipient,
            amount: request.amount,
            min_receive: request.min_receive,
            deadline: request.deadline,
            nonce: request.nonce,
        })
    }
}

fn max_uint160() -> U256 {
    (U256::from(1) << 160) - U256::from(1)
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{Address, B256};

    use super::*;

    fn pin(address_byte: u8, hash_byte: u8) -> RuntimeContractPin {
        RuntimeContractPin {
            address: Address::with_last_byte(address_byte),
            runtime_code_hash: B256::with_last_byte(hash_byte),
        }
    }

    fn verified(pin: RuntimeContractPin) -> VerifiedRuntimeContractPin {
        VerifiedRuntimeContractPin::verify(
            pin,
            ObservedRuntimeContract {
                chain_id: ROBINHOOD_CHAIN_ID,
                address: pin.address,
                runtime_code_hash: pin.runtime_code_hash,
            },
        )
        .unwrap()
    }

    fn warm_state() -> Permit2WarmState {
        Permit2WarmState {
            chain_id: ROBINHOOD_CHAIN_ID,
            permit2: Some(verified(pin(1, 11))),
            router: Some(verified(pin(2, 12))),
        }
    }

    fn request() -> FollowerPermit2Request {
        FollowerPermit2Request {
            owner: Address::with_last_byte(3),
            token: Address::with_last_byte(4),
            spender: Address::with_last_byte(2),
            recipient: Address::with_last_byte(5),
            amount: U256::from(1_000),
            min_receive: U256::from(900),
            deadline: 1_100,
            nonce: 7,
        }
    }

    #[test]
    fn builds_fresh_follower_owned_plan() {
        let plan = warm_state().plan(request(), 1_000).unwrap();
        assert_eq!(plan.permit2, Address::with_last_byte(1));
        assert_eq!(plan.router, Address::with_last_byte(2));
        assert_eq!(plan.owner, Address::with_last_byte(3));
        assert_eq!(plan.recipient, Address::with_last_byte(5));
        assert_eq!(plan.amount, U256::from(1_000));
        assert_eq!(plan.min_receive, U256::from(900));
        assert_eq!(plan.deadline, 1_100);
        assert_eq!(plan.nonce, 7);

        let mut independent = request();
        independent.amount = U256::from(333);
        independent.min_receive = U256::from(222);
        independent.deadline = 1_200;
        independent.nonce = 8;
        independent.recipient = Address::with_last_byte(6);
        let independent_plan = warm_state().plan(independent, 1_000).unwrap();
        assert_eq!(independent_plan.amount, U256::from(333));
        assert_eq!(independent_plan.min_receive, U256::from(222));
        assert_eq!(independent_plan.deadline, 1_200);
        assert_eq!(independent_plan.nonce, 8);
        assert_eq!(independent_plan.recipient, Address::with_last_byte(6));
    }

    #[test]
    fn execution_is_default_off_without_both_verified_pins() {
        let state = Permit2WarmState::default();
        assert_eq!(
            state.plan(request(), 1_000),
            Err(Permit2PlanError::MissingPermit2Pin)
        );

        let state = Permit2WarmState {
            permit2: Some(verified(pin(1, 11))),
            ..Permit2WarmState::default()
        };
        assert_eq!(
            state.plan(request(), 1_000),
            Err(Permit2PlanError::MissingRouterPin)
        );
    }

    #[test]
    fn rejects_wrong_runtime_pins() {
        let expected = pin(1, 11);
        let wrong_address = ObservedRuntimeContract {
            chain_id: ROBINHOOD_CHAIN_ID,
            address: Address::with_last_byte(9),
            runtime_code_hash: expected.runtime_code_hash,
        };
        assert_eq!(
            VerifiedRuntimeContractPin::verify(expected, wrong_address),
            Err(Permit2PinError::AddressMismatch)
        );

        let wrong_hash = ObservedRuntimeContract {
            address: expected.address,
            runtime_code_hash: B256::with_last_byte(99),
            ..wrong_address
        };
        assert_eq!(
            VerifiedRuntimeContractPin::verify(expected, wrong_hash),
            Err(Permit2PinError::RuntimeCodeHashMismatch)
        );
    }

    #[test]
    fn rejects_wrong_chain_for_pinning_and_planning() {
        let expected = pin(1, 11);
        let observed = ObservedRuntimeContract {
            chain_id: 8_453,
            address: expected.address,
            runtime_code_hash: expected.runtime_code_hash,
        };
        assert_eq!(
            VerifiedRuntimeContractPin::verify(expected, observed),
            Err(Permit2PinError::WrongChain {
                expected: ROBINHOOD_CHAIN_ID,
                actual: 8_453,
            })
        );

        let state = Permit2WarmState {
            chain_id: 8_453,
            ..warm_state()
        };
        assert_eq!(
            state.plan(request(), 1_000),
            Err(Permit2PlanError::WrongChain {
                expected: ROBINHOOD_CHAIN_ID,
                actual: 8_453,
            })
        );
    }

    #[test]
    fn rejects_expiry_zeroes_and_wrong_spender() {
        let state = warm_state();
        let mut invalid = request();
        invalid.deadline = 1_000;
        assert_eq!(state.plan(invalid, 1_000), Err(Permit2PlanError::Expired));

        invalid = request();
        invalid.amount = U256::ZERO;
        assert_eq!(
            state.plan(invalid, 1_000),
            Err(Permit2PlanError::ZeroAmount)
        );

        invalid = request();
        invalid.min_receive = U256::ZERO;
        assert_eq!(
            state.plan(invalid, 1_000),
            Err(Permit2PlanError::ZeroMinReceive)
        );

        invalid = request();
        invalid.spender = Address::with_last_byte(99);
        assert_eq!(
            state.plan(invalid, 1_000),
            Err(Permit2PlanError::WrongSpender)
        );
    }

    #[test]
    fn rejects_permit2_field_overflow() {
        let state = warm_state();
        let mut invalid = request();
        invalid.amount = U256::from(1) << 160;
        assert_eq!(
            state.plan(invalid, 1_000),
            Err(Permit2PlanError::AmountOverflow)
        );

        invalid = request();
        invalid.deadline = MAX_PERMIT2_EXPIRATION + 1;
        assert_eq!(
            state.plan(invalid, 1_000),
            Err(Permit2PlanError::DeadlineOverflow)
        );

        invalid = request();
        invalid.nonce = MAX_PERMIT2_NONCE + 1;
        assert_eq!(
            state.plan(invalid, 1_000),
            Err(Permit2PlanError::NonceOverflow)
        );
    }

    #[test]
    fn rejects_zero_pin_components() {
        let observed = ObservedRuntimeContract {
            chain_id: ROBINHOOD_CHAIN_ID,
            address: Address::with_last_byte(1),
            runtime_code_hash: B256::with_last_byte(1),
        };
        assert_eq!(
            VerifiedRuntimeContractPin::verify(
                RuntimeContractPin {
                    address: Address::ZERO,
                    runtime_code_hash: B256::with_last_byte(1),
                },
                observed,
            ),
            Err(Permit2PinError::ZeroAddress)
        );
        assert_eq!(
            VerifiedRuntimeContractPin::verify(
                RuntimeContractPin {
                    address: Address::with_last_byte(1),
                    runtime_code_hash: B256::ZERO,
                },
                observed,
            ),
            Err(Permit2PinError::ZeroRuntimeCodeHash)
        );
    }
}
