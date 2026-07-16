use alloy_primitives::{Address, B256};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::robinhood::CHAIN_ID;

pub const FLAP_PORTAL_PROXY: Address =
    alloy_primitives::address!("26605f322f7ff986f381bb9a6e3f5dab0beaeb09");
pub const FLAP_PORTAL_IMPLEMENTATION: Address =
    alloy_primitives::address!("d9c9981d784a3765d8264d6104650b901c4e36b1");
pub const FLAP_VAULT_PORTAL_PROXY: Address =
    alloy_primitives::address!("e9f7ab7de8fb8756acbb6a1cd13316a43308197b");
pub const FLAP_VAULT_PORTAL_IMPLEMENTATION: Address =
    alloy_primitives::address!("2813cd0b6089f76f3407792f79276e5d4f80935a");

pub const FLAP_PORTAL_PROXY_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("cecb292d9c022858199c9348abf0d5836f9ea4dab5cf03710e1dcf41fd9a4c35");
pub const FLAP_PORTAL_IMPLEMENTATION_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("85facd83c203c88ea8f37c4f00c328f983e90c5045b06ec20ef18639c818186b");
pub const FLAP_VAULT_PORTAL_PROXY_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("e7109718479fd7c6d05b829ffc6a1469e4c949ae282497c15d179b2af4e5e3a9");
pub const FLAP_VAULT_PORTAL_IMPLEMENTATION_RUNTIME_KECCAK256: B256 =
    alloy_primitives::b256!("4f096b230a8db270585d54fdd549982efda99462daad9c4b3e771a62e7071f56");
pub const FLAP_PORTAL_VERSION: &str = "v5.14.16";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlapPortalVariant {
    Portal,
    VaultPortal,
}

impl FlapPortalVariant {
    pub const fn proxy(self) -> Address {
        match self {
            Self::Portal => FLAP_PORTAL_PROXY,
            Self::VaultPortal => FLAP_VAULT_PORTAL_PROXY,
        }
    }

    pub const fn implementation(self) -> Address {
        match self {
            Self::Portal => FLAP_PORTAL_IMPLEMENTATION,
            Self::VaultPortal => FLAP_VAULT_PORTAL_IMPLEMENTATION,
        }
    }

    pub const fn proxy_runtime_hash(self) -> B256 {
        match self {
            Self::Portal => FLAP_PORTAL_PROXY_RUNTIME_KECCAK256,
            Self::VaultPortal => FLAP_VAULT_PORTAL_PROXY_RUNTIME_KECCAK256,
        }
    }

    pub const fn implementation_runtime_hash(self) -> B256 {
        match self {
            Self::Portal => FLAP_PORTAL_IMPLEMENTATION_RUNTIME_KECCAK256,
            Self::VaultPortal => FLAP_VAULT_PORTAL_IMPLEMENTATION_RUNTIME_KECCAK256,
        }
    }

    /// VaultPortal's current implementation source is not verified, so a
    /// matching runtime identity permits observation but never execution.
    pub const fn source_verified(self) -> bool {
        matches!(self, Self::Portal)
    }
}

/// Control-plane observations collected before the feed candidate path is
/// armed. This value contains no client and candidate validation is pure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlapStartupIdentity<'a> {
    pub chain_id: u64,
    pub variant: FlapPortalVariant,
    pub proxy: Address,
    pub proxy_runtime_hash: B256,
    pub implementation: Address,
    pub implementation_runtime_hash: B256,
    pub version: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FlapIdentityError {
    #[error("Flap identity was resolved on a chain other than Robinhood mainnet 4663")]
    ChainMismatch,
    #[error("Flap proxy address changed or does not match its declared Portal variant")]
    ProxyMismatch,
    #[error("Flap proxy runtime code hash changed")]
    ProxyRuntimeMismatch,
    #[error("Flap EIP-1967 implementation address changed")]
    ImplementationMismatch,
    #[error("Flap implementation runtime code hash changed")]
    ImplementationRuntimeMismatch,
    #[error("Flap Portal version changed or is unavailable")]
    VersionMismatch,
}

/// Validate one registry entry independently so drift disables only the
/// affected Portal variant. Callers must not arm that variant on any error.
pub fn validate_flap_startup_identity(
    identity: &FlapStartupIdentity<'_>,
) -> Result<(), FlapIdentityError> {
    let variant = identity.variant;
    if identity.chain_id != CHAIN_ID {
        return Err(FlapIdentityError::ChainMismatch);
    }
    if identity.proxy != variant.proxy() {
        return Err(FlapIdentityError::ProxyMismatch);
    }
    if identity.proxy_runtime_hash != variant.proxy_runtime_hash() {
        return Err(FlapIdentityError::ProxyRuntimeMismatch);
    }
    if identity.implementation != variant.implementation() {
        return Err(FlapIdentityError::ImplementationMismatch);
    }
    if identity.implementation_runtime_hash != variant.implementation_runtime_hash() {
        return Err(FlapIdentityError::ImplementationRuntimeMismatch);
    }
    if variant == FlapPortalVariant::Portal && identity.version != Some(FLAP_PORTAL_VERSION) {
        return Err(FlapIdentityError::VersionMismatch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(variant: FlapPortalVariant) -> FlapStartupIdentity<'static> {
        FlapStartupIdentity {
            chain_id: CHAIN_ID,
            variant,
            proxy: variant.proxy(),
            proxy_runtime_hash: variant.proxy_runtime_hash(),
            implementation: variant.implementation(),
            implementation_runtime_hash: variant.implementation_runtime_hash(),
            version: (variant == FlapPortalVariant::Portal).then_some(FLAP_PORTAL_VERSION),
        }
    }

    #[test]
    fn accepts_pinned_portal_and_observation_only_vault_portal() {
        validate_flap_startup_identity(&fixture(FlapPortalVariant::Portal)).unwrap();
        validate_flap_startup_identity(&fixture(FlapPortalVariant::VaultPortal)).unwrap();
        assert!(FlapPortalVariant::Portal.source_verified());
        assert!(!FlapPortalVariant::VaultPortal.source_verified());
    }

    #[test]
    fn proxy_or_implementation_change_fails_closed() {
        let mut portal = fixture(FlapPortalVariant::Portal);
        portal.proxy_runtime_hash = B256::with_last_byte(1);
        assert_eq!(
            validate_flap_startup_identity(&portal),
            Err(FlapIdentityError::ProxyRuntimeMismatch)
        );

        let mut portal = fixture(FlapPortalVariant::Portal);
        portal.implementation = Address::with_last_byte(1);
        assert_eq!(
            validate_flap_startup_identity(&portal),
            Err(FlapIdentityError::ImplementationMismatch)
        );

        let mut portal = fixture(FlapPortalVariant::Portal);
        portal.implementation_runtime_hash = B256::with_last_byte(2);
        assert_eq!(
            validate_flap_startup_identity(&portal),
            Err(FlapIdentityError::ImplementationRuntimeMismatch)
        );

        let mut portal = fixture(FlapPortalVariant::Portal);
        portal.version = Some("v5.14.17");
        assert_eq!(
            validate_flap_startup_identity(&portal),
            Err(FlapIdentityError::VersionMismatch)
        );
    }

    #[test]
    fn chain_and_cross_variant_replays_fail_closed() {
        let mut portal = fixture(FlapPortalVariant::Portal);
        portal.chain_id = 8_453;
        assert_eq!(
            validate_flap_startup_identity(&portal),
            Err(FlapIdentityError::ChainMismatch)
        );

        let mut portal = fixture(FlapPortalVariant::Portal);
        portal.proxy = FLAP_VAULT_PORTAL_PROXY;
        assert_eq!(
            validate_flap_startup_identity(&portal),
            Err(FlapIdentityError::ProxyMismatch)
        );
    }
}
