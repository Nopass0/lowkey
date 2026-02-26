//! Lightweight Telegram Bot API wrapper.
//!
//! Only the `sendMessage` method is needed for admin OTP delivery, so we call
//! the HTTP API directly with `reqwest` instead of pulling in a full bot SDK.
//!
//! # Setup
//! 1. Create a bot via [@BotFather](https://t.me/BotFather) and copy the token.
//! 2. Start a conversation with the bot so it can send you DMs.
//! 3. Set `TG_BOT_TOKEN` and `TG_ADMIN_CHAT_ID` environment variables (or
//!    pass `--tg-bot-token` / `--tg-admin-chat-id` CLI flags).
//! 4. The admin chat ID can be found via `https://api.telegram.org/bot<TOKEN>/getUpdates`
//!    after sending the bot a message.

/// Send a plain Markdown text message to a Telegram chat.
///
/// # Arguments
/// * `bot_token` — Telegram Bot API token (`123456:AABBcc…`)
/// * `chat_id`   — Numeric chat ID or `@username` string
/// * `text`      — Message body.  Markdown formatting is enabled
///                 (`parse_mode = "Markdown"`).
///
/// # Errors
/// Returns an error if the HTTP request fails or Telegram returns a non-2xx
/// status code (e.g. invalid token, chat not found, bot blocked).
///
/// The function uses a 5-second timeout to avoid blocking the admin login
/// request handler indefinitely.
pub async fn send_message(bot_token: &str, chat_id: &str, text: &str) -> anyhow::Result<()> {
    let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
    reqwest::Client::new()
        .post(&url)
        .json(&serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "Markdown"
        }))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await?
        .error_for_status()?; // turn 4xx/5xx into Err
    Ok(())
}
