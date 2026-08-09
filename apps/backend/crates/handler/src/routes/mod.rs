use axum::http::{HeaderValue, header};
use tower_http::set_header::SetResponseHeaderLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

pub mod admin;
pub mod auth;
pub mod drive;
pub mod github;
pub mod personal_tokens;
pub mod tenants;
pub mod users;

pub fn create_routes() -> OpenApiRouter<AppState> {
    // ドライブはユーザーがアップロードしたファイルをそのまま配信するため、
    // ブラウザの MIME スニッフィング（application/octet-stream を HTML と解釈する等）を全経路で止める。
    OpenApiRouter::new()
        .nest(
            "/v1",
            OpenApiRouter::new()
                .nest("/admin", crate::routes::admin::routes())
                .nest("/auth", crate::routes::auth::routes())
                .nest("/personal_tokens", crate::routes::personal_tokens::routes())
                .nest("/users", crate::routes::users::routes())
                .nest("/tenants", crate::routes::tenants::routes())
                .nest("/github", crate::routes::github::public_github_routes())
                .nest("/drive", crate::routes::drive::public_routes()),
        )
        // レイヤーは登録済みのルートにのみ適用されるため、nest の後に付ける。
        .layer(SetResponseHeaderLayer::if_not_present(
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ))
}
