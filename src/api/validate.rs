//! 入力値の検証で繰り返し使う小さな判定。422 のメッセージは画面ごとに違うため、
//! 呼び出し側から渡す。

use crate::error::AppError;

/// 空文字列なら `message` の422にする。前後の空白は落とさない。
pub(super) fn non_empty(value: &str, message: &str) -> Result<(), AppError> {
    if value.is_empty() {
        return Err(AppError::Validation(message.to_string()));
    }
    Ok(())
}

/// 前後の空白を落とし、空になれば `message` の422にする。
pub(super) fn trimmed_non_empty(value: &str, message: &str) -> Result<String, AppError> {
    let value = value.trim();
    non_empty(value, message)?;
    Ok(value.to_string())
}

/// 前後の空白を落とし、空になれば `None` にする。
pub(super) fn trimmed_or_none(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// 自動で付ける名前が既存と同じで区別がつかなくなるとき、`base (1)`, `base (2)` のように
/// 連番を付けて最初に空いているものを返す。`is_taken` は候補が使用済みかを答える。
/// 大文字小文字を同一視するかは、呼び出し側の比べ方で決める。
pub(super) fn first_free_name(base: &str, is_taken: impl Fn(&str) -> bool) -> String {
    if !is_taken(base) {
        return base.to_string();
    }
    (1..)
        .map(|n| format!("{base} ({n})"))
        .find(|candidate| !is_taken(candidate))
        .expect("an unbounded range always yields a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimmed_non_empty_trims_and_rejects_blank_values() {
        assert_eq!(
            trimmed_non_empty("  newbie  ", "required").expect("通るはず"),
            "newbie"
        );
        for value in ["", "   ", "\t\n"] {
            assert!(
                matches!(
                    trimmed_non_empty(value, "required"),
                    Err(AppError::Validation(_))
                ),
                "{value:?}"
            );
        }
    }

    /// パスワードのように、前後の空白も値の一部として残す。
    #[test]
    fn non_empty_keeps_surrounding_whitespace() {
        assert!(non_empty(" ", "required").is_ok());
        assert!(matches!(
            non_empty("", "required"),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn trimmed_or_none_treats_blank_values_as_none() {
        assert_eq!(trimmed_or_none(Some(" a ")), Some("a"));
        assert_eq!(trimmed_or_none(Some("  ")), None);
        assert_eq!(trimmed_or_none(None), None);
    }

    #[test]
    fn a_free_name_is_kept_as_is() {
        assert_eq!(first_free_name("a", |_| false), "a");
    }

    #[test]
    fn a_taken_name_gets_the_first_free_number() {
        let taken = ["a", "a (1)", "a (3)"];
        assert_eq!(first_free_name("a", |name| taken.contains(&name)), "a (2)");
    }
}
