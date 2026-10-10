use anyhow::{Context, Result, bail};
use zeroize::ZeroizeOnDrop;

use crate::config::CredentialConfig;

/// A resolved credential that deliberately has no `Debug` or `Display` implementation.
#[derive(ZeroizeOnDrop)]
pub struct ResolvedCredential {
    secret: String,
    #[zeroize(skip)]
    source: &'static str,
}

impl ResolvedCredential {
    pub(crate) fn expose_to_http_client(&self) -> &str {
        &self.secret
    }

    pub fn source(&self) -> &'static str {
        debug_assert!(!self.secret.is_empty());
        self.source
    }
}

pub fn resolve(config: &CredentialConfig) -> Result<ResolvedCredential> {
    if let Ok(secret) = std::env::var(&config.api_key_env) {
        return validate(secret, "environment");
    }

    let secret = get_from_credential_store(config)?;
    validate(secret, "credential store")
}

#[cfg(target_os = "macos")]
fn get_from_credential_store(config: &CredentialConfig) -> Result<String> {
    use std::process::Command;

    // Apple's signed broker avoids granting each changing development binary direct Keychain ACLs.
    let output = Command::new("/usr/bin/security")
        .args([
            "find-generic-password",
            "-s",
            &config.keyring_service,
            "-a",
            &config.keyring_account,
            "-w",
        ])
        .output()
        .context("failed to invoke the macOS Keychain broker")?;
    if !output.status.success() {
        bail!("Telnyx API key was not found in the configured macOS Keychain entry");
    }
    String::from_utf8(output.stdout)
        .map(|secret| secret.trim_end_matches(['\r', '\n']).to_owned())
        .context("macOS Keychain returned a non-UTF-8 credential")
}

#[cfg(not(target_os = "macos"))]
fn get_from_credential_store(config: &CredentialConfig) -> Result<String> {
    let entry = keyring::Entry::new(&config.keyring_service, &config.keyring_account)
        .context("failed to open the operating-system credential store")?;
    entry
        .get_password()
        .context("Telnyx API key was not found in the configured credential-store entry")
}

fn validate(secret: String, source: &'static str) -> Result<ResolvedCredential> {
    if secret.trim().len() < 16 {
        bail!("resolved Telnyx API key is unexpectedly short");
    }
    Ok(ResolvedCredential { secret, source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_secrets_without_echoing_them() {
        let error = match validate("too-short".to_owned(), "test") {
            Ok(_) => panic!("short credential unexpectedly passed validation"),
            Err(error) => error,
        };
        assert_eq!(
            error.to_string(),
            "resolved Telnyx API key is unexpectedly short"
        );
    }
}
