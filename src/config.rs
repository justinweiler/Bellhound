use std::{fs, net::SocketAddr, path::Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub credentials: CredentialConfig,
    pub relay: RelayConfig,
    pub protocol: ProtocolConfig,
    pub telnyx: Option<TelnyxConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct CredentialConfig {
    /// Environment variable checked before the operating-system credential store.
    pub api_key_env: String,
    /// Credential-store service name.
    pub keyring_service: String,
    /// Credential-store account or username.
    pub keyring_account: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RelayConfig {
    /// Local listener. Put a TLS tunnel or reverse proxy in front of this address.
    pub bind: SocketAddr,
    /// WebSocket path before the secret token segment.
    pub path: String,
    /// HTTP webhook path before the secret token segment.
    pub webhook_path: String,
    /// Environment variable containing the unguessable URL token.
    pub token_env: String,
    /// Hard stop for one connected call.
    pub max_call_seconds: u64,
    /// Emit interim STT fragments as events. Final utterances are always emitted.
    pub emit_partial_transcripts: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ProtocolConfig {
    /// Maximum text accepted in one `speak` command.
    pub max_speak_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TelnyxConfig {
    pub connection_id: String,
    pub from_number: String,
    pub to_number: String,
    /// Public WSS URL through the path segment, excluding the secret token.
    pub public_wss_base: String,
    #[serde(default = "default_voice")]
    pub voice: String,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_transcription_engine")]
    pub transcription_engine: String,
    #[serde(default = "default_transcription_model")]
    pub transcription_model: String,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8787".parse().expect("literal socket address"),
            path: "/conversation-relay".to_owned(),
            webhook_path: "/webhooks/telnyx".to_owned(),
            token_env: "BELLHOUND_RELAY_TOKEN".to_owned(),
            max_call_seconds: 300,
            emit_partial_transcripts: false,
        }
    }
}

impl Default for CredentialConfig {
    fn default() -> Self {
        Self {
            api_key_env: "TELNYX_API_KEY".to_owned(),
            keyring_service: "Telnyx API key".to_owned(),
            keyring_account: "your-account".to_owned(),
        }
    }
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            max_speak_chars: 2_000,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("failed to read configuration {}", path.display()))?;
        let config: Self = toml::from_str(&source)
            .with_context(|| format!("failed to parse configuration {}", path.display()))?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.credentials.api_key_env.trim().is_empty() {
            bail!("credentials.api_key_env must not be empty");
        }
        if self.credentials.keyring_service.trim().is_empty()
            || self.credentials.keyring_account.trim().is_empty()
        {
            bail!("credential-store service and account must not be empty");
        }
        if !self.relay.bind.ip().is_loopback() {
            bail!("relay.bind must be a loopback address; expose it through a TLS tunnel");
        }
        if !self.relay.path.starts_with('/') || self.relay.path.ends_with('/') {
            bail!("relay.path must start with '/' and must not end with '/'");
        }
        if !self.relay.webhook_path.starts_with('/') || self.relay.webhook_path.ends_with('/') {
            bail!("relay.webhook_path must start with '/' and must not end with '/'");
        }
        if self.relay.token_env.trim().is_empty() {
            bail!("relay.token_env must not be empty");
        }
        if self.relay.max_call_seconds == 0 {
            bail!("relay.max_call_seconds must be greater than zero");
        }
        if self.protocol.max_speak_chars == 0 {
            bail!("protocol.max_speak_chars must be greater than zero");
        }
        if let Some(telnyx) = &self.telnyx {
            if telnyx.connection_id.trim().is_empty() {
                bail!("telnyx.connection_id must not be empty");
            }
            if !looks_like_e164(&telnyx.from_number) || !looks_like_e164(&telnyx.to_number) {
                bail!("telnyx from_number and to_number must use +E.164 format");
            }
            if !telnyx.public_wss_base.starts_with("wss://") {
                bail!("telnyx.public_wss_base must start with wss://");
            }
            if telnyx.voice.trim().is_empty() {
                bail!("telnyx.voice must not be empty");
            }
            if telnyx.voice.eq_ignore_ascii_case("Telnyx.Ultra.Callie") {
                bail!(
                    "Telnyx.Ultra.Callie is not a valid Ultra voice ID; use a listed voice such as Telnyx.NaturalHD.astra"
                );
            }
        }
        Ok(())
    }

    pub fn write_example(path: &Path, force: bool) -> Result<()> {
        if path.exists() && !force {
            bail!(
                "configuration {} already exists; pass --force to replace it",
                path.display()
            );
        }
        let rendered = toml::to_string_pretty(&Self::default())?;
        fs::write(path, rendered)
            .with_context(|| format!("failed to write configuration {}", path.display()))?;
        println!("wrote configuration: {}", path.display());
        Ok(())
    }

    pub fn route(&self) -> String {
        format!("{}/{{token}}", self.relay.path)
    }

    pub fn webhook_route(&self) -> String {
        format!("{}/{{token}}", self.relay.webhook_path)
    }
}

fn looks_like_e164(value: &str) -> bool {
    value.starts_with('+')
        && value.len() >= 8
        && value.len() <= 16
        && value[1..].bytes().all(|byte| byte.is_ascii_digit())
}

fn default_voice() -> String {
    "Telnyx.NaturalHD.astra".to_owned()
}

fn default_language() -> String {
    "en-US".to_owned()
}

fn default_transcription_engine() -> String {
    "Deepgram".to_owned()
}

fn default_transcription_model() -> String {
    "deepgram/nova-3".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        Config::default().validate().unwrap();
    }

    #[test]
    fn rejects_non_loopback_bind() {
        let mut config = Config::default();
        config.relay.bind = "0.0.0.0:8787".parse().unwrap();
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("loopback")
        );
    }

    #[test]
    fn rejects_unknown_fields() {
        let source = r#"
            [relay]
            bind = "127.0.0.1:8787"
            surprise = true
        "#;
        assert!(toml::from_str::<Config>(source).is_err());
    }

    #[test]
    fn validates_optional_telnyx_settings() {
        let config = Config {
            telnyx: Some(TelnyxConfig {
                connection_id: "123".to_owned(),
                from_number: "+14245550100".to_owned(),
                to_number: "+18185550100".to_owned(),
                public_wss_base: "wss://example.test/conversation-relay".to_owned(),
                voice: default_voice(),
                language: default_language(),
                transcription_engine: default_transcription_engine(),
                transcription_model: default_transcription_model(),
            }),
            ..Config::default()
        };
        config.validate().unwrap();
    }

    #[test]
    fn rejects_known_invalid_ultra_voice_name() {
        let config = Config {
            telnyx: Some(TelnyxConfig {
                connection_id: "123".to_owned(),
                from_number: "+14245550100".to_owned(),
                to_number: "+18185550100".to_owned(),
                public_wss_base: "wss://example.test/conversation-relay".to_owned(),
                voice: "Telnyx.Ultra.Callie".to_owned(),
                language: default_language(),
                transcription_engine: default_transcription_engine(),
                transcription_model: default_transcription_model(),
            }),
            ..Config::default()
        };
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("not a valid Ultra voice ID")
        );
    }
}
