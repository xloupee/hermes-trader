use std::fmt;
use std::fs;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use alloy_primitives::Address;
use k256::ecdsa::SigningKey;
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

use crate::noxa_trade::{
    ApprovalTransactionPlan, PreparedRawTransaction, TradePlanError, TradeTransactionPlan,
};

const MAX_PASSWORD_BYTES: u64 = 4_096;

pub trait TradeSigner: Send + Sync {
    fn address(&self) -> Address;

    fn sign_trade(
        &self,
        plan: &TradeTransactionPlan,
    ) -> Result<PreparedRawTransaction, TradePlanError>;

    fn sign_approval(
        &self,
        plan: &ApprovalTransactionPlan,
    ) -> Result<PreparedRawTransaction, TradePlanError>;
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SignerLoadError {
    #[error("keystore path must be a regular file, not a symlink")]
    InvalidFile,
    #[error("keystore permissions must not grant group or other access")]
    InsecurePermissions,
    #[error("keystore parent directory must not be a symlink or group/other writable")]
    InsecureParent,
    #[error("keystore path changed while it was being decrypted")]
    FileChanged,
    #[error("could not read keystore metadata")]
    Metadata,
    #[error("keystore password is empty or exceeds 4096 bytes")]
    InvalidPassword,
    #[error("could not read keystore password")]
    PasswordRead,
    #[error("encrypted keystore could not be decrypted")]
    Decryption,
    #[error("decrypted keystore did not contain one valid secp256k1 key")]
    InvalidKey,
    #[error("keystore signer does not match the required trading address")]
    AddressMismatch,
}

/// In-memory signer loaded from a standard encrypted Web3 Secret Storage
/// keystore. Debug output is deliberately redacted and construction requires
/// binding the decrypted key to a preconfigured public address.
pub struct KeystoreTradeSigner {
    signing_key: SigningKey,
    address: Address,
}

impl KeystoreTradeSigner {
    /// Read the password from an already-open protected stream, such as an
    /// inherited file descriptor or a systemd credential. CLI arguments and
    /// environment-variable password values are intentionally unsupported.
    pub fn load_from_reader(
        keystore_path: &Path,
        password_reader: impl Read,
        expected_address: Address,
    ) -> Result<Self, SignerLoadError> {
        let validated_file = validate_keystore_file(keystore_path)?;
        if expected_address == Address::ZERO {
            return Err(SignerLoadError::AddressMismatch);
        }

        let mut password = Zeroizing::new(Vec::new());
        password_reader
            .take(MAX_PASSWORD_BYTES + 1)
            .read_to_end(&mut password)
            .map_err(|_| SignerLoadError::PasswordRead)?;
        if password.len() > MAX_PASSWORD_BYTES as usize {
            return Err(SignerLoadError::InvalidPassword);
        }
        while matches!(password.last(), Some(b'\n' | b'\r')) {
            password.pop();
        }
        if password.is_empty() {
            return Err(SignerLoadError::InvalidPassword);
        }

        let mut decrypted = Zeroizing::new(
            eth_keystore::decrypt_key(keystore_path, password.as_slice())
                .map_err(|_| SignerLoadError::Decryption)?,
        );
        validated_file.verify_unchanged(keystore_path)?;
        if decrypted.len() != 32 {
            decrypted.zeroize();
            return Err(SignerLoadError::InvalidKey);
        }
        let signing_key = SigningKey::from_slice(decrypted.as_slice())
            .map_err(|_| SignerLoadError::InvalidKey)?;
        decrypted.zeroize();
        let address = Address::from_private_key(&signing_key);
        if address != expected_address {
            return Err(SignerLoadError::AddressMismatch);
        }
        Ok(Self {
            signing_key,
            address,
        })
    }
}

impl TradeSigner for KeystoreTradeSigner {
    fn address(&self) -> Address {
        self.address
    }

    fn sign_trade(
        &self,
        plan: &TradeTransactionPlan,
    ) -> Result<PreparedRawTransaction, TradePlanError> {
        plan.sign(&self.signing_key)
    }

    fn sign_approval(
        &self,
        plan: &ApprovalTransactionPlan,
    ) -> Result<PreparedRawTransaction, TradePlanError> {
        plan.sign(&self.signing_key)
    }
}

impl fmt::Debug for KeystoreTradeSigner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("KeystoreTradeSigner")
            .field("address", &self.address)
            .field("signing_key", &"[REDACTED]")
            .finish()
    }
}

struct ValidatedKeystoreFile {
    _handle: File,
    device: u64,
    inode: u64,
}

impl ValidatedKeystoreFile {
    fn verify_unchanged(&self, path: &Path) -> Result<(), SignerLoadError> {
        let metadata = fs::symlink_metadata(path).map_err(|_| SignerLoadError::FileChanged)?;
        if metadata.file_type().is_symlink()
            || metadata.dev() != self.device
            || metadata.ino() != self.inode
        {
            return Err(SignerLoadError::FileChanged);
        }
        Ok(())
    }
}

fn validate_keystore_file(path: &Path) -> Result<ValidatedKeystoreFile, SignerLoadError> {
    let parent = path.parent().ok_or(SignerLoadError::InsecureParent)?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|_| SignerLoadError::Metadata)?;
    if parent_metadata.file_type().is_symlink()
        || !parent_metadata.is_dir()
        || parent_metadata.permissions().mode() & 0o022 != 0
    {
        return Err(SignerLoadError::InsecureParent);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| SignerLoadError::Metadata)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(SignerLoadError::InvalidFile);
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(SignerLoadError::InsecurePermissions);
    }
    let handle = File::open(path).map_err(|_| SignerLoadError::Metadata)?;
    let opened_metadata = handle.metadata().map_err(|_| SignerLoadError::Metadata)?;
    if !opened_metadata.is_file()
        || opened_metadata.dev() != metadata.dev()
        || opened_metadata.ino() != metadata.ino()
    {
        return Err(SignerLoadError::FileChanged);
    }
    Ok(ValidatedKeystoreFile {
        _handle: handle,
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Cursor;

    use rand::thread_rng;
    use tempfile::tempdir;

    use super::*;

    fn encrypted_keystore() -> (tempfile::TempDir, std::path::PathBuf, Address) {
        let directory = tempdir().unwrap();
        let key = [7_u8; 32];
        let expected = Address::from_private_key(&SigningKey::from_slice(&key).unwrap());
        eth_keystore::encrypt_key(
            directory.path(),
            &mut thread_rng(),
            key,
            b"correct horse battery staple",
            Some("trader.json"),
        )
        .unwrap();
        let path = directory.path().join("trader.json");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        (directory, path, expected)
    }

    #[test]
    fn loads_encrypted_keystore_and_redacts_debug_output() {
        let (_directory, path, expected) = encrypted_keystore();
        let signer = KeystoreTradeSigner::load_from_reader(
            &path,
            Cursor::new(b"correct horse battery staple\n"),
            expected,
        )
        .unwrap();
        assert_eq!(signer.address(), expected);
        let debug = format!("{signer:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("correct horse"));
    }

    #[test]
    fn rejects_wrong_expected_address_without_exposing_key_material() {
        let (_directory, path, _expected) = encrypted_keystore();
        let error = KeystoreTradeSigner::load_from_reader(
            &path,
            Cursor::new(b"correct horse battery staple"),
            Address::with_last_byte(1),
        )
        .unwrap_err();
        assert_eq!(error, SignerLoadError::AddressMismatch);
        assert_eq!(
            error.to_string(),
            "keystore signer does not match the required trading address"
        );
    }

    #[test]
    fn rejects_group_readable_keystore() {
        let (_directory, path, expected) = encrypted_keystore();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        let error = KeystoreTradeSigner::load_from_reader(
            &path,
            Cursor::new(b"correct horse battery staple"),
            expected,
        )
        .unwrap_err();
        assert_eq!(error, SignerLoadError::InsecurePermissions);
    }

    #[test]
    fn rejects_group_writable_keystore_directory() {
        let (directory, path, expected) = encrypted_keystore();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o770)).unwrap();
        let error = KeystoreTradeSigner::load_from_reader(
            &path,
            Cursor::new(b"correct horse battery staple"),
            expected,
        )
        .unwrap_err();
        assert_eq!(error, SignerLoadError::InsecureParent);
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }

    #[test]
    fn rejects_symlinked_keystore() {
        use std::os::unix::fs::symlink;

        let (directory, path, expected) = encrypted_keystore();
        let link = directory.path().join("link.json");
        symlink(path, &link).unwrap();
        let error = KeystoreTradeSigner::load_from_reader(
            &link,
            Cursor::new(b"correct horse battery staple"),
            expected,
        )
        .unwrap_err();
        assert_eq!(error, SignerLoadError::InvalidFile);
    }

    #[test]
    fn password_can_arrive_over_an_open_file_descriptor_stream() {
        let (directory, path, expected) = encrypted_keystore();
        let password_path = directory.path().join("credential");
        fs::write(&password_path, b"correct horse battery staple\n").unwrap();
        fs::set_permissions(&password_path, fs::Permissions::from_mode(0o600)).unwrap();
        let password = File::open(password_path).unwrap();
        let signer = KeystoreTradeSigner::load_from_reader(&path, password, expected).unwrap();
        assert_eq!(signer.address(), expected);
    }
}
