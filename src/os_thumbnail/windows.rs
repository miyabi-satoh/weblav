//! エクスプローラーと同じ Shell の仕組み (`IShellItemImageFactory`) で縮小画像を作る。

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};

use image::{DynamicImage, RgbaImage};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, GetDC, GetDIBits,
    GetObjectW, HBITMAP, HGDIOBJ, ReleaseDC,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Shell::{
    IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_THUMBNAILONLY,
};
use windows::core::PCWSTR;

use super::{Failure, TIMEOUT};

/// 同時に `GetImage` を待つスレッドの上限。打ち切った後もスレッドは戻るまで残るので、
/// 固まるファイルを見るたびにスレッドとハードリンクが増え続けないよう、残っている数ごと数える。
/// 4 は、普通の PC で縮小画像を同時に作る数 (`Thumbnails` の順番待ち。コアの半分) と同じ程度で、
/// 固まったスレッドが残っても数本で頭打ちになる大きさ。
const MAX_RUNNING: usize = 4;

static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// `RUNNING` に数えた1つ分。スレッドが戻るときに手放す。
struct Running;

impl Running {
    fn acquire() -> Option<Self> {
        RUNNING
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |running| {
                (running < MAX_RUNNING).then_some(running + 1)
            })
            .ok()
            .map(|_| Self)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) fn thumbnail(path: &Path, extension: &str, size: u32) -> Result<DynamicImage, Failure> {
    // 埋まっているのは固まったスレッドが残っているときなので、作れなかったとは決めず、次に見たときに頼み直す。
    let running = Running::acquire().ok_or(Failure::TimedOut)?;
    // Shell は拡張子で縮小画像を作るアプリを選ぶ。file コンテンツの実体は拡張子を持たないので、
    // 同じディレクトリに拡張子付きのハードリンクを作って渡す (複製しない)。
    // ハードリンクを作れないファイルシステム (exFAT など) では作らず、行はアイコンのままになる。
    let link = path
        .extension()
        .is_none_or(|ext| !ext.eq_ignore_ascii_case(extension))
        .then(|| ExtensionLink::new(path, extension))
        .transpose()
        .map_err(|_| Failure::Unavailable)?;
    let target = link
        .as_ref()
        .map_or_else(|| path.to_path_buf(), |link| link.path.clone());

    // `GetImage` は待つ長さを指定できず、縮小画像を作るアプリが固まると戻らない。
    // 別のスレッドで呼んで待つ長さを区切る。固まったスレッドとハードリンクは、戻ったときに片付く。
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _running = running;
        let _link = link;
        let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
        let image = shell_thumbnail(&target, size);
        if initialized {
            unsafe { CoUninitialize() };
        }
        let _ = sender.send(image);
    });
    match receiver.recv_timeout(TIMEOUT) {
        Ok(Some(image)) => Ok(image),
        Ok(None) | Err(RecvTimeoutError::Disconnected) => Err(Failure::Unavailable),
        Err(RecvTimeoutError::Timeout) => Err(Failure::TimedOut),
    }
}

fn shell_thumbnail(path: &Path, size: u32) -> Option<DynamicImage> {
    let wide: Vec<u16> = without_verbatim_prefix(path)
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect();
    let factory: IShellItemImageFactory =
        unsafe { SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None) }.ok()?;
    let side = i32::try_from(size).ok()?;
    // 縮小画像を作れないとき、代わりにファイルの種類のアイコンを返させない。
    let bitmap =
        unsafe { factory.GetImage(SIZE { cx: side, cy: side }, SIIGBF_THUMBNAILONLY) }.ok()?;
    let image = bitmap_to_image(bitmap);
    unsafe {
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
    }
    image
}

/// FIX: `SHCreateItemFromParsingName` は verbatim パス (`\\?\C:\...`) を解釈できず失敗する。
/// 渡ってくるパスは `canonicalize` 済みでこの形になっているので、接頭辞を外して渡す。
fn without_verbatim_prefix(path: &Path) -> PathBuf {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else {
        return path.to_path_buf();
    };
    let mut plain = match prefix.kind() {
        Prefix::VerbatimDisk(disk) => PathBuf::from(format!("{}:\\", char::from(disk))),
        Prefix::VerbatimUNC(server, share) => {
            let mut root = OsString::from(r"\\");
            root.push(server);
            root.push(r"\");
            root.push(share);
            root.push(r"\");
            PathBuf::from(root)
        }
        _ => return path.to_path_buf(),
    };
    plain.extend(components.filter(|component| !matches!(component, Component::RootDir)));
    plain
}

fn bitmap_to_image(bitmap: HBITMAP) -> Option<DynamicImage> {
    let mut info = BITMAP::default();
    let read = unsafe {
        GetObjectW(
            HGDIOBJ(bitmap.0),
            i32::try_from(std::mem::size_of::<BITMAP>()).ok()?,
            Some((&raw mut info).cast()),
        )
    };
    if read == 0 {
        return None;
    }
    let width = u32::try_from(info.bmWidth).ok()?;
    let height = info.bmHeight.unsigned_abs();

    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: u32::try_from(std::mem::size_of::<BITMAPINFOHEADER>()).ok()?,
            biWidth: info.bmWidth,
            // 負の高さで、上の行から順に受け取る。
            biHeight: -i32::try_from(height).ok()?,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; usize::try_from(u64::from(width) * u64::from(height) * 4).ok()?];
    let dc = unsafe { GetDC(None) };
    let lines = unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr().cast()),
            &raw mut header,
            DIB_RGB_COLORS,
        )
    };
    unsafe { ReleaseDC(None, dc) };
    if lines == 0 {
        return None;
    }

    // 並びは BGRA。透過を持たない縮小画像は、アルファがすべて 0 で返る。
    let opaque = pixels.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 0);
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
        if opaque {
            pixel[3] = u8::MAX;
        }
    }
    RgbaImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgba8)
}

/// 拡張子付きのハードリンク。手放すときに消す。
struct ExtensionLink {
    path: PathBuf,
}

impl ExtensionLink {
    fn new(source: &Path, extension: &str) -> std::io::Result<Self> {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let mut name = source.file_name().unwrap_or_default().to_os_string();
        name.push(format!(
            ".thumbnail-{}-{}.{extension}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let path = source.with_file_name(name);
        std::fs::hard_link(source, &path)?;
        Ok(Self { path })
    }
}

impl Drop for ExtensionLink {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_verbatim_prefix_turns_verbatim_paths_into_plain_ones() {
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\C:\Users\me\clip.mp4")),
            Path::new(r"C:\Users\me\clip.mp4")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"\\?\UNC\server\share\docs\print.pdf")),
            Path::new(r"\\server\share\docs\print.pdf")
        );
        assert_eq!(
            without_verbatim_prefix(Path::new(r"C:\Users\me\clip.mp4")),
            Path::new(r"C:\Users\me\clip.mp4")
        );
    }
}
