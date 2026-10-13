use std::{fs, net::SocketAddr, path::Path};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const DEFAULT_ACKNOWLEDGEMENT_SOUND_URL: &str =
    "https://raw.githubusercontent.com/justinweiler/Bellhound-Sounds/master/bellhound-thinking.mp3";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub credentials: CredentialConfig,
    pub relay: RelayConfig,
    pub protocol: ProtocolConfig,
    pub acknowledgement: AcknowledgementConfig,
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
    /// Private loopback listener used by model-neutral CLI adapters.
    pub control_bind: SocketAddr,
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
    /// Quiet period after the latest transcript frame before finalizing a caller turn.
    pub continuation_window_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ProtocolConfig {
    /// Maximum text accepted in one `speak` command.
    pub max_speak_chars: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct AcknowledgementConfig {
    /// Speak a deterministic holding phrase when an agent response is not immediate.
    pub enabled: bool,
    /// Time to wait after a final utterance before speaking the holding phrase.
    pub delay_ms: u64,
    /// Prefer the built-in sound cue or speak the configured text.
    pub mode: AcknowledgementMode,
    /// Optional public HTTPS media URL used by sound mode instead of the embedded endpoint.
    pub sound_url: Option<String>,
    /// Text spoken by Bellhound. This is transport behavior, not model output.
    pub text: String,
    /// Short social, closing, or voicemail phrases that should not trigger acknowledgement.
    pub suppress_phrases: Vec<String>,
    /// Phrases that suppress acknowledgements for the remainder of the call.
    pub suppress_for_session_phrases: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AcknowledgementMode {
    Sound,
    #[default]
    Speech,
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
            control_bind: "127.0.0.1:8788".parse().expect("literal socket address"),
            path: "/conversation-relay".to_owned(),
            webhook_path: "/webhooks/telnyx".to_owned(),
            token_env: "BELLHOUND_RELAY_TOKEN".to_owned(),
            max_call_seconds: 300,
            emit_partial_transcripts: false,
            continuation_window_ms: 2_500,
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

impl Default for AcknowledgementConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            delay_ms: 500,
            mode: AcknowledgementMode::Speech,
            sound_url: Some(DEFAULT_ACKNOWLEDGEMENT_SOUND_URL.to_owned()),
            text: "Got it. Give me a moment.".to_owned(),
            suppress_phrases: vec![
                "thank you".to_owned(),
                "thanks".to_owned(),
                "bye".to_owned(),
                "goodbye".to_owned(),
                "good night".to_owned(),
                "goodnight".to_owned(),
                "love you".to_owned(),
            ],
            suppress_for_session_phrases: vec![
                "call has been forwarded to an automated voice messaging system".to_owned(),
                "the person you are trying to reach is not available".to_owned(),
            ],
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
        if !self.relay.control_bind.ip().is_loopback() {
            bail!("relay.control_bind must be a loopback address");
        }
        if self.relay.control_bind == self.relay.bind {
            bail!("relay.control_bind must differ from relay.bind");
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
        if !(30..=3600).contains(&self.relay.max_call_seconds) {
            bail!("relay.max_call_seconds must be between 30 and 3600");
        }
        if self.relay.continuation_window_ms > 5_000 {
            bail!("relay.continuation_window_ms must not exceed 5000");
        }
        if self.protocol.max_speak_chars == 0 {
            bail!("protocol.max_speak_chars must be greater than zero");
        }
        if self.acknowledgement.enabled && self.acknowledgement.text.trim().is_empty() {
            bail!("acknowledgement.text must not be empty when acknowledgements are enabled");
        }
        if self.acknowledgement.text.chars().count() > self.protocol.max_speak_chars {
            bail!("acknowledgement.text exceeds protocol.max_speak_chars");
        }
        if self.acknowledgement.delay_ms > 10_000 {
            bail!("acknowledgement.delay_ms must not exceed 10000");
        }
        if let Some(sound_url) = &self.acknowledgement.sound_url
            && !sound_url.starts_with("https://")
        {
            bail!("acknowledgement.sound_url must start with https://");
        }
        if self
            .acknowledgement
            .suppress_phrases
            .iter()
            .chain(self.acknowledgement.suppress_for_session_phrases.iter())
            .any(|phrase| phrase.trim().is_empty())
        {
            bail!("acknowledgement suppression phrases must not contain empty phrases");
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

    pub fn acknowledgement_audio_route(&self) -> String {
        format!("{}/{{token}}/thinking.mp3", self.relay.path)
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
        let config = Config::default();
        config.validate().unwrap();
        assert!(config.acknowledgement.enabled);
        assert_eq!(config.relay.continuation_window_ms, 2_500);
        assert_eq!(config.acknowledgement.mode, AcknowledgementMode::Speech);
        assert_eq!(
            config.acknowledgement.sound_url.as_deref(),
            Some(DEFAULT_ACKNOWLEDGEMENT_SOUND_URL)
        );
    }

    #[test]
    fn rejects_empty_enabled_acknowledgement() {
        let mut config = Config::default();
        config.acknowledgement.text.clear();
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("acknowledgement.text")
        );
    }

    #[test]
    fn rejects_insecure_acknowledgement_sound_url() {
        let mut config = Config::default();
        config.acknowledgement.sound_url = Some("http://example.test/thinking.mp3".to_owned());
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("sound_url")
        );
    }

    #[test]
    fn rejects_excessive_continuation_window() {
        let mut config = Config::default();
        config.relay.continuation_window_ms = 5_001;
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("continuation_window_ms")
        );
    }

    #[test]
    fn rejects_unsafe_call_duration_limits() {
        for seconds in [0, 29, 3_601] {
            let mut config = Config::default();
            config.relay.max_call_seconds = seconds;
            assert!(
                config
                    .validate()
                    .unwrap_err()
                    .to_string()
                    .contains("max_call_seconds")
            );
        }
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
