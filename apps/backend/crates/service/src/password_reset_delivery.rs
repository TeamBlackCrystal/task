//! パスワードリセットメール本文の組み立てと SMTP 送信。

use crate::settings::Settings;
use crate::{password_reset, smtp::SmtpClient};

pub fn build_reset_url(settings: &Settings, token: &str) -> String {
    let encoded = urlencoding::encode(token);
    format!(
        "{}/password-reset?token={}",
        settings.email_verification_app_url.trim_end_matches('/'),
        encoded
    )
}

pub async fn send_password_reset_email(
    smtp: &SmtpClient,
    email: &str,
    settings: &Settings,
    token: &str,
) -> Result<(), anyhow::Error> {
    let reset_url = build_reset_url(settings, token);
    let mins = password_reset::TOKEN_TTL_SECS / 60;
    smtp.send_email(
        email,
        "パスワードリセット",
        &format!(
            "以下のリンクから新しいパスワードを設定してください（有効期限は約{mins}分です）。\n{reset_url}",
        ),
        Some(&format!(
            "<p>以下のリンクから新しいパスワードを設定してください（有効期限は約{mins}分です）。</p><p><a href=\"{reset_url}\">{reset_url}</a></p>",
        )),
    )
    .await
    .map_err(|e| anyhow::anyhow!("send password reset email: {e}"))?;
    Ok(())
}
