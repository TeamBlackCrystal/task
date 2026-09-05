use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::{Validate, ValidationError};

/// Public user profile — excludes password_hash, sessions_revoked_at, and other secrets.
#[derive(Debug, Clone, Serialize, ToSchema, serde::Deserialize)]
pub struct UserResponse {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    #[schema(value_type = String, format = "username")]
    pub username: String,
    #[schema(nullable)]
    pub bio: Option<String>,
    #[schema(nullable)]
    pub avatar_url: Option<String>,
    #[schema(value_type = String, format = "email")]
    pub email: String,
    pub email_verified: bool,
    pub is_admin: bool,
    pub is_suspended: bool,
    pub totp_enabled: bool,
    /// パスワードでサインインできるか。ハッシュを出さずに認証方法の表示を出し分けるために返す。
    pub has_password: bool,
}

/// 他リソースのレスポンスに埋め込む軽量なユーザー情報
#[derive(Debug, Clone, Serialize, ToSchema, serde::Deserialize)]
pub struct UserSummary {
    #[schema(value_type = String, format = "uuid")]
    pub id: Uuid,
    pub username: String,
    #[schema(nullable)]
    pub avatar_url: Option<String>,
}

impl From<entity::users::Model> for UserSummary {
    fn from(model: entity::users::Model) -> Self {
        Self {
            id: model.id,
            username: model.username,
            avatar_url: model.avatar_url,
        }
    }
}

impl From<entity::users::Model> for UserResponse {
    fn from(model: entity::users::Model) -> Self {
        Self {
            id: model.id,
            username: model.username,
            bio: model.bio,
            avatar_url: model.avatar_url,
            email: model.email,
            email_verified: model.email_verified,
            is_admin: model.is_admin,
            is_suspended: model.is_suspended,
            totp_enabled: model.totp_enabled,
            has_password: model.password_hash.is_some(),
        }
    }
}

/// アバター URL は `<img src>` に流し込むため、HTTPS だけ通す。
fn validate_avatar_url(value: &str) -> Result<(), ValidationError> {
    let lowered = value.to_ascii_lowercase();
    if lowered.starts_with("https://") {
        Ok(())
    } else {
        Err(ValidationError::new("avatar_url_scheme"))
    }
}

fn validate_profile_update(value: &UpdateProfileRequest) -> Result<(), ValidationError> {
    if value.clear_avatar_url && value.avatar_url.is_some() {
        Err(ValidationError::new("avatar_url_conflict"))
    } else {
        Ok(())
    }
}

/// ログイン中ユーザーが自分で編集できる項目。
///
/// 省略したフィールドは変更しない。`avatar_url` を空にするときは、空文字が URL 検証を
/// 通らないため、既存の PATCH（`UpdateTaskRequest`）と同じく `clear_*` フラグで指定する。
/// `bio` は空文字が有効な値なのでフラグを持たない。
#[derive(Debug, Deserialize, ToSchema, Validate)]
#[validate(schema(function = "validate_profile_update"))]
pub struct UpdateProfileRequest {
    #[schema(value_type = Option<String>, format = "username")]
    #[validate(length(min = 3, max = 255))]
    pub username: Option<String>,
    #[validate(length(max = 1000))]
    pub bio: Option<String>,
    #[validate(length(max = 2048), custom(function = "validate_avatar_url"))]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub clear_avatar_url: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(avatar_url: Option<&str>, clear_avatar_url: bool) -> UpdateProfileRequest {
        UpdateProfileRequest {
            username: None,
            bio: None,
            avatar_url: avatar_url.map(str::to_owned),
            clear_avatar_url,
        }
    }

    #[test]
    fn profile_avatar_url_requires_https() {
        assert!(
            request(Some("https://example.com/a.png"), false)
                .validate()
                .is_ok()
        );
        assert!(
            request(Some("http://example.com/a.png"), false)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn profile_avatar_url_cannot_be_set_and_cleared_together() {
        assert!(
            request(Some("https://example.com/a.png"), true)
                .validate()
                .is_err()
        );
    }

    #[test]
    fn profile_lengths_are_counted_in_unicode_code_points() {
        let mut payload = request(None, false);
        payload.username = Some("😀😀".to_string());
        assert!(payload.validate().is_err());

        payload.username = Some("😀😀😀".to_string());
        payload.bio = Some("😀".repeat(1000));
        assert!(payload.validate().is_ok());
    }
}
