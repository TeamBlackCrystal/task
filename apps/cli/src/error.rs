//! 終了コード付きのエラー。
//!
//! CLI はマージ前ゲート（`review summary`）と CI から使われるので、失敗の理由を
//! 終了コードで区別できることが仕様の一部になっている（`docs/features/review-findings.md` §6）。

use serde_json::json;

pub type Result<T> = std::result::Result<T, CliError>;

/// 標準エラーに出すメッセージと、プロセスの終了コード。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    pub message: String,
    pub exit_code: i32,
}

impl CliError {
    /// 分類のない失敗。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 1,
        }
    }

    /// 送信前に弾いた入力の誤り、または設定不足。
    ///
    /// ゲート不成立（1）と区別できないと、CI が「直せば通る」のか
    /// 「レビューが足りない」のかを出し分けられない。
    pub fn validation(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 2,
        }
    }

    /// 参照先が見つからない（CLI 側で判明したもの）。
    pub fn not_found(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            exit_code: 5,
        }
    }

    /// API のエラー応答。認証・権限・不在は終了コードで区別する。
    pub fn http(status: u16, body: &str) -> Self {
        match status {
            401 => Self {
                message: "Authentication failed: invalid or expired token".into(),
                exit_code: 3,
            },
            403 => Self {
                message: "Permission denied: insufficient access for this resource".into(),
                exit_code: 4,
            },
            404 => Self {
                message: "Resource not found".into(),
                exit_code: 5,
            },
            _ => Self {
                message: json!({
                    "error": "api_error",
                    "message": extract_api_message(body),
                    "status": status,
                })
                .to_string(),
                exit_code: 1,
            },
        }
    }
}

impl From<reqwest::Error> for CliError {
    fn from(err: reqwest::Error) -> Self {
        Self::new(err.to_string())
    }
}

/// エラー応答の本文から、人が読む一文を取り出す。
///
/// バックエンドは `{"message": "..."}` を返すが、プロキシが挟まると素の文字列や
/// HTML が来ることもある。取り出せなければ本文をそのまま見せる。
fn extract_api_message(body: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(serde_json::Value::String(text)) => text,
        Ok(serde_json::Value::Object(map)) => match map.get("message") {
            Some(serde_json::Value::String(text)) => text.clone(),
            Some(other) => other.to_string(),
            None => serde_json::Value::Object(map).to_string(),
        },
        Ok(other) => other.to_string(),
        Err(_) => body.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_auth_and_permission_statuses_to_distinct_exit_codes() {
        assert_eq!(CliError::http(401, "").exit_code, 3);
        assert_eq!(CliError::http(403, "").exit_code, 4);
        assert_eq!(CliError::http(404, "").exit_code, 5);
        assert_eq!(CliError::http(500, "").exit_code, 1);
    }

    #[test]
    fn reports_the_server_message_for_unclassified_statuses() {
        let err = CliError::http(422, r#"{"message":"title is too long"}"#);
        assert_eq!(
            err.message,
            r#"{"error":"api_error","message":"title is too long","status":422}"#
        );
    }

    #[test]
    fn falls_back_to_the_raw_body_when_it_is_not_json() {
        let err = CliError::http(502, "<html>bad gateway</html>");
        assert!(
            err.message.contains("<html>bad gateway</html>"),
            "{}",
            err.message
        );
    }
}
