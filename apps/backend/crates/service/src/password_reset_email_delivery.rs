use crate::settings::Settings;
use crate::{password_reset, smtp::SmtpClient};

pub async fn send_password_reset_email(
    smtp: &SmtpClient,
    email: &str,
    settings: &Settings,
    token: &str,
) -> Result<(), anyhow::Error> {
    let encoded = urlencoding::encode(token);
    let reset_url = format!(
        "{}/auth/reset-password?token={}",
        settings.email_verification_app_url.trim_end_matches('/'),
        encoded
    );
    let mins = password_reset::TOKEN_TTL_SECS / 60;
    let subject = "パスワードリセットのご案内";
    let text = format!(
        "以下のリンクをクリックしてパスワードをリセットしてください。\n\
         このリンクは {mins} 分間有効です。\n\n{reset_url}\n\n\
         このメールに心当たりのない場合は無視してください。"
    );
    let html = format!(
        "<p>以下のリンクからパスワードをリセットしてください（約{mins}分有効）。</p>\
         <p><a href=\"{reset_url}\">{reset_url}</a></p>"
    );
    smtp.send_email(email, subject, &text, Some(&html))
        .await
        .map_err(|e| anyhow::anyhow!("send password reset email: {e}"))
}
