//! WebLAV のどこにも出さないもの (→ docs/folders.md「表示しないもの」)。
//!
//! 3段で決める。
//! - 固定: シンボリックリンク・WebLAV のデータと作業用のファイル。共有の外へ出られる・DB や鍵が漏れるため、変えさせない。
//! - サイトの設定: 名前のパターン (既定は `DEFAULT_SITE_PATTERNS`) と、Windows の隠し属性の切り替え。どの共有にも効く。
//! - 共有の除外: その共有の一番上から見るパターン。その場所を含むいちばん内側の共有のものを当てる。
//!
//! どれも存在を伏せる (一覧に出さず、パスを指定されても 404)。一覧・数え上げ・索引・検索・配信・共有の中の操作の
//! すべてがここの判定を通る。フォルダーのコンテンツの対象と除外のパターン (`file_filter`) は、その画面での見せ方で、別のもの。

use std::fs::FileType;
use std::path::{Path as FsPath, PathBuf};

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;
use walkdir::WalkDir;

use super::error_detail::ValidationDetail;
use super::file_filter::{FileFilter, lines, normalize_patterns};
use super::local::LocalRequest;
use super::operations::{FieldChange, Op, OpRecord, record_or_warn};
use super::roots::OwnDirs;
use crate::auth::{AdminUser, ConfirmedAdmin};
use crate::error::{AppError, AppJson, run_blocking};
use crate::folder_reach::{OTHERS_WAIT, Reach};
use crate::state::AppState;

/// サイト全体で表示しないものの既定。OS と NAS が勝手に作るもので、見せても利用者の役に立たないもの。
/// ドット始まりは、macOS の `.DS_Store` や設定のフォルダーなど、隠すつもりで付けた名前のため。
pub(super) const DEFAULT_SITE_PATTERNS: &str = ".*
@eaDir/
#recycle/
#snapshot/
@Recycle/
$RECYCLE.BIN/
System Volume Information/
lost+found/
Thumbs.db
desktop.ini";

/// WebLAV が作業中に置くファイルの名前の頭 (アップロード・コピー・上書きの途中)。サイトの設定によらず出さない。
/// ドット始まりを外す設定にしても、作りかけのものが見えないようにするため。
pub(super) const WORK_FILE_PREFIX: &str = ".weblav-";

/// サイトの設定のパターンを読む。名前に当てるので、`/` は末尾 (フォルダーにだけ当てる印) にしか書けない。
pub(super) fn site_filter(raw: Option<&str>) -> Result<FileFilter, AppError> {
    let raw = raw.unwrap_or(DEFAULT_SITE_PATTERNS);
    for pattern in lines(Some(raw)) {
        if pattern.trim_end_matches('/').contains('/') {
            return Err(AppError::ValidationDetailed {
                message: format!("a hidden pattern must be a name: {pattern}"),
                detail: ValidationDetail::HiddenPatternNotName {
                    pattern: pattern.to_string(),
                },
            });
        }
    }
    FileFilter::parse(None, Some(raw))
}

/// 共有の除外のパターンを読む。
pub(super) fn share_filter(raw: Option<&str>) -> Result<FileFilter, AppError> {
    FileFilter::parse(None, raw)
}

/// 判定の材料のうち、DB と共有の様子から読むもの。`run_blocking` の中で `resolve` して使う。
#[derive(Clone)]
pub(super) struct HiddenRules {
    own_dirs: Vec<PathBuf>,
    site: FileFilter,
    windows_hidden: bool,
    /// 登録中の共有の実体のパスと除外 (除外の無いものも持つ。サイトのパターンを共有の一番上から当てるため)。
    shares: Vec<(PathBuf, FileFilter)>,
}

impl HiddenRules {
    /// 今の設定を読む。共有は実体を辿れるものだけ (→ `roots::load_roots`)。辿れない共有の中は配らないので、
    /// その除外が無くても見えるものは増えない。
    pub(super) async fn load(state: &AppState) -> Result<Self, AppError> {
        let settings = sqlx::query!(
            r#"SELECT hidden_patterns, hide_windows_hidden as "hide_windows_hidden: bool"
               FROM site_settings WHERE id = 1"#
        )
        .fetch_one(&state.pool)
        .await?;
        let rows = sqlx::query!("SELECT path, exclude_patterns FROM roots WHERE deleted_at IS NULL")
        .fetch_all(&state.pool)
        .await?;
        let reaches =
            super::roots::check_all(state, rows.iter().map(|row| row.path.clone()).collect(), OTHERS_WAIT)
                .await;
        let mut shares = Vec::new();
        for (row, reach) in rows.into_iter().zip(reaches) {
            if let Reach::Ready(path) = reach {
                shares.push((path, share_filter(row.exclude_patterns.as_deref())?));
            }
        }
        Ok(Self {
            own_dirs: state.own_dirs.clone(),
            site: site_filter(settings.hidden_patterns.as_deref())?,
            windows_hidden: settings.hide_windows_hidden,
            shares,
        })
    }

    /// 判定に使える形にする。ブロッキングI/Oを行うため、`spawn_blocking` の中で呼ぶこと。
    pub(super) fn resolve(&self) -> Hidden {
        Hidden {
            own_dirs: OwnDirs::resolve(&self.own_dirs),
            site: self.site.clone(),
            windows_hidden: self.windows_hidden,
            shares: self.shares.clone(),
        }
    }
}

/// 表示しないものの判定。要求ごとに `HiddenRules::resolve` で1回だけ作って持ち回る。
///
/// パスはどれも実体のパス (canonicalize 済みか、canonicalize 済みの根からリンクを辿らずに得たもの) で渡す。
/// 共有のパスと前方一致で比べるため。
pub(super) struct Hidden {
    own_dirs: OwnDirs,
    site: FileFilter,
    // 属性を見るのは Windows だけ (→ `hides_by_attribute`)。
    #[cfg_attr(not(windows), allow(dead_code))]
    windows_hidden: bool,
    shares: Vec<(PathBuf, FileFilter)>,
}

impl Hidden {
    /// 保存してある綴りのまま判定する (ファイルシステムに触らない)。索引の行を、設定を変えた直後に外すのに使う。
    /// 索引には WebLAV の置き場が入らないので、置き場は持たない。
    fn from_stored(site: FileFilter, shares: Vec<(PathBuf, FileFilter)>) -> Self {
        Self {
            own_dirs: OwnDirs::default(),
            site,
            windows_hidden: false,
            shares,
        }
    }

    /// テスト用。サイトの設定は既定のまま、`shares` は除外の無い共有。
    #[cfg(test)]
    pub(super) fn for_test(own_dirs: &[PathBuf], shares: &[PathBuf]) -> Self {
        Self {
            own_dirs: OwnDirs::resolve(own_dirs),
            site: site_filter(None).expect("既定は正しいはず"),
            windows_hidden: true,
            shares: shares
                .iter()
                .map(|share| (share.clone(), FileFilter::default()))
                .collect(),
        }
    }

    pub(super) fn own_dirs(&self) -> &OwnDirs {
        &self.own_dirs
    }

    /// `path` (名前は `name`) を、名前と置き場所だけで隠すか。リンクかどうかと属性は見ない (呼び出し側が見る)。
    ///
    /// 共有そのものは、サイトのパターンにも外側の共有の除外にも当てない。選んだのは設置者のため。
    pub(super) fn hides_name(&self, path: &FsPath, name: &str, is_dir: bool) -> bool {
        if name.starts_with(WORK_FILE_PREFIX) || self.own_dirs.contains(path) {
            return true;
        }
        if self.shares.iter().any(|(share, _)| share == path) {
            return false;
        }
        let by_site = if is_dir {
            !self.site.allows_dir(name)
        } else {
            !self.site.allows_file(name)
        };
        by_site || self.hides_by_share(path, is_dir)
    }

    /// `path` を、それを含むいちばん内側の共有の除外で隠すか。共有そのものは隠さない。
    fn hides_by_share(&self, path: &FsPath, is_dir: bool) -> bool {
        // 走査ではエントリごとに呼ぶので、除外がどこにも無いときは共有を探さない。
        if self.shares.iter().all(|(_, filter)| filter.is_empty()) {
            return false;
        }
        let Some((share, filter)) = self
            .shares
            .iter()
            .filter(|(share, _)| path.starts_with(share) && path != share)
            .max_by_key(|(share, _)| share.components().count())
        else {
            return false;
        };
        let Some(rel) = rel_string(share, path) else {
            return false;
        };
        if filter.is_empty() {
            return false;
        }
        if is_dir {
            !filter.allows_dir(&rel)
        } else {
            !filter.allows_file(&rel)
        }
    }

    /// `path` の、それを含むいちばん内側の共有から下に、隠すものがあるか (名前と置き場所だけで見る)。
    /// `path` が共有の外なら、WebLAV の置き場でなければ `false` (共有の内外は呼び出し側が見る)。
    ///
    /// いちばん内側の共有から見るのは、外側の共有で隠した場所の中に設置者が共有を作ったなら、その共有は見せるため。
    pub(super) fn hides_path(&self, path: &FsPath, is_dir: bool) -> bool {
        if self.own_dirs.contains(path) {
            return true;
        }
        let Some((base, _)) = self
            .shares
            .iter()
            .filter(|(share, _)| path.starts_with(share))
            .max_by_key(|(share, _)| share.components().count())
        else {
            return false;
        };
        let Ok(rest) = path.strip_prefix(base) else {
            return false;
        };
        let count = rest.components().count();
        let mut current = base.clone();
        for (index, component) in rest.components().enumerate() {
            current.push(component);
            let name = component.as_os_str().to_string_lossy();
            if self.hides_name(&current, &name, index + 1 < count || is_dir) {
                return true;
            }
        }
        false
    }

    /// Windows の隠し属性で隠すか (→ `fs::is_hidden_by_attribute`)。サイトの設定で切れる。
    #[cfg(windows)]
    pub(super) fn hides_by_attribute(&self, metadata: &std::fs::Metadata) -> bool {
        self.windows_hidden && super::fs::is_hidden_by_attribute(metadata)
    }

    /// 一覧で `std::fs::DirEntry` を並べるか。並べるなら `(名前, 種別)`。
    /// 種別を取れないもの・シンボリックリンクは並べない (呼び出し側はリンクを辿らずに種別を得ること)。
    pub(super) fn visible_entry(&self, entry: &std::fs::DirEntry) -> Option<(String, FileType)> {
        let name = entry.file_name().to_string_lossy().into_owned();
        let file_type = entry.file_type().ok()?;
        if file_type.is_symlink() || self.hides_name(&entry.path(), &name, file_type.is_dir()) {
            return None;
        }
        // 属性が読めないエントリは隠しでないものとして扱う。名前と種別で通った以上、
        // 一覧から消すより出して開けない方が分かりやすい。
        #[cfg(windows)]
        if entry
            .metadata()
            .is_ok_and(|metadata| self.hides_by_attribute(&metadata))
        {
            return None;
        }
        Some((name, file_type))
    }

    /// 再帰の走査で辿るエントリか。根 (深さ 0) は見ない。選んだ場所そのものは、呼び出し側が確かめてある。
    pub(super) fn walkable(&self, entry: &walkdir::DirEntry) -> bool {
        if entry.depth() == 0 {
            return true;
        }
        let name = entry.file_name().to_string_lossy();
        let file_type = entry.file_type();
        if file_type.is_symlink() || self.hides_name(entry.path(), &name, file_type.is_dir()) {
            return false;
        }
        #[cfg(windows)]
        if entry
            .metadata()
            .is_ok_and(|metadata| self.hides_by_attribute(&metadata))
        {
            return false;
        }
        true
    }

    /// 起点 `root` (canonicalize 済み) から、要求の相対パス `rel` を辿った先 (実体は `canonical`) を隠すか。
    ///
    /// リンクと属性は要求のパスで見る (リンクを見分けるには辿る前のパスが要る)。起点そのものは見ない。
    /// 名前と置き場所は実体のパスで見る (`hides_path`)。Windows の 8.3 短縮名 (`ENV~1`) は、
    /// ドットで始まらないまま `.env` に解決されるため。調べられない階層は隠すものとして扱う。
    ///
    /// ブロッキングI/Oを行うため、呼び出し側は `spawn_blocking` の中で呼ぶこと。
    pub(super) fn hides_below(&self, root: &FsPath, rel: &FsPath, canonical: &FsPath) -> bool {
        let mut current = root.to_path_buf();
        for component in rel.components() {
            current.push(component);
            let Ok(metadata) = std::fs::symlink_metadata(&current) else {
                return true;
            };
            if metadata.file_type().is_symlink() {
                return true;
            }
            #[cfg(windows)]
            if self.hides_by_attribute(&metadata) {
                return true;
            }
        }
        self.hides_path(canonical, canonical.is_dir())
    }

    /// `path` の中に、共有の除外で隠したものがあるか。消す・動かす・名前を変える・写すのを断るのに使う
    /// (→ docs/shares.md「隠したものを含むもの」)。サイトのパターンと属性で隠したもの (`.DS_Store` など) は見ない。
    /// 名前で決まるので、どこへ動かしても隠れたままのため。
    ///
    /// ブロッキングI/Oを行うため、呼び出し側は `spawn_blocking` の中で呼ぶこと。
    pub(super) fn has_share_hidden_inside(&self, path: &FsPath) -> bool {
        // 中を辿るのは、除外が当たりうるときだけ。消すたびに大きなフォルダーを辿らないため。
        let may_apply = self.shares.iter().any(|(share, filter)| {
            !filter.is_empty() && (path.starts_with(share) || share.starts_with(path))
        });
        if !may_apply {
            return false;
        }
        // 読めない場所は、隠したものがあるとみなす。分からないまま消したり動かしたりしないため。
        WalkDir::new(path)
            .follow_links(false)
            .min_depth(1)
            .into_iter()
            .any(|entry| match entry {
                Ok(entry) => {
                    let file_type = entry.file_type();
                    !file_type.is_symlink()
                        && self.hides_by_share(entry.path(), file_type.is_dir())
                }
                Err(_) => true,
            })
    }

    /// `src` を `dest` (まだ無くてよい) へ動かす・写すと、中のものが共有の除外で隠れるか。
    /// 置き先の共有の除外が違うときや、`/` で始まる除外の当たり方が変わるときに起きる。
    /// 隠れたものは見えず、消すことも動かすこともできなくなるので、操作を断るのに使う。`src` そのものの名前は見ない
    /// (呼び出し側が `hides_path` で見る)。
    ///
    /// ブロッキングI/Oを行うため、呼び出し側は `spawn_blocking` の中で呼ぶこと。
    pub(super) fn hides_inside_at(&self, src: &FsPath, dest: &FsPath) -> bool {
        let may_apply = self.shares.iter().any(|(share, filter)| {
            !filter.is_empty() && (dest.starts_with(share) || share.starts_with(dest))
        });
        if !may_apply || !src.is_dir() {
            return false;
        }
        WalkDir::new(src)
            .follow_links(false)
            .min_depth(1)
            .into_iter()
            .any(|entry| match entry {
                Ok(entry) => {
                    let file_type = entry.file_type();
                    let Ok(rel) = entry.path().strip_prefix(src) else {
                        return true;
                    };
                    !file_type.is_symlink()
                        && self.hides_by_share(&dest.join(rel), file_type.is_dir())
                }
                Err(_) => true,
            })
    }
}

/// `base` から `path` までの相対パス ('/' 区切り)。UTF-8 でない文字は置き換えて照らす
/// (照らせないからといって除外を素通りさせないため)。
fn rel_string(base: &FsPath, path: &FsPath) -> Option<String> {
    let rel = path.strip_prefix(base).ok()?;
    Some(
        rel.components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct HiddenSettingsResponse {
    /// サイト全体で、名前で表示しないもののパターン (1行に1つ)。既定のままなら既定の値。
    patterns: String,
    /// `patterns` が既定のままか。
    is_default: bool,
    /// 既定のパターン。「既定に戻す」で入れる。
    default_patterns: String,
    /// Windows の隠し属性の付いたものを表示しないか。Windows で動いていなければ `None` (効かないので画面に出さない)。
    windows_hidden: Option<bool>,
}

async fn load_settings(
    executor: impl sqlx::SqliteExecutor<'_>,
) -> Result<HiddenSettingsResponse, AppError> {
    let row = sqlx::query!(
        r#"SELECT hidden_patterns, hide_windows_hidden as "hide_windows_hidden: bool"
           FROM site_settings WHERE id = 1"#
    )
    .fetch_one(executor)
    .await?;
    Ok(HiddenSettingsResponse {
        is_default: row.hidden_patterns.is_none(),
        patterns: row
            .hidden_patterns
            .unwrap_or_else(|| DEFAULT_SITE_PATTERNS.to_string()),
        default_patterns: DEFAULT_SITE_PATTERNS.to_string(),
        windows_hidden: cfg!(windows).then_some(row.hide_windows_hidden),
    })
}

/// 表示しないものの今の設定。LAN の端末の管理者も見られる (変えられるのはサーバーの PC の前だけ)。
#[utoipa::path(
    get,
    path = "/admin/hidden-settings",
    responses(
        (status = OK, body = HiddenSettingsResponse, description = "表示しないものの設定"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
    )
)]
async fn get_hidden_settings(
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<HiddenSettingsResponse>, AppError> {
    Ok(Json(load_settings(&state.pool).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct UpdateHiddenSettingsRequest {
    /// 1行に1つ。`null` か、既定と同じ内容なら既定のままにする。
    patterns: Option<String>,
    /// Windows の隠し属性の付いたものを表示しないか。省略したら今のまま。
    windows_hidden: Option<bool>,
}

/// 表示しないものを変える。外すと見える範囲が広がるので、共有の追加と同じくサーバーの PC の前の管理者だけ
/// (→ docs/folders.md「登録と削除」)。
#[utoipa::path(
    put,
    path = "/admin/hidden-settings",
    request_body = UpdateHiddenSettingsRequest,
    responses(
        (status = OK, body = HiddenSettingsResponse, description = "更新後の設定"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外・パスワードの確かめ直しが要る"),
        (status = 404, body = crate::error::ErrorResponse, description = "サーバーの PC の外から"),
        (status = 422, body = crate::error::ErrorResponse, description = "パターンが正しくない"),
    )
)]
async fn update_hidden_settings(
    _: LocalRequest,
    admin: ConfirmedAdmin,
    State(state): State<AppState>,
    AppJson(payload): AppJson<UpdateHiddenSettingsRequest>,
) -> Result<Json<HiddenSettingsResponse>, AppError> {
    let patterns = stored_patterns(payload.patterns.as_deref());
    site_filter(Some(patterns.as_deref().unwrap_or(DEFAULT_SITE_PATTERNS)))?;
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let before = load_settings(&mut *tx).await?;
    // 送られなかった切り替えは今のまま残す。Windows の外では画面に出さないため送られない。
    sqlx::query!(
        "UPDATE site_settings SET hidden_patterns = ?,
                hide_windows_hidden = COALESCE(?, hide_windows_hidden) WHERE id = 1",
        patterns,
        payload.windows_hidden
    )
    .execute(&mut *tx)
    .await?;
    let after = load_settings(&mut *tx).await?;
    tx.commit().await?;

    let mut fields = Vec::new();
    if before.patterns != after.patterns || before.is_default != after.is_default {
        fields.push(FieldChange::named("hiddenPatterns"));
    }
    if before.windows_hidden != after.windows_hidden {
        fields.push(FieldChange::changed(
            "windowsHidden",
            before.windows_hidden,
            after.windows_hidden,
        ));
    }
    if !fields.is_empty() {
        record_or_warn(
            &state.pool,
            &OpRecord::new(Op::UpdateSiteSettings, &admin).fields(fields),
        )
        .await;
        apply_to_indexes(&state, None).await;
    }
    Ok(Json(after))
}

/// 保存する形。前後の空白と空の行を落とし、既定と同じなら `None` (既定のまま)。
/// 既定のままにしておくと、版を上げて既定を足したときに届くため。空は空のまま残す (何も隠さない)。
fn stored_patterns(raw: Option<&str>) -> Option<String> {
    let raw = raw?;
    let normalized = normalize_patterns(Some(raw)).unwrap_or_default();
    (normalize_patterns(Some(DEFAULT_SITE_PATTERNS)).as_deref() != Some(normalized.as_str()))
        .then_some(normalized)
}

/// 表示しないものを変えた後に、一覧の表示の索引を合わせる (→ docs/archive.md「スキャン」)。
/// `share` があれば、その共有に関わるもの (中にあるか、中に含むもの) だけ。
///
/// 設定は保存し終えているので、失敗はログに出すだけにして、残りの索引も合わせる。
/// 合わせ損ねた索引も、次に一覧を開いたときの読み直しかスキャンで直る。
pub(super) async fn apply_to_indexes(state: &AppState, share: Option<&str>) {
    let archives = match sqlx::query!(
        r#"SELECT id as "id!", path as "path!" FROM contents
           WHERE type = 'folder' AND list_view = 1 AND path IS NOT NULL"#
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(archives) => archives,
        Err(err) => {
            tracing::warn!(error = %err, "listing indexes to apply hidden patterns failed");
            return;
        }
    };
    for archive in archives {
        let related = share.is_none_or(|share| {
            super::roots::contains(share, &archive.path)
                || super::roots::contains(&archive.path, share)
        });
        if !related {
            continue;
        }
        let Ok(guard) = state.archive_scans.acquire(archive.id) else {
            // スキャンの途中なら、終わってから合わせる。スキャンは始めたときの設定で読むため。
            let state = state.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(SCAN_RETRY_INTERVAL).await;
                    if let Ok(guard) = state.archive_scans.acquire(archive.id) {
                        if let Err(err) = apply_to_index(&state, archive.id, &archive.path, guard).await {
                            tracing::warn!(archive_id = archive.id, error = %crate::error::error_chain(&err), "applying hidden patterns to an index failed");
                        }
                        return;
                    }
                }
            });
            continue;
        };
        if let Err(err) = apply_to_index(state, archive.id, &archive.path, guard).await {
            tracing::warn!(archive_id = archive.id, error = %crate::error::error_chain(&err), "applying hidden patterns to an index failed");
        }
    }
}

/// スキャンの終わりを待つ間に、排他を取り直す間隔。待つのは設定を変えた後の1回だけなので、短くても負荷にならない。
const SCAN_RETRY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// 1つの一覧の表示の索引を、今の設定に合わせる。`_guard` は、その索引の排他。
///
/// 隠すことになった行はすぐに外す。次に一覧を開くまで、題が検索などに出続けないように。
/// 見えるようになったものは索引に無いので、フォルダーの更新日時を忘れさせ、次に一覧を開いたときの読み直しで足す。
/// 判定は保存してある綴りのまま行う (ファイルシステムに触らない。つながらない NAS でも外せるように)。
async fn apply_to_index(
    state: &AppState,
    id: i64,
    path: &str,
    _guard: crate::state::ArchiveScanGuard,
) -> Result<(), AppError> {
    let settings = sqlx::query_scalar!("SELECT hidden_patterns FROM site_settings WHERE id = 1")
        .fetch_one(&state.pool)
        .await?;
    let mut shares = Vec::new();
    for root in sqlx::query!("SELECT path, exclude_patterns FROM roots WHERE deleted_at IS NULL")
        .fetch_all(&state.pool)
        .await?
    {
        shares.push((
            PathBuf::from(&root.path),
            share_filter(root.exclude_patterns.as_deref())?,
        ));
    }
    let hidden = Hidden::from_stored(site_filter(settings.as_deref())?, shares);
    let items = sqlx::query!(
        "SELECT id, rel_path FROM archive_items WHERE archive_id = ?",
        id
    )
    .fetch_all(&state.pool)
    .await?;
    let dirs = sqlx::query_scalar!("SELECT rel_path FROM archive_dirs WHERE archive_id = ?", id)
        .fetch_all(&state.pool)
        .await?;
    let base = PathBuf::from(path);
    let (hidden_items, hidden_dirs) = run_blocking(move || {
        let under = |rel: &str| {
            let mut path = base.clone();
            path.extend(rel.split('/').filter(|part| !part.is_empty()));
            path
        };
        let items: Vec<i64> = items
            .iter()
            .filter(|item| hidden.hides_path(&under(&item.rel_path), false))
            .map(|item| item.id)
            .collect();
        let dirs: Vec<String> = dirs
            .into_iter()
            .filter(|rel| !rel.is_empty() && hidden.hides_path(&under(rel), true))
            .collect();
        (items, dirs)
    })
    .await?;

    let mut tx = crate::db::begin_write(&state.pool).await?;
    for item in hidden_items {
        sqlx::query!("DELETE FROM archive_items WHERE id = ?", item)
            .execute(&mut *tx)
            .await?;
    }
    for rel in hidden_dirs {
        sqlx::query!(
            "DELETE FROM archive_dirs WHERE archive_id = ? AND rel_path = ?",
            id,
            rel
        )
        .execute(&mut *tx)
        .await?;
    }
    sqlx::query!(
        "UPDATE archive_dirs SET modified_ns = NULL WHERE archive_id = ?",
        id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// 判定の材料を読んで、`run_blocking` の中で判定に使える形にして `f` に渡す。
pub(super) async fn with_hidden<T: Send + 'static>(
    state: &AppState,
    f: impl FnOnce(&Hidden) -> Result<T, AppError> + Send + 'static,
) -> Result<T, AppError> {
    let rules = HiddenRules::load(state).await?;
    run_blocking(move || f(&rules.resolve())).await?
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(get_hidden_settings))
        .routes(routes!(update_hidden_settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hidden(site: Option<&str>, shares: &[(&str, &str)]) -> Hidden {
        Hidden::from_stored(
            site_filter(site).expect("正しいパターンのはず"),
            shares
                .iter()
                .map(|(path, patterns)| {
                    (
                        PathBuf::from(path),
                        share_filter(Some(patterns)).expect("正しいパターンのはず"),
                    )
                })
                .collect(),
        )
    }

    #[test]
    fn the_default_hides_dot_names_and_files_that_nas_and_os_make() {
        let hidden = hidden(None, &[]);
        let path = FsPath::new("/share/x");
        assert!(hidden.hides_name(path, ".DS_Store", false));
        assert!(hidden.hides_name(path, ".git", true));
        assert!(hidden.hides_name(path, "@eaDir", true));
        assert!(hidden.hides_name(path, "thumbs.db", false), "大文字小文字を区別しないはず");
        assert!(!hidden.hides_name(path, "@eaDir", false), "フォルダーの印はファイルに当てないはず");
        assert!(!hidden.hides_name(path, "教材.pdf", false));
    }

    #[test]
    fn work_files_are_hidden_even_when_dot_names_are_shown() {
        let hidden = hidden(Some(""), &[]);
        let path = FsPath::new("/share/x");
        assert!(!hidden.hides_name(path, ".env", false));
        assert!(hidden.hides_name(path, ".weblav-upload-1.part", false));
    }

    #[test]
    fn a_site_pattern_must_be_a_name() {
        assert!(site_filter(Some("tmp/\n*.bak")).is_ok());
        let Err(AppError::ValidationDetailed { detail, .. }) = site_filter(Some("docs/private"))
        else {
            panic!("名前でないパターンは断るはず");
        };
        assert!(matches!(
            detail,
            ValidationDetail::HiddenPatternNotName { pattern } if pattern == "docs/private"
        ));
    }

    #[test]
    fn a_share_exclude_applies_below_its_share_only() {
        let hidden = hidden(Some(""), &[("/nas", "private/\n/top.txt")]);
        assert!(hidden.hides_path(FsPath::new("/nas/a/private"), true));
        assert!(hidden.hides_path(FsPath::new("/nas/a/private/b.jpg"), false));
        assert!(hidden.hides_path(FsPath::new("/nas/top.txt"), false));
        assert!(!hidden.hides_path(FsPath::new("/nas/a/top.txt"), false));
        assert!(!hidden.hides_path(FsPath::new("/other/private/b.jpg"), false));
    }

    /// 外側の共有で隠した場所の中に作った共有は、設置者が選んだものなので見せる。
    #[test]
    fn the_innermost_share_decides() {
        let hidden = hidden(Some(""), &[("/nas", "private/"), ("/nas/private/photos", "*.raw")]);
        assert!(!hidden.hides_path(FsPath::new("/nas/private/photos/a.jpg"), false));
        assert!(hidden.hides_path(FsPath::new("/nas/private/photos/a.raw"), false));
        assert!(hidden.hides_path(FsPath::new("/nas/private/memo.txt"), false));
    }

    #[test]
    fn a_share_itself_is_never_hidden_by_names() {
        let hidden = hidden(None, &[("/home/.media", "")]);
        assert!(!hidden.hides_path(FsPath::new("/home/.media"), true));
        assert!(!hidden.hides_path(FsPath::new("/home/.media/a.mp3"), false));
        assert!(hidden.hides_path(FsPath::new("/home/.media/.cache/a"), false));
    }

    #[test]
    fn saving_the_default_patterns_keeps_the_default() {
        assert_eq!(stored_patterns(None), None);
        assert_eq!(stored_patterns(Some(&format!(" {DEFAULT_SITE_PATTERNS}\n\n"))), None);
        assert_eq!(stored_patterns(Some("")), Some(String::new()));
        assert_eq!(stored_patterns(Some(" *.bak \n")), Some("*.bak".to_string()));
    }

    #[test]
    fn moving_into_a_share_with_other_excludes_is_seen() {
        let base = crate::test_support::project_temp_dir("hidden-tests", "moving");
        std::fs::create_dir_all(base.path().join("a/trip")).expect("フォルダーを作れなかった");
        std::fs::create_dir_all(base.path().join("b")).expect("フォルダーを作れなかった");
        std::fs::write(base.path().join("a/trip/x.raw"), b"").expect("ファイルを作れなかった");
        let root = std::fs::canonicalize(base.path()).expect("実体のパスを得られなかった");
        let site = site_filter(None).expect("既定は正しいはず");
        let hidden = Hidden::from_stored(
            site,
            vec![
                (root.join("a"), FileFilter::default()),
                (root.join("b"), share_filter(Some("*.raw")).expect("正しいはず")),
            ],
        );
        let src = root.join("a/trip");
        assert!(hidden.hides_inside_at(&src, &root.join("b/trip")));
        assert!(!hidden.hides_inside_at(&src, &root.join("a/trip2")));
    }

    #[test]
    fn only_share_excludes_count_as_hidden_inside() {
        let base = crate::test_support::project_temp_dir("hidden-tests", "inside");
        let share = base.path().join("share");
        std::fs::create_dir_all(share.join("album/private")).expect("フォルダーを作れなかった");
        std::fs::write(share.join("album/.DS_Store"), b"").expect("ファイルを作れなかった");
        let share = std::fs::canonicalize(&share).expect("実体のパスを得られなかった");
        let site = site_filter(None).expect("既定は正しいはず");

        let without = Hidden::from_stored(site.clone(), vec![]);
        assert!(!without.has_share_hidden_inside(&share.join("album")));

        let with = Hidden::from_stored(
            site,
            vec![(share.clone(), share_filter(Some("private/")).expect("正しいはず"))],
        );
        assert!(with.has_share_hidden_inside(&share.join("album")));
        assert!(!with.has_share_hidden_inside(&share.join("album/private")));
    }
}
