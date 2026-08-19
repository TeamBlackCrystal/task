use std::sync::LazyLock;

use regex::Regex;

pub static COLOR_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^#[0-9A-Fa-f]{6}$").unwrap());
