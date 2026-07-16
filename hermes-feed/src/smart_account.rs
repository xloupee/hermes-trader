//! Strict, I/O-free ERC-4337 smart-account observation decoding.
//!
//! This module intentionally supports one small profile: EntryPoint v0.7
//! `handleOps(PackedUserOperation[], address)` calls whose single user operation
//! invokes either a startup-pinned account's `execute(address,uint256,bytes)`
//! method or the canonical ERC-7579 single-call
//! `execute(bytes32,bytes)` method. It does not support account deployment,
//! paymasters, batches, custom execution modes, arbitrary account ABIs, or
//! candidate-time code discovery.

use alloy_primitives::{Address, B256, Bytes, U256};
use alloy_sol_types::{SolCall, sol};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const ROBINHOOD_CHAIN_ID: u64 = 4_663;
pub const ENTRY_POINT_V07: Address =
    alloy_primitives::address!("0000000071727de22e5e9d8baf0edac6f37da032");
pub const MAX_USER_OPERATIONS: usize = 4;
pub const MAX_INNER_CALLS: usize = 2;
pub const MAX_UNWRAP_DEPTH: usize = 2;
pub const MAX_ENTRY_POINT_CALLDATA_BYTES: usize = 64 * 1024;
pub const MAX_ACCOUNT_CALLDATA_BYTES: usize = 32 * 1024;

sol! {
    struct PackedUserOperation {
        address sender;
        uint256 nonce;
        bytes initCode;
        bytes callData;
        bytes32 accountGasLimits;
        uint256 preVerificationGas;
        bytes32 gasFees;
        bytes paymasterAndData;
        bytes signature;
    }

    function handleOps(PackedUserOperation[] ops, address payable beneficiary) external;

    function execute(address dest, uint256 value, bytes func) external;
}

mod erc7579 {
    use alloy_sol_types::sol;

    sol! {
        function execute(bytes32 mode, bytes executionCalldata) external;
    }
}

pub const ENTRY_POINT_V07_HANDLE_OPS_SELECTOR: [u8; 4] = handleOpsCall::SELECTOR;
pub const ACCOUNT_EXECUTE_SELECTOR: [u8; 4] = executeCall::SELECTOR;
pub const ERC7579_EXECUTE_SELECTOR: [u8; 4] = erc7579::executeCall::SELECTOR;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountExecutionProfile {
    ExecuteAddressValueBytes,
    Erc7579SingleCall,
}

/// A contract identity verified and frozen during startup.
///
/// Supplying this value asserts that `runtime_code_hash` was observed for
/// `address` before candidate processing began. The decoder performs no RPC or
/// other I/O and rejects zero/ambiguous pins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractPin {
    pub address: Address,
    pub runtime_code_hash: B256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmartAccountPin {
    pub account: ContractPin,
    pub execution_profile: AccountExecutionProfile,
    /// Optional deployed-account provenance. `initCode` remains forbidden even
    /// when a factory is pinned; this decoder accepts already-deployed accounts
    /// only.
    pub factory: Option<ContractPin>,
    /// Required for EIP-7702 accounts whose account code is the
    /// `ef0100 || implementation` designator. The designator hash belongs in
    /// `account`; the delegated implementation runtime hash belongs here.
    pub delegation_implementation: Option<ContractPin>,
}

/// Immutable startup state used by the candidate decoder.
#[derive(Debug, Clone, Copy)]
pub struct SmartAccountPins<'a> {
    pub entry_point: ContractPin,
    pub accounts: &'a [SmartAccountPin],
    pub allowed_targets: &'a [ContractPin],
}

/// Startup-validated smart-account identities for allocation-free reuse by the
/// candidate path. Construction performs all duplicate and overlap checks.
#[derive(Debug, Clone, Copy)]
pub struct ValidatedSmartAccountPins<'a> {
    pins: SmartAccountPins<'a>,
}

impl<'a> ValidatedSmartAccountPins<'a> {
    pub fn new(pins: SmartAccountPins<'a>) -> Result<Self, SmartAccountDecodeError> {
        validate_pins(pins)?;
        Ok(Self { pins })
    }
}

/// Owned startup-validated smart-account identities. This is suitable for
/// long-lived runtimes that cannot safely store a self-referential borrowed
/// [`ValidatedSmartAccountPins`]. Candidate access only borrows the immutable
/// slices and performs no allocation or repeated pin validation.
#[derive(Debug, Clone)]
pub struct OwnedValidatedSmartAccountPins {
    entry_point: ContractPin,
    accounts: Box<[SmartAccountPin]>,
    allowed_targets: Box<[ContractPin]>,
}

impl OwnedValidatedSmartAccountPins {
    pub fn new(
        entry_point: ContractPin,
        accounts: Vec<SmartAccountPin>,
        allowed_targets: Vec<ContractPin>,
    ) -> Result<Self, SmartAccountDecodeError> {
        validate_pins(SmartAccountPins {
            entry_point,
            accounts: &accounts,
            allowed_targets: &allowed_targets,
        })?;
        Ok(Self {
            entry_point,
            accounts: accounts.into_boxed_slice(),
            allowed_targets: allowed_targets.into_boxed_slice(),
        })
    }

    pub const fn entry_point(&self) -> ContractPin {
        self.entry_point
    }

    pub fn validated(&self) -> ValidatedSmartAccountPins<'_> {
        ValidatedSmartAccountPins {
            pins: SmartAccountPins {
                entry_point: self.entry_point,
                accounts: &self.accounts,
                allowed_targets: &self.allowed_targets,
            },
        }
    }
}

/// The outer transaction facts plus the EntryPoint code identity observed in
/// the validated warm snapshot.
#[derive(Debug, Clone, Copy)]
pub struct EntryPointCall<'a> {
    pub chain_id: u64,
    pub destination: ContractPin,
    pub outer_bundler: Address,
    pub calldata: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnwrappedSmartAccountCall {
    /// ERC-4337 leader identity. This is the validated UserOperation sender,
    /// never the bundler, EntryPoint, beneficiary, paymaster, or factory.
    pub leader: Address,
    pub outer_bundler: Address,
    pub entry_point: Address,
    pub beneficiary: Address,
    pub account_factory: Option<Address>,
    pub execution_profile: AccountExecutionProfile,
    pub delegation_implementation: Option<Address>,
    pub target: Address,
    pub value: U256,
    pub calldata: Bytes,
    pub unwrap_depth: usize,
    pub inner_call_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SmartAccountDecodeError {
    #[error("chain ID {actual} is not Robinhood Chain {expected}")]
    WrongChain { expected: u64, actual: u64 },
    #[error("invalid zero {role} pin")]
    InvalidPin { role: &'static str },
    #[error("ambiguous duplicate or overlapping pin for {address}")]
    AmbiguousPin { address: Address },
    #[error("outer destination is not the pinned EntryPoint")]
    EntryPointAddressMismatch,
    #[error("outer destination runtime code hash does not match the pinned EntryPoint")]
    EntryPointCodeHashMismatch,
    #[error("EntryPoint calldata is {actual} bytes; maximum is {maximum}")]
    EntryPointCalldataTooLarge { actual: usize, maximum: usize },
    #[error("account calldata is {actual} bytes; maximum is {maximum}")]
    AccountCalldataTooLarge { actual: usize, maximum: usize },
    #[error("calldata does not use EntryPoint v0.7 handleOps")]
    WrongEntryPointSelector,
    #[error("EntryPoint v0.7 calldata is malformed or noncanonical")]
    MalformedEntryPointCall,
    #[error("handleOps contains no user operations")]
    NoUserOperations,
    #[error("handleOps contains {actual} user operations; maximum is {maximum}")]
    ExcessiveUserOperations { actual: usize, maximum: usize },
    #[error("handleOps contains {actual} user operations and has ambiguous leader/call semantics")]
    AmbiguousUserOperations { actual: usize },
    #[error("user operation for {sender} contains initCode")]
    InitCodeNotAllowed { sender: Address },
    #[error("user operation for {sender} contains paymasterAndData")]
    PaymasterNotAllowed { sender: Address },
    #[error("user operation sender {sender} is not a pinned smart account")]
    UnknownSmartAccount { sender: Address },
    #[error("smart-account calldata does not use a supported pinned execute ABI")]
    WrongAccountSelector,
    #[error("smart-account calldata selector does not match its pinned execution profile")]
    ExecutionProfileMismatch,
    #[error("smart-account execute calldata is malformed or noncanonical")]
    MalformedAccountCall,
    #[error("ERC-7579 execution mode is not the canonical single-call revert mode")]
    UnsupportedErc7579Mode,
    #[error("ERC-7579 single-call execution data is malformed")]
    MalformedErc7579Execution,
    #[error("inner target {target} is neither an allowed target nor a pinned smart account")]
    UnknownTarget { target: Address },
    #[error("intermediate smart-account hop {target} carries non-zero native value")]
    IntermediateValueNotAllowed { target: Address },
    #[error("inner call count exceeded fixed maximum {maximum}")]
    ExcessiveInnerCalls { maximum: usize },
    #[error("smart-account unwrap depth exceeded fixed maximum {maximum}")]
    ExcessiveUnwrapDepth { maximum: usize },
}

/// Decode one unambiguous EntryPoint v0.7 operation into its final allowlisted
/// inner call.
///
/// All contract identities and allowlists come from `pins`. This function is
/// deterministic and performs no network, filesystem, database, logging, or
/// control-plane operation.
pub fn decode_entry_point_v07(
    observed: EntryPointCall<'_>,
    pins: SmartAccountPins<'_>,
) -> Result<UnwrappedSmartAccountCall, SmartAccountDecodeError> {
    let pins = ValidatedSmartAccountPins::new(pins)?;
    decode_entry_point_v07_prevalidated(observed, pins)
}

/// Candidate-time decoder for identities validated once during startup.
pub fn decode_entry_point_v07_prevalidated(
    observed: EntryPointCall<'_>,
    validated: ValidatedSmartAccountPins<'_>,
) -> Result<UnwrappedSmartAccountCall, SmartAccountDecodeError> {
    let pins = validated.pins;
    if observed.chain_id != ROBINHOOD_CHAIN_ID {
        return Err(SmartAccountDecodeError::WrongChain {
            expected: ROBINHOOD_CHAIN_ID,
            actual: observed.chain_id,
        });
    }
    if observed.destination.address != pins.entry_point.address {
        return Err(SmartAccountDecodeError::EntryPointAddressMismatch);
    }
    if observed.destination.runtime_code_hash != pins.entry_point.runtime_code_hash {
        return Err(SmartAccountDecodeError::EntryPointCodeHashMismatch);
    }
    if observed.calldata.len() > MAX_ENTRY_POINT_CALLDATA_BYTES {
        return Err(SmartAccountDecodeError::EntryPointCalldataTooLarge {
            actual: observed.calldata.len(),
            maximum: MAX_ENTRY_POINT_CALLDATA_BYTES,
        });
    }
    if observed.calldata.get(..4) != Some(ENTRY_POINT_V07_HANDLE_OPS_SELECTOR.as_slice()) {
        return Err(SmartAccountDecodeError::WrongEntryPointSelector);
    }

    // Read the array count before alloy allocates decoded dynamic fields. The
    // canonical top-level offset for two arguments is exactly two words.
    let operation_count = bounded_operation_count(observed.calldata)?;
    if operation_count == 0 {
        return Err(SmartAccountDecodeError::NoUserOperations);
    }
    if operation_count > MAX_USER_OPERATIONS {
        return Err(SmartAccountDecodeError::ExcessiveUserOperations {
            actual: operation_count,
            maximum: MAX_USER_OPERATIONS,
        });
    }

    let call = handleOpsCall::abi_decode(observed.calldata)
        .map_err(|_| SmartAccountDecodeError::MalformedEntryPointCall)?;
    if call.abi_encode().as_slice() != observed.calldata {
        return Err(SmartAccountDecodeError::MalformedEntryPointCall);
    }
    if call.ops.len() != operation_count {
        return Err(SmartAccountDecodeError::MalformedEntryPointCall);
    }
    if call.ops.len() != 1 {
        return Err(SmartAccountDecodeError::AmbiguousUserOperations {
            actual: call.ops.len(),
        });
    }

    let mut operations = call.ops;
    let operation = operations.pop().expect("one operation checked above");
    if !operation.initCode.is_empty() {
        return Err(SmartAccountDecodeError::InitCodeNotAllowed {
            sender: operation.sender,
        });
    }
    if !operation.paymasterAndData.is_empty() {
        return Err(SmartAccountDecodeError::PaymasterNotAllowed {
            sender: operation.sender,
        });
    }

    let account_pin = find_account_pin(operation.sender, pins.accounts)?;
    let account_factory = account_pin.factory.map(|factory| factory.address);
    let (target, value, calldata, unwrap_depth, inner_call_count) =
        unwrap_execute_chain(operation.callData, account_pin, pins)?;

    Ok(UnwrappedSmartAccountCall {
        leader: operation.sender,
        outer_bundler: observed.outer_bundler,
        entry_point: pins.entry_point.address,
        beneficiary: call.beneficiary,
        account_factory,
        execution_profile: account_pin.execution_profile,
        delegation_implementation: account_pin
            .delegation_implementation
            .map(|implementation| implementation.address),
        target,
        value,
        calldata,
        unwrap_depth,
        inner_call_count,
    })
}

fn bounded_operation_count(calldata: &[u8]) -> Result<usize, SmartAccountDecodeError> {
    // selector + two-word head + array length
    if calldata.len() < 4 + 3 * 32 {
        return Err(SmartAccountDecodeError::MalformedEntryPointCall);
    }
    let operations_offset = U256::from_be_slice(&calldata[4..36]);
    if operations_offset != U256::from(64) {
        return Err(SmartAccountDecodeError::MalformedEntryPointCall);
    }
    // The beneficiary address must have canonical zero padding. Full canonical
    // encoding is checked again after decoding.
    if calldata[36..48].iter().any(|byte| *byte != 0) {
        return Err(SmartAccountDecodeError::MalformedEntryPointCall);
    }
    let count = U256::from_be_slice(&calldata[68..100]);
    if count > U256::from(MAX_USER_OPERATIONS) {
        return Err(SmartAccountDecodeError::ExcessiveUserOperations {
            actual: count.saturating_to::<usize>(),
            maximum: MAX_USER_OPERATIONS,
        });
    }
    Ok(count.to::<usize>())
}

fn unwrap_execute_chain(
    initial_calldata: Bytes,
    initial_account: SmartAccountPin,
    pins: SmartAccountPins<'_>,
) -> Result<(Address, U256, Bytes, usize, usize), SmartAccountDecodeError> {
    let mut current_calldata = initial_calldata;
    let mut depth = 0usize;
    let mut calls = 0usize;
    let mut current_account = initial_account;

    loop {
        if depth >= MAX_UNWRAP_DEPTH {
            return Err(SmartAccountDecodeError::ExcessiveUnwrapDepth {
                maximum: MAX_UNWRAP_DEPTH,
            });
        }
        if calls >= MAX_INNER_CALLS {
            return Err(SmartAccountDecodeError::ExcessiveInnerCalls {
                maximum: MAX_INNER_CALLS,
            });
        }
        if current_calldata.len() > MAX_ACCOUNT_CALLDATA_BYTES {
            return Err(SmartAccountDecodeError::AccountCalldataTooLarge {
                actual: current_calldata.len(),
                maximum: MAX_ACCOUNT_CALLDATA_BYTES,
            });
        }
        let (target, value, calldata) =
            decode_account_execute(&current_calldata, current_account.execution_profile)?;

        depth += 1;
        calls += 1;

        if find_target_pin(target, pins.allowed_targets)?.is_some() {
            return Ok((target, value, calldata, depth, calls));
        }
        if let Some(next_account) = find_account_pin_optional(target, pins.accounts)? {
            if value != U256::ZERO {
                return Err(SmartAccountDecodeError::IntermediateValueNotAllowed { target });
            }
            current_calldata = calldata;
            current_account = next_account;
            continue;
        }
        return Err(SmartAccountDecodeError::UnknownTarget { target });
    }
}

fn decode_account_execute(
    calldata: &[u8],
    profile: AccountExecutionProfile,
) -> Result<(Address, U256, Bytes), SmartAccountDecodeError> {
    match (profile, calldata.get(..4)) {
        (AccountExecutionProfile::ExecuteAddressValueBytes, Some(selector))
            if selector == ACCOUNT_EXECUTE_SELECTOR =>
        {
            let call = executeCall::abi_decode(calldata)
                .map_err(|_| SmartAccountDecodeError::MalformedAccountCall)?;
            if call.abi_encode().as_slice() != calldata {
                return Err(SmartAccountDecodeError::MalformedAccountCall);
            }
            Ok((call.dest, call.value, call.func))
        }
        (AccountExecutionProfile::Erc7579SingleCall, Some(selector))
            if selector == ERC7579_EXECUTE_SELECTOR =>
        {
            let call = erc7579::executeCall::abi_decode(calldata)
                .map_err(|_| SmartAccountDecodeError::MalformedAccountCall)?;
            if call.abi_encode().as_slice() != calldata {
                return Err(SmartAccountDecodeError::MalformedAccountCall);
            }
            // ERC-7579 mode layout is callType || execType || unused ||
            // modeSelector || modePayload. All-zero is the canonical single
            // CALL that reverts on failure, with no vendor extension.
            if call.mode != B256::ZERO {
                return Err(SmartAccountDecodeError::UnsupportedErc7579Mode);
            }
            // Single-call executionCalldata is abi.encodePacked(target, value,
            // callData): exactly 20 bytes, then 32 bytes, then the opaque call.
            if call.executionCalldata.len() < 52 {
                return Err(SmartAccountDecodeError::MalformedErc7579Execution);
            }
            let target = Address::from_slice(&call.executionCalldata[..20]);
            let value = U256::from_be_slice(&call.executionCalldata[20..52]);
            let inner = Bytes::copy_from_slice(&call.executionCalldata[52..]);
            Ok((target, value, inner))
        }
        (_, Some(selector))
            if selector == ACCOUNT_EXECUTE_SELECTOR || selector == ERC7579_EXECUTE_SELECTOR =>
        {
            Err(SmartAccountDecodeError::ExecutionProfileMismatch)
        }
        _ => Err(SmartAccountDecodeError::WrongAccountSelector),
    }
}

fn validate_pins(pins: SmartAccountPins<'_>) -> Result<(), SmartAccountDecodeError> {
    validate_contract_pin(pins.entry_point, "EntryPoint")?;
    if pins.entry_point.address != ENTRY_POINT_V07 {
        return Err(SmartAccountDecodeError::EntryPointAddressMismatch);
    }
    for (index, account) in pins.accounts.iter().enumerate() {
        validate_contract_pin(account.account, "smart account")?;
        if let Some(factory) = account.factory {
            validate_contract_pin(factory, "smart-account factory")?;
        }
        if let Some(implementation) = account.delegation_implementation {
            validate_contract_pin(implementation, "delegation implementation")?;
        }
        match (account.execution_profile, account.delegation_implementation) {
            (AccountExecutionProfile::ExecuteAddressValueBytes, None)
            | (AccountExecutionProfile::Erc7579SingleCall, Some(_)) => {}
            (AccountExecutionProfile::ExecuteAddressValueBytes, Some(_))
            | (AccountExecutionProfile::Erc7579SingleCall, None) => {
                return Err(SmartAccountDecodeError::InvalidPin {
                    role: "execution profile/delegation pair",
                });
            }
        }
        if pins.accounts[index + 1..]
            .iter()
            .any(|other| other.account.address == account.account.address)
        {
            return Err(SmartAccountDecodeError::AmbiguousPin {
                address: account.account.address,
            });
        }
        if pins
            .allowed_targets
            .iter()
            .any(|target| target.address == account.account.address)
        {
            return Err(SmartAccountDecodeError::AmbiguousPin {
                address: account.account.address,
            });
        }
    }
    for (index, target) in pins.allowed_targets.iter().enumerate() {
        validate_contract_pin(*target, "inner target")?;
        if pins.allowed_targets[index + 1..]
            .iter()
            .any(|other| other.address == target.address)
        {
            return Err(SmartAccountDecodeError::AmbiguousPin {
                address: target.address,
            });
        }
    }
    Ok(())
}

fn validate_contract_pin(
    pin: ContractPin,
    role: &'static str,
) -> Result<(), SmartAccountDecodeError> {
    if pin.address == Address::ZERO || pin.runtime_code_hash == B256::ZERO {
        return Err(SmartAccountDecodeError::InvalidPin { role });
    }
    Ok(())
}

fn find_account_pin(
    address: Address,
    accounts: &[SmartAccountPin],
) -> Result<SmartAccountPin, SmartAccountDecodeError> {
    find_account_pin_optional(address, accounts)?
        .ok_or(SmartAccountDecodeError::UnknownSmartAccount { sender: address })
}

fn find_account_pin_optional(
    address: Address,
    accounts: &[SmartAccountPin],
) -> Result<Option<SmartAccountPin>, SmartAccountDecodeError> {
    let mut matches = accounts
        .iter()
        .copied()
        .filter(|pin| pin.account.address == address);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(SmartAccountDecodeError::AmbiguousPin { address });
    }
    Ok(first)
}

fn find_target_pin(
    address: Address,
    targets: &[ContractPin],
) -> Result<Option<ContractPin>, SmartAccountDecodeError> {
    let mut matches = targets.iter().copied().filter(|pin| pin.address == address);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(SmartAccountDecodeError::AmbiguousPin { address });
    }
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY_POINT: Address = ENTRY_POINT_V07;
    const BUNDLER: Address = alloy_primitives::address!("256b3cc1e516d124b3027ecd083aa5a940d1328e");
    const LEADER: Address = alloy_primitives::address!("ff89978cb8171132395741b785d4a1f7e3efa124");
    const SECOND_ACCOUNT: Address =
        alloy_primitives::address!("1111111111111111111111111111111111111111");
    const THIRD_ACCOUNT: Address =
        alloy_primitives::address!("2222222222222222222222222222222222222222");
    const TARGET: Address = alloy_primitives::address!("eb7c034704ef8dcd2d32324c1545f62fb4ad0862");
    const UNKNOWN: Address = alloy_primitives::address!("3333333333333333333333333333333333333333");
    const FACTORY: Address = alloy_primitives::address!("4444444444444444444444444444444444444444");
    const BENEFICIARY: Address =
        alloy_primitives::address!("5555555555555555555555555555555555555555");
    const ENTRY_POINT_HASH: B256 =
        alloy_primitives::b256!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    const ACCOUNT_HASH: B256 =
        alloy_primitives::b256!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    const BANKR_DESIGNATOR_HASH: B256 =
        alloy_primitives::b256!("4542cbf1da24ba964614e4f5585736e22884e23e97c0f0915de4f602585d2dd4");
    const TARGET_HASH: B256 =
        alloy_primitives::b256!("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc");
    const FACTORY_HASH: B256 =
        alloy_primitives::b256!("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd");
    const DELEGATE: Address =
        alloy_primitives::address!("d6cedde84be40893d153be9d467cd6ad37875b28");
    const DELEGATE_HASH: B256 =
        alloy_primitives::b256!("6f6d6691dc11fda98d3102802f20e7e816ccc576c16c9279ee1a884a51d1935d");

    fn contract(address: Address, runtime_code_hash: B256) -> ContractPin {
        ContractPin {
            address,
            runtime_code_hash,
        }
    }

    fn account(address: Address) -> SmartAccountPin {
        SmartAccountPin {
            account: contract(address, ACCOUNT_HASH),
            execution_profile: AccountExecutionProfile::ExecuteAddressValueBytes,
            factory: Some(contract(FACTORY, FACTORY_HASH)),
            delegation_implementation: None,
        }
    }

    fn erc7579_account(address: Address) -> SmartAccountPin {
        SmartAccountPin {
            account: contract(address, BANKR_DESIGNATOR_HASH),
            execution_profile: AccountExecutionProfile::Erc7579SingleCall,
            factory: None,
            delegation_implementation: Some(contract(DELEGATE, DELEGATE_HASH)),
        }
    }

    fn encode_execute(target: Address, value: U256, calldata: Bytes) -> Bytes {
        executeCall {
            dest: target,
            value,
            func: calldata,
        }
        .abi_encode()
        .into()
    }

    fn encode_erc7579_execute(mode: B256, target: Address, value: U256, calldata: Bytes) -> Bytes {
        let mut packed = Vec::with_capacity(52 + calldata.len());
        packed.extend_from_slice(target.as_slice());
        packed.extend_from_slice(&value.to_be_bytes::<32>());
        packed.extend_from_slice(&calldata);
        erc7579::executeCall {
            mode,
            executionCalldata: packed.into(),
        }
        .abi_encode()
        .into()
    }

    fn user_operation(sender: Address, call_data: Bytes) -> PackedUserOperation {
        PackedUserOperation {
            sender,
            nonce: U256::from(7),
            initCode: Bytes::new(),
            callData: call_data,
            accountGasLimits: B256::repeat_byte(0x11),
            preVerificationGas: U256::from(21_000),
            gasFees: B256::repeat_byte(0x22),
            paymasterAndData: Bytes::new(),
            signature: Bytes::from_static(&[0x12, 0x34]),
        }
    }

    fn encode_handle_ops(ops: Vec<PackedUserOperation>) -> Vec<u8> {
        handleOpsCall {
            ops,
            beneficiary: BENEFICIARY,
        }
        .abi_encode()
    }

    fn decode_with<'a>(
        calldata: &'a [u8],
        destination: ContractPin,
        accounts: &'a [SmartAccountPin],
        targets: &'a [ContractPin],
    ) -> Result<UnwrappedSmartAccountCall, SmartAccountDecodeError> {
        decode_entry_point_v07(
            EntryPointCall {
                chain_id: ROBINHOOD_CHAIN_ID,
                destination,
                outer_bundler: BUNDLER,
                calldata,
            },
            SmartAccountPins {
                entry_point: contract(ENTRY_POINT, ENTRY_POINT_HASH),
                accounts,
                allowed_targets: targets,
            },
        )
    }

    #[test]
    fn unwraps_canonical_v07_execute_and_keeps_leader_distinct_from_bundler() {
        let protocol_calldata = Bytes::from_static(&[0xde, 0xad, 0xbe, 0xef, 0x01]);
        let calldata = encode_handle_ops(vec![user_operation(
            LEADER,
            encode_execute(TARGET, U256::from(123), protocol_calldata.clone()),
        )]);
        let accounts = [account(LEADER)];
        let targets = [contract(TARGET, TARGET_HASH)];

        let decoded = decode_with(
            &calldata,
            contract(ENTRY_POINT, ENTRY_POINT_HASH),
            &accounts,
            &targets,
        )
        .unwrap();

        assert_eq!(decoded.leader, LEADER);
        assert_eq!(decoded.outer_bundler, BUNDLER);
        assert_ne!(decoded.leader, decoded.outer_bundler);
        assert_eq!(decoded.entry_point, ENTRY_POINT);
        assert_eq!(decoded.beneficiary, BENEFICIARY);
        assert_eq!(decoded.account_factory, Some(FACTORY));
        assert_eq!(decoded.target, TARGET);
        assert_eq!(decoded.value, U256::from(123));
        assert_eq!(decoded.calldata, protocol_calldata);
        assert_eq!(decoded.unwrap_depth, 1);
        assert_eq!(decoded.inner_call_count, 1);
    }

    #[test]
    fn unwraps_bankr_eip7702_erc7579_proof_profile() {
        // Exact identity/selector/value facts from proof transaction
        // c6597fe8...f0609. The full live transaction is also checked by the
        // read-only pin snapshot binary.
        let protocol_calldata = Bytes::from_static(&[0x88, 0x2d, 0xb7, 0x07]);
        let account_calldata =
            encode_erc7579_execute(B256::ZERO, TARGET, U256::ZERO, protocol_calldata.clone());
        assert_eq!(
            account_calldata.get(..4),
            Some(ERC7579_EXECUTE_SELECTOR.as_slice())
        );
        let calldata = encode_handle_ops(vec![user_operation(LEADER, account_calldata)]);
        let accounts = [erc7579_account(LEADER)];
        let targets = [contract(TARGET, TARGET_HASH)];

        let decoded = decode_with(
            &calldata,
            contract(ENTRY_POINT, ENTRY_POINT_HASH),
            &accounts,
            &targets,
        )
        .unwrap();

        assert_eq!(decoded.leader, LEADER);
        assert_eq!(
            decoded.execution_profile,
            AccountExecutionProfile::Erc7579SingleCall
        );
        assert_eq!(decoded.delegation_implementation, Some(DELEGATE));
        assert_eq!(decoded.target, TARGET);
        assert_eq!(decoded.value, U256::ZERO);
        assert_eq!(decoded.calldata, protocol_calldata);
        assert_eq!(decoded.unwrap_depth, 1);
        assert_eq!(decoded.inner_call_count, 1);
    }

    #[test]
    fn rejects_selector_profile_mismatch_and_missing_delegation_pin() {
        let direct = encode_execute(TARGET, U256::ZERO, Bytes::new());
        let erc7579 = encode_erc7579_execute(B256::ZERO, TARGET, U256::ZERO, Bytes::new());
        let targets = [contract(TARGET, TARGET_HASH)];

        let direct_for_erc = encode_handle_ops(vec![user_operation(LEADER, direct)]);
        assert_eq!(
            decode_with(
                &direct_for_erc,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &[erc7579_account(LEADER)],
                &targets,
            ),
            Err(SmartAccountDecodeError::ExecutionProfileMismatch)
        );

        let erc_for_direct = encode_handle_ops(vec![user_operation(LEADER, erc7579)]);
        assert_eq!(
            decode_with(
                &erc_for_direct,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &[account(LEADER)],
                &targets,
            ),
            Err(SmartAccountDecodeError::ExecutionProfileMismatch)
        );

        let missing_delegation = SmartAccountPin {
            account: contract(LEADER, BANKR_DESIGNATOR_HASH),
            execution_profile: AccountExecutionProfile::Erc7579SingleCall,
            factory: None,
            delegation_implementation: None,
        };
        assert_eq!(
            decode_with(
                &erc_for_direct,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &[missing_delegation],
                &targets,
            ),
            Err(SmartAccountDecodeError::InvalidPin {
                role: "execution profile/delegation pair"
            })
        );
    }

    #[test]
    fn rejects_erc7579_batch_try_and_custom_modes() {
        let accounts = [erc7579_account(LEADER)];
        let targets = [contract(TARGET, TARGET_HASH)];

        for mode_prefix in [[0x01, 0x00], [0x00, 0x01], [0x00, 0x00]] {
            let mut mode = [0u8; 32];
            mode[..2].copy_from_slice(&mode_prefix);
            if mode_prefix == [0x00, 0x00] {
                mode[6] = 1;
            }
            let account_calldata =
                encode_erc7579_execute(B256::from(mode), TARGET, U256::ZERO, Bytes::new());
            let calldata = encode_handle_ops(vec![user_operation(LEADER, account_calldata)]);
            assert_eq!(
                decode_with(
                    &calldata,
                    contract(ENTRY_POINT, ENTRY_POINT_HASH),
                    &accounts,
                    &targets,
                ),
                Err(SmartAccountDecodeError::UnsupportedErc7579Mode)
            );
        }
    }

    #[test]
    fn rejects_short_erc7579_single_call_data() {
        let account_calldata = erc7579::executeCall {
            mode: B256::ZERO,
            executionCalldata: Bytes::from(vec![0u8; 51]),
        }
        .abi_encode();
        let calldata = encode_handle_ops(vec![user_operation(LEADER, account_calldata.into())]);
        let accounts = [erc7579_account(LEADER)];
        let targets = [contract(TARGET, TARGET_HASH)];

        assert_eq!(
            decode_with(
                &calldata,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            ),
            Err(SmartAccountDecodeError::MalformedErc7579Execution)
        );
    }

    #[test]
    fn unwraps_one_pinned_nested_account() {
        let protocol_calldata = Bytes::from_static(&[0xaa, 0xbb, 0xcc, 0xdd]);
        let nested = encode_execute(TARGET, U256::from(5), protocol_calldata.clone());
        let outer = encode_execute(SECOND_ACCOUNT, U256::ZERO, nested);
        let calldata = encode_handle_ops(vec![user_operation(LEADER, outer)]);
        let accounts = [account(LEADER), account(SECOND_ACCOUNT)];
        let targets = [contract(TARGET, TARGET_HASH)];

        let decoded = decode_with(
            &calldata,
            contract(ENTRY_POINT, ENTRY_POINT_HASH),
            &accounts,
            &targets,
        )
        .unwrap();
        assert_eq!(decoded.leader, LEADER);
        assert_eq!(decoded.target, TARGET);
        assert_eq!(decoded.value, U256::from(5));
        assert_eq!(decoded.calldata, protocol_calldata);
        assert_eq!(decoded.unwrap_depth, 2);
        assert_eq!(decoded.inner_call_count, 2);
    }

    #[test]
    fn prevalidated_pins_preserve_nested_fail_closed_decoding() {
        let protocol_calldata = Bytes::from_static(&[0xaa, 0xbb, 0xcc, 0xdd]);
        let nested = encode_execute(TARGET, U256::from(5), protocol_calldata.clone());
        let outer = encode_execute(SECOND_ACCOUNT, U256::ZERO, nested);
        let calldata = encode_handle_ops(vec![user_operation(LEADER, outer)]);
        let accounts = [account(LEADER), account(SECOND_ACCOUNT)];
        let targets = [contract(TARGET, TARGET_HASH)];
        let pins = ValidatedSmartAccountPins::new(SmartAccountPins {
            entry_point: contract(ENTRY_POINT, ENTRY_POINT_HASH),
            accounts: &accounts,
            allowed_targets: &targets,
        })
        .unwrap();

        let decoded = decode_entry_point_v07_prevalidated(
            EntryPointCall {
                chain_id: ROBINHOOD_CHAIN_ID,
                destination: contract(ENTRY_POINT, ENTRY_POINT_HASH),
                outer_bundler: BUNDLER,
                calldata: &calldata,
            },
            pins,
        )
        .unwrap();
        assert_eq!(decoded.leader, LEADER);
        assert_eq!(decoded.target, TARGET);
        assert_eq!(decoded.calldata, protocol_calldata);
        assert_eq!(decoded.unwrap_depth, 2);
    }

    #[test]
    fn rejects_value_bearing_intermediate_account_hop() {
        let protocol_calldata = Bytes::from_static(&[0xaa, 0xbb, 0xcc, 0xdd]);
        let nested = encode_execute(TARGET, U256::from(5), protocol_calldata);
        let outer = encode_execute(SECOND_ACCOUNT, U256::from(1), nested);
        let calldata = encode_handle_ops(vec![user_operation(LEADER, outer)]);
        let accounts = [account(LEADER), account(SECOND_ACCOUNT)];
        let targets = [contract(TARGET, TARGET_HASH)];

        assert_eq!(
            decode_with(
                &calldata,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            ),
            Err(SmartAccountDecodeError::IntermediateValueNotAllowed {
                target: SECOND_ACCOUNT,
            })
        );
    }

    #[test]
    fn rejects_wrong_chain() {
        let calldata = encode_handle_ops(vec![user_operation(
            LEADER,
            encode_execute(TARGET, U256::ZERO, Bytes::new()),
        )]);
        let accounts = [account(LEADER)];
        let targets = [contract(TARGET, TARGET_HASH)];
        let error = decode_entry_point_v07(
            EntryPointCall {
                chain_id: 8_453,
                destination: contract(ENTRY_POINT, ENTRY_POINT_HASH),
                outer_bundler: BUNDLER,
                calldata: &calldata,
            },
            SmartAccountPins {
                entry_point: contract(ENTRY_POINT, ENTRY_POINT_HASH),
                accounts: &accounts,
                allowed_targets: &targets,
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            SmartAccountDecodeError::WrongChain { actual: 8_453, .. }
        ));
    }

    #[test]
    fn rejects_entry_point_lookalike_address_and_code_hash() {
        let calldata = encode_handle_ops(vec![user_operation(
            LEADER,
            encode_execute(TARGET, U256::ZERO, Bytes::new()),
        )]);
        let accounts = [account(LEADER)];
        let targets = [contract(TARGET, TARGET_HASH)];

        let wrong_address = decode_with(
            &calldata,
            contract(UNKNOWN, ENTRY_POINT_HASH),
            &accounts,
            &targets,
        )
        .unwrap_err();
        assert_eq!(
            wrong_address,
            SmartAccountDecodeError::EntryPointAddressMismatch
        );

        let wrong_hash = decode_with(
            &calldata,
            contract(ENTRY_POINT, TARGET_HASH),
            &accounts,
            &targets,
        )
        .unwrap_err();
        assert_eq!(
            wrong_hash,
            SmartAccountDecodeError::EntryPointCodeHashMismatch
        );
    }

    #[test]
    fn rejects_unknown_account_and_unknown_target() {
        let unknown_account_call = encode_handle_ops(vec![user_operation(
            UNKNOWN,
            encode_execute(TARGET, U256::ZERO, Bytes::new()),
        )]);
        let accounts = [account(LEADER)];
        let targets = [contract(TARGET, TARGET_HASH)];
        assert_eq!(
            decode_with(
                &unknown_account_call,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            )
            .unwrap_err(),
            SmartAccountDecodeError::UnknownSmartAccount { sender: UNKNOWN }
        );

        let unknown_target_call = encode_handle_ops(vec![user_operation(
            LEADER,
            encode_execute(UNKNOWN, U256::ZERO, Bytes::new()),
        )]);
        assert_eq!(
            decode_with(
                &unknown_target_call,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            )
            .unwrap_err(),
            SmartAccountDecodeError::UnknownTarget { target: UNKNOWN }
        );
    }

    #[test]
    fn rejects_init_code_and_paymaster_data_even_with_factory_pin() {
        let accounts = [account(LEADER)];
        let targets = [contract(TARGET, TARGET_HASH)];
        let execute = encode_execute(TARGET, U256::ZERO, Bytes::new());

        let mut with_init_code = user_operation(LEADER, execute.clone());
        with_init_code.initCode = Bytes::from_static(&[0x01]);
        let calldata = encode_handle_ops(vec![with_init_code]);
        assert!(matches!(
            decode_with(
                &calldata,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            ),
            Err(SmartAccountDecodeError::InitCodeNotAllowed { sender: LEADER })
        ));

        let mut with_paymaster = user_operation(LEADER, execute);
        with_paymaster.paymasterAndData = Bytes::from_static(&[0x02]);
        let calldata = encode_handle_ops(vec![with_paymaster]);
        assert!(matches!(
            decode_with(
                &calldata,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            ),
            Err(SmartAccountDecodeError::PaymasterNotAllowed { sender: LEADER })
        ));
    }

    #[test]
    fn rejects_malformed_and_noncanonical_abi() {
        let accounts = [account(LEADER)];
        let targets = [contract(TARGET, TARGET_HASH)];
        let canonical = encode_handle_ops(vec![user_operation(
            LEADER,
            encode_execute(TARGET, U256::ZERO, Bytes::from_static(&[0x01])),
        )]);

        let mut truncated = canonical.clone();
        truncated.pop();
        assert!(matches!(
            decode_with(
                &truncated,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            ),
            Err(SmartAccountDecodeError::MalformedEntryPointCall)
        ));

        let mut trailing = canonical;
        trailing.extend_from_slice(&[0; 32]);
        assert!(matches!(
            decode_with(
                &trailing,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            ),
            Err(SmartAccountDecodeError::MalformedEntryPointCall)
        ));

        let malformed_execute = encode_handle_ops(vec![user_operation(
            LEADER,
            Bytes::from_static(&ACCOUNT_EXECUTE_SELECTOR),
        )]);
        assert!(matches!(
            decode_with(
                &malformed_execute,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            ),
            Err(SmartAccountDecodeError::MalformedAccountCall)
        ));
    }

    #[test]
    fn rejects_ambiguous_and_excessive_operations_before_unwrapping() {
        let operation = || user_operation(LEADER, encode_execute(TARGET, U256::ZERO, Bytes::new()));
        let accounts = [account(LEADER)];
        let targets = [contract(TARGET, TARGET_HASH)];

        let ambiguous = encode_handle_ops(vec![operation(), operation()]);
        assert_eq!(
            decode_with(
                &ambiguous,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            )
            .unwrap_err(),
            SmartAccountDecodeError::AmbiguousUserOperations { actual: 2 }
        );

        let excessive = encode_handle_ops((0..=MAX_USER_OPERATIONS).map(|_| operation()).collect());
        assert_eq!(
            decode_with(
                &excessive,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            )
            .unwrap_err(),
            SmartAccountDecodeError::ExcessiveUserOperations {
                actual: MAX_USER_OPERATIONS + 1,
                maximum: MAX_USER_OPERATIONS,
            }
        );
    }

    #[test]
    fn rejects_excessive_nested_account_depth() {
        let protocol = encode_execute(TARGET, U256::ZERO, Bytes::new());
        let third = encode_execute(THIRD_ACCOUNT, U256::ZERO, protocol);
        let second = encode_execute(SECOND_ACCOUNT, U256::ZERO, third);
        let calldata = encode_handle_ops(vec![user_operation(LEADER, second)]);
        let accounts = [
            account(LEADER),
            account(SECOND_ACCOUNT),
            account(THIRD_ACCOUNT),
        ];
        let targets = [contract(TARGET, TARGET_HASH)];

        assert_eq!(
            decode_with(
                &calldata,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            )
            .unwrap_err(),
            SmartAccountDecodeError::ExcessiveUnwrapDepth {
                maximum: MAX_UNWRAP_DEPTH,
            }
        );
    }

    #[test]
    fn rejects_ambiguous_pins_and_zero_runtime_hashes() {
        let calldata = encode_handle_ops(vec![user_operation(
            LEADER,
            encode_execute(TARGET, U256::ZERO, Bytes::new()),
        )]);
        let duplicate_accounts = [account(LEADER), account(LEADER)];
        let targets = [contract(TARGET, TARGET_HASH)];
        assert_eq!(
            decode_with(
                &calldata,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &duplicate_accounts,
                &targets,
            )
            .unwrap_err(),
            SmartAccountDecodeError::AmbiguousPin { address: LEADER }
        );

        let accounts = [SmartAccountPin {
            account: contract(LEADER, B256::ZERO),
            execution_profile: AccountExecutionProfile::ExecuteAddressValueBytes,
            factory: None,
            delegation_implementation: None,
        }];
        assert_eq!(
            decode_with(
                &calldata,
                contract(ENTRY_POINT, ENTRY_POINT_HASH),
                &accounts,
                &targets,
            )
            .unwrap_err(),
            SmartAccountDecodeError::InvalidPin {
                role: "smart account",
            }
        );
    }
}
