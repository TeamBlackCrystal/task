//! 長い本文を引数以外から受け取る経路。
//!
//! タスクの本文やコメントは複数行になるうえ、引数に置くとシェルの履歴と
//! プロセス一覧に残る。ファイルか標準入力から読めるようにする。

use std::io::{IsTerminal, Read};

use crate::error::{CliError, Result};

/// 標準入力を丸ごと読む。読めるものが繋がっていなければ `None`。
///
/// 端末から実行されたときは待ち受けない（貼り付け待ちで固まって見えるのを避ける）。
/// 「空を読んだ」と「端末なので読んでいない」を呼び出し側で区別できるよう、
/// 端末は `None` で返す。前者を本文として採用すると、`--x-file -` の指定が
/// 黙って「本文を空にする」指定に化ける。
/// trim は用途で変わる（トークンは全体、本文は末尾だけ）ので呼び出し側に任せる。
pub fn read_stdin(what: &str) -> Result<Option<String>> {
    if std::io::stdin().is_terminal() {
        return Ok(None);
    }
    let mut buffer = String::new();
    std::io::stdin()
        .lock()
        .read_to_string(&mut buffer)
        .map_err(|err| CliError::new(format!("Cannot read the {what} from stdin: {err}")))?;
    Ok(Some(buffer))
}

/// ファイル（`-` は標準入力）から本文を読む。
///
/// 末尾の改行だけ落とす。エディタや `echo` が付ける改行をそのまま本文に含めない
/// ため。先頭や行頭の空白は Markdown で意味を持つので触らない。
pub fn read_body_from_file(path: &str, what: &str) -> Result<String> {
    read_body_with(path, what, read_stdin)
}

/// 標準入力の読み取りを差し替えられる形。端末のときの分岐をテストするため。
fn read_body_with(
    path: &str,
    what: &str,
    read: impl FnOnce(&str) -> Result<Option<String>>,
) -> Result<String> {
    let raw = if path == "-" {
        // `-` は「これから標準入力で渡す」という明示的な指定。読めるものが無いなら
        // 空文字を採用せずに落とす。採用すると既存の本文を消してしまう
        read(what)?.ok_or_else(|| {
            CliError::validation(format!(
                "Cannot read the {what} from stdin: nothing was piped in"
            ))
        })?
    } else {
        std::fs::read_to_string(path).map_err(|err| {
            CliError::validation(format!("Cannot read the {what} from {path}: {err}"))
        })?
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

    /// `-` は「標準入力で渡す」という明示的な指定。読めるものが無いのに空文字を
    /// 採用すると、`update --description-file -` が本文の消去に化ける。
    #[test]
    fn refuses_stdin_when_nothing_is_piped_in() {
        let err = read_body_with("-", "description", |_| Ok(None)).unwrap_err();

        assert_eq!(err.exit_code, 2, "検証エラーは 2（README の終了コード表）");
        assert!(
            err.message.contains("nothing was piped in"),
            "何が起きたか分かる文言にする: {}",
            err.message
        );
    }

    #[test]
    fn reads_stdin_when_something_is_piped_in() {
        let body = read_body_with("-", "description", |_| Ok(Some("本文\n".into()))).unwrap();
        assert_eq!(body, "本文");
    }

    /// パイプで明示的に空を渡したときは、その意図（本文を空にする）を尊重する。
    #[test]
    fn keeps_an_explicitly_piped_empty_body() {
        let body = read_body_with("-", "description", |_| Ok(Some(String::new()))).unwrap();
        assert_eq!(body, "");
    }

    #[test]
    fn reports_the_path_when_the_file_is_missing() {
        let err = read_body_from_file("/does/not/exist", "description").unwrap_err();
        assert_eq!(err.exit_code, 2, "検証エラーは 2（README の終了コード表）");
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
