//! フォルダーの中のファイル・サブフォルダーの公開・非公開の印 (→ docs/folders.md「公開と非公開」)。
//!
//! 見せるかどうかはパスごとに1つだけ持ち、フォルダーの表示と一覧の表示の両方が同じ印を見る。
//! 実効の公開は、パスとその祖先のうち一番深い印で決まり、印が無ければフォルダーの
//! 既定に従う。ライブラリは、スキャンで初めて見つけたときの「新しく見つかったファイル」の設定
//! (`archive_items.initial_public`) を保ち、まだ見つけていないものは今の設定に従う。ライブラリでなければ公開。
//! 見せるファイルの絞り込み (→ `file_filter`) で外れるものは、印を付ける対象にもならない。
//! パスは登録先からの相対で、実体の綴りを '/' でつなぐ。照らす側も実体の綴りで照らす
//! (一覧は `read_dir` の名前、配信は canonicalize した実体のパスから求める)。

use std::collections::BTreeMap;
use std::path::Path as FsPath;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::contents::{self, ContentType, Visibility};
use super::file_filter::FileFilter;
use super::fs::{COUNT_LIMIT, walk_entries};
use crate::auth::AuthUser;
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

/// 1つのフォルダーの印と、印の無いものの扱い。
pub(super) struct Marks {
    default_public: bool,
    filter: FileFilter,
    marks: BTreeMap<String, bool>,
    /// 索引の行のうち、初めて見つけたときの公開が `default_public` と違うもの。
    /// 違うものだけを持つ。数十万件の索引でも、設定を変えていなければ空で済むため。
    initial: BTreeMap<String, bool>,
}

impl Marks {
    pub(super) async fn load(pool: &SqlitePool, content_id: i64) -> Result<Self, AppError> {
        let content = sqlx::query!(
            r#"SELECT list_view as "list_view: bool", new_files_public as "new_files_public: bool",
                      include_patterns, exclude_patterns
               FROM contents WHERE id = ?"#,
            content_id
        )
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
        let mut conn = pool.acquire().await?;
        Self::load_with(
            &mut conn,
            content_id,
            content.list_view,
            content.new_files_public,
            FileFilter::parse(
                content.include_patterns.as_deref(),
                content.exclude_patterns.as_deref(),
            )?,
        )
        .await
    }

    /// 今の印と索引を、与えた設定で読む。保存の前に、見える内容の変わり方を数えるためにも使う。
    async fn load_with(
        conn: &mut sqlx::SqliteConnection,
        content_id: i64,
        list_view: bool,
        new_files_public: bool,
        filter: FileFilter,
    ) -> Result<Self, AppError> {
        let rows = sqlx::query!(
            r#"SELECT path, is_dir as "is_dir: bool", public as "public: bool"
               FROM folder_marks WHERE content_id = ?"#,
            content_id
        )
        .fetch_all(&mut *conn)
        .await?;
        // ライブラリでないフォルダーは初期値を見ない (→ `default_public`)。
        let initial = if list_view {
            sqlx::query_scalar!(
                "SELECT rel_path FROM archive_items WHERE archive_id = ? AND initial_public != ?",
                content_id,
                new_files_public
            )
            .fetch_all(&mut *conn)
            .await?
        } else {
            Vec::new()
        };
        Self::build(
            default_public(list_view, new_files_public),
            filter,
            rows.into_iter()
                .map(|row| (row.path, row.is_dir, row.public)),
            initial.into_iter(),
        )
        .await
    }

    /// 印の表を組む。全部を切り替えたフォルダーでは印が件数ぶん (数十万) あるので、非同期のワーカーを塞がない。
    /// 絞り込みで外れるものの印は読まない。その上のフォルダーを辿れるようにしないため。
    /// 消さずに残すので、絞り込みを戻すと前の印に戻る。
    async fn build(
        default_public: bool,
        filter: FileFilter,
        rows: impl Iterator<Item = (String, bool, bool)> + Send + 'static,
        initial: impl Iterator<Item = String> + Send + 'static,
    ) -> Result<Self, AppError> {
        run_blocking(move || {
            let marks = rows
                .filter(|(path, is_dir, _)| {
                    if *is_dir {
                        filter.allows_dir(path)
                    } else {
                        filter.allows_file(path)
                    }
                })
                .map(|(path, _, public)| (path, public))
                .collect();
            let initial = initial
                .filter(|path| filter.allows_file(path))
                .map(|path| (path, !default_public))
                .collect();
            Self {
                default_public,
                filter,
                marks,
                initial,
            }
        })
        .await
    }

    /// `rel` ('/' 区切り。空なら登録先そのもの) が公開か。一番深い印で決まり、印が無ければ初期値に従う。
    pub(super) fn is_public(&self, rel: &str) -> bool {
        self.inherited(rel, true)
    }

    /// `rel` 自身の印を除いて、印を外したときの公開か。
    fn inherited(&self, rel: &str, include_self: bool) -> bool {
        if rel.is_empty() {
            return self.default_public;
        }
        let mut mark = None;
        for (index, _) in rel.match_indices('/') {
            if let Some(&found) = self.marks.get(&rel[..index]) {
                mark = Some(found);
            }
        }
        if include_self && let Some(&found) = self.marks.get(rel) {
            mark = Some(found);
        }
        mark.or_else(|| self.initial.get(rel).copied())
            .unwrap_or(self.default_public)
    }

    /// サブフォルダー `rel` を閲覧で辿れるか。それ自身が公開か、中に公開のものがあれば辿れる。
    /// 選んで公開するフォルダーで、公開したファイルの上のフォルダーを辿れるようにするため。
    /// 登録先そのものは、中に公開したものが無くても開ける。ホームなどの入口に載っていて存在は
    /// 伏せられないので、開けないエラーにせず、見せるものが無いと伝える。
    pub(super) fn dir_reachable(&self, rel: &str) -> bool {
        if rel.is_empty() {
            return true;
        }
        if !self.filter.allows_dir(rel) {
            return false;
        }
        if self.is_public(rel) {
            return true;
        }
        let prefix = format!("{rel}/");
        let under = |map: &'_ BTreeMap<String, bool>| {
            map.range(prefix.clone()..)
                .take_while(|(path, _)| path.starts_with(&prefix))
                .any(|(path, &public)| public && self.is_public(path))
        };
        under(&self.marks) || under(&self.initial)
    }

    /// 閲覧する人にファイル `rel` が見えるか。
    pub(super) fn file_visible(&self, rel: &str) -> bool {
        self.in_scope(rel) && self.is_public(rel)
    }

    /// ファイル `rel` が絞り込みの対象か。外れるものは編集できる人にも出さず、印も付けない。
    pub(super) fn in_scope(&self, rel: &str) -> bool {
        self.filter.allows_file(rel)
    }

    /// フォルダー `rel` が絞り込みで除外されていないか。
    pub(super) fn dir_in_scope(&self, rel: &str) -> bool {
        self.filter.allows_dir(rel)
    }
}

/// 印の無いものの公開。ライブラリでないフォルダーは、置いたものがそのまま見える
/// (選んで見せる設定を持たない。→ docs/folders.md「公開と非公開」)。
fn default_public(list_view: bool, new_files_public: bool) -> bool {
    !list_view || new_files_public
}

/// `root` (登録先の実体のパス) の下にある実体のパス `path` の、登録先からの相対パス ('/' 区切り)。
pub(super) fn rel_of(root: &FsPath, path: &FsPath) -> Result<String, AppError> {
    let rel = path.strip_prefix(root).map_err(|_| AppError::NotFound)?;
    Ok(rel
        .components()
        .map(|component| component.as_os_str().to_str().map(str::to_string))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| AppError::Validation("path is not valid UTF-8".to_string()))?
        .join("/"))
}

/// 印を付け直す。`targets` はどれも登録先からの相対パスと、フォルダーか。
///
/// 印を外したときと同じになる印は消す。印を、利用者が決めて変えたものだけにしておくため。
/// フォルダーに付けるときは、中のものの印も消す。フォルダーを非公開にしたのに、中に前に公開した
/// ものが残って見える、ということを起こさないため。
pub(super) async fn set_marks(
    pool: &SqlitePool,
    content_id: i64,
    targets: Vec<(String, bool)>,
    public: bool,
) -> Result<(), AppError> {
    let mut tx = crate::db::begin_write(pool).await?;
    for (path, is_dir) in &targets {
        if *is_dir {
            // LIKE は ASCII の大文字・小文字を区別しないので、頭の一致は substr で照らす。
            // 大文字・小文字を区別するファイルシステムで、綴りの違う隣のフォルダーの印まで消さないため。
            let prefix = format!("{path}/");
            sqlx::query!(
                "DELETE FROM folder_marks WHERE content_id = ?1 AND substr(path, 1, length(?2)) = ?2",
                content_id,
                prefix
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    // 祖先の印は、ここで付け直すものの中にあることもあるので、消した後で読み直す。
    let marks = load_in(&mut tx, content_id).await?;
    // まとめて書く。一覧の「すべて選択」では数十万件になり、1件ずつ書くと書き込みを10秒近く塞ぐため
    // (→ docs/archive.md「まとめて公開を切り替える」)。パスの列は JSON にして `json_each` で展開する。
    // 振り分けと JSON にするのは件数に比例するので、非同期のワーカーを塞がない。
    let (removed, files, dirs) = run_blocking(move || {
        let mut removed: Vec<&str> = Vec::new();
        let mut files: Vec<&str> = Vec::new();
        let mut dirs: Vec<&str> = Vec::new();
        for (path, is_dir) in &targets {
            if marks.inherited(path, false) == public {
                removed.push(path);
            } else if *is_dir {
                dirs.push(path);
            } else {
                files.push(path);
            }
        }
        let json = |paths: Vec<&str>| {
            (!paths.is_empty())
                .then(|| serde_json::to_string(&paths).expect("文字列の列は JSON にできる"))
        };
        (json(removed), json(files), json(dirs))
    })
    .await?;
    if let Some(paths) = removed {
        sqlx::query!(
            "DELETE FROM folder_marks WHERE content_id = ? AND path IN (SELECT value FROM json_each(?))",
            content_id,
            paths
        )
        .execute(&mut *tx)
        .await?;
    }
    for (paths, is_dir) in [(files, false), (dirs, true)] {
        let Some(paths) = paths else {
            continue;
        };
        // `WHERE true` は、SELECT から入れる INSERT の ON CONFLICT を SQLite が結合の一部と読まないための決まり。
        sqlx::query!(
            r#"INSERT INTO folder_marks (content_id, path, is_dir, public, created_at)
               SELECT ?, value, ?, ?, unixepoch() FROM json_each(?) WHERE true
               ON CONFLICT (content_id, path) DO UPDATE
               SET is_dir = excluded.is_dir, public = excluded.public, created_at = excluded.created_at"#,
            content_id,
            is_dir,
            public,
            paths
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn load_in(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    content_id: i64,
) -> Result<Marks, AppError> {
    let content = sqlx::query!(
        r#"SELECT list_view as "list_view: bool", new_files_public as "new_files_public: bool"
           FROM contents WHERE id = ?"#,
        content_id
    )
    .fetch_one(&mut **tx)
    .await?;
    // ここで見るのは祖先のフォルダーの印と行の初期値だけなので、絞り込みは当てない。
    Marks::load_with(
        tx,
        content_id,
        content.list_view,
        content.new_files_public,
        FileFilter::default(),
    )
    .await
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FolderMark {
    id: i64,
    /// フォルダーの登録先からの相対パス ('/' 区切り)。
    path: String,
    is_dir: bool,
    public: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct SetFolderMarkRequest {
    /// 閲覧の画面と同じ、フォルダーの登録先からの相対パス。
    path: String,
    public: bool,
}

/// `user` が書き換えられるフォルダーの登録先。フォルダーでなければ 404、書き換えられなければ 403 か 404。
async fn editable_folder(pool: &SqlitePool, user: &AuthUser, id: i64) -> Result<String, AppError> {
    let row = sqlx::query!(
        r#"SELECT path, visibility as "visibility: Visibility", created_by
           FROM contents WHERE id = ? AND type = ?"#,
        id,
        ContentType::Folder
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    contents::ensure_editable(user, row.visibility, row.created_by)?;
    contents::folder_path(row.path)
}

/// 公開・非公開の印の一覧。編集ページで、新しく見つかったファイルの扱いと違うものを並べて取り消すため。
#[utoipa::path(
    get,
    path = "/contents/{id}/marks",
    params(("id" = i64, Path)),
    responses(
        (status = OK, body = Vec<FolderMark>, description = "印を付けた順の新しいものから"),
        (status = 403, body = crate::error::ErrorResponse, description = "書き換えられるのは管理者と作成者だけ"),
        (status = 404, body = crate::error::ErrorResponse, description = "見えない・存在しない、またはフォルダーでない"),
    )
)]
async fn list_marks(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<FolderMark>>, AppError> {
    editable_folder(&state.pool, &user, id).await?;
    let (_, marks) = super::archive_index::marks(&state, id).await?;
    let mut rows = sqlx::query_as!(
        FolderMark,
        r#"SELECT id as "id!", path, is_dir as "is_dir: bool", public as "public: bool"
           FROM folder_marks WHERE content_id = ? ORDER BY created_at DESC, id DESC"#,
        id
    )
    .fetch_all(&state.pool)
    .await?;
    rows.retain(|row| {
        if row.is_dir {
            marks.dir_in_scope(&row.path)
        } else {
            marks.in_scope(&row.path)
        }
    });
    Ok(Json(rows))
}

/// フォルダーの中のファイルかサブフォルダーを、公開か非公開にする。サブフォルダーなら中ごと。
#[utoipa::path(
    put,
    path = "/contents/{id}/marks",
    params(("id" = i64, Path)),
    request_body = SetFolderMarkRequest,
    responses(
        (status = NO_CONTENT),
        (status = 403, body = crate::error::ErrorResponse, description = "書き換えられるのは管理者と作成者だけ"),
        (status = 404, body = crate::error::ErrorResponse, description = "見えない・存在しない、フォルダーでない、またはフォルダーの中に無いパス"),
        (status = 503, body = crate::error::ErrorResponse, description = "フォルダーのある NAS やディスクにつながらない"),
    )
)]
async fn set_mark(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<SetFolderMarkRequest>,
) -> Result<StatusCode, AppError> {
    let root = editable_folder(&state.pool, &user, id).await?;
    // 登録先そのものには付けない (全体は「新しく見つかったファイル」の設定と公開範囲で決める)。
    if payload.path.is_empty() {
        return Err(AppError::NotFound);
    }
    // 書き換えられる人は非公開のものも開けるので、印で外さずに解く (`None`)。
    let target = contents::resolve_path(&state, &root, &payload.path, None).await?;
    let (path, is_dir) = run_blocking(move || {
        let root = std::fs::canonicalize(&root).map_err(|_| AppError::NotFound)?;
        Ok::<_, AppError>((rel_of(&root, &target)?, target.is_dir()))
    })
    .await??;
    let (_, marks) = super::archive_index::marks(&state, id).await?;
    let in_scope = if is_dir {
        marks.dir_in_scope(&path)
    } else {
        marks.in_scope(&path)
    };
    if !in_scope {
        return Err(AppError::NotFound);
    }
    set_marks(&state.pool, id, vec![(path, is_dir)], payload.public).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct VisibilityPreviewRequest {
    list_view: bool,
    new_files_public: bool,
    include_patterns: Option<String>,
    exclude_patterns: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct VisibilityPreview {
    /// 今は見えず、保存すると見えるようになるファイルの数。
    gained: usize,
    /// 今は見えて、保存すると見えなくなるファイルの数。
    lost: usize,
    /// ファイルが多く、一部だけを数えたか。
    truncated: bool,
}

/// 見る人に見えるファイルの増減を数える。`files` は登録先からの相対パス。
fn count_visibility_changes<'a>(
    files: impl Iterator<Item = &'a str>,
    before: &Marks,
    after: &Marks,
) -> (usize, usize) {
    let (mut gained, mut lost) = (0, 0);
    for rel in files {
        match (before.file_visible(rel), after.file_visible(rel)) {
            (false, true) => gained += 1,
            (true, false) => lost += 1,
            _ => {}
        }
    }
    (gained, lost)
}

/// 設定を保存すると、見る人に見えるファイルがどう変わるかを数える。保存の前に知らせるため。
#[utoipa::path(
    post,
    path = "/contents/{id}/marks/preview",
    params(("id" = i64, Path)),
    request_body = VisibilityPreviewRequest,
    responses(
        (status = OK, body = VisibilityPreview),
        (status = 403, body = crate::error::ErrorResponse, description = "書き換えられるのは管理者と作成者だけ"),
        (status = 404, body = crate::error::ErrorResponse, description = "見えない・存在しない、またはフォルダーでない"),
        (status = 422, body = crate::error::ErrorResponse, description = "パターンが正しくない"),
        (status = 503, body = crate::error::ErrorResponse, description = "フォルダーのある NAS やディスクにつながらない"),
    )
)]
async fn preview_visibility(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<VisibilityPreviewRequest>,
) -> Result<Json<VisibilityPreview>, AppError> {
    let root = editable_folder(&state.pool, &user, id).await?;
    super::roots::ensure_reachable(&state, &root).await?;
    let (_, before) = super::archive_index::marks(&state, id).await?;
    let filter = FileFilter::parse(
        payload.include_patterns.as_deref(),
        payload.exclude_patterns.as_deref(),
    )?;
    let mut conn = state.pool.acquire().await?;
    let after = Marks::load_with(
        &mut conn,
        id,
        payload.list_view,
        payload.new_files_public,
        filter,
    )
    .await?;
    drop(conn);
    let own_dirs = state.own_dirs.clone();
    let preview = run_blocking(move || {
        let own_dirs = super::roots::OwnDirs::resolve(&own_dirs);
        let root = std::fs::canonicalize(&root).map_err(|_| AppError::NotFound)?;
        let (entries, truncated) = walk_entries(&root, COUNT_LIMIT, &own_dirs);
        let files = entries
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| entry.rel_path.as_str());
        let (gained, lost) = count_visibility_changes(files, &before, &after);
        Ok::<_, AppError>(VisibilityPreview {
            gained,
            lost,
            truncated,
        })
    })
    .await??;
    Ok(Json(preview))
}

/// 印を取り消し、祖先か既定に従わせる。
#[utoipa::path(
    delete,
    path = "/contents/{id}/marks/{mark_id}",
    params(("id" = i64, Path), ("mark_id" = i64, Path)),
    responses(
        (status = 204),
        (status = 403, body = crate::error::ErrorResponse, description = "書き換えられるのは管理者と作成者だけ"),
        (status = 404, body = crate::error::ErrorResponse, description = "見えない・存在しない、またはこのフォルダーの印でない"),
    )
)]
async fn delete_mark(
    user: AuthUser,
    State(state): State<AppState>,
    Path((id, mark_id)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    editable_folder(&state.pool, &user, id).await?;
    let deleted = sqlx::query!(
        "DELETE FROM folder_marks WHERE id = ? AND content_id = ?",
        mark_id,
        id
    )
    .execute(&state.pool)
    .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_marks, set_mark))
        .routes(routes!(preview_visibility))
        .routes(routes!(delete_mark))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(default_public: bool, marks: &[(&str, bool)]) -> Marks {
        Marks {
            default_public,
            filter: FileFilter::default(),
            initial: BTreeMap::new(),
            marks: marks
                .iter()
                .map(|(path, public)| ((*path).to_string(), *public))
                .collect(),
        }
    }

    #[test]
    fn deepest_mark_decides_whether_a_path_is_public() {
        let marks = marks(true, &[("a", false), ("a/b", true), ("a/b/c.jpg", false)]);
        assert!(marks.is_public("x.jpg"), "印が無ければ既定に従うはず");
        assert!(
            !marks.is_public("a/x.jpg"),
            "非公開のフォルダーの中は非公開のはず"
        );
        assert!(marks.is_public("a/b/x.jpg"), "より深い公開の印が勝つはず");
        assert!(!marks.is_public("a/b/c.jpg"), "ファイル自身の印が勝つはず");
    }

    #[test]
    fn mark_does_not_cover_a_sibling_sharing_its_name_prefix() {
        let marks = marks(true, &[("a", false)]);
        assert!(
            marks.is_public("ab/x.jpg"),
            "名前の頭が同じ別のフォルダーには掛からないはず"
        );
    }

    #[test]
    fn folder_that_is_not_a_library_shows_unmarked_files() {
        assert!(default_public(false, false));
        assert!(!default_public(true, false));
        assert!(default_public(true, true));
    }

    #[test]
    fn visibility_changes_count_files_in_both_directions() {
        let before = Marks {
            filter: FileFilter::parse(Some("*.pdf"), None).expect("正しいパターンのはず"),
            ..marks(false, &[("a.pdf", true)])
        };
        let after = marks(true, &[("hidden.pdf", false)]);
        let files = ["a.pdf", "b.pdf", "c.txt", "hidden.pdf"];
        // b.pdf (既定で公開) と c.txt (拡張子を外した) が増える。a.pdf の印は after に無いが既定で見える。
        assert_eq!(
            count_visibility_changes(files.into_iter(), &before, &after),
            (2, 0)
        );
        assert_eq!(
            count_visibility_changes(files.into_iter(), &after, &before),
            (0, 2)
        );
    }

    #[test]
    fn unmarked_file_keeps_the_value_from_when_it_was_found() {
        // 見つけた後に設定を公開へ変えたフォルダー。old.jpg は非公開で見つけた。
        let marks = Marks {
            initial: BTreeMap::from([("dir/old.jpg".to_string(), false)]),
            ..marks(true, &[("dir/shown.jpg", true)])
        };
        assert!(!marks.is_public("dir/old.jpg"));
        assert!(marks.is_public("dir/not-scanned.jpg"));
    }

    #[test]
    fn folder_with_a_file_found_public_is_reachable_unless_marked_private() {
        // 見つけた後に設定を非公開へ変えたフォルダー。a/old.jpg と b/old.jpg は公開で見つけた。
        let marks = Marks {
            initial: BTreeMap::from([
                ("a/old.jpg".to_string(), true),
                ("b/old.jpg".to_string(), true),
            ]),
            ..marks(false, &[("b", false)])
        };
        assert!(marks.dir_reachable("a"));
        assert!(!marks.dir_reachable("b"));
    }

    #[test]
    fn folder_root_is_reachable_when_nothing_is_public() {
        assert!(marks(false, &[]).dir_reachable(""));
    }

    #[test]
    fn private_folder_is_reachable_when_something_inside_is_public() {
        let marks = marks(false, &[("2024/運動会/001.jpg", true)]);
        assert!(
            marks.dir_reachable("2024"),
            "公開したファイルの上のフォルダーは辿れるはず"
        );
        assert!(
            marks.dir_reachable("2024/運動会"),
            "公開したファイルの親は辿れるはず"
        );
        assert!(
            !marks.dir_reachable("2023"),
            "公開したものが無いフォルダーは辿れないはず"
        );
        assert!(
            !marks.is_public("2024/運動会/002.jpg"),
            "印の無い隣のファイルは非公開のはず"
        );
    }

    #[sqlx::test]
    async fn marking_a_folder_keeps_marks_of_a_sibling_spelled_in_other_case(pool: SqlitePool) {
        let content_id = sqlx::query_scalar!(
            "INSERT INTO contents (type, title, path, visibility, list_view, new_files_public) VALUES ('folder', 'f', '/f', 'public', 1, 0) RETURNING id"
        )
        .fetch_one(&pool)
        .await
        .expect("フォルダーを作れるはず");
        set_marks(
            &pool,
            content_id,
            vec![("Photos/a_1.jpg".to_string(), false)],
            true,
        )
        .await
        .expect("ファイルに印を付けられるはず");
        set_marks(&pool, content_id, vec![("photos".to_string(), true)], false)
            .await
            .expect("フォルダーに印を付けられるはず");
        let marks = Marks::load(&pool, content_id)
            .await
            .expect("印を読めるはず");
        assert!(
            marks.marks.contains_key("Photos/a_1.jpg"),
            "綴りの違う隣のフォルダーの印は残るはず"
        );
    }
}
