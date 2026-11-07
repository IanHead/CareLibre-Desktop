//! Secure storage for the refresh token.
//!
//! The OS keychain is used when available (Windows Credential Manager, macOS
//! Keychain, Linux Secret Service). If no keychain is available — for example
//! inside a minimal container — an encrypted local vault is used instead. The
//! vault key is derived from the machine identifier and a random salt, so the
//! encrypted token cannot be moved to another machine.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const SERVICE: &str = "CareLibre Desktop";
const VAULT_VERSION: u8 = 1;

#[derive(Default, Serialize, Deserialize)]
struct Vault {
    version: u8,
    salt: String,
    entries: BTreeMap<String, VaultEntry>,
}

#[derive(Serialize, Deserialize)]
struct VaultEntry {
    nonce: String,
    data: String,
}

pub fn save(server: &str, email: &str, refresh_token: &str) -> Result<(), String> {
    let account = account(server, email);

    if let Ok(entry) = keyring::Entry::new(SERVICE, &account)
        && entry.set_password(refresh_token).is_ok()
    {
        return Ok(());
    }

    save_fallback(server, email, refresh_token)
}

pub fn load(server: &str, email: &str) -> Result<Option<String>, String> {
    let account = account(server, email);

    if let Ok(entry) = keyring::Entry::new(SERVICE, &account)
        && let Ok(token) = entry.get_password()
    {
        return Ok(Some(token));
    }

    load_fallback(server, email)
}

pub fn delete(server: &str, email: &str) -> Result<(), String> {
    let account = account(server, email);

    if let Ok(entry) = keyring::Entry::new(SERVICE, &account) {
        let _ = entry.delete_credential();
    }

    delete_fallback(server, email)
}

fn account(server: &str, email: &str) -> String {
    format!("{}|{}", server.trim_end_matches('/'), email.to_lowercase())
}

fn save_fallback(server: &str, email: &str, refresh_token: &str) -> Result<(), String> {
    let path = vault_path()?;
    let mut vault = read_vault(&path)?;

    if vault.salt.is_empty() {
        let mut salt = [0u8; 16];
        getrandom::getrandom(&mut salt).map_err(|error| format!("random source: {error}"))?;
        vault.salt = hex::encode(salt);
    }

    let key = derive_key(&vault.salt)?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(&key).map_err(|_| "invalid credential key".to_owned())?;
    let mut nonce = [0u8; 24];
    getrandom::getrandom(&mut nonce).map_err(|error| format!("random source: {error}"))?;

    let encrypted = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: refresh_token.as_bytes(),
                aad: account(server, email).as_bytes(),
            },
        )
        .map_err(|_| "could not encrypt the credential".to_owned())?;

    vault.entries.insert(
        account(server, email),
        VaultEntry {
            nonce: hex::encode(nonce),
            data: hex::encode(encrypted),
        },
    );
    vault.version = VAULT_VERSION;
    write_vault(&path, &vault)
}

fn load_fallback(server: &str, email: &str) -> Result<Option<String>, String> {
    let path = vault_path()?;
    let vault = read_vault(&path)?;
    let Some(entry) = vault.entries.get(&account(server, email)) else {
        return Ok(None);
    };

    let key = derive_key(&vault.salt)?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(&key).map_err(|_| "invalid credential key".to_owned())?;
    let nonce = hex::decode(&entry.nonce).map_err(|_| "invalid credential nonce".to_owned())?;
    let data = hex::decode(&entry.data).map_err(|_| "invalid credential data".to_owned())?;

    let decrypted = cipher
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &data,
                aad: account(server, email).as_bytes(),
            },
        )
        .map_err(|_| "could not decrypt the saved credential".to_owned())?;

    String::from_utf8(decrypted)
        .map(Some)
        .map_err(|_| "saved credential is not valid text".to_owned())
}

fn delete_fallback(server: &str, email: &str) -> Result<(), String> {
    let path = vault_path()?;
    let mut vault = read_vault(&path)?;
    if vault.entries.remove(&account(server, email)).is_some() {
        write_vault(&path, &vault)?;
    }
    Ok(())
}

fn derive_key(salt_hex: &str) -> Result<[u8; 32], String> {
    let machine = machine_uid::get().map_err(|error| {
        format!("secure credential storage is unavailable on this device: {error}")
    })?;
    let mut hasher = Sha256::new();
    hasher.update(b"CareLibre Desktop credential vault v1");
    hasher.update(machine.as_bytes());
    hasher.update(salt_hex.as_bytes());
    Ok(hasher.finalize().into())
}

fn vault_path() -> Result<PathBuf, String> {
    directories::ProjectDirs::from("au", "CareLibre", "CareLibre Desktop")
        .map(|dirs| dirs.config_dir().join("credentials.vault"))
        .ok_or_else(|| "could not locate a config directory for credentials".to_owned())
}

fn read_vault(path: &Path) -> Result<Vault, String> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| format!("could not read the credential vault: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vault::default()),
        Err(error) => Err(format!("could not read {}: {error}", path.display())),
    }
}

fn write_vault(path: &Path, vault: &Vault) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }

    let contents = serde_json::to_vec_pretty(vault)
        .map_err(|error| format!("could not serialise credentials: {error}"))?;

    let temporary = path.with_extension("vault.tmp");
    let mut file = fs::File::create(&temporary)
        .map_err(|error| format!("could not create {}: {error}", temporary.display()))?;
    file.write_all(&contents)
        .map_err(|error| format!("could not write credentials: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("could not flush credentials: {error}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600));
    }

    #[cfg(windows)]
    {
        let _ = fs::remove_file(path);
    }

    fs::rename(&temporary, path)
        .map_err(|error| format!("could not store credentials at {}: {error}", path.display()))
}
