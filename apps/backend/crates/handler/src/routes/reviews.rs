use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;

/// `/{project_id}/reviews` 配下。
pub fn review_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::<AppState>::new()
        .routes(routes!(crate::handlers::reviews::list_reviews))
        .routes(routes!(crate::handlers::reviews::create_review))
        // `/summary` は `/{id}` より先に静的一致する（matchit の優先度）
        .routes(routes!(crate::handlers::reviews::get_review_summary))
        .routes(routes!(crate::handlers::reviews::get_review))
}

/// `/{project_id}/review-findings` 配下。
pub fn finding_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::<AppState>::new()
        .routes(routes!(crate::handlers::reviews::list_review_findings))
        .routes(routes!(
            crate::handlers::reviews::update_review_finding_state
        ))
}
