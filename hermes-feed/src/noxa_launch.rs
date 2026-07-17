use alloy_primitives::{Address, I256, U256};
use thiserror::Error;

use crate::noxa_abi::{
    NoxaLaunchEvent, ReceiptLog, V3PoolEvent, decode_pool_created, decode_token_launched,
    decode_v3_pool_event,
};
use crate::robinhood::{
    NOXA_DEX_ID_UNISWAP, NOXA_LAUNCH_CONFIG_ID_WETH, NOXA_LAUNCH_FACTORY, NOXA_POOL_FEE,
    NOXA_RESTRICTION_L1_BLOCKS, NOXA_TICK_SPACING, UNISWAP_V3_FACTORY, UNISWAP_V3_SWAP_ROUTER_02,
    WETH,
};
use crate::v3_pool::{V3PoolError, V3PoolState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HydratedNoxaLaunch {
    pub launch: NoxaLaunchEvent,
    pub launch_l1_block: u64,
    pub launch_l2_block: u64,
    pub pool: V3PoolState,
    pub mint_events: usize,
    pub swap_events: usize,
}

#[derive(Debug, Error)]
pub enum NoxaHydrationError {
    #[error("receipt must contain exactly one canonical TokenLaunched event")]
    MissingOrDuplicateLaunch,
    #[error("launch event does not match the pinned Robinhood NOXA configuration: {0}")]
    ConfigurationMismatch(&'static str),
    #[error("restriction end L1 block does not match the launch configuration")]
    RestrictionWindowMismatch,
    #[error("receipt must contain exactly one matching V3 PoolCreated event")]
    MissingOrDuplicatePoolCreated,
    #[error("pool event order is invalid")]
    InvalidPoolEventOrder,
    #[error("launch receipt contains a burn and is not a canonical creation path")]
    UnexpectedBurn,
    #[error("initial buy was reported but no post-launch V3 Swap event was found")]
    MissingInitialSwap,
    #[error("receipt contains a V3 Swap even though the launch reported no initial buy")]
    UnexpectedInitialSwap,
    #[error("post-launch V3 Swap does not match the reported initial buy")]
    InitialSwapMismatch,
    #[error("integer does not fit the expected Robinhood field")]
    IntegerRange,
    #[error(transparent)]
    Pool(#[from] V3PoolError),
}

pub fn hydrate_noxa_launch_receipt(
    logs: &[ReceiptLog],
    launch_l1_block: u64,
    launch_l2_block: u64,
) -> Result<HydratedNoxaLaunch, NoxaHydrationError> {
    let mut ordered: Vec<&ReceiptLog> = logs.iter().collect();
    ordered.sort_by_key(|log| log.log_index);
    if ordered
        .windows(2)
        .any(|pair| pair[0].log_index == pair[1].log_index)
    {
        return Err(NoxaHydrationError::InvalidPoolEventOrder);
    }

    let launches: Vec<_> = ordered
        .iter()
        .filter(|log| log.address == NOXA_LAUNCH_FACTORY)
        .filter_map(|log| decode_token_launched(log).map(|event| (*log, event)))
        .collect();
    let [(launch_log, launch)] = launches.as_slice() else {
        return Err(NoxaHydrationError::MissingOrDuplicateLaunch);
    };
    validate_launch(launch, launch_l1_block)?;

    let created: Vec<_> = ordered
        .iter()
        .filter(|log| log.address == launch.dex_factory)
        .filter_map(|log| decode_pool_created(log).map(|event| (*log, event)))
        .filter(|(_, event)| event.pool == launch.pool)
        .collect();
    let [(created_log, created)] = created.as_slice() else {
        return Err(NoxaHydrationError::MissingOrDuplicatePoolCreated);
    };
    if created_log.log_index >= launch_log.log_index {
        return Err(NoxaHydrationError::InvalidPoolEventOrder);
    }
    let (expected_token0, expected_token1) = sorted_tokens(launch.pair_token, launch.token);
    if created.token0 != expected_token0
        || created.token1 != expected_token1
        || created.fee != NOXA_POOL_FEE
        || created.tick_spacing != NOXA_TICK_SPACING
    {
        return Err(NoxaHydrationError::ConfigurationMismatch("pool creation"));
    }

    let mut pool: Option<V3PoolState> = None;
    let mut mint_events = 0_usize;
    let mut swap_events = 0_usize;
    for log in ordered.iter().filter(|log| log.address == launch.pool) {
        let Some(event) = decode_v3_pool_event(log) else {
            continue;
        };
        match event {
            V3PoolEvent::Initialize {
                sqrt_price_x96,
                tick,
            } => {
                if log.log_index <= created_log.log_index
                    || log.log_index >= launch_log.log_index
                    || pool.is_some()
                {
                    return Err(NoxaHydrationError::InvalidPoolEventOrder);
                }
                pool = Some(V3PoolState::new(
                    launch.pool,
                    created.token0,
                    created.token1,
                    created.fee,
                    created.tick_spacing,
                    sqrt_price_x96,
                    tick,
                    0,
                )?);
            }
            V3PoolEvent::Mint {
                tick_lower,
                tick_upper,
                amount,
            } => {
                if log.log_index >= launch_log.log_index {
                    return Err(NoxaHydrationError::InvalidPoolEventOrder);
                }
                let state = pool
                    .as_mut()
                    .ok_or(NoxaHydrationError::InvalidPoolEventOrder)?;
                state.add_position(tick_lower, tick_upper, amount)?;
                mint_events += 1;
            }
            V3PoolEvent::Burn { .. } => return Err(NoxaHydrationError::UnexpectedBurn),
            V3PoolEvent::Swap {
                sender,
                recipient,
                amount0,
                amount1,
                sqrt_price_x96,
                liquidity,
                tick,
            } => {
                if log.log_index <= launch_log.log_index || swap_events != 0 {
                    return Err(NoxaHydrationError::InvalidPoolEventOrder);
                }
                validate_initial_swap(launch, created.token0, sender, recipient, amount0, amount1)?;
                let state = pool
                    .as_mut()
                    .ok_or(NoxaHydrationError::InvalidPoolEventOrder)?;
                state.set_observation(sqrt_price_x96, tick, liquidity)?;
                swap_events += 1;
            }
        }
    }
    let pool = pool.ok_or(NoxaHydrationError::InvalidPoolEventOrder)?;
    if mint_events == 0 {
        return Err(NoxaHydrationError::InvalidPoolEventOrder);
    }
    if launch.initial_buy_amount != U256::ZERO && swap_events == 0 {
        return Err(NoxaHydrationError::MissingInitialSwap);
    }
    if launch.initial_buy_amount == U256::ZERO && swap_events != 0 {
        return Err(NoxaHydrationError::UnexpectedInitialSwap);
    }

    Ok(HydratedNoxaLaunch {
        launch: launch.clone(),
        launch_l1_block,
        launch_l2_block,
        pool,
        mint_events,
        swap_events,
    })
}

fn validate_initial_swap(
    launch: &NoxaLaunchEvent,
    token0: Address,
    sender: Address,
    recipient: Address,
    amount0: I256,
    amount1: I256,
) -> Result<(), NoxaHydrationError> {
    if launch.initial_buy_amount == U256::ZERO {
        return Err(NoxaHydrationError::UnexpectedInitialSwap);
    }
    if launch.initial_buy_amount >= (U256::from(1_u8) << 255)
        || sender != UNISWAP_V3_SWAP_ROUTER_02
        || recipient != launch.deployer
    {
        return Err(NoxaHydrationError::InitialSwapMismatch);
    }
    let expected_input = I256::from_raw(launch.initial_buy_amount);
    let matches = if token0 == WETH {
        amount0 == expected_input && amount1 < I256::ZERO
    } else {
        amount1 == expected_input && amount0 < I256::ZERO
    };
    if !matches {
        return Err(NoxaHydrationError::InitialSwapMismatch);
    }
    Ok(())
}

fn validate_launch(
    launch: &NoxaLaunchEvent,
    launch_l1_block: u64,
) -> Result<(), NoxaHydrationError> {
    if launch.dex_factory != UNISWAP_V3_FACTORY {
        return Err(NoxaHydrationError::ConfigurationMismatch("DEX factory"));
    }
    if launch.pair_token != WETH {
        return Err(NoxaHydrationError::ConfigurationMismatch("pair token"));
    }
    if launch.dex_id != U256::from(NOXA_DEX_ID_UNISWAP)
        || launch.launch_config_id != U256::from(NOXA_LAUNCH_CONFIG_ID_WETH)
    {
        return Err(NoxaHydrationError::ConfigurationMismatch(
            "DEX or launch config ID",
        ));
    }
    let expected = launch_l1_block
        .checked_add(NOXA_RESTRICTION_L1_BLOCKS)
        .ok_or(NoxaHydrationError::IntegerRange)?;
    let actual = u64::try_from(launch.restrictions_end_l1_block)
        .map_err(|_| NoxaHydrationError::IntegerRange)?;
    if expected != actual {
        return Err(NoxaHydrationError::RestrictionWindowMismatch);
    }
    Ok(())
}

fn sorted_tokens(left: Address, right: Address) -> (Address, Address) {
    if left < right {
        (left, right)
    } else {
        (right, left)
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{B256, Bytes, address, b256};

    use super::*;

    fn topic_address(value: Address) -> B256 {
        B256::left_padding_from(value.as_slice())
    }

    fn word_topic(value: &str) -> B256 {
        B256::from_slice(&hex::decode(value).unwrap())
    }

    fn log(address: Address, log_index: u64, topics: Vec<B256>, data: &str) -> ReceiptLog {
        ReceiptLog {
            address,
            log_index,
            topics,
            data: Bytes::from(hex::decode(data).unwrap()),
        }
    }

    fn live_receipt_logs() -> Vec<ReceiptLog> {
        let factory = NOXA_LAUNCH_FACTORY;
        let token = address!("955b339944cbd4834156366d766c260c80956b44");
        let deployer = address!("4ba04830e5f615dc0e7d80a7dc4352c241ccbdc2");
        let pool = address!("efd703d89b7febc0ae43fdd72edd257819366272");
        vec![
            log(
                UNISWAP_V3_FACTORY,
                0x17,
                vec![
                    b256!("783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118"),
                    topic_address(WETH),
                    topic_address(token),
                    word_topic("0000000000000000000000000000000000000000000000000000000000002710"),
                ],
                concat!(
                    "00000000000000000000000000000000000000000000000000000000000000c8",
                    "000000000000000000000000efd703d89b7febc0ae43fdd72edd257819366272"
                ),
            ),
            log(
                pool,
                0x18,
                vec![b256!(
                    "98636036cb66a9c19a37435efc1e90142190214e8abeb821bdba3f2990dd4c95"
                )],
                concat!(
                    "00000000000000000000000000000000000006a17b32fc5d4d48f7124aa2fdba",
                    "0000000000000000000000000000000000000000000000000000000000031da8"
                ),
            ),
            log(
                pool,
                0x1a,
                vec![
                    b256!("7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde"),
                    topic_address(crate::robinhood::UNISWAP_V3_POSITION_MANAGER),
                    word_topic("fffffffffffffffffffffffffffffffffffffffffffffffffffffffffff27660"),
                    word_topic("0000000000000000000000000000000000000000000000000000000000031da8"),
                ],
                concat!(
                    "00000000000000000000000073991a25c818bf1f1128deaab1492d45638de0d3",
                    "0000000000000000000000000000000000000000000007cbf9d9985f0629c56e",
                    "0000000000000000000000000000000000000000000000000000000000000000",
                    "0000000000000000000000000000000000000000033b2e3c9fd0803ce7ffd018"
                ),
            ),
            // TokenLaunched precedes the launcher's optional initial swap.
            log(
                factory,
                0x20,
                vec![
                    b256!("db51ea9ad51ab453a65a4cb7e60c3cb378c9501bb002609f8f97778fb6c4235a"),
                    topic_address(token),
                    topic_address(deployer),
                    topic_address(UNISWAP_V3_FACTORY),
                ],
                concat!(
                    "0000000000000000000000000bd7d308f8e1639fab988df18a8011f41eacad73",
                    "000000000000000000000000efd703d89b7febc0ae43fdd72edd257819366272",
                    "0000000000000000000000000000000000000000000000000000000000000000",
                    "0000000000000000000000000000000000000000000000000000000000000000",
                    "000000000000000000000000000000000000000000000000000000000001339e",
                    "0000000000000000000000000000000000000000000000000000000001853d61",
                    "00000000000000000000000000000000000000000000000000b1a2bc2ec50000"
                ),
            ),
            log(
                pool,
                0x24,
                vec![
                    b256!("c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67"),
                    topic_address(crate::robinhood::UNISWAP_V3_SWAP_ROUTER_02),
                    topic_address(deployer),
                ],
                concat!(
                    "00000000000000000000000000000000000000000000000000b1a2bc2ec50000",
                    "ffffffffffffffffffffffffffffffffffffffffffe2dc50ec12756bb636d1b0",
                    "000000000000000000000000000000000000665aef7589c635534122e931e613",
                    "0000000000000000000000000000000000000000000007cbf9d9985f0629c56e",
                    "0000000000000000000000000000000000000000000000000000000000031ada"
                ),
            ),
        ]
    }

    #[test]
    fn hydrates_complete_live_launch_receipt_in_log_order() {
        let hydrated =
            hydrate_noxa_launch_receipt(&live_receipt_logs(), 25_508_851, 6_880_646).unwrap();
        assert_eq!(
            hydrated.launch.token,
            address!("955b339944cbd4834156366d766c260c80956b44")
        );
        assert_eq!(hydrated.mint_events, 1);
        assert_eq!(hydrated.swap_events, 1);
        assert_eq!(hydrated.pool.tick, 203_482);
        assert_eq!(
            hydrated.pool.sqrt_price_x96,
            U256::from_str_radix("665aef7589c635534122e931e613", 16).unwrap()
        );
    }

    #[test]
    fn refuses_to_stop_at_token_launched_before_initial_swap() {
        let mut logs = live_receipt_logs();
        logs.retain(|log| log.log_index != 0x24);
        let error = hydrate_noxa_launch_receipt(&logs, 25_508_851, 6_880_646).unwrap_err();
        assert!(matches!(error, NoxaHydrationError::MissingInitialSwap));
    }

    #[test]
    fn rejects_swap_before_launch_and_wrong_initial_input() {
        let mut reordered = live_receipt_logs();
        reordered
            .iter_mut()
            .find(|log| log.log_index == 0x24)
            .unwrap()
            .log_index = 0x1f;
        assert!(matches!(
            hydrate_noxa_launch_receipt(&reordered, 25_508_851, 6_880_646),
            Err(NoxaHydrationError::InvalidPoolEventOrder)
        ));

        let mut wrong_amount = live_receipt_logs();
        let swap = wrong_amount
            .iter_mut()
            .find(|log| log.log_index == 0x24)
            .unwrap();
        let mut swap_data = swap.data.to_vec();
        swap_data[..32].fill(0);
        swap_data[31] = 1;
        swap.data = Bytes::from(swap_data);
        assert!(matches!(
            hydrate_noxa_launch_receipt(&wrong_amount, 25_508_851, 6_880_646),
            Err(NoxaHydrationError::InitialSwapMismatch)
        ));
    }

    #[test]
    fn rejects_swap_when_launch_reports_zero_initial_buy() {
        let mut logs = live_receipt_logs();
        let launch = logs.iter_mut().find(|log| log.log_index == 0x20).unwrap();
        let mut launch_data = launch.data.to_vec();
        let last_word = launch_data.len() - 32;
        launch_data[last_word..].fill(0);
        launch.data = Bytes::from(launch_data);
        assert!(matches!(
            hydrate_noxa_launch_receipt(&logs, 25_508_851, 6_880_646),
            Err(NoxaHydrationError::UnexpectedInitialSwap)
        ));
    }

    #[test]
    fn rejects_l2_height_used_as_restriction_clock() {
        let error =
            hydrate_noxa_launch_receipt(&live_receipt_logs(), 6_880_646, 6_880_646).unwrap_err();
        assert!(matches!(
            error,
            NoxaHydrationError::RestrictionWindowMismatch
        ));
    }
}
