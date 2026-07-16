//! Strict, I/O-free ERC-4337 smart-account observation decoding.
//!
//! This module intentionally supports one small profile: EntryPoint v0.7
//! `handleOps(PackedUserOperation[], address)` calls whose single user operation
//! invokes a startup-pinned account's `execute(address,uint256,bytes)` method.
//! It does not support account deployment, paymasters, batches, arbitrary
//! account ABIs, or candidate-time code discovery.

use alloy_primitives::{Address, B256, Bytes, U256};
use alloy_sol_types::{SolCall, sol};
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

pub const ENTRY_POINT_V07_HANDLE_OPS_SELECTOR: [u8; 4] = handleOpsCall::SELECTOR;
pub const ACCOUNT_EXECUTE_SELECTOR: [u8; 4] = executeCall::SELECTOR;

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
    /// Optional deployed-account provenance. `initCode` remains forbidden even
    /// when a factory is pinned; this decoder accepts already-deployed accounts
    /// only.
    pub factory: Option<ContractPin>,
}

/// Immutable startup state used by the candidate decoder.
#[derive(Debug, Clone, Copy)]
pub struct SmartAccountPins<'a> {
    pub entry_point: ContractPin,
    pub accounts: &'a [SmartAccountPin],
    pub allowed_targets: &'a [ContractPin],
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
    #[error("smart-account calldata does not use pinned execute(address,uint256,bytes)")]
    WrongAccountSelector,
    #[error("smart-account execute calldata is malformed or noncanonical")]
    MalformedAccountCall,
    #[error("inner target {target} is neither an allowed target nor a pinned smart account")]
    UnknownTarget { target: Address },
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
    if observed.chain_id != ROBINHOOD_CHAIN_ID {
        return Err(SmartAccountDecodeError::WrongChain {
            expected: ROBINHOOD_CHAIN_ID,
            actual: observed.chain_id,
        });
    }
    validate_pins(pins)?;
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

    let operation = &call.ops[0];
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
        unwrap_execute_chain(operation.callData.as_ref(), pins)?;

    Ok(UnwrappedSmartAccountCall {
        leader: operation.sender,
        outer_bundler: observed.outer_bundler,
        entry_point: pins.entry_point.address,
        beneficiary: call.beneficiary,
        account_factory,
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
    initial_calldata: &[u8],
    pins: SmartAccountPins<'_>,
) -> Result<(Address, U256, Bytes, usize, usize), SmartAccountDecodeError> {
    let mut current_calldata = Bytes::copy_from_slice(initial_calldata);
    let mut depth = 0usize;
    let mut calls = 0usize;

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
        if current_calldata.get(..4) != Some(ACCOUNT_EXECUTE_SELECTOR.as_slice()) {
            return Err(SmartAccountDecodeError::WrongAccountSelector);
        }
        let call = executeCall::abi_decode(current_calldata.as_ref())
            .map_err(|_| SmartAccountDecodeError::MalformedAccountCall)?;
        if call.abi_encode().as_slice() != current_calldata.as_ref() {
            return Err(SmartAccountDecodeError::MalformedAccountCall);
        }

        depth += 1;
        calls += 1;

        if find_target_pin(call.dest, pins.allowed_targets)?.is_some() {
            return Ok((call.dest, call.value, call.func, depth, calls));
        }
        if find_account_pin_optional(call.dest, pins.accounts)?.is_some() {
            current_calldata = call.func;
            continue;
        }
        return Err(SmartAccountDecodeError::UnknownTarget { target: call.dest });
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
    const TARGET_HASH: B256 =
        alloy_primitives::b256!("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc");
    const FACTORY_HASH: B256 =
        alloy_primitives::b256!("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd");

    fn contract(address: Address, runtime_code_hash: B256) -> ContractPin {
        ContractPin {
            address,
            runtime_code_hash,
        }
    }

    fn account(address: Address) -> SmartAccountPin {
        SmartAccountPin {
            account: contract(address, ACCOUNT_HASH),
            factory: Some(contract(FACTORY, FACTORY_HASH)),
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
            factory: None,
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
