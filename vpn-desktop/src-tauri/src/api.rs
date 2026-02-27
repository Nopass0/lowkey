//! HTTP API client for communicating with the Lowkey VPN backend.

use anyhow::Result;
use reqwest::Client;

fn client() -> Client {
    Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default()
}

pub async fn login(api_url: &str, login: &str, password: &str) -> Result<serde_json::Value> {
    let res = client()
        .post(format!("{api_url}/auth/login"))
        .json(&serde_json::json!({ "login": login, "password": password }))
        .send()
        .await?;
    let status = res.status();
    let json: serde_json::Value = res.json().await?;
    if !status.is_success() {
        anyhow::bail!("{}", json.as_str().unwrap_or("Login failed"));
    }
    Ok(json)
}

pub async fn get_user(api_url: &str, token: &str) -> Result<serde_json::Value> {
    let res = client()
        .get(format!("{api_url}/auth/me"))
        .bearer_auth(token)
        .send()
        .await?;
    Ok(res.json().await?)
}

pub async fn register_peer(api_url: &str, token: &str) -> Result<serde_json::Value> {
    let res = client()
        .post(format!("{api_url}/api/peers/register"))
        .bearer_auth(token)
        .json(&serde_json::json!({}))
        .send()
        .await?;
    let status = res.status();
    let json: serde_json::Value = res.json().await?;
    if !status.is_success() {
        anyhow::bail!("Peer registration failed: {:?}", json);
    }
    Ok(json)
}

pub async fn get_plans(api_url: &str) -> Result<serde_json::Value> {
    let res = client()
        .get(format!("{api_url}/subscription/plans"))
        .send()
        .await?;
    Ok(res.json().await?)
}

pub async fn create_payment(
    api_url: &str,
    token: &str,
    amount: f64,
    purpose: &str,
    plan_id: Option<&str>,
) -> Result<serde_json::Value> {
    let body = serde_json::json!({
        "amount": amount,
        "purpose": purpose,
        "plan_id": plan_id,
    });
    let res = client()
        .post(format!("{api_url}/payment/sbp/create"))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await?;
    let status = res.status();
    let json: serde_json::Value = res.json().await?;
    if !status.is_success() {
        anyhow::bail!("{}", json.as_str().unwrap_or("Payment creation failed"));
    }
    Ok(json)
}

pub async fn payment_status(api_url: &str, token: &str, payment_id: u64) -> Result<serde_json::Value> {
    let res = client()
        .get(format!("{api_url}/payment/sbp/status/{payment_id}"))
        .bearer_auth(token)
        .send()
        .await?;
    Ok(res.json().await?)
}

pub async fn referral_stats(api_url: &str, token: &str) -> Result<serde_json::Value> {
    let res = client()
        .get(format!("{api_url}/referral/stats"))
        .bearer_auth(token)
        .send()
        .await?;
    Ok(res.json().await?)
}

/// Fetch the latest release info for a platform from the public API.
pub async fn get_latest_release(api_url: &str, platform: &str) -> Result<serde_json::Value> {
    let res = client()
        .get(format!("{api_url}/api/version/{platform}"))
        .send()
        .await?;
    let status = res.status();
    let json: serde_json::Value = res.json().await?;
    if !status.is_success() {
        anyhow::bail!("No release found for {platform}");
    }
    Ok(json)
}
