use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
    OpenApiRouter::<AppState>::new()
        // routes!マクロは一つのエンドポイントのメソッドをまとめてルーティングするためのマクロっぽい...?同じメソッドを複数定義しようとするとエラーになる。
        .routes(routes!(crate::handlers::auth::login))
        .routes(routes!(crate::handlers::auth::register))
        .routes(routes!(crate::handlers::auth::verify_email))
        .routes(routes!(crate::handlers::auth::resend_verification_email))
        .routes(routes!(crate::handlers::auth::logout))
        .routes(routes!(crate::handlers::auth::me))
        .routes(routes!(crate::handlers::oauth::list_providers))
        .routes(routes!(crate::handlers::oauth::oauth_start))
        .routes(routes!(crate::handlers::oauth::oauth_callback))
        .routes(routes!(crate::handlers::oauth::list_connections))
        .routes(routes!(crate::handlers::oauth::disconnect_connection))
        .routes(routes!(crate::handlers::oauth::set_initial_password))
        .routes(routes!(crate::handlers::passkeys::registration_start))
        .routes(routes!(crate::handlers::passkeys::registration_finish))
        .routes(routes!(crate::handlers::passkeys::authentication_start))
        .routes(routes!(crate::handlers::passkeys::authentication_finish))
        .routes(routes!(crate::handlers::passkeys::list_passkeys))
        .routes(routes!(crate::handlers::passkeys::rename_passkey))
        .routes(routes!(crate::handlers::passkeys::delete_passkey))
        .routes(routes!(crate::handlers::auth_2fa::totp_setup))
        .routes(routes!(crate::handlers::auth_2fa::totp_verify_setup))
        .routes(routes!(crate::handlers::auth_2fa::verify_2fa))
        .routes(routes!(crate::handlers::auth_2fa::delete_totp))
        .routes(routes!(
            crate::handlers::auth_2fa::regenerate_recovery_codes
        ))
        .routes(routes!(
            crate::handlers::password_reset::password_reset_request
        ))
        .routes(routes!(
            crate::handlers::password_reset::password_reset_verify
        ))
        .routes(routes!(
            crate::handlers::password_reset::password_reset_complete
        ))
        .routes(routes!(crate::handlers::password_reset::password_change))
}
