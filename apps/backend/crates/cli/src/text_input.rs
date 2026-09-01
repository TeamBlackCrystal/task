//! 長い本文を引数以外から受け取る経路。
//!
//! タスクの本文やコメントは複数行になるうえ、引数に置くとシェルの履歴と
//! プロセス一覧に残る。ファイルか標準入力から読めるようにする。

use std::io::{IsTerminal, Read};

use crate::error::{CliError, Result};

/// 標準入力を丸ごと読む。
///
/// 端末から実行されたときは待ち受けない（貼り付け待ちで固まって見えるのを避ける）。
/// trim は用途で変わる（トークンは全体、本文は末尾だけ）ので呼び出し側に任せる。
pub fn read_stdin(what: &str) -> Result<String> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Ok(String::new());
    }
    let mut buffer = String::new();
    stdin
        .lock()
        .read_to_string(&mut buffer)
        .map_err(|err| CliError::new(format!("Cannot read the {what} from stdin: {err}")))?;
    Ok(buffer)
}

/// ファイル（`-` は標準入力）から本文を読む。
///
/// 末尾の改行だけ落とす。エディタや `echo` が付ける改行をそのまま本文に含めない
/// ため。先頭や行頭の空白は Markdown で意味を持つので触らない。
pub fn read_body_from_file(path: &str, what: &str) -> Result<String> {
    let raw = if path == "-" {
        read_stdin(what)?
    } else {
        std::fs::read_to_string(path)
            .map_err(|err| CliError::new(format!("Cannot read the {what} from {path}: {err}")))?
    };
    Ok(raw.trim_end_matches(['\n', '\r']).to_string())
}

/// `--x` と `--x-file` のどちらで渡された本文かを解く。
///
/// 両方の同時指定は clap の `conflicts_with` で弾く。ここへは片方だけが来る。
pub fn resolve_body(
    inline: Option<String>,
    file: Option<String>,
    what: &str,
) -> Result<Option<String>> {
    if let Some(text) = inline {
        return Ok(Some(text));
    }
    match file {
        Some(path) => Ok(Some(read_body_from_file(&path, what)?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(contents: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("create temp file");
        file.write_all(contents.as_bytes()).expect("write");
        file.flush().expect("flush");
        file
    }

    #[test]
    fn reads_a_file_and_drops_only_the_trailing_newline() {
        let file = temp_file("## 見出し\n\n  字下げは残す\n");
        let body = read_body_from_file(file.path().to_str().unwrap(), "description").unwrap();
        assert_eq!(body, "## 見出し\n\n  字下げは残す");
    }

    #[test]
    fn keeps_blank_lines_inside_the_body() {
        let file = temp_file("1 行目\n\n\n4 行目\n\n\n");
        let body = read_body_from_file(file.path().to_str().unwrap(), "description").unwrap();
        assert_eq!(body, "1 行目\n\n\n4 行目");
    }

    #[test]
    fn reports_the_path_when_the_file_is_missing() {
        let err = read_body_from_file("/does/not/exist", "description").unwrap_err();
        assert!(
            err.message.contains("/does/not/exist"),
            "path should be in the message: {}",
            err.message
        );
    }

    #[test]
    fn inline_wins_when_no_file_is_given() {
        let body = resolve_body(Some("本文".into()), None, "description").unwrap();
        assert_eq!(body.as_deref(), Some("本文"));
    }

    #[test]
    fn returns_none_when_neither_is_given() {
        assert!(resolve_body(None, None, "description").unwrap().is_none());
    }

    #[test]
    fn reads_the_file_when_only_the_file_is_given() {
        let file = temp_file("ファイルの本文\n");
        let body = resolve_body(
            None,
            Some(file.path().to_str().unwrap().to_string()),
            "description",
        )
        .unwrap();
        assert_eq!(body.as_deref(), Some("ファイルの本文"));
    }

    /// 空文字も「渡された」として扱う。本文を空にする指定を落とさないため。
    #[test]
    fn keeps_an_empty_inline_value() {
        let body = resolve_body(Some(String::new()), None, "description").unwrap();
        assert_eq!(body.as_deref(), Some(""));
    }
}
