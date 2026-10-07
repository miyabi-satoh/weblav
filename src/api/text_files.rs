//! テキストのビューアで見せるファイルの判定 (→ docs/ui.md「PDF・動画・テキストのビューア」)。
//!
//! 先頭に NUL のバイトがあるかで決める (git がバイナリかを決めるのと同じ考え方)。

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::file_ext::has_extension_in;

/// ビューアが読み込むテキストの上限。画面の `text-view.svelte` の `MAX_BYTES` と揃える。
/// これより大きいファイルはビューアで開けないので、先頭も読まない。
const TEXT_VIEWER_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// NUL を探す先頭のバイト数。git と同じ 8000 バイト。
const SNIFF_BYTES: u64 = 8000;

/// 画面が拡張子で振り分ける (音声・PDF・動画・ビューアの画像・Office) ので、中身を見ない拡張子。
/// 画面の `file-kind.ts` と揃える。一覧のたびに開くファイルを減らし、短い音声が NUL を含まずに
/// テキストと判定されることも避ける。プレビューする画像の拡張子は `thumbnails` が持つ。
const KIND_BY_EXTENSION: [&str; 22] = [
    "mp3", "m4a", "wav", "ogg", "oga", "flac", "aac", "wma", "opus", "pdf", "mp4", "m4v", "webm",
    "mov", "heic", "heif", "avif", "tif", "tiff", "docx", "xlsx", "pptx",
];

/// `name` の拡張子で種類が決まらないファイルを、テキストのビューアで見せるか。
/// `path` は中身の場所 (file コンテンツの実体は拡張子を持たないので、名前とは別に受け取る)、
/// `size` はその大きさ。読めないファイルは `false`。
/// ブロッキング I/O なので `run_blocking` の中から呼ぶ。
pub(super) fn is_text_file(name: &str, path: &Path, size: u64) -> bool {
    if size > TEXT_VIEWER_MAX_BYTES || kind_by_extension(name) {
        return false;
    }
    let mut prefix = Vec::new();
    let Ok(file) = File::open(path) else {
        return false;
    };
    if file.take(SNIFF_BYTES).read_to_end(&mut prefix).is_err() {
        return false;
    }
    !prefix.contains(&0)
}

/// 画像・音声・PDF・動画など、拡張子で開き方の決まるファイルか。URL のファイルの中継も、
/// これで中継するかを決める (`remote_file`。画面の `file-kind.ts` と同じ一覧)。
pub(super) fn kind_by_extension(name: &str) -> bool {
    super::thumbnails::is_image_file_name(name) || has_extension_in(name, &KIND_BY_EXTENSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> crate::test_support::TempDir {
        crate::test_support::project_temp_dir("text-files", name)
    }

    #[test]
    fn is_text_file_accepts_files_without_nul_bytes() {
        let dir = temp_dir("text");
        let yaml = dir.join("config.yml");
        std::fs::write(&yaml, "name: weblav\n").expect("failed to write file");
        assert!(is_text_file("config.yml", &yaml, 13));
        // Shift_JIS も NUL を含まないので、テキストとして扱う。
        let sjis = dir.join("memo.ini");
        std::fs::write(&sjis, b"\x83\x65\x83\x4c\x83\x58\x83\x67").expect("failed to write file");
        assert!(is_text_file("memo.ini", &sjis, 8));
    }

    #[test]
    fn is_text_file_rejects_files_with_nul_bytes() {
        let dir = temp_dir("binary");
        let path = dir.join("data.bin");
        std::fs::write(&path, b"MZ\x90\x00\x03").expect("failed to write file");
        assert!(!is_text_file("data.bin", &path, 5));
    }

    #[test]
    fn is_text_file_skips_files_larger_than_the_viewer_accepts() {
        let dir = temp_dir("large");
        let path = dir.join("huge.log");
        std::fs::write(&path, "x").expect("failed to write file");
        assert!(!is_text_file("huge.log", &path, TEXT_VIEWER_MAX_BYTES + 1));
    }

    #[test]
    fn is_text_file_skips_files_whose_kind_the_extension_decides() {
        let dir = temp_dir("by-extension");
        let path = dir.join("short");
        std::fs::write(&path, "ID3").expect("failed to write file");
        assert!(is_text_file("short.txt", &path, 3));
        assert!(!is_text_file("listening.MP3", &path, 3));
        assert!(!is_text_file("broken.jpg", &path, 3));
        assert!(!is_text_file("scan.tiff", &path, 3));
    }
}
