use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{config::Config, credentials};

#[derive(Serialize)]
struct DialRequest<'a> {
    connection_id: &'a str,
    from: &'a str,
    to: &'a str,
    time_limit_secs: u64,
    command_id: String,
    conversation_relay_config: ConversationRelayConfig<'a>,
}

#[derive(Serialize)]
struct ConversationRelayConfig<'a> {
    url: String,
    greeting: &'static str,
    voice: &'a str,
    tts_provider: &'static str,
    language: &'a str,
    transcription_engine: &'a str,
    transcription_engine_config: TranscriptionConfig<'a>,
    dtmf_detection: bool,
    interruptible: &'static str,
    interruptible_greeting: &'static str,
}

#[derive(Serialize)]
struct TranscriptionConfig<'a> {
    transcription_model: &'a str,
}

#[derive(Debug, Deserialize)]
struct DialResponse {
    data: DialData,
}

#[derive(Debug, Deserialize)]
struct DialData {
    call_control_id: String,
    call_leg_id: String,
    call_session_id: String,
}

#[derive(Serialize)]
struct HangupRequest {
    command_id: String,
}

pub async fn dial(config: &Config) -> Result<()> {
    let telnyx = config
        .telnyx
        .as_ref()
        .context("the [telnyx] configuration section is required for dialing")?;
    let token = std::env::var(&config.relay.token_env).with_context(|| {
        format!(
            "required relay token environment variable {} is not set",
            config.relay.token_env
        )
    })?;
    if token.len() < 24 {
        bail!("relay token must contain at least 24 characters");
    }
    let api_key = credentials::resolve(&config.credentials)?;
    let websocket_url = format!("{}/{}", telnyx.public_wss_base.trim_end_matches('/'), token);
    let request = DialRequest {
        connection_id: &telnyx.connection_id,
        from: &telnyx.from_number,
        to: &telnyx.to_number,
        time_limit_secs: config.relay.max_call_seconds.max(30),
        command_id: Uuid::new_v4().to_string(),
        conversation_relay_config: ConversationRelayConfig {
            url: websocket_url.clone(),
            // A short audible primer completes and warms Telnyx's greeting path. Bellhound sends
            // the measured, call-specific opening after the setup frame arrives.
            greeting: "Bellhound.",
            voice: &telnyx.voice,
            tts_provider: "telnyx",
            language: &telnyx.language,
            transcription_engine: &telnyx.transcription_engine,
            transcription_engine_config: TranscriptionConfig {
                transcription_model: &telnyx.transcription_model,
            },
            dtmf_detection: true,
            interruptible: "speech",
            interruptible_greeting: "speech",
        },
    };

    let response = reqwest::Client::new()
        .post("https://api.telnyx.com/v2/calls")
        .bearer_auth(api_key.expose_to_http_client())
        .json(&request)
        .send()
        .await
        .context("failed to send Telnyx Dial request")?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!(
            "Telnyx Dial failed with HTTP {status}: {}",
            sanitize_error(
                &body,
                &[
                    &token,
                    &telnyx.from_number,
                    &telnyx.to_number,
                    &websocket_url,
                ],
            )
        );
    }
    let response: DialResponse = response
        .json()
        .await
        .context("Telnyx returned an invalid Dial response")?;
    println!(
        "{}",
        serde_json::json!({
            "event": "dial_accepted",
            "call_control_id": response.data.call_control_id,
            "call_leg_id": response.data.call_leg_id,
            "call_session_id": response.data.call_session_id,
        })
    );
    Ok(())
}

pub async fn hangup(config: &Config, call_control_id: &str) -> Result<()> {
    if call_control_id.trim().is_empty() {
        bail!("call control ID must not be empty");
    }
    let api_key = credentials::resolve(&config.credentials)?;
    let endpoint = format!("https://api.telnyx.com/v2/calls/{call_control_id}/actions/hangup");
    let request = HangupRequest {
        command_id: Uuid::new_v4().to_string(),
    };
    let response = reqwest::Client::new()
        .post(&endpoint)
        .bearer_auth(api_key.expose_to_http_client())
        .json(&request)
        .send()
        .await
        .context("failed to send Telnyx Hangup request")?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        bail!(
            "Telnyx Hangup failed with HTTP {status}: {}",
            sanitize_error(&body, &[call_control_id, &endpoint])
        );
    }
    Ok(())
}

fn sanitize_error(body: &str, secrets: &[&str]) -> String {
    let mut compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    for secret in secrets {
        if !secret.is_empty() {
            compact = compact.replace(secret, "[REDACTED]");
        }
    }
    compact.chars().take(1_000).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_sanitizer_is_bounded_and_single_line() {
        let source = format!("line one\n{}", "x".repeat(2_000));
        let sanitized = sanitize_error(&source, &[]);
        assert!(!sanitized.contains('\n'));
        assert_eq!(sanitized.chars().count(), 1_000);
    }

    #[test]
    fn error_sanitizer_redacts_tokens_and_numbers() {
        let sanitized = sanitize_error(
            "bad token-secret for +18185550100",
            &["token-secret", "+18185550100"],
        );
        assert_eq!(sanitized, "bad [REDACTED] for [REDACTED]");
    }
}
