use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::{config::Config, protocol::AgentCommand};

const TOKEN_HEADER: &str = "x-bellhound-token";

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
