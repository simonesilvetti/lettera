//! Passwords and refresh tokens, stored only in the system keyring
//! under service `lettera`, user `<email>` (shown as `lettera/<email>`).

use crate::error::{Error, Result};

const SERVICE: &str = "lettera";

fn entry(email: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, email).map_err(|e| Error::Keyring(e.to_string()))
}

pub fn set(email: &str, secret: &str) -> Result<()> {
    entry(email)?
        .set_password(secret)
        .map_err(|e| Error::Keyring(e.to_string()))
}

pub fn get(email: &str) -> Result<String> {
    entry(email)?.get_password().map_err(|e| match e {
        keyring::Error::NoEntry => Error::Keyring(format!("nessun segreto per {email}")),
        other => Error::Keyring(other.to_string()),
    })
}

pub fn delete(email: &str) -> Result<()> {
    match entry(email)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(Error::Keyring(e.to_string())),
    }
}
