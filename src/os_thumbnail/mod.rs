//! OS が持つ縮小画像の仕組みで、画像でないファイルの縮小画像を作る (→ docs/ui.md「画像のプレビュー」)。
//! macOS は Quick Look、Windows はエクスプローラーと同じ Shell の仕組みを呼ぶ。ほかの OS は作らない。

use std::path::Path;

use image::DynamicImage;

use crate::file_ext::{extension, has_extension_in};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

/// この OS で縮小画像を作れるか。作れない OS では、一覧に縮小画像の行を出さない。
pub const SUPPORTED: bool = cfg!(any(target_os = "macos", windows));

/// OS の返事を待つ長さ。大きな動画でも数秒で返るが、縮小画像を作るアプリが固まったときに、
/// 縮小画像の順番待ち (`Thumbnails::permits`) を握り続けないため。
#[cfg(any(target_os = "macos", windows))]
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// OS に縮小画像を頼む拡張子。動画・PDF・Office の文書・ブラウザによって表示できない画像。
/// 何でも頼むと、縮小画像の無いファイル (テキストなど) のたびに OS を呼ぶことになるので絞る。
const EXTENSIONS: [&str; 19] = [
    "mp4", "m4v", "mov", "webm", "avi", "wmv", "mkv", "pdf", "doc", "docx", "xls", "xlsx", "ppt",
    "pptx", "heic", "heif", "avif", "tif", "tiff",
];

/// 文書 (ページの形のもの)。縮小画像を正方形に切り抜くとき、真ん中でなく上を残す。
const DOCUMENT_EXTENSIONS: [&str; 7] = ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx"];

/// 一覧の行に、OS が作る縮小画像を出してみるか。作れなかったときは画面がアイコンに戻す。
pub fn is_candidate(file_name: &str) -> bool {
    SUPPORTED && has_extension_in(file_name, &EXTENSIONS)
}

/// 縮小画像を、上を残して切り抜く文書か。
pub fn is_document(file_name: &str) -> bool {
    has_extension_in(file_name, &DOCUMENT_EXTENSIONS)
}

/// OS が縮小画像を返さなかった理由。
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    /// 作れなかった。同じファイルで頼み直しても作れない。
    Unavailable,
    /// 時間内に返らなかった。混んでいただけのこともあるので、次に見たときに頼み直す。
    TimedOut,
}

/// `path` の縮小画像を、`size` px の四角に収まる大きさで作る。
/// `file_name` は種類の判定に使う (file コンテンツの実体は拡張子を持たないため)。
/// OS の処理を待つので `run_blocking` の中から呼ぶ。
#[allow(unused_variables)]
pub fn thumbnail(path: &Path, file_name: &str, size: u32) -> Result<DynamicImage, Failure> {
    let extension = extension(file_name).ok_or(Failure::Unavailable)?;
    #[cfg(target_os = "macos")]
    return macos::thumbnail(path, extension, size);
    #[cfg(windows)]
    return windows::thumbnail(path, extension, size);
    #[cfg(not(any(target_os = "macos", windows)))]
    Err(Failure::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_candidate_matches_os_thumbnail_extensions_when_supported() {
        assert_eq!(is_candidate("clip.MOV"), SUPPORTED);
        assert_eq!(is_candidate("print.pdf"), SUPPORTED);
        assert!(!is_candidate("photo.jpg"));
        assert!(!is_candidate("notes.txt"));
        assert!(!is_candidate("pdf"));
    }

    #[test]
    fn is_document_keeps_the_top_of_pages() {
        assert!(is_document("print.PDF"));
        assert!(is_document("slides.pptx"));
        assert!(!is_document("clip.mov"));
    }
}
