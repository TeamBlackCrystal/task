use sea_orm::prelude::Uuid;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use validator::{Validate, ValidationError};

/// Public user profile — excludes password_hash, sessions_revoked_at, and other secrets.
#[derive(Debug, Clone, Serialize, ToSchema)]
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
}

/// 他リソースのレスポンスに埋め込む軽量なユーザー情報
#[derive(Debug, Clone, Serialize, ToSchema)]
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
        }
    }
}

/// アバター URL は `<img src>` に流し込むため、`javascript:` や `data:` を弾いて
/// http/https だけ通す。
fn validate_avatar_url(value: &str) -> Result<(), ValidationError> {
    let lowered = value.to_ascii_lowercase();
    if lowered.starts_with("https://") || lowered.starts_with("http://") {
        Ok(())
    } else {
        Err(ValidationError::new("avatar_url_scheme"))
    }
}

/// ログイン中ユーザーが自分で編集できる項目。
///
/// 省略したフィールドは変更しない。`avatar_url` を空にするときは、空文字が URL 検証を
/// 通らないため、既存の PATCH（`UpdateTaskRequest`）と同じく `clear_*` フラグで指定する。
/// `bio` は空文字が有効な値なのでフラグを持たない。
#[derive(Debug, Deserialize, ToSchema, Validate)]
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
