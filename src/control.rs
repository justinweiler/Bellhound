use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::{config::Config, protocol::AgentCommand};

const TOKEN_HEADER: &str = "x-bellhound-token";

pub enum BatchWait {
    Events(serde_json::Value),
    Timeout,
    HistoryGap { after: u64, oldest_event_id: u64 },
}

#[derive(Debug, serde::Deserialize)]
pub struct CommandReceipt {
    pub accepted: bool,
    pub duplicate: bool,
    pub request_id: String,
}

fn relay_token(config: &Config) -> Result<String> {
    let token = std::env::var(&config.relay.token_env).with_context(|| {
        format!(
            "required relay token environment variable {} is not set",
            config.relay.token_env
        )
    })?;
    if token.len() < 24 {
        bail!("relay token must contain at least 24 characters");
    }
    Ok(token)
}

fn base_url(config: &Config) -> String {
    format!("http://{}", config.relay.control_bind)
}

pub async fn wait(config: &Config, after: u64, timeout_seconds: u64) -> Result<()> {
    let token = relay_token(config)?;
    let timeout_seconds = timeout_seconds.clamp(1, 300);
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds + 5))
        .build()?
        .get(format!("{}/v1/events/next", base_url(config)))
        .header(TOKEN_HEADER, token)
        .query(&[
            ("after", after.to_string()),
            ("timeout_seconds", timeout_seconds.to_string()),
        ])
        .send()
        .await
        .context("failed to reach the local Bellhound control listener")?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        println!(
            "{}",
            serde_json::json!({"event": "wait_timeout", "after": after})
        );
        return Ok(());
    }
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("Bellhound control wait failed with HTTP {status}: {body}");
    }
    println!("{}", response.text().await?);
    Ok(())
}

pub async fn status(config: &Config) -> Result<serde_json::Value> {
    let token = relay_token(config)?;
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?
        .get(format!("{}/v1/status", base_url(config)))
        .header(TOKEN_HEADER, token)
        .send()
        .await
        .context("failed to reach the local Bellhound control listener")?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("Bellhound control status failed with HTTP {status}: {body}");
    }
    response
        .json()
        .await
        .context("Bellhound control listener returned invalid status JSON")
}

pub async fn wait_batch(
    config: &Config,
    after: u64,
    timeout_seconds: u64,
    limit: usize,
) -> Result<BatchWait> {
    let token = relay_token(config)?;
    let timeout_seconds = timeout_seconds.clamp(1, 30);
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_seconds + 5))
        .build()?
        .get(format!("{}/v1/events", base_url(config)))
        .header(TOKEN_HEADER, token)
        .query(&[
            ("after", after.to_string()),
            ("timeout_seconds", timeout_seconds.to_string()),
            ("limit", limit.clamp(1, 32).to_string()),
        ])
        .send()
        .await
        .context("failed to reach the local Bellhound control listener")?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(BatchWait::Timeout);
    }
    if response.status() == reqwest::StatusCode::CONFLICT {
        let body: serde_json::Value = response
            .json()
            .await
            .context("Bellhound control listener returned invalid history-gap JSON")?;
        if body.get("error").and_then(serde_json::Value::as_str) == Some("event_history_gap") {
            return Ok(BatchWait::HistoryGap {
                after: body
                    .get("after")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(after),
                oldest_event_id: body
                    .get("oldest_event_id")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or_default(),
            });
        }
        bail!("Bellhound control event wait reported an unknown conflict");
    }
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("Bellhound control event wait failed with HTTP {status}: {body}");
    }
    response
        .json()
        .await
        .map(BatchWait::Events)
        .context("Bellhound control listener returned invalid event JSON")
}

pub async fn command(config: &Config, command: AgentCommand) -> Result<()> {
    let token = relay_token(config)?;
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!("{}/v1/commands", base_url(config)))
        .header(TOKEN_HEADER, token)
        .json(&command)
        .send()
        .await
        .context("failed to reach the local Bellhound control listener")?;
    if !response.status().is_success() {
        bail!(
            "Bellhound control command failed with HTTP {}",
            response.status()
        );
    }
    println!("{}", serde_json::json!({"accepted": true}));
    Ok(())
}

pub async fn command_idempotent(
    config: &Config,
    request_id: uuid::Uuid,
    command: AgentCommand,
) -> Result<CommandReceipt> {
    let token = relay_token(config)?;
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!("{}/v1/mcp/commands", base_url(config)))
        .header(TOKEN_HEADER, token)
        .json(&serde_json::json!({
            "request_id": request_id.to_string(),
            "command": command,
        }))
        .send()
        .await
        .context("failed to reach the local Bellhound control listener")?;
    if response.status() == reqwest::StatusCode::CONFLICT {
        bail!("request_id was already used with different command arguments");
    }
    if !response.status().is_success() {
        bail!(
            "Bellhound idempotent control command failed with HTTP {}",
            response.status()
        );
    }
    response
        .json()
        .await
        .context("Bellhound control listener returned an invalid command receipt")
}

pub async fn prepare_opening(config: &Config, opening: &str) -> Result<()> {
    let token = relay_token(config)?;
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!("{}/v1/opening", base_url(config)))
        .header(TOKEN_HEADER, token)
        .json(&serde_json::json!({"text": opening}))
        .send()
        .await
        .context("failed to prepare the opening on the local Bellhound relay")?;
    if !response.status().is_success() {
        bail!(
            "Bellhound opening preparation failed with HTTP {}",
            response.status()
        );
    }
    Ok(())
}

pub async fn clear_opening(config: &Config) -> Result<()> {
    let token = relay_token(config)?;
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?
        .delete(format!("{}/v1/opening", base_url(config)))
        .header(TOKEN_HEADER, token)
        .send()
        .await
        .context("failed to clear the opening on the local Bellhound relay")?;
    if !response.status().is_success() {
        bail!(
            "Bellhound opening cleanup failed with HTTP {}",
            response.status()
        );
    }
    Ok(())
}
