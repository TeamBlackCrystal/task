//! 「省略」と「明示的な null」を区別するためのデシリアライザ。
//!
//! `Option<Option<T>>` を素の serde で受けると、`null` は外側の `None`（= 省略）へ
//! 落ちる。「フィールド省略時は変更なし、`null` でクリア（ルートへ移動）」という
//! PATCH の契約はこれでは表せないため、`#[serde(default, deserialize_with = ...)]`
//! と組み合わせて使う。
//!
//! - フィールドなし → `None`（変更しない）
//! - `"field": null` → `Some(None)`（クリアする）
//! - `"field": <値>` → `Some(Some(値))`

use serde::{Deserialize, Deserializer};

pub fn deserialize_some<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::prelude::Uuid;

    #[derive(Debug, Deserialize)]
    struct Patch {
        #[serde(default, deserialize_with = "deserialize_some")]
        folder_id: Option<Option<Uuid>>,
    }

    #[test]
    fn omitted_field_means_no_change() {
        let patch: Patch = serde_json::from_str("{}").expect("parse");
        assert_eq!(patch.folder_id, None);
    }

    #[test]
    fn explicit_null_means_clear() {
        let patch: Patch = serde_json::from_str(r#"{"folder_id": null}"#).expect("parse");
        assert_eq!(patch.folder_id, Some(None));
    }

    #[test]
    fn a_value_is_kept() {
        let raw = "0191d4b0-0000-7000-8000-000000000001";
        let patch: Patch =
            serde_json::from_str(&format!(r#"{{"folder_id": "{raw}"}}"#)).expect("parse");
        assert_eq!(
            patch.folder_id,
            Some(Some(raw.parse::<Uuid>().expect("uuid")))
        );
    }
}
