//! 一覧を継ぐためのカーソル。
//!
//! `offset` は「取得しているあいだに行が増減すると境界がずれる」ので、
//! 続きを読む用途では使わない。1 ページ目と 2 ページ目のあいだに行が 1 本抜けると
//! 境界の行が 2 ページ目の先頭より前へ詰まり、**どちらのページにも現れない**。
//! 並び順のキーそのものを持ち回り、次のページを「このキーより後ろ」で引く。
//!
//! 中身は base64 で畳むだけで、秘匿はしない（並び順のキーは行そのものに出ている）。
//! 利用者が値を作れるので、壊れたカーソルは 500 ではなく 400 で返す。

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Serialize, de::DeserializeOwned};

use crate::error::AppError;

/// カーソルを URL のクエリに載る文字列へ畳む。
pub fn encode_cursor<T: Serialize>(cursor: &T) -> String {
    let payload = serde_json::to_string(cursor).expect("cursor json");
    URL_SAFE_NO_PAD.encode(payload.as_bytes())
}

/// 受け取ったカーソルを開く。
pub fn decode_cursor<T: DeserializeOwned>(cursor: &str) -> Result<T, AppError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor.trim())
        .map_err(|_| AppError::BadRequest)?;
    let s = String::from_utf8(bytes).map_err(|_| AppError::BadRequest)?;
    serde_json::from_str(&s).map_err(|_| AppError::BadRequest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Sample {
        at: String,
        id: u32,
    }

    #[test]
    fn round_trips() {
        let sample = Sample {
            at: "2026-09-05T00:00:00Z".into(),
            id: 7,
        };
        let decoded: Sample = decode_cursor(&encode_cursor(&sample)).expect("decode");
        assert_eq!(decoded, sample);
    }

    #[test]
    fn encodes_without_padding_or_url_unsafe_chars() {
        // クエリ文字列にそのまま載せるので、`+` `/` `=` が出ないこと
        let encoded = encode_cursor(&Sample {
            at: "2026-09-05T00:00:00.123456+09:00".into(),
            id: 4_294_967_295,
        });
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));
        assert!(!encoded.contains('='));
    }

    #[test]
    fn rejects_broken_cursor_as_bad_request() {
        // base64 として壊れている / base64 は通るが JSON でない / JSON だが形が違う
        for broken in [
            "!!!!",
            &URL_SAFE_NO_PAD.encode(b"not json"),
            &URL_SAFE_NO_PAD.encode(br#"{"at":"x"}"#),
        ] {
            let result = decode_cursor::<Sample>(broken);
            assert!(matches!(result, Err(AppError::BadRequest)), "{broken}");
        }
    }
}
