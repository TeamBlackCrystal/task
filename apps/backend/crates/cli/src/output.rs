//! 出力。`--json` は機械向け、既定は人間向けの 1 行表示。

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputOptions {
    pub json: bool,
}

impl OutputOptions {
    pub fn new(json: bool) -> Self {
        Self { json }
    }
}

/// `--json` なら型どおりの JSON、そうでなければ人間向けに畳んで出す。
pub fn print<T: Serialize>(value: &T, opts: OutputOptions) {
    println!("{}", render(value, opts));
}

pub fn render<T: Serialize>(value: &T, opts: OutputOptions) -> String {
    if opts.json {
        // 型の宣言順をそのまま出す。フィールド順が変わると差分の読み取りが荒れる
        return serde_json::to_string_pretty(value).unwrap_or_else(|err| err.to_string());
    }
    let value = serde_json::to_value(value).unwrap_or(Value::Null);
    let mut lines = Vec::new();
    render_human(&value, &mut lines);
    lines.join("\n")
}

/// 一覧はキーと名前だけの 1 行に畳み、畳めないものは JSON のまま見せる。
fn render_human(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Null => {}
        Value::Array(items) => {
            for item in items {
                render_human(item, out);
            }
        }
        Value::Object(map) => {
            if let Some(Value::Array(tasks)) = map.get("tasks") {
                for task in tasks {
                    render_human(task, out);
                }
                return;
            }
            if map.contains_key("seq_key") && map.contains_key("title") {
                // `seq_key` が null の行もあるので、その場合は id へ落とす
                let key = map
                    .get("seq_key")
                    .filter(|value| !value.is_null())
                    .or_else(|| map.get("id"));
                out.push(format!("{}\t{}", as_text(key), as_text(map.get("title"))));
                return;
            }
            if map.contains_key("key") && map.contains_key("name") {
                out.push(format!(
                    "{}\t{}",
                    as_text(map.get("key")),
                    as_text(map.get("name"))
                ));
                return;
            }
            out.push(serde_json::to_string_pretty(value).unwrap_or_default());
        }
        other => out.push(as_text(Some(other))),
    }
}

/// JSON の値を「表示用の文字列」にする。文字列は引用符を外す。
fn as_text(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn human(value: serde_json::Value) -> String {
        render(&value, OutputOptions::new(false))
    }

    #[test]
    fn prints_pretty_json_when_asked_for_json() {
        assert_eq!(
            render(&json!({ "b": 2 }), OutputOptions::new(true)),
            "{\n  \"b\": 2\n}"
        );
    }

    #[test]
    fn folds_a_task_into_key_and_title() {
        assert_eq!(
            human(json!({ "seq_key": "APP-7", "title": "Ship it" })),
            "APP-7\tShip it"
        );
    }

    #[test]
    fn falls_back_to_the_id_when_a_task_has_no_sequence_key() {
        assert_eq!(
            human(json!({ "seq_key": null, "id": "task-1", "title": "T" })),
            "task-1\tT"
        );
    }

    #[test]
    fn folds_a_named_resource_into_key_and_name() {
        assert_eq!(human(json!({ "key": "APP", "name": "App" })), "APP\tApp");
    }

    #[test]
    fn unwraps_a_task_list_into_one_line_per_task() {
        let listing = json!({
            "tasks": [
                { "seq_key": "APP-1", "title": "First" },
                { "seq_key": "APP-2", "title": "Second" },
            ],
            "total": 2,
        });
        assert_eq!(human(listing), "APP-1\tFirst\nAPP-2\tSecond");
    }

    /// `projects statuses` の人間向け出力。名前の配列を渡すと 1 行ずつになる。
    /// オブジェクトで包むと畳めず（特別扱いは `tasks` だけ）、id や日時まで出てしまう。
    #[test]
    fn renders_a_string_list_as_one_line_each() {
        let names = json!(["Backlog", "Todo", "In Progress"]);
        assert_eq!(human(names), "Backlog\nTodo\nIn Progress");
    }

    /// 畳めないオブジェクトで包むと用途に合わない出力になることの裏取り。
    /// これが `projects statuses` の通常出力で起きていた。
    #[test]
    fn does_not_fold_a_statuses_object() {
        let listing = json!({ "statuses": [{ "name": "Todo", "position": 0 }] });
        assert!(
            human(listing).starts_with('{'),
            "オブジェクトは JSON のまま出る"
        );
    }

    #[test]
    fn keeps_json_for_objects_that_do_not_fold() {
        assert_eq!(
            human(json!({ "username": "yupix" })),
            "{\n  \"username\": \"yupix\"\n}"
        );
    }

    #[test]
    fn prints_an_empty_listing_as_nothing() {
        assert_eq!(human(json!({ "tasks": [], "total": 0 })), "");
        assert_eq!(human(json!([])), "");
    }

    #[test]
    fn prints_scalars_without_quoting_them() {
        assert_eq!(human(json!("done")), "done");
        assert_eq!(human(json!(7)), "7");
    }
}
