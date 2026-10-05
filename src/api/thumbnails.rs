//! 画像のプレビュー (→ docs/ui.md「画像のプレビュー」)。
//!
//! - 一覧に出す画像の大きさ (`file_preview`)。ページ内のビューアが開く前に要る。
//! - 一覧の行に出す縮小画像 (`thumbnail_response`)。作ったものは `Thumbnails::dir` に置いて使い回す。
//!   画像でないファイル (動画・PDF など) は OS に作らせる (`crate::os_thumbnail`)。

use std::fs::File;
use std::io::{BufRead, BufReader, Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use image::codecs::jpeg::JpegEncoder;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use utoipa::ToSchema;

use crate::config::create_owner_only_dir;
use crate::error::{AppError, run_blocking};
use crate::file_ext::has_extension_in;
use crate::os_thumbnail::Failure;

/// プレビューする画像の拡張子。どのブラウザも表示でき、`image` crate で縮小画像を作れる形式に絞る。
const IMAGE_EXTENSIONS: [&str; 6] = ["jpg", "jpeg", "png", "gif", "webp", "bmp"];

/// 大きさを読むのに渡す先頭のバイト数。
/// FIX: `image` の JPEG デコーダーは生成時に入力を全部読むので、写真の並ぶ一覧で
/// 大きさだけを知りたいときも全体を読んでしまう。先頭だけを渡してヘッダーを読ませる。
/// EXIF (APP1) は 64KB までなので、大きさを持つ SOF はたいていこの中にある。
const HEADER_PREFIX_BYTES: u64 = 256 * 1024;

/// 縮小画像の一辺 (px)。行のアイコン枠 (最大 44px) を、画面密度 3 倍の端末でもぼやけずに埋める。
const THUMBNAIL_SIZE: u32 = 144;

/// 縮小画像の作り方を変えたら上げる。キャッシュのキーに入れ、前の作り方のものを使わない。
const THUMBNAIL_VERSION: u32 = 2;

/// 縮小画像とリンクのカードの画像を作るときに、デコードに使ってよいメモリの上限。
/// 1億画素 (RGBA で 400MB) 級のパノラマなどはプレビューせず、ファイルのアイコンのままにする。
/// 同時に作る数 (`Thumbnails::new` と `link_preview::LinkPreviews::new`) を掛けた分が、
/// サーバーの PC のメモリを圧迫しない大きさにする。
const DECODE_MAX_ALLOC: u64 = 256 * 1024 * 1024;

/// 縮小画像とリンクのカードの画像の JPEG の品質。どちらも小さく出すので、これ以上上げても見た目は変わらずサイズだけ増える。
const THUMBNAIL_JPEG_QUALITY: u8 = 80;

/// 縮小画像の口が作ろうとするファイルか。画像と、OS が縮小画像を作れる種類のファイル。
fn has_thumbnail(file_name: &str) -> bool {
    is_image_file_name(file_name) || crate::os_thumbnail::is_candidate(file_name)
}

/// 一覧の行に縮小画像を出してみるか。画像は大きさを読めたもの (`image`) だけにする。
/// OS が作るものは作ってみるまで分からないので、作れなければ画面がアイコンに戻す。
fn shows_thumbnail(file_name: &str, image: Option<ImageSize>) -> bool {
    image.is_some() || crate::os_thumbnail::is_candidate(file_name)
}

/// ファイル名から、プレビューする画像かを判定する。
pub(super) fn is_image_file_name(name: &str) -> bool {
    has_extension_in(name, &IMAGE_EXTENSIONS)
}

/// 表示するときの画像の縦横 (px)。EXIF の向きを当てた後の値。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub(super) struct ImageSize {
    width: u32,
    height: u32,
}

/// 一覧の行に添える、プレビューのための情報。一覧ごとの型 (アーカイブのアイテム・フォルダの
/// エントリ・file コンテンツ) に同じ名前のフィールドで写す。
pub(super) struct FilePreview {
    /// ページ内でプレビューする画像なら、その大きさ。
    pub(super) image: Option<ImageSize>,
    /// テキストのビューアで見せるファイルか。
    pub(super) is_text: bool,
    /// 行に縮小画像を出してみるか。
    pub(super) thumbnail: bool,
}

/// ファイル `name` (中身は `path`、大きさは `size`) のプレビューの情報。
/// 種類は `name` の拡張子で決める (file コンテンツの実体は拡張子を持たないため)。
/// ブロッキング I/O なので `run_blocking` の中から呼ぶ。
pub(super) fn file_preview(name: &str, path: &Path, size: u64) -> FilePreview {
    let image = is_image_file_name(name)
        .then(|| read_image_size(path))
        .flatten();
    FilePreview {
        image,
        is_text: super::text_files::is_text_file(name, path, size),
        thumbnail: shows_thumbnail(name, image),
    }
}

/// `path` の画像の、表示するときの大きさ。読めないファイルは `None`。
/// 拡張子は見ないので、呼び出し側が名前で画像かを判定してから呼ぶ。
fn read_image_size(path: &Path) -> Option<ImageSize> {
    let mut prefix = Vec::new();
    File::open(path)
        .ok()?
        .take(HEADER_PREFIX_BYTES)
        .read_to_end(&mut prefix)
        .ok()?;
    let truncated = prefix.len() as u64 == HEADER_PREFIX_BYTES;
    size_from_reader(Cursor::new(prefix)).or_else(|| {
        // 先頭だけでは読めない形式 (EXIF がファイルの後ろにある WebP など) は全体を読む。
        truncated
            .then(|| size_from_reader(BufReader::new(File::open(path).ok()?)))
            .flatten()
    })
}

fn size_from_reader(reader: impl BufRead + Seek) -> Option<ImageSize> {
    let mut decoder = ImageReader::new(reader)
        .with_guessed_format()
        .ok()?
        .into_decoder()
        .ok()?;
    let (width, height) = decoder.dimensions();
    let orientation = decoder.orientation().ok()?;
    Some(oriented_size(width, height, orientation))
}

/// ブラウザは EXIF の向きを当てて表示するので、90度回すものは縦横を入れ替える。
fn oriented_size(width: u32, height: u32, orientation: Orientation) -> ImageSize {
    match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => ImageSize {
            width: height,
            height: width,
        },
        Orientation::NoTransforms
        | Orientation::Rotate180
        | Orientation::FlipHorizontal
        | Orientation::FlipVertical => ImageSize { width, height },
    }
}

/// 縮小画像の置き場と、同時に作る数の上限。
pub struct Thumbnails {
    dir: PathBuf,
    /// デコードは CPU とメモリを大きく使うので、同時に作る数をコアの半分までに絞る。
    /// 教室の端末が一斉に同じフォルダを開いても、配信や他の操作を止めないため。
    permits: Semaphore,
}

impl Thumbnails {
    /// `dir` の作成は最初に縮小画像を作るときまで遅らせる (DB・blob と同じ)。
    pub fn new(dir: PathBuf) -> Self {
        let permits = std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).max(1));
        Self {
            dir,
            permits: Semaphore::new(permits),
        }
    }
}

/// 縮小画像の形式。透過のある画像を JPEG にすると透過部分が黒く塗られるので PNG にする。
#[derive(Clone, Copy)]
pub(super) enum ThumbnailFormat {
    Jpeg,
    Png,
}

impl ThumbnailFormat {
    pub(super) const ALL: [Self; 2] = [Self::Jpeg, Self::Png];

    pub(super) fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
        }
    }

    pub(super) fn mime(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
        }
    }
}

/// 置き場から読んだもの。
enum Cached {
    Thumbnail(Vec<u8>, ThumbnailFormat),
    /// 前に作れなかった。一覧を開くたびに作り直そうとして OS を呼ばないよう、作れなかったことも置いておく。
    Unavailable,
}

/// `source` (配信の検証を通した実体のパス) の縮小画像を返す。
/// `file_name` は拡張子の判定に使う (file コンテンツの実体は拡張子を持たないため)。
///
/// 元のファイルが変わるとキーが変わるので、ブラウザには毎回 ETag で問い合わせさせる。
/// 縮小画像を作れないファイルは 404 にして、画面にはファイルのアイコンを出させる。
pub(super) async fn thumbnail_response(
    thumbnails: &Thumbnails,
    source: PathBuf,
    file_name: &str,
    request_headers: &HeaderMap,
) -> Result<Response, AppError> {
    if !has_thumbnail(file_name) {
        return Err(AppError::NotFound);
    }

    let key = {
        let source = source.clone();
        let file_name = file_name.to_owned();
        run_blocking(move || cache_key(&source, &file_name)).await??
    };
    let etag = HeaderValue::from_str(&format!("\"{key}\""))
        .map_err(|_| AppError::DataIntegrity("thumbnail key is not a valid header value"))?;

    if if_none_match_contains(request_headers, &etag) {
        return Ok(with_cache_headers(
            StatusCode::NOT_MODIFIED.into_response(),
            etag,
        ));
    }

    let cached = {
        let dir = thumbnails.dir.clone();
        let key = key.clone();
        run_blocking(move || read_cached(&dir, &key)).await?
    };
    let thumbnail = match cached {
        Some(Cached::Thumbnail(bytes, format)) => (bytes, format),
        Some(Cached::Unavailable) => return Err(AppError::NotFound),
        None => {
            let _permit = thumbnails
                .permits
                .acquire()
                .await
                .map_err(|_| AppError::DataIntegrity("thumbnail semaphore is closed"))?;
            let dir = thumbnails.dir.clone();
            let file_name = file_name.to_owned();
            // 順番を待つ間に、同じ画像を別のリクエストが作り終えていることがある。
            run_blocking(move || match read_cached(&dir, &key) {
                Some(Cached::Thumbnail(bytes, format)) => Ok(Some((bytes, format))),
                Some(Cached::Unavailable) => Ok(None),
                None => create_and_store(&dir, &key, &source, &file_name),
            })
            .await??
            .ok_or(AppError::NotFound)?
        }
    };

    let (bytes, format) = thumbnail;
    let mut response = bytes.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(format.mime()),
    );
    Ok(with_cache_headers(response, etag))
}

fn with_cache_headers(mut response: Response, etag: HeaderValue) -> Response {
    let headers = response.headers_mut();
    headers.insert(header::ETAG, etag);
    // 公開範囲で見えるかが変わるので、共有のキャッシュには置かせない。
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-cache"),
    );
    response
}

/// `If-None-Match` は弱い比較で見る (RFC 9110 13.1.2)。途中で `W/` を付けられた値や `*` も一致とする。
fn if_none_match_contains(headers: &HeaderMap, etag: &HeaderValue) -> bool {
    let etag = etag.to_str().unwrap_or_default();
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag)
}

/// 元のファイルの場所・大きさ・更新日時・拡張子と、縮小画像の作り方から決まるキー。
/// どれかが変われば別のキーになるので、古い縮小画像を返さない。
/// 拡張子は `file_name` から取る。file コンテンツの実体は中身が同じなら名前の違うファイルでも共有され、
/// 縮小画像の作り方 (画像として読むか、OS にどの種類として頼むか) は拡張子で決まるため。
fn cache_key(source: &Path, file_name: &str) -> Result<String, AppError> {
    let metadata = std::fs::metadata(source)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos());
    let extension = Path::new(file_name)
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(
        format!(
            "{THUMBNAIL_VERSION}\0{THUMBNAIL_SIZE}\0{}\0{}\0{modified}\0{extension}",
            source.to_string_lossy(),
            metadata.len()
        )
        .as_bytes(),
    );
    Ok(super::hex_encode(&hasher.finalize()))
}

/// キーの先頭2文字で分ける。1つのディレクトリに数万のファイルが並ぶのを避けるため。
/// `extension` は縮小画像の形式か、作れなかった印 (`UNAVAILABLE_EXTENSION`)。
fn cache_path(dir: &Path, key: &str, extension: &str) -> PathBuf {
    dir.join(&key[..2]).join(format!("{key}.{extension}"))
}

/// 作れなかった印の拡張子。中身は空。
const UNAVAILABLE_EXTENSION: &str = "none";

fn read_cached(dir: &Path, key: &str) -> Option<Cached> {
    ThumbnailFormat::ALL
        .into_iter()
        .find_map(|format| {
            let bytes = std::fs::read(cache_path(dir, key, format.extension())).ok()?;
            Some(Cached::Thumbnail(bytes, format))
        })
        .or_else(|| {
            cache_path(dir, key, UNAVAILABLE_EXTENSION)
                .is_file()
                .then_some(Cached::Unavailable)
        })
}

/// 縮小画像を作って置く。作れなければ `Ok(None)`。
fn create_and_store(
    dir: &Path,
    key: &str,
    source: &Path,
    file_name: &str,
) -> Result<Option<(Vec<u8>, ThumbnailFormat)>, AppError> {
    match create_thumbnail(source, file_name) {
        Ok((bytes, format)) => {
            store(&cache_path(dir, key, format.extension()), key, &bytes)?;
            Ok(Some((bytes, format)))
        }
        Err(Failure::Unavailable) => {
            store(&cache_path(dir, key, UNAVAILABLE_EXTENSION), key, &[])?;
            Ok(None)
        }
        Err(Failure::TimedOut) => Ok(None),
    }
}

/// `dest` に書く。`key` は書きかけの一時ファイルの名前に使う。リンクのカードの画像の置き場でも使う。
pub(super) fn store(dest: &Path, key: &str, bytes: &[u8]) -> Result<(), AppError> {
    let parent = dest
        .parent()
        .ok_or(AppError::DataIntegrity("thumbnail path has no parent"))?;
    create_owner_only_dir(parent)?;
    // 書きかけのファイルを別のリクエストに読ませないよう、別名で書いてから置き換える。
    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);
    let temp = parent.join(format!(
        "{key}.{}.{}.tmp",
        std::process::id(),
        TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&temp, bytes)?;
    if let Err(err) = std::fs::rename(&temp, dest) {
        let _ = std::fs::remove_file(&temp);
        // 同じキーを別のリクエストが先に置き終えていれば、中身は同じなのでそれで足りる。
        if !dest.is_file() {
            return Err(err.into());
        }
    }
    Ok(())
}

/// 正方形に切り抜いて縮める。行の枠は正方形で、画面側も切り抜いて見せるため。
fn create_thumbnail(source: &Path, file_name: &str) -> Result<(Vec<u8>, ThumbnailFormat), Failure> {
    let (image, anchor) = if is_image_file_name(file_name) {
        (
            decode_image(source).ok_or(Failure::Unavailable)?,
            CropAnchor::Center,
        )
    } else {
        // 短い辺が縮小画像の一辺に届くよう、収める四角を広めに頼む (OS は長い辺をこの大きさに収める)。
        let image = crate::os_thumbnail::thumbnail(source, file_name, THUMBNAIL_SIZE * 2)?;
        let anchor = if crate::os_thumbnail::is_document(file_name) {
            CropAnchor::Top
        } else {
            CropAnchor::Center
        };
        (image, anchor)
    };

    let thumbnail = fill_square(&image, THUMBNAIL_SIZE, anchor);
    encode(&thumbnail, false).ok_or(Failure::Unavailable)
}

/// 縮めた画像を書き出す。透過があるか `always_png` なら PNG、無ければ JPEG にする。
pub(super) fn encode(image: &DynamicImage, always_png: bool) -> Option<(Vec<u8>, ThumbnailFormat)> {
    let mut bytes = Vec::new();
    if always_png || has_transparency(image) {
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .ok()?;
        Some((bytes, ThumbnailFormat::Png))
    } else {
        JpegEncoder::new_with_quality(&mut bytes, THUMBNAIL_JPEG_QUALITY)
            .encode_image(&image.to_rgb8())
            .ok()?;
        Some((bytes, ThumbnailFormat::Jpeg))
    }
}

/// 透過する画素があるか。
fn has_transparency(image: &DynamicImage) -> bool {
    image.color().has_alpha() && image.to_rgba8().pixels().any(|pixel| pixel[3] < u8::MAX)
}

fn decode_image(source: &Path) -> Option<DynamicImage> {
    decode(BufReader::new(File::open(source).ok()?))
}

/// メモリの上限を付けて読み、写真の向き (Exif) を直す。
pub(super) fn decode(source: impl BufRead + Seek) -> Option<DynamicImage> {
    let mut reader = ImageReader::new(source).with_guessed_format().ok()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(DECODE_MAX_ALLOC);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    image.apply_orientation(orientation);
    Some(image)
}

/// 正方形に切り抜くとき、長い辺のどこを残すか。
#[derive(Clone, Copy)]
enum CropAnchor {
    Center,
    /// 縦長なら上を残す。横長は真ん中。
    Top,
}

/// 短い辺を `side` に合わせて縮め、長い辺を `anchor` の位置で切り抜く。
/// 先に縮めてから切り抜くのは、元の大きさのまま切り抜くと写真1枚分の複製が要るため。
/// `side` より小さい画像は拡大しない。
fn fill_square(image: &DynamicImage, side: u32, anchor: CropAnchor) -> DynamicImage {
    let (width, height) = (image.width(), image.height());
    let shorter = width.min(height);
    if shorter == 0 {
        return image.clone();
    }
    let side = side.min(shorter);
    let scale = |length: u32| {
        u32::try_from(u64::from(length) * u64::from(side) / u64::from(shorter))
            .unwrap_or(u32::MAX)
            .max(side)
    };
    let resized = image.thumbnail_exact(scale(width), scale(height));
    let (resized_width, resized_height) = (resized.width(), resized.height());
    let top = match anchor {
        CropAnchor::Center => (resized_height - side) / 2,
        CropAnchor::Top => 0,
    };
    resized.crop_imm((resized_width - side) / 2, top, side, side)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage, Rgba, RgbaImage};

    fn temp_dir(name: &str) -> crate::test_support::TempDir {
        crate::test_support::project_temp_dir("thumbnails", name)
    }

    #[test]
    fn if_none_match_contains_uses_weak_comparison() {
        let etag = HeaderValue::from_static("\"abc\"");
        let matches = |value: &'static str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static(value));
            if_none_match_contains(&headers, &etag)
        };
        assert!(matches("\"abc\""));
        assert!(matches("W/\"abc\""));
        assert!(matches("\"xyz\", W/\"abc\""));
        assert!(matches("*"));
        assert!(!matches("\"xyz\""));
        assert!(!matches("W/\"xyz\""));
    }

    #[test]
    fn is_image_file_name_matches_known_extensions_case_insensitively() {
        assert!(is_image_file_name("photo.JPG"));
        assert!(is_image_file_name("scan.webp"));
        assert!(!is_image_file_name("notes.pdf"));
        assert!(!is_image_file_name("photo.heic"));
        assert!(!is_image_file_name("jpg"));
    }

    #[test]
    fn file_preview_reads_png_dimensions_and_shows_a_thumbnail() {
        let dir = temp_dir("thumbnails-size");
        let path = dir.join("wide.png");
        RgbImage::new(300, 100)
            .save(&path)
            .expect("failed to write png");
        let size = std::fs::metadata(&path).expect("failed to stat png").len();
        let preview = file_preview("wide.png", &path, size);
        assert_eq!(
            preview.image,
            Some(ImageSize {
                width: 300,
                height: 100
            })
        );
        assert!(preview.thumbnail);
        assert!(!preview.is_text);
    }

    #[test]
    fn file_preview_judges_the_kind_by_name_not_by_path() {
        // file コンテンツの実体のように、拡張子の無い場所でも名前で画像と分かる。
        let dir = temp_dir("thumbnails-blob");
        let path = dir.join("blob");
        RgbImage::new(10, 20)
            .save_with_format(&path, ImageFormat::Png)
            .expect("failed to write png");
        let size = std::fs::metadata(&path).expect("failed to stat png").len();
        assert_eq!(
            file_preview("tall.png", &path, size).image,
            Some(ImageSize {
                width: 10,
                height: 20
            })
        );
        // 名前が画像でなければ、中身が画像でも大きさは読まない。
        assert_eq!(file_preview("blob.bin", &path, size).image, None);
    }

    #[test]
    fn file_preview_ignores_files_that_are_not_images() {
        let dir = temp_dir("thumbnails-not-image");
        let path = dir.join("broken.jpg");
        std::fs::write(&path, b"not an image").expect("failed to write file");
        let broken = file_preview("broken.jpg", &path, 12);
        assert_eq!(broken.image, None);
        assert!(!broken.thumbnail);
        let pdf = dir.join("doc.pdf");
        std::fs::write(&pdf, b"%PDF-1.4").expect("failed to write file");
        assert_eq!(file_preview("doc.pdf", &pdf, 8).image, None);
    }

    #[test]
    fn oriented_size_swaps_dimensions_for_quarter_turns() {
        assert_eq!(
            oriented_size(400, 300, Orientation::Rotate90),
            ImageSize {
                width: 300,
                height: 400
            }
        );
        assert_eq!(
            oriented_size(400, 300, Orientation::Rotate180),
            ImageSize {
                width: 400,
                height: 300
            }
        );
    }

    #[test]
    fn fill_square_crops_the_center_to_the_requested_side() {
        let mut image = RgbImage::new(600, 200);
        // 中央の 200x200 だけを白くして、切り抜いた結果が中央だけになることを確かめる。
        for x in 200..400 {
            for y in 0..200 {
                image.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        let thumbnail = fill_square(&DynamicImage::ImageRgb8(image), 100, CropAnchor::Center);
        assert_eq!((thumbnail.width(), thumbnail.height()), (100, 100));
        let rgb = thumbnail.to_rgb8();
        assert_eq!(rgb.get_pixel(5, 50), &Rgb([255, 255, 255]));
        assert_eq!(rgb.get_pixel(95, 50), &Rgb([255, 255, 255]));
    }

    #[test]
    fn fill_square_keeps_the_top_of_tall_documents() {
        let mut image = RgbImage::new(200, 600);
        // 上の 200x200 だけを白くして、上を残して切り抜くことを確かめる。
        for x in 0..200 {
            for y in 0..200 {
                image.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        let thumbnail = fill_square(&DynamicImage::ImageRgb8(image), 100, CropAnchor::Top);
        let rgb = thumbnail.to_rgb8();
        assert_eq!(rgb.get_pixel(50, 5), &Rgb([255, 255, 255]));
        assert_eq!(rgb.get_pixel(50, 95), &Rgb([255, 255, 255]));
    }

    #[test]
    fn opaque_rgba_thumbnails_become_jpeg() {
        let dir = temp_dir("thumbnails-opaque-rgba");
        let source = dir.join("opaque-rgba.png");
        RgbaImage::from_pixel(300, 300, Rgba([10, 20, 30, 255]))
            .save(&source)
            .expect("failed to write png");
        let (_, format) =
            create_thumbnail(&source, "opaque-rgba.png").expect("thumbnail of opaque rgba png");
        assert_eq!(format.mime(), "image/jpeg");
    }

    #[test]
    fn fill_square_does_not_upscale_small_images() {
        let thumbnail = fill_square(
            &DynamicImage::ImageRgb8(RgbImage::new(40, 20)),
            144,
            CropAnchor::Center,
        );
        assert_eq!((thumbnail.width(), thumbnail.height()), (20, 20));
    }

    #[test]
    fn create_thumbnail_keeps_transparency_as_png() {
        let dir = temp_dir("thumbnails-alpha");
        let opaque = dir.join("opaque.png");
        RgbImage::new(300, 300)
            .save(&opaque)
            .expect("failed to write png");
        let transparent = dir.join("transparent.png");
        RgbaImage::from_pixel(300, 300, Rgba([0, 0, 0, 0]))
            .save(&transparent)
            .expect("failed to write png");

        let (_, format) = create_thumbnail(&opaque, "opaque.png").expect("thumbnail of opaque png");
        assert_eq!(format.mime(), "image/jpeg");
        let (_, format) = create_thumbnail(&transparent, "transparent.png")
            .expect("thumbnail of transparent png");
        assert_eq!(format.mime(), "image/png");
    }

    #[test]
    fn create_and_store_reuses_the_cached_thumbnail() {
        let dir = temp_dir("thumbnails-cache");
        let source = dir.join("photo.png");
        RgbImage::new(300, 200)
            .save(&source)
            .expect("failed to write png");
        let cache = dir.join("thumbnails");
        let key = cache_key(&source, "photo.png").expect("cache key");

        assert!(read_cached(&cache, &key).is_none());
        let (created, _) = create_and_store(&cache, &key, &source, "photo.png")
            .expect("store thumbnail")
            .expect("source is an image");
        let Some(Cached::Thumbnail(cached, _)) = read_cached(&cache, &key) else {
            panic!("thumbnail should be cached");
        };
        assert_eq!(created, cached);
    }

    #[test]
    fn create_and_store_remembers_files_without_a_thumbnail() {
        let dir = temp_dir("thumbnails-unavailable");
        let source = dir.join("broken.png");
        std::fs::write(&source, b"not a png").expect("failed to write file");
        let cache = dir.join("thumbnails");
        let key = cache_key(&source, "broken.png").expect("cache key");

        let created = create_and_store(&cache, &key, &source, "broken.png").expect("store marker");
        assert!(created.is_none());
        assert!(matches!(
            read_cached(&cache, &key),
            Some(Cached::Unavailable)
        ));
    }

    #[test]
    fn cache_key_changes_when_the_source_changes() {
        let dir = temp_dir("thumbnails-key");
        let source = dir.join("photo.png");
        RgbImage::new(10, 10)
            .save(&source)
            .expect("failed to write png");
        let before = cache_key(&source, "photo.png").expect("cache key");
        RgbImage::new(20, 10)
            .save(&source)
            .expect("failed to rewrite png");
        assert_ne!(before, cache_key(&source, "photo.png").expect("cache key"));
        assert_ne!(
            cache_key(&source, "photo.png").expect("cache key"),
            cache_key(&source, "photo.heic").expect("cache key")
        );
    }
}
