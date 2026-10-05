//! ファイル名の拡張子で種類を決める判定。種類ごとの拡張子の一覧は、使う側がそれぞれ持つ。

use std::path::Path;

/// `file_name` の拡張子。無いもの・UTF-8 でないものは `None`。
pub fn extension(file_name: &str) -> Option<&str> {
    Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())
}

/// `file_name` の拡張子が `list` のどれかか。大文字小文字は区別しない (`PHOTO.JPG` も画像として扱う)。
pub fn has_extension_in(file_name: &str, list: &[&str]) -> bool {
    extension(file_name).is_some_and(|ext| list.iter().any(|known| known.eq_ignore_ascii_case(ext)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_extension_in_ignores_ascii_case() {
        assert!(has_extension_in("photo.JPG", &["jpg"]));
        assert!(has_extension_in("archive.tar.gz", &["gz"]));
        assert!(!has_extension_in("notes.pdf", &["jpg"]));
    }

    #[test]
    fn has_extension_in_rejects_names_without_an_extension() {
        assert!(!has_extension_in("jpg", &["jpg"]));
        assert!(!has_extension_in(".jpg", &["jpg"]));
    }
}
