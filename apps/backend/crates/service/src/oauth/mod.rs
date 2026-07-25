//! OAuth ログインのアプリ固有層。
//!
//! プロトコル層は `auth-core` クレート群にある。ここに置くのは、このアプリの
//! 設定（環境変数の読み込み・プロバイダー slug の一覧）と、state payload の型、
//! そして Redis を使った [`auth_core::state::StateStore`] の実装。

pub mod registry;
pub mod settings;
pub mod state;

pub use auth_core::provider::ProviderConfig;
pub use registry::{get_credentials, resolve_provider};
pub use settings::{OAuthSettings, OidcConfig};
pub use state::{OAuthStatePayload, RedisStateStore, consume_state, store_state};
