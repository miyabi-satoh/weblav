//! サーバー側ディレクトリの選択と、登録前の確認のためのAPI (→ docs/folders.md「サーバー側ディレクトリの選択 UI」)。
//!
//! いずれもログイン済みなら `role` を問わない (→ docs/access.md「ロールと操作」)。`folder`/`archive` の
//! 登録先を選ばせるためだけの読み取り専用エンドポイント。
//!
//! 登録先に選べるのは「公開できるフォルダ」(→ `api::roots`) の配下だけ。判定そのものは
//! `api::roots` に置き、ここは辿り方 (`listing`) を持つ。

use std::fs::{DirEntry, FileType};
use std::path::{Path as FsPath, PathBuf};

use axum::Json;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use walkdir::WalkDir;

use crate::auth::AuthUser;
use crate::error::{AppError, run_blocking};
use crate::state::AppState;

use super::roots::{OwnDirs, Root};

/// 1回の走査で索引できるファイル数の上限。登録前の確認も、この件数を超えたら打ち切る。
///
/// `SCAN_LIMIT` とは別に持つのは、走査したエントリのほとんどが対象になる
/// ツリーでも、DBへ書き込む行数の側で歯止めが要るため。
pub(super) const ITEM_LIMIT: usize = 20_000;

/// 走査するエントリ数の上限。`ITEM_LIMIT` だけでは、対象拡張子に一致しないファイルや
/// ディレクトリしか無い巨大なツリーで打ち切りが効かず、`spawn_blocking` のワーカーを
/// 長時間占有できてしまう。数えたかどうかに関わらず、見たエントリの総数で打ち切る。
/// `ITEM_LIMIT` より大きくしてあるのは、拡張子を絞った場合でも上限の件数までは
/// 数え切れる余地を残すため。
pub(super) const SCAN_LIMIT: usize = 500_000;

/// 一覧・数え上げ・索引の対象にするかの判定 (名前と種別だけで決まる部分)。
///
/// ドット始まり・シンボリックリンクを除く、という規則を
/// `read_dirs`・`contents::read_entries`・`scan_files` と配信 (`has_hidden_component`) で
/// 揃えるために1箇所に置いている。片方だけ変えると、選択UIで数えた件数と実際に
/// 見える一覧・索引の中身が食い違い、一覧に出ないものが配信だけ通る。
///
/// 種別を `Option` で受けるのは、種別を取れないエントリ (`std::fs`) も
/// 取れることが保証されたエントリ (`walkdir`) も同じ判定に通すため。
/// 取れない場合は対象から外す。
///
/// シンボリックリンクは壊れているかどうかに関わらず除く。呼び出し側はいずれも
/// リンクを辿らない方法 (`DirEntry::file_type()` の lstat 相当、`walkdir` の
/// `follow_links(false)`) で種別を得ること。
///
/// Windowsの隠し属性はここでは見ない (名前と種別だけで決まる部分ではないため)。
/// 属性の判定は `is_hidden_by_attribute` で、エントリを持っている呼び出し側が行う。
pub(super) fn is_visible_entry(name: &str, file_type: Option<FileType>) -> bool {
    if name.starts_with('.') {
        return false;
    }
    match file_type {
        Some(file_type) => !file_type.is_symlink(),
        None => false,
    }
}

/// Windowsの隠し属性が付いているか (→ 実機テスト 14)。
///
/// Windowsでは `AppData` のように名前がドットで始まらない隠しディレクトリがあり、
/// ドット始まりの除外だけでは一覧に出てしまう。
///
/// Unix向けにはこの関数自体を用意しない。隠しはドット始まりで表す慣習で、
/// `Metadata` を読むための追加のシステムコールを払う理由がないため。
#[cfg(windows)]
pub(super) fn is_hidden_by_attribute(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    /// `FILE_ATTRIBUTE_HIDDEN` (winnt.h)。この定数のためだけに windows-sys を
    /// 足さない。
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

    metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

/// `root` から `rel` までの途中と末尾に、一覧に出さないもの (ドット始まり・シンボリックリンク・
/// Windows の隠し属性) が含まれるか。配信の経路で、一覧と同じ規則を掛けるために使う
/// (→ docs/archive.md「スキャン」・docs/folders.md「公開できるフォルダ」)。`root` そのものは見ない (登録先を選んだのは設置者のため)。
///
/// `rel` は `Component::Normal` だけであること (→ `contents::reject_traversal_components`)。
/// リンクを辿らずに調べる (`symlink_metadata`)。調べられない階層は含まれるものとして扱う。
///
/// ブロッキングI/Oを行うため、呼び出し側は `spawn_blocking` の中で呼ぶこと。
pub(super) fn has_hidden_component(root: &FsPath, rel: &FsPath) -> bool {
    let mut current = root.to_path_buf();
    for component in rel.components() {
        current.push(component);
        let name = component.as_os_str().to_string_lossy();
        let Ok(metadata) = std::fs::symlink_metadata(&current) else {
            return true;
        };
        if !is_visible_entry(&name, Some(metadata.file_type())) {
            return true;
        }
        #[cfg(windows)]
        if is_hidden_by_attribute(&metadata) {
            return true;
        }
    }
    false
}

/// 実体パス `canonical` の、`canonical_root` から先の名前にドット始まりが含まれるか。
/// 両方とも canonicalize 済みで、`canonical` が `canonical_root` の中であること。
pub(super) fn has_dot_component(canonical_root: &FsPath, canonical: &FsPath) -> bool {
    canonical.strip_prefix(canonical_root).is_ok_and(|rel| {
        rel.components()
            .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
    })
}

/// `is_visible_entry` を `std::fs::DirEntry` に適用し、対象なら `(名前, 種別)` を返す。
/// Windowsでは隠し属性も見る。
pub(super) fn visible_entry(entry: &DirEntry) -> Option<(String, FileType)> {
    let name = entry.file_name().to_string_lossy().into_owned();
    let file_type = entry.file_type().ok();
    if !is_visible_entry(&name, file_type) {
        return None;
    }
    // 属性が読めないエントリは隠しでないものとして扱う。名前と種別で通った以上、
    // 一覧から消すより出して開けない方が分かりやすい。
    #[cfg(windows)]
    if entry
        .metadata()
        .is_ok_and(|metadata| is_hidden_by_attribute(&metadata))
    {
        return None;
    }
    Some((name, file_type?))
}

#[derive(Debug, Deserialize, IntoParams)]
pub(super) struct DirsQuery {
    /// 一覧を取得するディレクトリの絶対パス。省略時(空文字列)は登録済みの「公開できるフォルダ」の一覧。
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct DirEntryItem {
    name: String,
    /// このエントリの絶対パス。次の階層を開くときにそのまま `path` に渡す。
    /// ADR: 名前だけを返してクライアント側で連結させない。パス区切り文字と
    /// ドライブルートの扱いがOSごとに違い、フロントに分岐を持ち込むことになるため。
    path: String,
    selectable: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct DirsResponse {
    /// 正規化済みの現在位置。ルートの一覧では空文字列。
    pub path: String,
    /// 1つ上の階層。上位の一覧では `None`。登録済みの「公開できるフォルダ」そのものからは
    /// 空文字列 (= 上位の一覧) へ戻る。
    parent: Option<String>,
    /// パンくずの起点。現在位置を含む「公開できるフォルダ」。上位の一覧では `None`。
    /// パンくずを辿れるところから始めるために使う (→ docs/folders.md「一覧 API」)。
    root: Option<String>,
    /// `root` の名前。パンくずの先頭に、フルパスの代わりに出す (→ docs/folders.md「公開できるフォルダ」)。
    root_name: Option<String>,
    /// 現在位置そのものを登録先に選べるか。ルートの一覧では `false`。
    selectable: bool,
    entries: Vec<DirEntryItem>,
}

/// `path` が空のときの一覧。登録済みの「公開できるフォルダ」を、登録した名前で並べる (→ docs/folders.md「公開できるフォルダ」)。
/// `roots` は実体を辿れたものだけ (→ `roots::load_roots`)。
fn top_entries(roots: &[Root], own_dirs: &OwnDirs) -> Vec<DirEntryItem> {
    roots
        .iter()
        .filter_map(|root| {
            let path = path_to_string(root.path.clone()).ok()?;
            Some(DirEntryItem {
                name: root.name.clone(),
                path,
                selectable: super::roots::is_within_roots(roots, &root.path, own_dirs),
            })
        })
        .collect()
}

/// `dir` 直下のディレクトリを列挙する。ファイルは返さない
/// (それ以外の除外規則は `visible_entry` に集約。→ docs/archive.md「スキャン」)。
/// weblav 自身の置き場 (設定とデータ) は、選んでも中身が出ないので並べない (→ docs/folders.md「公開できるフォルダ」)。
fn read_dirs(
    dir: &FsPath,
    roots: &[Root],
    own_dirs: &OwnDirs,
) -> std::io::Result<Vec<DirEntryItem>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let Some((name, file_type)) = visible_entry(&entry) else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        // 一覧に出す名前・パスは辿ってきた `dir` からの見た目のままにし、
        // `selectable` の判定だけ実体パスに対して行う。canonicalize できない
        // (権限が無い等) エントリは、開こうとした時点でエラーになれば足りるので
        // 一覧からは消さず選べないものとして出す (→ docs/folders.md「一覧 API」)。
        let canonical = std::fs::canonicalize(&path).ok();
        if canonical
            .as_deref()
            .is_some_and(|canonical| own_dirs.contains(canonical))
        {
            continue;
        }
        let entry_selectable = canonical
            .map(|canonical| super::roots::is_within_roots(roots, &canonical, own_dirs))
            .unwrap_or(false);
        // 表示・次の階層の要求に使う値なので、`path_to_string` と同じ形に揃える。
        let Ok(path) = path_to_string(path) else {
            continue;
        };
        entries.push(DirEntryItem {
            name,
            path,
            selectable: entry_selectable,
        });
    }
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    Ok(entries)
}

/// 入力された絶対パスを canonicalize する。存在しない・アクセスできない・
/// ディレクトリでない・UTF-8として扱えない場合は422にする (存在を隠すための404にはしない)。
///
/// **理由を区別して返すので、admin かつサーバーと同じ端末からの経路でだけ使う**
/// (「公開できるフォルダ」の登録)。ログイン済みなら誰でも叩ける経路は
/// `canonical_dir_within_roots` を使う。
pub(super) fn canonical_dir(path: &str) -> Result<PathBuf, AppError> {
    let input = require_absolute(path)?;
    let canonical = std::fs::canonicalize(&input).map_err(|_| {
        AppError::Validation("path does not exist or is not accessible".to_string())
    })?;
    require_dir(&canonical)?;
    Ok(canonical)
}

/// canonicalize し、「公開できるフォルダ」の外なら422にする。
///
/// 選択UIを用意しても、APIを直接叩けば外のパスを登録できてしまう。歯止めは登録操作の
/// 側に置く (→ docs/folders.md「公開できるフォルダ」)。
///
/// **ルートの外は理由を1つに寄せる**。ログイン済みなら誰でも叩ける口なので
/// (→ docs/access.md「ロールと操作」)、「存在しない」「ディレクトリでない」の区別がルートの外にも
/// 及ぶと、任意の絶対パスの存在を確かめる道具になる。ルートの中は区別したままにする。
/// 選んだのがフォルダでなかった、という直せる誤りを伝えるため。
///
/// **判定の順は「canonicalize → ルートの内外 → ディレクトリか」で、canonicalize は1回だけ**。
/// 存在するパスと存在しないパスで触るI/Oの量が変わると、文言を寄せても処理時間の差から
/// 存在を読み取れる。絶対パスかどうかは入力の文字列だけで決まるので、先に区別して返してよい。
///
/// ブロッキングI/Oを行うため、呼び出し側は `spawn_blocking` の中で呼ぶこと。
pub(super) fn canonical_dir_within_roots(
    path: &str,
    roots: &[Root],
    own_dirs: &OwnDirs,
) -> Result<PathBuf, AppError> {
    let input = require_absolute(path)?;
    let Ok(canonical) = std::fs::canonicalize(&input) else {
        return Err(outside_roots());
    };
    if !super::roots::is_within_roots(roots, &canonical, own_dirs) {
        return Err(outside_roots());
    }
    require_dir(&canonical)?;
    Ok(canonical)
}

/// 絶対パスでなければ422。入力の文字列だけで決まり、ファイルシステムには触らない。
fn require_absolute(path: &str) -> Result<PathBuf, AppError> {
    let input = PathBuf::from(path);
    if !input.is_absolute() {
        return Err(AppError::Validation("path must be absolute".to_string()));
    }
    Ok(input)
}

/// ディレクトリでなければ422。
fn require_dir(canonical: &FsPath) -> Result<(), AppError> {
    if !canonical.is_dir() {
        return Err(AppError::Validation("path is not a directory".to_string()));
    }
    Ok(())
}

/// 「公開できるフォルダ」の外を指されたときの 422。理由を1つに寄せて、
/// ルートの外のことは何も教えない (→ docs/folders.md「公開できるフォルダ」)。
fn outside_roots() -> AppError {
    AppError::Validation("path is not inside a shared folder".to_string())
}

/// weblav 自身の置き場 (設定とデータ) の中を指されたときの 422 (→ docs/folders.md「公開できるフォルダ」)。
pub(super) fn inside_own_dirs() -> AppError {
    AppError::ValidationDetailed {
        message: "path is inside a weblav directory".to_string(),
        detail: super::error_detail::ValidationDetail::RootInsideOwnDirs,
    }
}

/// Windowsの `canonicalize` が返す verbatim パス (`\\?\C:\...`) を、人が読める形に戻す。
///
/// この形のまま画面に出すと、パンくず・登録確認・管理一覧のどこでも接頭辞が邪魔になり、
/// 折り返しも効かない (→ 実機テスト 5・20・23)。DBにもこの形で溜まる。
/// 保存した値は読み出すたびに `canonicalize` し直してから比較するので、接頭辞を
/// 落としても突き合わせは崩れない。
///
/// **接頭辞を落とせない場合はそのまま返す**。verbatim 形式でしか表現できないパス
/// (260文字を超えるもの・UNCのホスト名部分) を短くすると、開けないパスになる。
/// - `\\?\UNC\server\share` → `\\server\share` (長さの条件は同じ)
/// - それ以外は `\\?\` の直後がドライブ文字のときだけ落とす
///
/// Unixでは該当する接頭辞が付かないため、常に素通りする。
fn simplify_verbatim(path: String) -> String {
    /// レガシーAPIが扱えるパス長の上限。これを超えるパスは verbatim 形式でしか
    /// 開けないため、接頭辞を落とさない。
    const MAX_PATH: usize = 260;

    let simplified = if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        let is_drive_path =
            matches!(rest.as_bytes(), [drive, b':', b'\\', ..] if drive.is_ascii_alphabetic());
        if !is_drive_path {
            return path;
        }
        rest.to_string()
    } else {
        return path;
    };

    if simplified.len() > MAX_PATH {
        return path;
    }
    simplified
}

/// `PathBuf` を UTF-8 文字列にする。DBにもレスポンスにも文字列として載せるため、
/// 扱えない場合はここで422にする。Windowsの verbatim 接頭辞はここで落とす
/// (→ `simplify_verbatim`)。
pub(super) fn path_to_string(path: PathBuf) -> Result<String, AppError> {
    path.into_os_string()
        .into_string()
        .map(simplify_verbatim)
        .map_err(|_| AppError::Validation("path is not valid UTF-8".to_string()))
}

/// サーバー上のディレクトリ一覧。
#[utoipa::path(
    get,
    path = "/admin/fs/dirs",
    params(DirsQuery),
    responses(
        (status = OK, body = DirsResponse, description = "ディレクトリの一覧"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 422, body = crate::error::ErrorResponse, description = "パスが存在しない・ディレクトリでない・公開できるフォルダの外"),
    )
)]
async fn list_dirs(
    _user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<DirsQuery>,
) -> Result<Json<DirsResponse>, AppError> {
    let requested = query.path;
    let own_dirs = state.own_dirs.clone();
    // 判定のたびに読む (→ `roots::load_roots`)。`run_blocking` の中ではDBに触れない。
    let roots = super::roots::load_roots(&state.pool).await?;

    run_blocking(move || {
        Ok(Json(listing(
            &requested,
            &roots,
            &OwnDirs::resolve(&own_dirs),
        )?))
    })
    .await?
}

/// ディレクトリ1階層ぶんの一覧を組み立てる。登録済みの「公開できるフォルダ」の配下だけを辿る
/// (外は一覧そのものを見せない)。`roots` は canonicalize 済み (→ `roots::load_roots`)。
///
/// ブロッキングI/Oを行うため、呼び出し側は `spawn_blocking` の中で呼ぶこと。
fn listing(requested: &str, roots: &[Root], own_dirs: &OwnDirs) -> Result<DirsResponse, AppError> {
    if requested.is_empty() {
        return Ok(DirsResponse {
            path: String::new(),
            parent: None,
            root: None,
            root_name: None,
            selectable: false,
            entries: top_entries(roots, own_dirs),
        });
    }

    let canonical = canonical_dir_within_roots(requested, roots, own_dirs)?;
    let entries = read_dirs(&canonical, roots, own_dirs)
        .map_err(|_| AppError::Validation("cannot read this directory".to_string()))?;
    let anchor = super::roots::containing_root(roots, &canonical);
    // 上位の一覧に並ぶ位置 (公開できるフォルダそのもの) からは、上位の一覧 (`path: ""`) へ戻す。
    let parent = if anchor.map(|anchor| anchor.path.as_path()) == Some(canonical.as_path()) {
        Some(String::new())
    } else {
        match canonical.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                Some(path_to_string(parent.to_path_buf())?)
            }
            _ => Some(String::new()),
        }
    };

    Ok(DirsResponse {
        selectable: super::roots::is_within_roots(roots, &canonical, own_dirs),
        path: path_to_string(canonical)?,
        parent,
        root_name: anchor.map(|anchor| anchor.name.clone()),
        root: anchor
            .map(|anchor| path_to_string(anchor.path.clone()))
            .transpose()?,
        entries,
    })
}

#[derive(Debug, Deserialize, IntoParams)]
struct CountQuery {
    /// 数える対象のディレクトリの絶対パス。
    path: String,
    /// カンマ区切りの拡張子。指定するとその拡張子のファイルだけを数える。
    /// 省略時は全ファイル。先頭の `.` の有無・大文字小文字は問わない。
    extensions: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct CountResponse {
    /// 数え上げたファイル数。`truncated` が `true` なら下限値。
    count: usize,
    /// 上限を超えて走査を打ち切ったか。`true` なら実際のファイル数は `count` 以上ある。
    ///
    /// 打ち切りの理由は区別しない。件数の上限を超えた場合と、走査したエントリ数の
    /// 上限を超えた場合 (対象拡張子に一致しないファイルやディレクトリばかりのツリー)
    /// があり、後者では `count` が上限より小さいまま `true` になる。
    /// UI側の扱いはどちらも「これ以上ある」で同じなので、理由は分けない。
    truncated: bool,
    /// 最初に読めなかった場所の相対パス (登録先そのものなら空文字)。`count` はそこを除いた数。
    /// アーカイブは読めない場所があると再スキャンできないので (→ docs/archive.md「スキャン」)、登録の前に知らせる。
    unreadable: Option<String>,
}

/// カンマ区切りの拡張子指定を比較用の小文字リストに直す。空の要素は無視する。
///
/// 選択UIの件数表示とアーカイブの索引 (`scan_files`) の両方から呼ばれる。
/// 登録前に数えた件数と、実際に索引される件数を一致させるため規則を1箇所にまとめてある。
pub(super) fn parse_extensions(raw: Option<&str>) -> Option<Vec<String>> {
    let list: Vec<String> = raw?
        .split(',')
        .map(|ext| ext.trim().trim_start_matches('.').to_lowercase())
        .filter(|ext| !ext.is_empty())
        .collect();
    (!list.is_empty()).then_some(list)
}

pub(super) fn matches_extensions(path: &FsPath, extensions: Option<&[String]>) -> bool {
    let Some(extensions) = extensions else {
        return true;
    };
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            let ext = ext.to_lowercase();
            extensions.contains(&ext)
        })
}

/// アーカイブの相対パス。区切りを `/` に正規化して保存する
/// (→ docs/archive.md「スキャン」)。Windows と他プラットフォームで同じ値になるようにするため。
///
/// 純粋関数にしてあるのは、Windows 形式の入力を開発機(macOS)でも検査できるように
/// するため (Windows 実機が無く、固有の経路を実機で踏めない)。
fn normalize_rel_path(rel: &str) -> String {
    rel.replace('\\', "/")
}

/// 走査の結果。索引対象のファイルの相対パス (`/` 区切り) を集めたもの。
pub(super) struct ScanResult {
    pub(super) rel_paths: Vec<String>,
    /// 上限を超えて打ち切ったか。打ち切った場合、DBの同期は行わない
    /// (途中までの結果で同期すると、走査できなかった分が「消えた」と見なされて
    /// 公開フラグごと消える)。
    pub(super) truncated: bool,
    /// 最初に読めなかった場所の相対パス (`/` 区切り。登録先そのものなら空文字)。
    /// 読めなかった分は `rel_paths` から欠けるので、再スキャンはこれがあれば同期しない
    /// (欠けた分が「消えた」と見なされて公開フラグごと消えるため)。
    pub(super) unreadable: Option<String>,
}

/// `root` 配下を再帰的に走査し、索引対象のファイルを集める。
///
/// 再スキャン (`archive::rescan`) と登録前の確認 (`count_target_files`) の両方がこれを使う。
/// 数え方を分けて持つと、確認では打ち切られなかった件数で再スキャンだけが 422 になる。
///
/// ドット始まり・シンボリックリンクの除外は `is_visible_entry` と揃える
/// (→ docs/archive.md「スキャン」)。`follow_links(false)` は「リンク先のディレクトリへ
/// 再帰しない」だけで、リンクをエントリとして拾わない意味ではないため、
/// リンク自体の除外は明示的に行う。
///
/// 読み取れないディレクトリは読み飛ばし、最初の1か所を `unreadable` に残す。
/// 登録前の確認は欠けたまま数え、再スキャンは同期をやめる (→ docs/archive.md「スキャン」)。
///
/// `own_dirs` (→ `roots::OwnDirs`) の中は索引しない
/// (→ docs/folders.md「公開できるフォルダ」)。
pub(super) fn scan_files(
    root: &FsPath,
    extensions: Option<&[String]>,
    scan_limit: usize,
    item_limit: usize,
    own_dirs: &OwnDirs,
) -> ScanResult {
    let mut rel_paths = Vec::new();
    let mut scanned = 0;
    let mut unreadable = None;

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            // ルート自身は名前で判定しない。登録先そのものがドット始まりでも、
            // 選んだのは管理者なので索引の対象にする。
            if entry.depth() == 0 {
                return true;
            }
            let name = entry.file_name().to_string_lossy();
            if !is_visible_entry(&name, Some(entry.file_type())) {
                return false;
            }
            // リンクを辿らないので、根が実体パスならエントリのパスも実体パスのまま比べられる。
            if own_dirs.contains(entry.path()) {
                return false;
            }
            // Windowsの隠し属性 (理由は `is_hidden_by_attribute`)。
            #[cfg(windows)]
            if entry
                .metadata()
                .is_ok_and(|metadata| is_hidden_by_attribute(&metadata))
            {
                return false;
            }
            true
        });

    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            // 走査の途中で消えたものは、見つからなかったものと同じに扱う (同期で行が外れる)。
            // 登録先そのもの (深さ0) が消えたときは読み飛ばさない。空の結果で同期すると全件が消えるため。
            Err(error)
                if error.depth() > 0
                    && error
                        .io_error()
                        .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
            {
                continue;
            }
            Err(error) => {
                if unreadable.is_none() {
                    let rel = error
                        .path()
                        .and_then(|path| path.strip_prefix(root).ok())
                        .map(|rel| normalize_rel_path(&rel.to_string_lossy()))
                        .unwrap_or_default();
                    unreadable = Some(rel);
                }
                continue;
            }
        };
        // 登録先そのもの (depth 0) は、配下のエントリではないので数えない。
        if entry.depth() == 0 {
            continue;
        }
        scanned += 1;
        if scanned > scan_limit {
            return ScanResult {
                rel_paths,
                truncated: true,
                unreadable,
            };
        }
        if !entry.file_type().is_file() {
            continue;
        }
        if !matches_extensions(entry.path(), extensions) {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        // UTF-8 として扱えないパスは索引しない。DBにもレスポンスにも文字列として
        // 載せるため、ここで落とす方が後段で扱いを分けるより単純になる。
        let Some(rel) = rel.to_str() else {
            continue;
        };
        rel_paths.push(normalize_rel_path(rel));
        if rel_paths.len() > item_limit {
            return ScanResult {
                rel_paths,
                truncated: true,
                unreadable,
            };
        }
    }

    ScanResult {
        rel_paths,
        truncated: false,
        unreadable,
    }
}

/// 登録前の確認用に、対象になるファイル数を数える。
#[utoipa::path(
    get,
    path = "/admin/fs/count",
    params(CountQuery),
    responses(
        (status = OK, body = CountResponse, description = "対象ファイル数"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 422, body = crate::error::ErrorResponse, description = "パスが存在しない・ディレクトリでない・公開できるフォルダの外"),
    )
)]
async fn count_target_files(
    _user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<CountQuery>,
) -> Result<Json<CountResponse>, AppError> {
    let requested = query.path;
    let extensions = parse_extensions(query.extensions.as_deref());
    let own_dirs = state.own_dirs.clone();
    let roots = super::roots::load_roots(&state.pool).await?;

    run_blocking(move || {
        let own_dirs = OwnDirs::resolve(&own_dirs);
        let canonical = canonical_dir_within_roots(&requested, &roots, &own_dirs)?;
        let result = scan_files(
            &canonical,
            extensions.as_deref(),
            SCAN_LIMIT,
            ITEM_LIMIT,
            &own_dirs,
        );
        Ok(Json(CountResponse {
            count: result.rel_paths.len(),
            truncated: result.truncated,
            unreadable: result.unreadable,
        }))
    })
    .await?
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_dirs))
        .routes(routes!(count_target_files))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(path: &FsPath) -> Root {
        Root {
            name: "materials".to_string(),
            path: path.to_path_buf(),
        }
    }

    fn temp_dir(name: &str) -> crate::test_support::TempDir {
        crate::test_support::project_temp_dir("fs-tests", name)
    }

    /// 判定に関わらないデータ置き場。テスト対象のディレクトリの配下にならない場所に作る
    /// (置き場の中は「公開できるフォルダ」から外れるため)。
    /// `name` はテストごとに変える (同じパスを複数のテストが共有すると、片方の
    /// 後始末でもう片方が消える)。
    fn unrelated_data_dir(name: &str) -> crate::test_support::TempDir {
        crate::test_support::project_temp_dir("fs-tests-data", name)
    }

    /// 登録済みのフォルダの配下だけが通る。外は422。
    #[test]
    fn canonical_dir_within_roots_accepts_only_paths_under_a_root() {
        let data = unrelated_data_dir("within-roots");
        let own_dirs = OwnDirs::resolve(&[data.path().to_path_buf()]);
        let base = temp_dir("within-roots");
        let root = base.path().join("materials");
        let inside = root.join("2026");
        std::fs::create_dir_all(&inside).expect("ディレクトリを作れなかった");
        let outside = base.path().join("private");
        std::fs::create_dir_all(&outside).expect("ディレクトリを作れなかった");

        let roots = vec![test_root(&root)];
        let path = |dir: &std::path::Path| dir.to_string_lossy().into_owned();

        assert!(canonical_dir_within_roots(&path(&root), &roots, &own_dirs).is_ok());
        assert!(canonical_dir_within_roots(&path(&inside), &roots, &own_dirs).is_ok());
        assert!(matches!(
            canonical_dir_within_roots(&path(&outside), &roots, &own_dirs),
            Err(AppError::Validation(_))
        ));
        // 登録が0件なら、どこも選べない。
        assert!(matches!(
            canonical_dir_within_roots(&path(&root), &[], &own_dirs),
            Err(AppError::Validation(_))
        ));
    }

    /// ルートの外は、存在するファイル・存在しないパスのどちらも同じ文言になる。
    /// 中だけ「ディレクトリでない」を残す。
    #[test]
    fn canonical_dir_within_roots_hides_why_outside_a_root() {
        let data = unrelated_data_dir("within-roots-reason");
        let own_dirs = OwnDirs::resolve(&[data.path().to_path_buf()]);
        let base = temp_dir("within-roots-reason");
        let root = base.path().join("materials");
        std::fs::create_dir_all(&root).expect("ディレクトリを作れなかった");
        let file_inside = root.join("note.txt");
        std::fs::write(&file_inside, b"x").expect("ファイルを作れなかった");
        let file_outside = base.path().join("secret.txt");
        std::fs::write(&file_outside, b"x").expect("ファイルを作れなかった");
        let missing_outside = base.path().join("no-such-entry");
        let missing_inside = root.join("no-such-entry");

        let roots = vec![test_root(&root)];
        let path = |dir: &std::path::Path| dir.to_string_lossy().into_owned();
        let message = |target: &std::path::Path| match canonical_dir_within_roots(
            &path(target),
            &roots,
            &own_dirs,
        ) {
            Err(AppError::Validation(message)) => message,
            other => panic!("422を期待したが {other:?} だった"),
        };

        // 外は、実在するかどうかを問わず同じ文言。
        let outside_message = match outside_roots() {
            AppError::Validation(message) => message,
            other => panic!("422を期待したが {other:?} だった"),
        };
        assert_eq!(message(&file_outside), outside_message);
        assert_eq!(message(&missing_outside), outside_message);
        // 中でも、無いパスは外と同じ文言 (無いものは外か中かを決められない)。
        assert_eq!(message(&missing_inside), outside_message);

        // 中は、直せる誤りとして理由を残す。
        assert_eq!(message(&file_inside), "path is not a directory");

        // 相対パスは入力の文字列だけで決まるので、外の文言に寄せない。
        assert_ne!(
            match canonical_dir_within_roots("materials", &roots, &own_dirs) {
                Err(AppError::Validation(message)) => message,
                other => panic!("422を期待したが {other:?} だった"),
            },
            outside_message
        );
    }

    /// 登録済みのフォルダそのものからは、上位の一覧へ戻す (`parent` が空文字列)。
    #[test]
    fn listing_within_roots_returns_the_roots_at_the_top() {
        let data = unrelated_data_dir("listing-roots");
        let own_dirs = OwnDirs::resolve(&[data.path().to_path_buf()]);
        let base = temp_dir("listing-roots");
        let root = base.path().join("materials");
        std::fs::create_dir_all(root.join("2026")).expect("ディレクトリを作れなかった");
        let roots = vec![test_root(&root)];

        let top = listing("", &roots, &own_dirs).expect("上位の一覧を取れなかった");
        assert_eq!(top.parent, None);
        assert_eq!(top.entries.len(), 1);
        assert!(top.entries[0].selectable);
        // 名前はフルパスではなく、登録した名前。
        assert_eq!(top.entries[0].name, "materials");

        let at_root =
            listing(&root.to_string_lossy(), &roots, &own_dirs).expect("一覧を取れなかった");
        assert_eq!(at_root.parent.as_deref(), Some(""));
        assert!(at_root.selectable);
        assert_eq!(at_root.root, path_to_string(root.clone()).ok());

        // 配下では、パンくずを始める位置として登録済みのフォルダを返す。
        let inside = listing(&root.join("2026").to_string_lossy(), &roots, &own_dirs)
            .expect("一覧を取れなかった");
        assert_eq!(inside.root, path_to_string(root.clone()).ok());
        assert_eq!(inside.root_name.as_deref(), Some("materials"));
    }

    /// 登録済みのフォルダの外は、一覧そのものを見せない。
    #[test]
    fn listing_within_roots_rejects_a_path_outside() {
        let data = unrelated_data_dir("listing-outside");
        let own_dirs = OwnDirs::resolve(&[data.path().to_path_buf()]);
        let base = temp_dir("listing-outside");
        let root = base.path().join("materials");
        std::fs::create_dir_all(&root).expect("ディレクトリを作れなかった");
        let roots = vec![test_root(&root)];

        assert!(matches!(
            listing(&base.path().to_string_lossy(), &roots, &own_dirs),
            Err(AppError::Validation(_))
        ));
    }

    #[test]
    fn simplify_verbatim_strips_the_prefix_only_when_it_is_safe() {
        assert_eq!(
            simplify_verbatim(r"\\?\C:\Users\me\materials".to_string()),
            r"C:\Users\me\materials"
        );
        assert_eq!(
            simplify_verbatim(r"\\?\UNC\server\share\materials".to_string()),
            r"\\server\share\materials"
        );
        // ドライブ文字で始まらない verbatim パスは落とさない。
        assert_eq!(
            simplify_verbatim(r"\\?\Volume{...}\materials".to_string()),
            r"\\?\Volume{...}\materials"
        );
        // 落とすと260文字を超えるパスも、verbatim 形式でしか開けないのでそのまま。
        let long = format!(r"\\?\C:\{}", "a".repeat(300));
        assert_eq!(simplify_verbatim(long.clone()), long);
        // Unixのパスは素通り。
        assert_eq!(
            simplify_verbatim("/Users/me/materials".to_string()),
            "/Users/me/materials"
        );
    }

    #[test]
    fn parse_extensions_normalizes_dots_and_case() {
        assert_eq!(
            parse_extensions(Some(".MP3, pdf ,,")),
            Some(vec!["mp3".to_string(), "pdf".to_string()])
        );
        assert_eq!(parse_extensions(Some(" , ")), None);
        assert_eq!(parse_extensions(None), None);
    }

    #[test]
    fn normalize_rel_path_converts_windows_separators() {
        // Windows 経路は実機が無く検証できないため、区切り文字を
        // 手で組み立てた入力で検査する。
        assert_eq!(
            normalize_rel_path(r"2024\第1回\listening.mp3"),
            "2024/第1回/listening.mp3"
        );
        assert_eq!(
            normalize_rel_path("2024/第1回/listening.mp3"),
            "2024/第1回/listening.mp3"
        );
        assert_eq!(normalize_rel_path("listening.mp3"), "listening.mp3");
    }

    /// 起点から先の実体の名前だけを見る。起点そのもののドットは見ない。
    #[test]
    fn has_dot_component_looks_below_the_root_only() {
        let root = FsPath::new("/srv/.shared");
        assert!(has_dot_component(root, &root.join(".env")));
        assert!(has_dot_component(root, &root.join(".git").join("config")));
        assert!(!has_dot_component(root, &root.join("a.mp3")));
        assert!(!has_dot_component(root, root));
    }

    /// 登録先がデータ置き場の祖先でも、データ置き場の中は索引しない。
    #[test]
    fn scan_files_skips_the_data_dir() {
        let tmp = temp_dir("scan-data-dir");
        let dir = std::fs::canonicalize(tmp.path()).expect("canonicalize できなかった");
        let data_dir = dir.join("weblav");
        std::fs::create_dir_all(&data_dir).expect("データ置き場を作れなかった");
        std::fs::write(data_dir.join("weblav.db"), b"").expect("ファイルを作れなかった");
        std::fs::write(dir.join("a.mp3"), b"").expect("ファイルを作れなかった");

        let result = scan_files(
            &dir,
            None,
            SCAN_LIMIT,
            ITEM_LIMIT,
            &OwnDirs::resolve(&[data_dir]),
        );
        assert_eq!(result.rel_paths, vec!["a.mp3".to_string()]);
    }

    /// 権限で読めなくするので Unix だけ。Windows の ACL は手元で組めない。
    #[cfg(unix)]
    #[test]
    fn scan_files_reports_the_first_unreadable_directory() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = temp_dir("scan-unreadable");
        let dir = std::fs::canonicalize(tmp.path()).expect("canonicalize できなかった");
        std::fs::create_dir_all(dir.join("ok")).expect("ディレクトリを作れなかった");
        std::fs::create_dir_all(dir.join("locked")).expect("ディレクトリを作れなかった");
        std::fs::write(dir.join("ok/a.mp3"), b"").expect("ファイルを作れなかった");
        std::fs::write(dir.join("locked/b.mp3"), b"").expect("ファイルを作れなかった");
        let set_mode = |mode| {
            std::fs::set_permissions(dir.join("locked"), std::fs::Permissions::from_mode(mode))
                .expect("権限を変えられなかった");
        };
        set_mode(0o000);
        let result = scan_files(&dir, None, SCAN_LIMIT, ITEM_LIMIT, &OwnDirs::default());
        // 一時ディレクトリを片付けられるよう、確かめる前に戻す。
        set_mode(0o755);

        assert_eq!(result.rel_paths, vec!["ok/a.mp3".to_string()]);
        assert_eq!(result.unreadable.as_deref(), Some("locked"));
    }

    #[test]
    fn scan_files_skips_dotfiles_and_filters_by_extension() {
        fn count(
            dir: &FsPath,
            extensions: Option<&[String]>,
            scan_limit: usize,
            item_limit: usize,
        ) -> (usize, bool) {
            let result = scan_files(dir, extensions, scan_limit, item_limit, &OwnDirs::default());
            (result.rel_paths.len(), result.truncated)
        }

        let tmp = temp_dir("scan");
        let dir = tmp.path();
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).expect("サブディレクトリを作れなかった");
        std::fs::write(dir.join("a.mp3"), b"").expect("ファイルを作れなかった");
        std::fs::write(dir.join(".hidden.mp3"), b"").expect("ファイルを作れなかった");
        std::fs::write(sub.join("b.MP3"), b"").expect("ファイルを作れなかった");
        std::fs::write(sub.join("c.pdf"), b"").expect("ファイルを作れなかった");

        assert_eq!(count(dir, None, SCAN_LIMIT, ITEM_LIMIT), (3, false));
        // 対象拡張子が1つも無くても、走査したエントリ数で打ち切られる。
        let none = vec!["zip".to_string()];
        assert_eq!(count(dir, Some(&none), 2, ITEM_LIMIT), (0, true));
        // 走査したエントリ数には、登録先そのものと除外したもの (ドット始まり) を含めない。
        assert_eq!(count(dir, None, 4, ITEM_LIMIT), (3, false));
        // 一致するファイルが上限を超えたら打ち切る。ちょうど上限なら打ち切らない。
        assert_eq!(count(dir, None, SCAN_LIMIT, 2), (3, true));
        assert_eq!(count(dir, None, SCAN_LIMIT, 3), (3, false));
        let mp3 = vec!["mp3".to_string()];
        assert_eq!(count(dir, Some(&mp3), SCAN_LIMIT, ITEM_LIMIT), (2, false));
    }
}
