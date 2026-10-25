//! フォルダーの中のファイル・サブフォルダーの公開・非公開の印 (→ docs/folders.md「公開と非公開」)。
//!
//! 見せるかどうかはパスごとに1つだけ持ち、フォルダーの表示と一覧の表示の両方が同じ印を見る。
//! 実効の公開は、パスとその祖先のうち一番深い印で決まり、印が無ければフォルダーの
//! 「新しく見つかったファイル」の設定 (`contents.new_files_public`) に従う。
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
use crate::auth::AuthUser;
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

/// 1つのフォルダーの印と、印の無いものの扱い。
pub(super) struct Marks {
    default_public: bool,
    marks: BTreeMap<String, bool>,
}

impl Marks {
    pub(super) async fn load(pool: &SqlitePool, content_id: i64) -> Result<Self, AppError> {
        let default_public = sqlx::query_scalar!(
            r#"SELECT new_files_public as "new_files_public: bool" FROM contents WHERE id = ?"#,
            content_id
        )
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
        let rows = sqlx::query!(
            r#"SELECT path, public as "public: bool" FROM folder_marks WHERE content_id = ?"#,
            content_id
        )
        .fetch_all(pool)
        .await?;
        Self::build(
            default_public,
            rows.into_iter().map(|row| (row.path, row.public)),
        )
        .await
    }

    /// 印の表を組む。全部を切り替えたフォルダーでは印が件数ぶん (数十万) あるので、非同期のワーカーを塞がない。
    async fn build(
        default_public: bool,
        rows: impl Iterator<Item = (String, bool)> + Send + 'static,
    ) -> Result<Self, AppError> {
        run_blocking(move || Self {
            default_public,
            marks: rows.collect(),
        })
        .await
    }

    /// `rel` ('/' 区切り。空なら登録先そのもの) が公開か。一番深い印で決まる。
    pub(super) fn is_public(&self, rel: &str) -> bool {
        self.inherited(rel, true)
    }

    /// `rel` 自身の印を除いて、祖先から受け継ぐ公開か。
    fn inherited(&self, rel: &str, include_self: bool) -> bool {
        let mut public = self.default_public;
        if rel.is_empty() {
            return public;
        }
        for (index, _) in rel.match_indices('/') {
            if let Some(&mark) = self.marks.get(&rel[..index]) {
                public = mark;
            }
        }
        if include_self && let Some(&mark) = self.marks.get(rel) {
            public = mark;
        }
        public
    }

    /// サブフォルダー `rel` を閲覧で辿れるか。それ自身が公開か、中に公開のものがあれば辿れる。
    /// 選んで公開するフォルダーで、公開したファイルの上のフォルダーを辿れるようにするため。
    pub(super) fn dir_reachable(&self, rel: &str) -> bool {
        if self.is_public(rel) {
            return true;
        }
        let prefix = if rel.is_empty() {
            String::new()
        } else {
            format!("{rel}/")
        };
        self.marks
            .range(prefix.clone()..)
            .take_while(|(path, _)| path.starts_with(&prefix))
            .any(|(_, &public)| public)
    }
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
/// 祖先から受け継ぐのと同じになる印は消す。印を、利用者が決めて変えたものだけにしておくため
/// (「新しく見つかったファイル」の設定を変えたときに、それに従うものを残す)。
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
    let default_public = sqlx::query_scalar!(
        r#"SELECT new_files_public as "new_files_public: bool" FROM contents WHERE id = ?"#,
        content_id
    )
    .fetch_one(&mut **tx)
    .await?;
    let rows = sqlx::query!(
        r#"SELECT path, public as "public: bool" FROM folder_marks WHERE content_id = ?"#,
        content_id
    )
    .fetch_all(&mut **tx)
    .await?;
    Marks::build(
        default_public,
        rows.into_iter().map(|row| (row.path, row.public)),
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
    Ok(Json(
        sqlx::query_as!(
            FolderMark,
            r#"SELECT id as "id!", path, is_dir as "is_dir: bool", public as "public: bool"
               FROM folder_marks WHERE content_id = ? ORDER BY created_at DESC, id DESC"#,
            id
        )
        .fetch_all(&state.pool)
        .await?,
    ))
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
    set_marks(&state.pool, id, vec![(path, is_dir)], payload.public).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 印を取り消し、祖先か「新しく見つかったファイル」の設定に従わせる。
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
        .routes(routes!(delete_mark))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marks(default_public: bool, marks: &[(&str, bool)]) -> Marks {
        Marks {
            default_public,
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
            "INSERT INTO contents (type, title, path, visibility, new_files_public) VALUES ('folder', 'f', '/f', 'public', 0) RETURNING id"
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
