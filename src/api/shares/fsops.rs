//! 共有の中のファイルを作る・名前を変える・コピー・移動・削除する処理 (ブロッキング)。
//!
//! どれもブロッキングI/Oなので、呼び出し側は `spawn_blocking` の中で呼ぶこと。
//! パスはどれも、共有の中であることを確かめ済みの実体のパス (→ `paths::resolve_existing`)。
//!
//! シンボリックリンクは辿らない。コピーではリンクを飛ばし、削除ではリンクそのものだけを消す。
//! 共有の中のリンクは一覧にも出さないので (→ `fs::is_visible_entry`)、リンクの先にある共有の外の
//! ものを、コピー・削除で巻き込まないため。

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::api::error_detail::ValidationDetail;
use crate::error::AppError;

/// 置き先に残しておく空き。ちょうど使い切ると、OS やほかのアプリが書けなくなって止まるため、
/// 置いた後にも 100MB は残るようにする。
const SPACE_RESERVE_BYTES: u64 = 100 * 1024 * 1024;

/// コピーで一度に読み書きする大きさ。大きなファイルでも、中止と進み具合が1秒たたずに効く程度。
const COPY_CHUNK_BYTES: usize = 1024 * 1024;

/// 作業中のコピーの名前の頭。ドットで始めて一覧に出さない (→ `fs::is_visible_entry`)。
const COPY_TEMP_PREFIX: &str = ".weblav-copy-";

/// コピー・移動の進み具合と中止の合図。別のスレッドから読み書きする。
#[derive(Debug, Default)]
pub(super) struct TransferProgress {
    /// 写し終えたバイト数。
    pub(super) bytes: AtomicU64,
    /// 写し終えた・動かし終えたファイルとフォルダーの数。
    pub(super) items: AtomicU64,
    cancelled: AtomicBool,
}

impl TransferProgress {
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn add_bytes(&self, bytes: u64) {
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    fn add_item(&self) {
        self.items.fetch_add(1, Ordering::Relaxed);
    }
}

/// 置き先に同じ名前があるときの 422。
pub(super) fn name_taken(name: &str) -> AppError {
    AppError::ValidationDetailed {
        message: "an entry with the same name already exists".to_string(),
        detail: ValidationDetail::EntryNameTaken {
            name: name.to_string(),
        },
    }
}

/// ファイル操作の失敗を、画面が直し方を伝えられる形にする。
pub(super) fn map_io_error(err: io::Error) -> AppError {
    match err.kind() {
        io::ErrorKind::NotFound => AppError::NotFound,
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => {
            AppError::ShareReadOnly
        }
        io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => AppError::InsufficientStorage,
        io::ErrorKind::ResourceBusy => AppError::EntryInUse,
        _ if is_sharing_violation(&err) => AppError::EntryInUse,
        _ => AppError::Io(err),
    }
}

/// Windows で、ほかのアプリが開いているために触れないか。std はこの2つを `ErrorKind` に割り当てていない。
fn is_sharing_violation(err: &io::Error) -> bool {
    /// `ERROR_SHARING_VIOLATION`・`ERROR_LOCK_VIOLATION` (winerror.h)。
    const SHARING_VIOLATIONS: [i32; 2] = [32, 33];
    cfg!(windows)
        && err
            .raw_os_error()
            .is_some_and(|code| SHARING_VIOLATIONS.contains(&code))
}

/// リンクを辿らずに、何かがあるか (壊れたリンクもあるとみなす)。
fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// `name` を名前と拡張子に分ける。拡張子は `.` を含む。ドットで始まる名前や拡張子の無い名前は、全部が名前。
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    }
}

/// `dir` の中で `name` が空いていればそのまま、ふさがっていれば `名前 (1).拡張子` のように番号を付けた最初の空き。
/// フォルダーには拡張子を考えない (`2026.04` が `2026 (1).04` にならないように)。
///
/// 大文字小文字を区別しないファイルシステムでは、ファイルシステムに聞くので、そのとおりに空きを判定する。
pub(super) fn free_name(dir: &Path, name: &str, is_dir: bool) -> String {
    let (stem, extension) = if is_dir {
        (name, "")
    } else {
        split_extension(name)
    };
    let stem = crate::api::validate::first_free_name(stem, |candidate| {
        exists(&dir.join(format!("{candidate}{extension}")))
    });
    format!("{stem}{extension}")
}

/// 先頭のバイトで見分けた画像の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ImageKind {
    Jpeg,
    Png,
    Gif,
    Webp,
    Heic,
    Heif,
    Bmp,
    Tiff,
}

/// 画像の拡張子。拡張子を直すのは、これのどれかが付いているか、拡張子が無いファイルだけ。
/// ほかの拡張子 (`.pdf` など) は、利用者が意図して付けたものとして触らない。
const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "png", "gif", "webp", "heic", "heif", "bmp", "tif", "tiff",
];

impl ImageKind {
    fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Gif => "gif",
            Self::Webp => "webp",
            Self::Heic => "heic",
            Self::Heif => "heif",
            Self::Bmp => "bmp",
            Self::Tiff => "tif",
        }
    }

    /// `extension` (小文字) がこの種類に合っているか。
    fn accepts(self, extension: &str) -> bool {
        match self {
            Self::Jpeg => matches!(extension, "jpg" | "jpeg" | "jpe"),
            Self::Heic | Self::Heif => matches!(extension, "heic" | "heif"),
            Self::Tiff => matches!(extension, "tif" | "tiff"),
            _ => extension == self.extension(),
        }
    }
}

/// 先頭のバイトから画像の種類を見分ける。見分けに要るのは先頭の 32 バイトまで。
pub(super) fn sniff_image(head: &[u8]) -> Option<ImageKind> {
    match head {
        [0xFF, 0xD8, 0xFF, ..] => Some(ImageKind::Jpeg),
        [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, ..] => Some(ImageKind::Png),
        [b'G', b'I', b'F', b'8', b'7' | b'9', b'a', ..] => Some(ImageKind::Gif),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(ImageKind::Webp),
        [b'I', b'I', 0x2A, 0x00, ..] | [b'M', b'M', 0x00, 0x2A, ..] => Some(ImageKind::Tiff),
        [_, _, _, _, b'f', b't', b'y', b'p', brand @ ..] if brand.len() >= 4 => match &brand[..4] {
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" => Some(ImageKind::Heic),
            b"mif1" | b"msf1" | b"heif" => Some(ImageKind::Heif),
            _ => None,
        },
        // "BM" だけでは文字のファイルと見分けられないので、続く情報の大きさ (DIB ヘッダー) も見る。
        [b'B', b'M', rest @ ..] if rest.len() >= 16 => {
            let dib = u32::from_le_bytes([rest[12], rest[13], rest[14], rest[15]]);
            matches!(dib, 12 | 40 | 52 | 56 | 64 | 108 | 124).then_some(ImageKind::Bmp)
        }
        _ => None,
    }
}

/// 中身が画像なのに拡張子が合っていないとき、直した名前。直さないなら `None`。
///
/// iPhone の写真が JPEG に変換されたのに `.HEIC` のまま届く、拡張子の無い画像が届く、といったときに、
/// 拡張子から種類を決める画面 (プレビュー・縮小画像) で開けるようにするため。
pub(super) fn fixed_extension(name: &str, head: &[u8]) -> Option<String> {
    let kind = sniff_image(head)?;
    let (stem, extension) = split_extension(name);
    if extension.is_empty() {
        return Some(format!("{name}.{}", kind.extension()));
    }
    let extension = extension[1..].to_ascii_lowercase();
    if kind.accepts(&extension) || !IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    Some(format!("{stem}.{}", kind.extension()))
}

/// フォルダーを作る。同じ名前があれば 422 (`entryNameTaken`)。
pub(super) fn create_dir_new(path: &Path) -> Result<(), AppError> {
    fs::create_dir(path).map_err(|err| {
        if err.kind() == io::ErrorKind::AlreadyExists {
            name_taken(&file_name(path))
        } else {
            map_io_error(err)
        }
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `a` と `b` が同じものを指しているか。大文字小文字を区別しないファイルシステムで、
/// `a.txt` を `A.txt` に変えるときは、置き先が「既にある」ように見えるので見分ける。
fn same_entry(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (fs::symlink_metadata(a), fs::symlink_metadata(b)) {
            (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
            _ => false,
        }
    }
    #[cfg(not(unix))]
    {
        match (fs::canonicalize(a), fs::canonicalize(b)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        }
    }
}

/// 名前を変える。同じフォルダーに同じ名前があれば 422 (`entryNameTaken`)。大文字小文字だけの変更は通す。
/// 変えた後のパスを返す。
///
/// 確かめてから変えるまでの間に同じ名前が作られると、ファイルなら上書きになりうる
/// (OS の rename は置き先のファイルを置き換えるため)。共有に書ける人どうしの行き違いに限られる。
pub(super) fn rename_entry(from: &Path, new_name: &str) -> Result<PathBuf, AppError> {
    let parent = from.parent().ok_or(AppError::NotFound)?;
    let to = parent.join(new_name);
    if to == from {
        return Ok(to);
    }
    if exists(&to) && !same_entry(from, &to) {
        return Err(name_taken(new_name));
    }
    fs::rename(from, &to).map_err(map_io_error)?;
    Ok(to)
}

/// フォルダーをそれ自身やその中へ移動・コピーしようとしていれば 422 (`transferIntoItself`)。
/// どちらも実体のパスであること。
pub(super) fn ensure_not_into_itself(src: &Path, dest_dir: &Path) -> Result<(), AppError> {
    if dest_dir.starts_with(src) {
        return Err(AppError::ValidationDetailed {
            message: "cannot move or copy a folder into itself".to_string(),
            detail: ValidationDetail::TransferIntoItself,
        });
    }
    Ok(())
}

/// `path` の中のファイルの大きさの合計と、ファイル・フォルダーの数 (`path` 自身を含む)。リンクは数えない。
pub(super) fn tree_size(path: &Path) -> io::Result<(u64, u64)> {
    let mut bytes = 0;
    let mut items = 0;
    for entry in walkdir::WalkDir::new(path).follow_links(false) {
        let entry = entry.map_err(io::Error::other)?;
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            continue;
        }
        items += 1;
        if file_type.is_file() {
            bytes += entry.metadata().map_err(io::Error::other)?.len();
        }
    }
    Ok((bytes, items))
}

/// `dir` に、あと `needed` バイトを置いても余裕が残るか。足りなければ 507。
pub(super) fn ensure_space(dir: &Path, needed: u64) -> Result<(), AppError> {
    let available = fs4::available_space(dir).map_err(map_io_error)?;
    if available < needed.saturating_add(SPACE_RESERVE_BYTES) {
        return Err(AppError::InsufficientStorage);
    }
    Ok(())
}

/// `src` (ファイルかフォルダー) を、`dest_dir` の中に `name` として写す。写した先を返す。中止されたら `None`。
///
/// 一覧に出ない作業用の名前に写してから、最後に名前を付ける。途中で止まったり失敗したりしても、
/// 半端なものが本来の名前で残らないようにするため (作業用のものは消す)。
/// 同じ名前が既にあれば 422 (`entryNameTaken`)。上書きは呼び出し側が先に消してから行う。
pub(super) fn copy_tree(
    src: &Path,
    dest_dir: &Path,
    name: &str,
    progress: &TransferProgress,
) -> Result<Option<PathBuf>, AppError> {
    let target = dest_dir.join(name);
    if exists(&target) {
        return Err(name_taken(name));
    }
    let temp = dest_dir.join(format!(
        "{COPY_TEMP_PREFIX}{}",
        crate::api::random_hex::<8>()?
    ));
    let copied = copy_entry(src, &temp, progress);
    let finished = match copied {
        Ok(()) if progress.is_cancelled() => Ok(None),
        Ok(()) if exists(&target) => Err(name_taken(name)),
        Ok(()) => fs::rename(&temp, &target)
            .map(|()| Some(target))
            .map_err(map_io_error),
        Err(err) => Err(map_io_error(err)),
    };
    if !matches!(finished, Ok(Some(_))) && exists(&temp) {
        // 片付けに失敗しても、元の失敗の方を返す (一覧には出ない名前なので、利用者の目には触れない)。
        let _ = remove_tree(&temp);
    }
    finished
}

/// `src` を `dest` に写す。リンクとファイル・フォルダー以外のもの (ソケットなど) は飛ばす。
fn copy_entry(src: &Path, dest: &Path, progress: &TransferProgress) -> io::Result<()> {
    if progress.is_cancelled() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(src)?;
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        fs::create_dir(dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_entry(&entry.path(), &dest.join(entry.file_name()), progress)?;
            if progress.is_cancelled() {
                return Ok(());
            }
        }
        // 読み取り専用のフォルダーでも中に写せるよう、属性は中身の後に写す。
        fs::set_permissions(dest, metadata.permissions())?;
        progress.add_item();
    } else if file_type.is_file() {
        copy_file(src, dest, &metadata, progress)?;
        progress.add_item();
    }
    Ok(())
}

/// ファイルを少しずつ写す。`fs::copy` は1回で写し切るので、大きなファイルの途中で中止も進み具合も効かないため。
fn copy_file(
    src: &Path,
    dest: &Path,
    metadata: &fs::Metadata,
    progress: &TransferProgress,
) -> io::Result<()> {
    let mut reader = File::open(src)?;
    let mut writer = File::create_new(dest)?;
    let mut buffer = vec![0; COPY_CHUNK_BYTES];
    loop {
        if progress.is_cancelled() {
            return Ok(());
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        progress.add_bytes(read as u64);
    }
    // 更新日時を残す。一覧の「新しい順」が、写した時刻で並び替わらないように。
    if let Ok(modified) = metadata.modified() {
        writer.set_modified(modified)?;
    }
    writer.set_permissions(metadata.permissions())?;
    Ok(())
}

/// `src` を `dest_dir` の中に `name` として動かす。動かした先を返す。中止されたら `None`。
///
/// 同じディスクの中なら名前を付け替えるだけで済む。別のディスクへは、写してから元を消す
/// (写し終えるまでは元を消さないので、途中で止まっても元は残る)。
pub(super) fn move_entry(
    src: &Path,
    dest_dir: &Path,
    name: &str,
    progress: &TransferProgress,
) -> Result<Option<PathBuf>, AppError> {
    let target = dest_dir.join(name);
    if exists(&target) && !same_entry(src, &target) {
        return Err(name_taken(name));
    }
    match fs::rename(src, &target) {
        Ok(()) => {
            progress.add_item();
            Ok(Some(target))
        }
        Err(err) if err.kind() == io::ErrorKind::CrossesDevices => {
            let Some(target) = copy_tree(src, dest_dir, name, progress)? else {
                return Ok(None);
            };
            remove_tree(src).map_err(map_io_error)?;
            Ok(Some(target))
        }
        Err(err) => Err(map_io_error(err)),
    }
}

/// リンクを辿らずに消す。フォルダーなら中身ごと (`fs::remove_dir_all` は中のリンクを辿らない)。
fn remove_tree(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// 消す。`permanent` でなければ OS のごみ箱へ送る。`trash` はごみ箱を使える場所か (→ `trash_available`)。
/// 使えない場所でごみ箱へ送ろうとしたら 409 (`trash_unavailable`)。黙って完全に消さない。
pub(super) fn delete(paths: &[PathBuf], permanent: bool, trash: bool) -> Result<(), AppError> {
    if permanent {
        for path in paths {
            remove_tree(path).map_err(map_io_error)?;
        }
        return Ok(());
    }
    if !trash {
        return Err(AppError::TrashUnavailable);
    }
    trash_context().delete_all(paths).map_err(trash_error)
}

fn trash_context() -> trash::TrashContext {
    #[cfg_attr(not(target_os = "macos"), expect(unused_mut))]
    let mut context = trash::TrashContext::default();
    // 既定の Finder 経由は、ほかのアプリを操作する許可を求めるダイアログを出す。サーバーとして動く
    // WebLAV では、PC の前に誰もいないことがあるので、許可の要らない NSFileManager を使う。
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        context.set_delete_method(DeleteMethod::NsFileManager);
    }
    context
}

fn trash_error(err: trash::Error) -> AppError {
    match err {
        #[cfg(all(unix, not(target_os = "macos")))]
        trash::Error::FileSystem { source, .. } => map_io_error(source),
        trash::Error::Os { code, .. } => map_io_error(io::Error::from_raw_os_error(code)),
        trash::Error::CouldNotAccess { .. } => AppError::NotFound,
        err => AppError::Io(io::Error::other(err.to_string())),
    }
}

/// `path` のある場所で、ごみ箱へ送れるか。確かに送れると分かる場所だけ真にする。
/// 送れない場所で送ろうとすると、OS によっては黙って完全に消すため (Windows のネットワークのドライブなど)。
///
/// - Windows: ネットワーク上の場所 (UNC・ネットワークのドライブ) でなければ送れる
/// - macOS: この Mac のディスク (`MNT_LOCAL`) なら送れる
/// - そのほか (Linux など): ホームフォルダーと同じディスクなら送れる。ほかのディスクのごみ箱
///   (`.Trash-<uid>`) は、作れるか・デスクトップから見えるかがディスクによって違うため
pub(super) fn trash_available(path: &Path) -> bool {
    platform_trash_available(path)
}

#[cfg(windows)]
fn platform_trash_available(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::core::PCWSTR;

    /// `DRIVE_REMOTE` (winbase.h)。この定数のためだけに Win32_System_WindowsProgramming を足さない。
    const DRIVE_REMOTE: u32 = 4;

    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return false;
    };
    let drive = match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
        // UNC (`\\server\share`) はネットワーク上。
        _ => return false,
    };
    let root: Vec<u16> = std::ffi::OsStr::new(&format!("{}:\\", drive as char))
        .encode_wide()
        .chain([0])
        .collect();
    let drive_type = unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) };
    drive_type != DRIVE_REMOTE
}

#[cfg(target_os = "macos")]
fn platform_trash_available(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    let Ok(path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    let mut stats = std::mem::MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: `path` は NUL で終わる文字列で、`stats` は statfs が書く大きさの領域。
    if unsafe { libc::statfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return false;
    }
    // SAFETY: statfs が成功したので、`stats` は書き終わっている。
    let stats = unsafe { stats.assume_init() };
    stats.f_flags & libc::MNT_LOCAL as u32 != 0
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_trash_available(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    match (fs::metadata(path), fs::metadata(home)) {
        (Ok(path), Ok(home)) => path.dev() == home.dev(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> crate::test_support::TempDir {
        crate::test_support::project_temp_dir("shares-fsops", name)
    }

    #[test]
    fn a_taken_name_gets_the_first_free_number_before_the_extension() {
        let dir = temp_dir("free-name");
        fs::write(dir.join("写真.jpg"), b"").expect("書けなかった");
        fs::write(dir.join("写真 (1).jpg"), b"").expect("書けなかった");
        fs::create_dir(dir.join("2026.04")).expect("作れなかった");
        assert_eq!(free_name(&dir, "写真.jpg", false), "写真 (2).jpg");
        assert_eq!(free_name(&dir, "メモ.txt", false), "メモ.txt");
        assert_eq!(free_name(&dir, "2026.04", true), "2026.04 (1)");
        assert_eq!(free_name(&dir, "a.tar.gz", false), "a.tar.gz");
    }

    #[test]
    fn image_kinds_are_told_from_their_first_bytes() {
        let mut bmp = b"BM".to_vec();
        bmp.extend([0; 12]);
        bmp.extend(40u32.to_le_bytes());
        let cases: &[(&[u8], Option<ImageKind>)] = &[
            (&[0xFF, 0xD8, 0xFF, 0xE0], Some(ImageKind::Jpeg)),
            (b"\x89PNG\r\n\x1a\n", Some(ImageKind::Png)),
            (b"GIF89a", Some(ImageKind::Gif)),
            (b"RIFF\0\0\0\0WEBPVP8 ", Some(ImageKind::Webp)),
            (b"\0\0\0\x18ftypheic", Some(ImageKind::Heic)),
            (b"\0\0\0\x18ftypmif1", Some(ImageKind::Heif)),
            (b"\0\0\0\x18ftypisom", None),
            (b"II*\0", Some(ImageKind::Tiff)),
            (&bmp, Some(ImageKind::Bmp)),
            // "BM" で始まるだけの文字は画像としない。
            (b"BM is not a bitmap header", None),
            (b"%PDF-1.7", None),
        ];
        for (head, kind) in cases {
            assert_eq!(sniff_image(head), *kind, "{head:?}");
        }
    }

    /// 画像の拡張子が中身と食い違うか、拡張子が無いときだけ直す。
    #[test]
    fn only_image_or_missing_extensions_are_fixed() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0];
        assert_eq!(
            fixed_extension("IMG_0001.HEIC", &jpeg).as_deref(),
            Some("IMG_0001.jpg")
        );
        assert_eq!(fixed_extension("写真", &jpeg).as_deref(), Some("写真.jpg"));
        assert_eq!(fixed_extension("写真.JPEG", &jpeg), None);
        assert_eq!(fixed_extension("資料.pdf", &jpeg), None);
        assert_eq!(fixed_extension("メモ.txt", b"plain text"), None);
        assert_eq!(fixed_extension("IMG.heif", b"\0\0\0\x18ftypheic"), None);
    }

    #[test]
    fn a_new_folder_does_not_replace_an_existing_one() {
        let dir = temp_dir("create-dir");
        create_dir_new(&dir.join("教材")).expect("作れるはず");
        let err = create_dir_new(&dir.join("教材")).expect_err("同じ名前は作れないはず");
        assert!(matches!(
            err,
            AppError::ValidationDetailed {
                detail: ValidationDetail::EntryNameTaken { ref name },
                ..
            } if name == "教材"
        ));
    }

    #[test]
    fn a_rename_refuses_a_taken_name() {
        let dir = temp_dir("rename");
        fs::write(dir.join("a.txt"), b"a").expect("書けなかった");
        fs::write(dir.join("b.txt"), b"b").expect("書けなかった");
        assert!(rename_entry(&dir.join("a.txt"), "b.txt").is_err());
        assert_eq!(fs::read(dir.join("b.txt")).expect("読めなかった"), b"b");
        let renamed = rename_entry(&dir.join("a.txt"), "c.txt").expect("変えられるはず");
        assert_eq!(renamed, dir.join("c.txt"));
        assert!(!dir.join("a.txt").exists());
        // 大文字小文字だけの変更は通す (区別しないファイルシステムでも)。
        rename_entry(&dir.join("c.txt"), "C.txt").expect("変えられるはず");
        assert!(dir.join("C.txt").exists());
    }

    #[test]
    fn a_folder_is_copied_with_its_contents_and_without_links() {
        let dir = temp_dir("copy");
        let src = dir.join("src");
        fs::create_dir_all(src.join("sub")).expect("作れなかった");
        fs::write(src.join("a.txt"), b"hello").expect("書けなかった");
        fs::write(src.join("sub").join("b.txt"), b"world!").expect("書けなかった");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", src.join("link")).expect("リンクを作れなかった");
        let dest = dir.join("dest");
        fs::create_dir(&dest).expect("作れなかった");

        let progress = TransferProgress::default();
        let copied = copy_tree(&src, &dest, "src", &progress)
            .expect("写せるはず")
            .expect("中止していない");
        assert_eq!(copied, dest.join("src"));
        assert_eq!(
            fs::read(copied.join("a.txt")).expect("読めなかった"),
            b"hello"
        );
        assert_eq!(
            fs::read(copied.join("sub").join("b.txt")).expect("読めなかった"),
            b"world!"
        );
        assert!(!exists(&copied.join("link")));
        assert_eq!(progress.bytes.load(Ordering::Relaxed), 11);
        assert_eq!(progress.items.load(Ordering::Relaxed), 4);
        assert_eq!(tree_size(&src).expect("数えられるはず"), (11, 4));
        // 作業用の名前は残らない。
        assert_eq!(fs::read_dir(&dest).expect("読めなかった").count(), 1);

        assert!(copy_tree(&src, &dest, "src", &progress).is_err());
    }

    #[test]
    fn a_cancelled_copy_leaves_nothing_behind() {
        let dir = temp_dir("copy-cancel");
        fs::write(dir.join("a.txt"), b"hello").expect("書けなかった");
        let dest = dir.join("dest");
        fs::create_dir(&dest).expect("作れなかった");
        let progress = TransferProgress::default();
        progress.cancel();
        assert!(
            copy_tree(&dir.join("a.txt"), &dest, "a.txt", &progress)
                .expect("失敗はしない")
                .is_none()
        );
        assert_eq!(fs::read_dir(&dest).expect("読めなかった").count(), 0);
    }

    #[test]
    fn an_entry_is_moved_and_a_folder_cannot_go_into_itself() {
        let dir = temp_dir("move");
        let src = dir.join("教材");
        fs::create_dir_all(src.join("英検")).expect("作れなかった");
        let dest = dir.join("dest");
        fs::create_dir(&dest).expect("作れなかった");

        assert!(matches!(
            ensure_not_into_itself(&src, &src.join("英検")),
            Err(AppError::ValidationDetailed {
                detail: ValidationDetail::TransferIntoItself,
                ..
            })
        ));
        assert!(ensure_not_into_itself(&src, &dest).is_ok());
        let moved = move_entry(&src, &dest, "教材", &TransferProgress::default())
            .expect("動かせるはず")
            .expect("中止していない");
        assert!(moved.join("英検").is_dir());
        assert!(!src.exists());
    }

    #[test]
    fn a_permanent_delete_removes_folders_and_leaves_link_targets() {
        let dir = temp_dir("delete");
        let target = dir.join("target");
        fs::create_dir_all(&target).expect("作れなかった");
        fs::write(target.join("keep.txt"), b"keep").expect("書けなかった");
        let doomed = dir.join("doomed");
        fs::create_dir_all(doomed.join("sub")).expect("作れなかった");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, doomed.join("link")).expect("リンクを作れなかった");
        fs::write(dir.join("a.txt"), b"a").expect("書けなかった");

        delete(&[doomed.clone(), dir.join("a.txt")], true, false).expect("消せるはず");
        assert!(!doomed.exists());
        assert!(!dir.join("a.txt").exists());
        assert!(target.join("keep.txt").exists());
    }

    /// ごみ箱を使えない場所では、ごみ箱へ送ろうとしても消さない。
    #[test]
    fn a_trash_delete_is_refused_where_there_is_no_trash() {
        let dir = temp_dir("no-trash");
        fs::write(dir.join("a.txt"), b"a").expect("書けなかった");
        assert!(matches!(
            delete(&[dir.join("a.txt")], false, false),
            Err(AppError::TrashUnavailable)
        ));
        assert!(dir.join("a.txt").exists());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn the_home_folder_has_a_trash() {
        let home = std::env::var_os("HOME").expect("HOME があるはず");
        assert!(trash_available(Path::new(&home)));
    }

    #[test]
    fn io_errors_become_errors_the_screen_can_explain() {
        let kind = |kind: io::ErrorKind| map_io_error(io::Error::from(kind));
        assert!(matches!(
            kind(io::ErrorKind::PermissionDenied),
            AppError::ShareReadOnly
        ));
        assert!(matches!(
            kind(io::ErrorKind::StorageFull),
            AppError::InsufficientStorage
        ));
        assert!(matches!(
            kind(io::ErrorKind::ResourceBusy),
            AppError::EntryInUse
        ));
        assert!(matches!(kind(io::ErrorKind::NotFound), AppError::NotFound));
    }

    #[test]
    fn there_is_space_for_nothing_but_not_for_everything() {
        let dir = temp_dir("space");
        ensure_space(&dir, 0).expect("空きはあるはず");
        assert!(matches!(
            ensure_space(&dir, u64::MAX),
            Err(AppError::InsufficientStorage)
        ));
    }
}
