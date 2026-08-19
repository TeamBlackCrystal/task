use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;

pub fn tenant_github_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::<AppState>::new()
        .routes(routes!(crate::handlers::github::start_github_install))
        .routes(routes!(crate::handlers::github::get_github_integration))
        .routes(routes!(crate::handlers::github::delete_github_integration))
}

pub fn public_github_routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::<AppState>::new()
        .routes(routes!(crate::handlers::github::github_callback))
        .routes(routes!(crate::handlers::github::github_webhook))
}
