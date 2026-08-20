//! GitHub App Wave 0 統合テスト（署名・OAuth state・リポジトリ選定）。

use backend::handlers::github::verify_webhook_signature;
use backend::utils::github::install_state::GithubOAuthStatePayload;
use backend::utils::github::repositories::select_primary_repository;
use forge_core::Repository;
use hmac::{Hmac, KeyInit, Mac};
use sea_orm::prelude::Uuid;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

fn sign_payload(secret: &str, body: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC can take key of any size");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    format!("sha256={}", hex::encode(digest))
}

#[test]
fn test_webhook_signature_validation() {
    let secret = "webhook-secret";
    let body = br#"{"installation":{"id":42}}"#;
    let signature = sign_payload(secret, body);
    assert!(verify_webhook_signature(secret, &signature, body));
    assert!(!verify_webhook_signature(secret, "sha256=deadbeef", body));
    assert!(!verify_webhook_signature("wrong-secret", &signature, body));
}

#[test]
fn test_oauth_state_with_installation() {
    let payload = GithubOAuthStatePayload {
        tenant_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        user_id: Uuid::new_v4(),
        installation_id: Some(99_001),
    };
    let json = serde_json::to_string(&payload).unwrap();
    let decoded: GithubOAuthStatePayload = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.installation_id, Some(99_001));
}

#[test]
fn test_oauth_state_without_installation_defaults_none() {
    let json = r#"{"tenant_id":"00000000-0000-0000-0000-000000000001","project_id":"00000000-0000-0000-0000-000000000002","user_id":"00000000-0000-0000-0000-000000000003"}"#;
    let decoded: GithubOAuthStatePayload = serde_json::from_str(json).unwrap();
    assert!(decoded.installation_id.is_none());
}

#[test]
fn test_primary_repository_selection_auto_selects_only_single_repo() {
    let single = vec![Repository::new("acme", "backend")];
    assert_eq!(
        select_primary_repository(&single).unwrap().to_string(),
        "acme/backend"
    );

    // 複数見えるときは自動選択せず、ユーザー選択に回す（#594）。
    let multiple = vec![
        Repository::new("acme", "backend"),
        Repository::new("acme", "frontend"),
    ];
    assert!(select_primary_repository(&multiple).is_none());
}
