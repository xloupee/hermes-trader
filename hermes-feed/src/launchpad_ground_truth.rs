use alloy_primitives::keccak256;

use crate::launchpad_adapter::LaunchpadId;
use crate::launchpad_adapters::{
    CLANKER_FACTORY, CLANKER_TOKEN_CREATED_TOPIC, DOPPLER_CREATE_EMITTER, DOPPLER_CREATE_TOPIC,
};
use crate::noxa_abi::ReceiptLog;
use crate::pons::{PONS_CURRENT_FACTORY, PONS_LEGACY_FACTORY, PONS_TOKEN_LAUNCHED_TOPIC};
use crate::robinhood::{BOW_LAUNCH_FACTORY, LAUNCHHOOD_V3_FACTORY};
use crate::tier2_curve::HOOD_FACTORY;

pub const BOW_LAUNCHED_SIGNATURE: &str = "Launched(address,address,address,uint256,uint256)";
pub const LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE: &str = "TokenLaunched(address,address,address,address,uint256,uint256,uint256,uint256,uint256,uint256)";
pub const HOOD_TOKEN_CREATED_SIGNATURE: &str =
    "TokenCreated(address,address,string,string,string,uint256,uint256,uint256)";
pub const HOOD_TRADE_SIGNATURE: &str =
    "Trade(address,address,bool,uint256,uint256,uint256,uint256,uint256)";

/// Classify only the reviewed exact emitter/topic pairs used as launchpad
/// ground truth. Callers must not classify the RPC address/topic Cartesian
/// product without this second exact-pair check.
pub fn launchpad_for_ground_truth_log(log: &ReceiptLog) -> Option<LaunchpadId> {
    let topic = *log.topics.first()?;
    match (log.address, topic) {
        (BOW_LAUNCH_FACTORY, topic) if topic == keccak256(BOW_LAUNCHED_SIGNATURE.as_bytes()) => {
            Some(LaunchpadId::Bow)
        }
        (LAUNCHHOOD_V3_FACTORY, topic)
            if topic == keccak256(LAUNCHHOOD_TOKEN_LAUNCHED_SIGNATURE.as_bytes()) =>
        {
            Some(LaunchpadId::LaunchHoodV3)
        }
        (CLANKER_FACTORY, CLANKER_TOKEN_CREATED_TOPIC) => Some(LaunchpadId::Clanker),
        (DOPPLER_CREATE_EMITTER, DOPPLER_CREATE_TOPIC) => Some(LaunchpadId::BankrDoppler),
        (PONS_CURRENT_FACTORY, PONS_TOKEN_LAUNCHED_TOPIC)
        | (PONS_LEGACY_FACTORY, PONS_TOKEN_LAUNCHED_TOPIC) => Some(LaunchpadId::Pons),
        (HOOD_FACTORY, topic)
            if topic == keccak256(HOOD_TOKEN_CREATED_SIGNATURE.as_bytes())
                || topic == keccak256(HOOD_TRADE_SIGNATURE.as_bytes()) =>
        {
            Some(LaunchpadId::HoodFun)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::{Address, B256, Bytes};

    use super::*;

    fn log(address: Address, topic: B256) -> ReceiptLog {
        ReceiptLog {
            address,
            topics: vec![topic],
            data: Bytes::new(),
            log_index: 0,
        }
    }

    #[test]
    fn rejects_cross_pairs_and_accepts_both_pons_generations() {
        assert_eq!(
            launchpad_for_ground_truth_log(&log(CLANKER_FACTORY, CLANKER_TOKEN_CREATED_TOPIC)),
            Some(LaunchpadId::Clanker)
        );
        assert_eq!(
            launchpad_for_ground_truth_log(&log(PONS_CURRENT_FACTORY, PONS_TOKEN_LAUNCHED_TOPIC)),
            Some(LaunchpadId::Pons)
        );
        assert_eq!(
            launchpad_for_ground_truth_log(&log(PONS_LEGACY_FACTORY, PONS_TOKEN_LAUNCHED_TOPIC)),
            Some(LaunchpadId::Pons)
        );
        assert_eq!(
            launchpad_for_ground_truth_log(&log(PONS_CURRENT_FACTORY, CLANKER_TOKEN_CREATED_TOPIC)),
            None
        );
    }
}
