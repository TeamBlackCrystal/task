use axum::http::{HeaderValue, header};
use tower_http::set_header::SetResponseHeaderLayer;
use utoipa_axum::router::OpenApiRouter;

use crate::AppState;

pub mod admin;
pub mod auth;
pub mod drive;
pub mod github;
pub mod personal_tokens;
pub mod reviews;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// operationId の重複は OpenAPI 仕様違反で、openapi-typescript の生成型が
    /// 重複同士のユニオンになって静かに壊れる（#618 / #620 のレビューで実発生）。
    /// utoipa は関数名を既定の operationId にするため、別モジュールの同名ハンドラーが
    /// 重複を作る。衝突したら片方の `#[utoipa::path]` に `operation_id` を明示すること。
    #[test]
    fn openapi_operation_ids_are_unique() {
        let (_, openapi) = OpenApiRouter::<crate::AppState>::new()
            .merge(create_routes())
            .split_for_parts();
        let doc = serde_json::to_value(&openapi).expect("serialize openapi");

        let mut locations_by_id: std::collections::BTreeMap<String, Vec<String>> =
            std::collections::BTreeMap::new();
        for (path, item) in doc["paths"].as_object().expect("paths object") {
            let Some(item) = item.as_object() else {
                continue;
            };
            for (method, op) in item {
                let Some(id) = op.as_object().and_then(|o| o.get("operationId")) else {
                    continue;
                };
                locations_by_id
                    .entry(id.as_str().expect("operationId string").to_string())
                    .or_default()
                    .push(format!("{method} {path}"));
            }
        }

        let duplicates: Vec<_> = locations_by_id
            .iter()
            .filter(|(_, locations)| locations.len() > 1)
            .collect();
        assert!(
            duplicates.is_empty(),
            "operationId が重複しています。該当ハンドラーの #[utoipa::path] に operation_id を明示してください: {duplicates:#?}"
        );
    }
}
