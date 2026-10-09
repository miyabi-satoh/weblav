//! 「公開できるフォルダー」(→ docs/folders.md「公開できるフォルダー」) の管理 API。
//!
//! コンテンツに使えるパスは、ここに登録したフォルダーの配下だけになる。
//! 登録・削除できるのは**サーバーの PC の前にいる管理者**だけ (`LocalRequest` + `AdminUser`)。
//! LAN の端末からは、この口があること自体を見せずに 404 にする。

use std::path::{Path as FsPath, PathBuf};

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AdminUser;
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

use super::error_detail::ValidationDetail;
use super::fs::{canonical_dir, inside_own_dirs, path_to_string};
use super::local::LocalRequest;

/// 登録済みの「公開できるフォルダー」1件。
///
/// 判定 (`is_within_roots`) には `path` だけを使う。`name` はコンテンツの登録側の画面で
/// 起点のフルパスの代わりに出す (→ docs/folders.md「公開できるフォルダー」)。
#[derive(Debug, Clone)]
pub(super) struct Root {
    pub(super) name: String,
    pub(super) path: PathBuf,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RootResponse {
    pub id: i64,
    /// 登録側の画面でフルパスの代わりに出す名前。同じ名前は無い (大文字小文字も区別しない)。
    pub name: String,
    /// canonicalize 済みの絶対パス。
    pub path: String,
    /// このフォルダーの中にある登録済みコンテンツの数。
    pub content_count: usize,
    /// このフォルダーを外すと見られなくなるコンテンツの数。削除の確認に出す (→ docs/folders.md「公開できるフォルダー」)。
    /// 登録中のほかの公開できるフォルダーにも含まれるもの (入れ子) は数えない。
    pub removed_content_count: usize,
}

#[utoipa::path(
    get,
    path = "/admin/roots",
    responses(
        (status = OK, body = Vec<RootResponse>, description = "登録済みの一覧"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 404, body = crate::error::ErrorResponse, description = "サーバーのPC以外からの要求"),
    )
)]
async fn list_roots(
    _: LocalRequest,
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<RootResponse>>, AppError> {
    let rows =
        sqlx::query!(r#"SELECT id as "id!", name, path FROM roots WHERE deleted_at IS NULL ORDER BY name COLLATE NOCASE"#)
            .fetch_all(&state.pool)
            .await?;
    let counts = ContentCounts::load(&state.pool).await?;
    let roots = rows
        .into_iter()
        .map(|row| RootResponse {
            content_count: counts.under(&row.path),
            removed_content_count: counts.removed_with(&row.path),
            id: row.id,
            name: row.name,
            path: row.path,
        })
        .collect();
    Ok(Json(roots))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct CreateRootRequest {
    /// 絶対パス。フォルダー選択の窓 (`/admin/roots/pick`) が返す値をそのまま渡す。
    path: String,
    /// 名前。前後の空白は除く。省くか空ならフォルダー名にする。
    #[serde(default)]
    name: Option<String>,
}

#[utoipa::path(
    post,
    path = "/admin/roots",
    request_body = CreateRootRequest,
    responses(
        (status = 201, body = RootResponse, description = "登録したフォルダー"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 404, body = crate::error::ErrorResponse, description = "サーバーのPC以外からの要求"),
        (status = 422, body = crate::error::ErrorResponse, description = "パスが存在しない・ディレクトリでない・weblav 自身の置き場の中・既存と重なる・同じ名前がある"),
    )
)]
async fn create_root(
    _: LocalRequest,
    _admin: AdminUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<CreateRootRequest>,
) -> Result<(StatusCode, Json<RootResponse>), AppError> {
    let own_dirs = state.own_dirs.clone();
    let requested_path = payload.path;
    let (path, folder_name) = run_blocking(move || {
        let canonical = canonical_dir(&requested_path)?;
        if OwnDirs::resolve(&own_dirs).contains(&canonical) {
            return Err(inside_own_dirs());
        }
        let path = path_to_string(canonical)?;
        // Windows の canonicalize は `\\?\` を付けて返し、その形ではドライブや共有のルートに
        // フォルダー名が無いと判定されるので、落とした後の形から求める。
        let folder_name = default_name(FsPath::new(&path));
        Ok((path, folder_name))
    })
    .await??;
    let requested_name = super::validate::trimmed_or_none(payload.name.as_deref());

    // 同じフォルダーの二重登録は拒む。入れ子は許す (→ docs/folders.md「公開できるフォルダー」)。削除済みは数えない。
    //
    // 確認と INSERT は同じ書き込みトランザクションで行う。別々にすると、2つの登録が
    // 同時に来たときに、互いを見ないまま両方が確認を通る。
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let existing = sqlx::query_scalar!(
        "SELECT name FROM roots WHERE path = ? AND deleted_at IS NULL",
        path
    )
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(name) = existing {
        return Err(AppError::ValidationDetailed {
            message: "path is already a root".to_string(),
            detail: ValidationDetail::RootAlreadyRegistered { name },
        });
    }
    // 名前を指定したときだけ、重なりを拒む。フォルダー名で付ける名前は連番で避ける。
    let name = match requested_name {
        Some(name) => {
            ensure_name_available(&mut tx, name, None).await?;
            name.to_string()
        }
        None => free_name(&mut tx, &folder_name, None).await?,
    };

    // 削除済みの同じパスがあれば、その行を戻す (`path` は UNIQUE)。
    let row = sqlx::query!(
        r#"INSERT INTO roots (name, path) VALUES (?, ?)
           ON CONFLICT (path) DO UPDATE SET name = excluded.name, deleted_at = NULL
           RETURNING id as "id!", name, path"#,
        name,
        path
    )
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    // 0件とは限らない。削除したフォルダーを登録し直すと、その中のコンテンツが戻る
    // (コンテンツの登録は削除時に消していない)。
    let counts = ContentCounts::load(&state.pool).await?;
    Ok((
        StatusCode::CREATED,
        Json(RootResponse {
            id: row.id,
            name: row.name,
            content_count: counts.under(&row.path),
            removed_content_count: counts.removed_with(&row.path),
            path: row.path,
        }),
    ))
}

/// `base` が他の行と重なるとき、`base (1)` のように連番を付けた最初の空き。
/// 名前を省いて自動で付けるときに使う。大文字小文字は Unicode 全体で同一視する
/// (`ensure_name_available` の NOCASE は ASCII だけ)。区別しにくい名前を避けるための連番なので、
/// 広く同一視して困ることはない。
async fn free_name(
    tx: &mut sqlx::SqliteConnection,
    base: &str,
    except: Option<i64>,
) -> Result<String, AppError> {
    let names = sqlx::query_scalar!(
        "SELECT name FROM roots WHERE deleted_at IS NULL AND id IS NOT ?",
        except
    )
    .fetch_all(&mut *tx)
    .await?;
    let taken: std::collections::HashSet<String> =
        names.iter().map(|name| name.to_lowercase()).collect();
    Ok(super::validate::first_free_name(base, |candidate| {
        taken.contains(&candidate.to_lowercase())
    }))
}

/// 名前が、削除済みを除く他の行と重ならないか。`except` は名前を変える行自身。
/// インデックスと同じ NOCASE で比べる。
async fn ensure_name_available(
    tx: &mut sqlx::SqliteConnection,
    name: &str,
    except: Option<i64>,
) -> Result<(), AppError> {
    let taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM roots
               WHERE name = ? COLLATE NOCASE AND deleted_at IS NULL AND id IS NOT ?
           ) as "taken!: bool""#,
        name,
        except
    )
    .fetch_one(&mut *tx)
    .await?;
    if taken {
        return Err(AppError::ValidationDetailed {
            message: "root name is already taken".to_string(),
            detail: ValidationDetail::RootNameTaken {
                name: name.to_string(),
            },
        });
    }
    Ok(())
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct RenameRootRequest {
    /// 新しい名前。前後の空白は除く。空ならフォルダー名にする。
    name: String,
}

#[utoipa::path(
    put,
    path = "/admin/roots/{id}",
    params(("id" = i64, Path)),
    request_body = RenameRootRequest,
    responses(
        (status = OK, body = RootResponse, description = "名前を変えたフォルダー"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 404, body = crate::error::ErrorResponse, description = "サーバーのPC以外からの要求・存在しない"),
        (status = 422, body = crate::error::ErrorResponse, description = "同じ名前がある"),
    )
)]
async fn rename_root(
    _: LocalRequest,
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<RenameRootRequest>,
) -> Result<Json<RootResponse>, AppError> {
    let mut tx = crate::db::begin_write(&state.pool).await?;
    let path = sqlx::query_scalar!(
        "SELECT path FROM roots WHERE id = ? AND deleted_at IS NULL",
        id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;
    let name = match super::validate::trimmed_or_none(Some(&payload.name)) {
        Some(name) => {
            ensure_name_available(&mut tx, name, Some(id)).await?;
            name.to_string()
        }
        None => free_name(&mut tx, &default_name(FsPath::new(&path)), Some(id)).await?,
    };
    sqlx::query!("UPDATE roots SET name = ? WHERE id = ?", name, id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let counts = ContentCounts::load(&state.pool).await?;
    Ok(Json(RootResponse {
        id,
        name,
        content_count: counts.under(&path),
        removed_content_count: counts.removed_with(&path),
        path,
    }))
}

#[utoipa::path(
    delete,
    path = "/admin/roots/{id}",
    params(("id" = i64, Path)),
    responses(
        (status = 204, description = "削除した"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 404, body = crate::error::ErrorResponse, description = "サーバーのPC以外からの要求・存在しない"),
    )
)]
async fn delete_root(
    _: LocalRequest,
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    // 行は消さずに削除済みの印を付ける。残ったコンテンツの場所を、起点を隠したまま出すため
    // (→ docs/folders.md「公開できるフォルダー」)。
    let deleted = sqlx::query!(
        "UPDATE roots SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ? AND deleted_at IS NULL",
        id
    )
    .execute(&state.pool)
    .await?
    .rows_affected();
    if deleted == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// 登録済みの「公開できるフォルダー」を、判定に使える形 (canonicalize 済み) で返す。
///
/// 判定のたびに読む。件数は多くても数件で、キャッシュを持つと登録・削除の直後に
/// 古い判定が残る (→ docs/folders.md「公開できるフォルダー」)。
///
/// **実体を辿れないものは落とす**。登録を消さずにフォルダーだけ消された場合、その中は配らない。
/// canonicalize しておくのは、判定の相手 (`fs::canonical_dir` の戻り値) と同じ形で
/// 比べるため。保存してある文字列は Windows の長いパスで `\\?\` の有無が食い違いうる
/// (→ `fs::simplify_verbatim`)。
pub(super) async fn load_roots(pool: &sqlx::SqlitePool) -> Result<Vec<Root>, AppError> {
    let rows = sqlx::query!(
        "SELECT name, path FROM roots WHERE deleted_at IS NULL ORDER BY name COLLATE NOCASE"
    )
    .fetch_all(pool)
    .await?;
    run_blocking(move || {
        rows.into_iter()
            .filter_map(|row| {
                let path = std::fs::canonicalize(&row.path).ok()?;
                Some(Root {
                    name: row.name,
                    path,
                })
            })
            .collect()
    })
    .await
}

/// 登録側の画面に出す場所。起点のフルパスの代わりに、公開できるフォルダーの名前と
/// そこから先のパスを返す (→ docs/folders.md「公開できるフォルダー」)。
#[derive(Debug, PartialEq, Eq)]
pub(super) struct RootLocation {
    pub(super) root_name: String,
    /// 公開できるフォルダーから先の相対パス。区切りは `/` にそろえる。フォルダーそのものなら空。
    pub(super) path_in_root: String,
    /// 含む公開できるフォルダーが削除済みか。
    pub(super) root_deleted: bool,
}

/// 場所の表示に使う、保存されている登録 (canonicalize しない)。判定には使わない。
#[derive(Debug, Clone)]
pub(super) struct RootRecord {
    pub(super) name: String,
    pub(super) path: String,
    pub(super) deleted: bool,
}

/// 削除済みも含めた登録。ファイルシステムに触らないので、フォルダーが消えた登録でも名前を出せる。
pub(super) async fn load_root_records(
    pool: &sqlx::SqlitePool,
) -> Result<Vec<RootRecord>, AppError> {
    Ok(
        sqlx::query!(r#"SELECT name, path, deleted_at IS NOT NULL as "deleted!: bool" FROM roots"#)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|row| RootRecord {
                name: row.name,
                path: row.path,
                deleted: row.deleted,
            })
            .collect(),
    )
}

/// `path` (コンテンツに保存された canonicalize 済みの絶対パス) を含む公開できるフォルダーを探し、
/// 画面に出す場所を組み立てる。どれにも含まれなければ `None`。
///
/// 登録中が当たれば、最も外側を採る (入れ子の起点の決まりは `containing_root` と同じ)。
/// 削除済みしか当たらなければ、最も内側を採る。
pub(super) fn locate(records: &[RootRecord], path: &str) -> Option<RootLocation> {
    let matching = || records.iter().filter(|record| contains(&record.path, path));
    let record = matching()
        .filter(|record| !record.deleted)
        .min_by_key(|record| record.path.len())
        .or_else(|| matching().max_by_key(|record| record.path.len()))?;
    let rest = FsPath::new(path)
        .strip_prefix(FsPath::new(&record.path))
        .ok()?;
    let path_in_root = rest
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    Some(RootLocation {
        root_name: record.name.clone(),
        path_in_root,
        root_deleted: record.deleted,
    })
}

/// 名前を省いたときの名前。フォルダー名で、ドライブのルートのように無ければパスそのもの。
///
/// **`path_to_string` を通した後のパスを渡すこと** (`\\?\` 付きの形では、ルートの判定が変わる)。
pub(super) fn default_name(path: &FsPath) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// `canonical` が、登録済みのどれかの中にあるか。
///
/// **パスを使う操作はすべてこれを通す** (→ docs/folders.md「公開できるフォルダー」)。登録時の検証だけでは、
/// 登録後にフォルダーを削除された場合にそのまま配信され続ける。
///
/// weblav 自身の置き場 (設定とデータ) の中も外す。公開できるフォルダーがその祖先 (`C:\` など) でも、
/// その中は登録先にも配信にも使わせない。
pub(super) fn is_within_roots(roots: &[Root], canonical: &FsPath, own_dirs: &OwnDirs) -> bool {
    !own_dirs.contains(canonical) && containing_root(roots, canonical).is_some()
}

/// `canonical` を含む登録済みのフォルダー。選択 UI のパンくずを、辿れるところから始めるのにも使う
/// (→ docs/folders.md「一覧 API」)。入れ子でいくつも当たれば、最も外側を採る。
/// 内側を採ると、外側から辿って内側へ入ったときに起点が切り替わり、「上へ」で外側へ戻れない。
///
/// **両方とも `std::fs::canonicalize` 済みのパスであること** (→ `load_roots`)。
pub(super) fn containing_root<'a>(roots: &'a [Root], canonical: &FsPath) -> Option<&'a Root> {
    roots
        .iter()
        .filter(|root| canonical.starts_with(&root.path))
        .min_by_key(|root| root.path.components().count())
}

/// weblav 自身の置き場 (設定とデータ) の実体パス。まだ作られていないものは除く
/// (中身が無いので、外すものも無い)。
///
/// どこを公開するかは設置者に任せるが、ここだけはアプリが必ず外す
/// (→ docs/folders.md「公開できるフォルダー」)。守りたいのは選ぶことではなく中身が出ることなので、祖先は選べる。
///
/// 一覧や走査ではエントリごとに比べるので、要求ごとに `resolve` で1回だけ求めて持ち回る。
#[derive(Debug, Default)]
pub(super) struct OwnDirs(Vec<PathBuf>);

impl OwnDirs {
    /// ブロッキングI/Oを行うため、`spawn_blocking` の中で呼ぶこと。
    pub(super) fn resolve(dirs: &[PathBuf]) -> Self {
        Self(
            dirs.iter()
                .filter_map(|dir| std::fs::canonicalize(dir).ok())
                .collect(),
        )
    }

    /// `path` が、置き場のどれかそのものか、その中か。**実体パスを渡すこと**
    /// (canonicalize 済みか、canonicalize 済みの根からリンクを辿らずに得たもの)。
    pub(super) fn contains(&self, path: &FsPath) -> bool {
        self.0.iter().any(|dir| path.starts_with(dir))
    }
}

/// 公開できるフォルダーごとのコンテンツの件数を数えるための材料。
///
/// パスを持つコンテンツ (`folder`・`archive`) の登録先と、登録中の公開できるフォルダー。
/// 多くても数百件なので、まとめて読んで数える。SQL で前方一致を書くと、区切り文字の扱いを
/// `contains` と二重に持つことになる。
struct ContentCounts {
    content_paths: Vec<String>,
    root_paths: Vec<String>,
}

impl ContentCounts {
    async fn load(pool: &sqlx::SqlitePool) -> Result<Self, AppError> {
        let content_paths = sqlx::query_scalar!("SELECT path FROM contents WHERE path IS NOT NULL")
            .fetch_all(pool)
            .await?
            .into_iter()
            .flatten()
            .collect();
        let root_paths = sqlx::query_scalar!("SELECT path FROM roots WHERE deleted_at IS NULL")
            .fetch_all(pool)
            .await?;
        Ok(Self {
            content_paths,
            root_paths,
        })
    }

    /// `root` の中にあるコンテンツの数。
    fn under(&self, root: &str) -> usize {
        self.content_paths
            .iter()
            .filter(|path| contains(root, path))
            .count()
    }

    /// `root` を外すと見られなくなるコンテンツの数。ほかの登録にも含まれるもの (入れ子) は数えない。
    fn removed_with(&self, root: &str) -> usize {
        self.content_paths
            .iter()
            .filter(|path| contains(root, path))
            .filter(|path| {
                !self
                    .root_paths
                    .iter()
                    .any(|other| other != root && contains(other, path))
            })
            .count()
    }
}

/// `parent` が `child` を含むか (同じ場所も含むとみなす)。
///
/// **どちらも `path_to_string` を通した値であること**。同じ場所は同じ文字列になる。
///
/// 文字列の前方一致ではなく `Path` の要素で比べる。`C:\教材` が `C:\教材2` を含むと
/// 判定されず、マルチバイトのパスでも文字の途中で切らない。大文字小文字も区別する
/// (canonicalize 済みなので、同じ場所なら同じ綴りになる)。
fn contains(parent: &str, child: &str) -> bool {
    FsPath::new(child).starts_with(FsPath::new(parent))
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct PickRootResponse {
    /// 選ばれたフォルダーの絶対パス。キャンセルされたら `null`。登録はしていないので、
    /// 登録するには `POST /admin/roots` へそのまま渡す。
    path: Option<String>,
}

/// サーバーの PC に OS 標準のフォルダー選択の窓を出し、選ばれたパスを返す (→ docs/folders.md「選び方」)。
///
/// 窓は同時に1つだけ。要求した側が待つのをやめても、窓が閉じるまでは開いている扱いにする
/// (番は `run_blocking` の中で持つ。ハンドラが drop されても、ブロッキングの処理は最後まで走る)。
#[utoipa::path(
    post,
    path = "/admin/roots/pick",
    responses(
        (status = OK, body = PickRootResponse, description = "選ばれたパス。キャンセルなら null"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 404, body = crate::error::ErrorResponse, description = "サーバーのPC以外からの要求"),
        (status = 409, body = crate::error::ErrorResponse, description = "窓がもう開いている"),
    )
)]
async fn pick_root(
    _: LocalRequest,
    _admin: AdminUser,
    State(state): State<AppState>,
) -> Result<Json<PickRootResponse>, AppError> {
    let session = state
        .folder_picker
        .begin()
        .ok_or_else(|| AppError::Conflict("a folder picker is already open".to_string()))?;
    let picked = run_blocking(move || {
        session
            .pick()
            .map_err(|_| AppError::Io(std::io::Error::other("the folder picker is not running")))
    })
    .await??;
    let Some(picked) = picked else {
        return Ok(Json(PickRootResponse { path: None }));
    };
    // 画面に出し、そのまま登録に渡す値なので、ほかの口と同じ形に揃える。
    let path = path_to_string(picked)?;
    // 窓で選んだ今のうちに、次の起動でも読めるようにしておく (→ docs/distribution.md「ビルド・配布の方法」)。
    // キーは登録と同じく実体のパス。
    let lookup = path.clone();
    let bookmark = run_blocking(move || -> Result<_, AppError> {
        let Ok(canonical) = canonical_dir(&lookup) else {
            return Ok(None);
        };
        // 作れないまま登録させると、サンドボックスでは起動し直した後に読めないフォルダーが残る。
        let Some(bookmark) = crate::folder_access::bookmark(&canonical).map_err(|err| {
            AppError::Io(std::io::Error::other(format!(
                "failed to create a folder bookmark: {err}"
            )))
        })?
        else {
            return Ok(None);
        };
        Ok(Some((path_to_string(canonical)?, bookmark)))
    })
    .await??;
    if let Some((canonical, bookmark)) = bookmark {
        crate::folder_access::save(&state.pool, &canonical, &bookmark).await?;
    }
    Ok(Json(PickRootResponse { path: Some(path) }))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_roots, create_root))
        .routes(routes!(pick_root))
        .routes(routes!(rename_root, delete_root))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_root_contains_its_descendants() {
        let sep = std::path::MAIN_SEPARATOR;
        let parent = format!("{sep}srv{sep}media");
        assert!(contains(&parent, &format!("{parent}{sep}2026")));
        assert!(contains(&parent, &parent));
    }

    /// 区切り文字まで見ないと、名前の前方一致で別のフォルダーを含むと誤判定する。
    #[test]
    fn a_sibling_with_a_longer_name_is_not_contained() {
        let sep = std::path::MAIN_SEPARATOR;
        let parent = format!("{sep}srv{sep}media");
        assert!(!contains(&parent, &format!("{sep}srv{sep}media2")));
    }

    /// 名前がマルチバイトで、隣のフォルダー名がその前方一致になる形。
    /// バイト単位で切ると文字の途中に当たって panic する。
    #[test]
    fn a_multibyte_sibling_is_not_contained() {
        let sep = std::path::MAIN_SEPARATOR;
        let parent = format!("{sep}srv{sep}教材");
        assert!(!contains(&parent, &format!("{sep}srv{sep}教材集")));
        assert!(contains(&parent, &format!("{sep}srv{sep}教材{sep}2026")));
    }

    /// 綴りの違うフォルダーを同じ場所とみなさない。Linux では別のディレクトリで、
    /// 大文字小文字を無視すると登録していない場所まで配ってしまう。
    #[test]
    fn a_differently_cased_path_is_not_contained() {
        let sep = std::path::MAIN_SEPARATOR;
        let parent = format!("{sep}srv{sep}media");
        assert!(!contains(&parent, &format!("{sep}srv{sep}MEDIA{sep}2026")));
    }

    #[test]
    fn a_location_is_the_root_name_and_the_rest() {
        let sep = std::path::MAIN_SEPARATOR;
        let root = format!("{sep}srv{sep}教材");
        let records = vec![record("教材", &root, false)];

        assert_eq!(
            locate(&records, &format!("{root}{sep}英検{sep}2024")),
            Some(RootLocation {
                root_name: "教材".to_string(),
                path_in_root: "英検/2024".to_string(),
                root_deleted: false,
            })
        );
        assert_eq!(
            locate(&records, &root),
            Some(RootLocation {
                root_name: "教材".to_string(),
                path_in_root: String::new(),
                root_deleted: false,
            })
        );
        // 前方一致だけの隣のフォルダーは含まない (`contains` と同じ)。
        assert_eq!(locate(&records, &format!("{root}集")), None);
    }

    fn record(name: &str, path: &str, deleted: bool) -> RootRecord {
        RootRecord {
            name: name.to_string(),
            path: path.to_string(),
            deleted,
        }
    }

    /// 登録中を削除済みより優先する。登録中どうしでは外側、削除済みどうしでは内側を採る。
    #[test]
    fn a_location_prefers_a_registered_root_then_the_innermost_deleted_one() {
        let sep = std::path::MAIN_SEPARATOR;
        let outer = format!("{sep}srv");
        let inner = format!("{sep}srv{sep}教材");
        let target = format!("{inner}{sep}英検");

        let deleted_only = vec![record("外", &outer, true), record("内", &inner, true)];
        let location = locate(&deleted_only, &target).expect("見つかるはず");
        assert_eq!(location.root_name, "内");
        assert!(location.root_deleted);

        let with_registered = vec![record("外", &outer, false), record("内", &inner, true)];
        let location = locate(&with_registered, &target).expect("見つかるはず");
        assert_eq!(location.root_name, "外");
        assert_eq!(location.path_in_root, "教材/英検");
        assert!(!location.root_deleted);

        let nested = vec![record("内", &inner, false), record("外", &outer, false)];
        let location = locate(&nested, &target).expect("見つかるはず");
        assert_eq!(location.root_name, "外");
    }

    #[test]
    fn the_default_name_is_the_folder_name() {
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            default_name(FsPath::new(&format!("{sep}srv{sep}教材"))),
            "教材"
        );
        let top = if cfg!(windows) { "C:\\" } else { "/" };
        assert_eq!(default_name(FsPath::new(top)), top);
    }

    /// 外すと見られなくなる件数は、ほかの登録にも含まれるもの (入れ子) を数えない。
    #[test]
    fn removed_count_leaves_out_contents_another_root_covers() {
        let sep = std::path::MAIN_SEPARATOR;
        let outer = format!("{sep}srv");
        let inner = format!("{outer}{sep}教材");
        let counts = ContentCounts {
            content_paths: vec![format!("{inner}{sep}英検"), format!("{outer}{sep}写真")],
            root_paths: vec![outer.clone(), inner.clone()],
        };
        assert_eq!(counts.under(&outer), 2);
        assert_eq!(counts.removed_with(&outer), 1);
        assert_eq!(counts.under(&inner), 1);
        assert_eq!(counts.removed_with(&inner), 0);
    }

    /// 入れ子では、最も外側を起点にする。
    #[test]
    fn the_containing_root_is_the_outermost() {
        let sep = std::path::MAIN_SEPARATOR;
        let root = |name: &str, path: &str| Root {
            name: name.to_string(),
            path: PathBuf::from(path),
        };
        let outer = format!("{sep}srv");
        let inner = format!("{outer}{sep}教材");
        let roots = vec![root("外", &outer), root("内", &inner)];
        let found =
            |path: &str| containing_root(&roots, FsPath::new(path)).map(|r| r.name.as_str());
        assert_eq!(found(&format!("{inner}{sep}英検")), Some("外"));
        assert_eq!(found(&format!("{outer}{sep}写真")), Some("外"));
    }

    /// 自身の置き場 (設定とデータ) そのものと中だけを外し、祖先 (`C:\` など) は外さない。
    #[test]
    fn only_own_dirs_and_inside_are_left_out() {
        let dir = crate::test_support::TempDir::new("roots-data-dir");
        let data_dir = dir.path().join("weblav");
        std::fs::create_dir_all(data_dir.join("blobs")).expect("データ置き場を作れなかった");
        let data_dir = std::fs::canonicalize(&data_dir).expect("canonicalize できなかった");
        let base = std::fs::canonicalize(dir.path()).expect("canonicalize できなかった");

        let config_dir = base.join("config");
        std::fs::create_dir_all(&config_dir).expect("設定の置き場を作れなかった");
        let own_dirs = OwnDirs::resolve(&[config_dir.clone(), data_dir.clone()]);

        assert!(!own_dirs.contains(&base));
        assert!(own_dirs.contains(&data_dir));
        assert!(own_dirs.contains(&data_dir.join("blobs")));
        assert!(own_dirs.contains(&config_dir));
        assert!(!own_dirs.contains(&base.join("other")));
    }
}
