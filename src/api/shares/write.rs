//! 共有の中に足す・名前を変える口 (アップロード・新しいフォルダー・名前の変更) と、ホームに載せる口。
//!
//! 足すには、その場所で `ShareAccess::Add` 以上、名前の変更には `ShareAccess::All` が要る。
//! 見られるが足りないときは 403、見られなければ 404 (→ `browse`)。

use std::path::{Path as FsPath, PathBuf};

use axum::Json;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::api::contents::{AdminContentResponse, Visibility};
use crate::auth::AuthUser;
use crate::error::{AppError, AppJson, multipart_error_to_app_error, run_blocking};
use crate::state::AppState;

use super::fsops;
use super::log::{LoggedPlace, OpRecord, ShareOp, record_or_warn};
use super::paths::{Resolved, ensure_not_protected, resolve_existing, resolve_parent_for_new};
use super::repoint;
use super::{ShareAccess, require};
use crate::api::fs::{join_rel, parent_rel};

/// 拡張子を直すかを決めるのに読む、ファイルの頭の大きさ (→ `fsops::sniff_image`)。
const SNIFF_BYTES: usize = 64;

/// multipartの受付上限(`DefaultBodyLimit`)は、許したい1つのファイルの大きさに、区切りの文字列などの
/// 分の余裕を足した値にする。厳密な上限は受け取りながら確かめる (`write_field`) ので、ここは
/// 話にならないほど大きい要求を早めに切るためのもの。
const UPLOAD_BODY_OVERHEAD_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize, IntoParams)]
struct UploadQuery {
    /// 置き先のフォルダーの、共有からの相対パス。省略時 (空文字列) は共有そのもの。
    #[serde(default)]
    path: String,
}

/// OpenAPI のための形。実際は `Multipart` で受け取る。
#[derive(Debug, Deserialize, ToSchema)]
#[allow(dead_code)]
struct ShareUploadRequest {
    /// 置くファイル。いくつでも並べられる。
    #[schema(value_type = Vec<String>, format = Binary)]
    file: Vec<Vec<u8>>,
}

/// 置いたファイル。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadedFile {
    /// 置いた名前。
    name: String,
    /// 同じ名前があったので、番号を付けた。
    renamed: bool,
    /// 中身が画像なのに拡張子が合っていなかったので、直した。
    extension_fixed: bool,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UploadResponse {
    /// 送った順。
    files: Vec<UploadedFile>,
}

/// 共有の中のフォルダーにファイルを置く。同じ名前があれば、番号を付けた名前で置く (上書きしない)。
///
/// 中身が画像なのに拡張子が合っていなければ直す (→ `fsops::fixed_extension`)。
/// いくつか送って途中で失敗したら、それまでに置いたものは残る (画面は1つずつ送る)。
#[utoipa::path(
    post,
    path = "/shares/{id}/upload",
    params(("id" = i64, Path), UploadQuery),
    request_body(content = ShareUploadRequest, content_type = "multipart/form-data"),
    responses(
        (status = CREATED, body = UploadResponse, description = "置いた"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "この場所に足せない"),
        (status = 404, body = crate::error::ErrorResponse, description = "共有・フォルダーが無い、見られない"),
        (status = 409, body = crate::error::ErrorResponse, description = "共有に書き込めない (`share_read_only`)"),
        (status = 413, body = crate::error::ErrorResponse, description = "1つのファイルが上限を超えた"),
        (status = 422, body = crate::error::ErrorResponse, description = "ファイルが無い、名前が使えない (`entryNameInvalid`)"),
        (status = 507, body = crate::error::ErrorResponse, description = "置き先の空きが足りない"),
    )
)]
async fn upload_to_share(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<UploadQuery>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<UploadResponse>), AppError> {
    let dir = resolve_existing(&state, &user, id, &query.path).await?;
    require(&dir, ShareAccess::Add)?;
    // 送られてくる大きさは、受け取り始める前に要求の大きさで見積もる (multipart の区切りの分だけ多めになる)。
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let path = dir.canonical.clone();
    run_blocking(move || {
        if !path.is_dir() {
            return Err(AppError::NotFound);
        }
        fsops::remove_stale_temps(&path);
        fsops::ensure_space(&path, declared)
    })
    .await??;

    let mut files = Vec::new();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(multipart_error_to_app_error)?
    {
        if field.name() != Some("file") {
            continue;
        }
        // 古いブラウザーは、送るファイルの場所まで名前に入れてくることがある。
        let sent = field.file_name().unwrap_or_default();
        let sent = sent.rsplit(['/', '\\']).next().unwrap_or_default();
        let name = super::paths::validate_name(sent)?;
        let received = receive_file(&mut field, &dir.canonical, state.max_upload_bytes).await?;
        let file = place_upload(&state, &dir, &name, received).await?;
        record_or_warn(
            &state.pool,
            &OpRecord::new(
                ShareOp::Upload,
                &user,
                dir.root_id,
                &dir.root_name,
                vec![join_rel(&dir.rel, &file.0.name)],
            )
            .counts(1, file.1),
        )
        .await;
        files.push(file.0);
    }
    if files.is_empty() {
        return Err(AppError::Validation("no file was sent".to_string()));
    }
    // 置いたものが、一覧の表示を使うフォルダーの一覧に次の再スキャンを待たずに出るようにする。
    match repoint::list_views_containing(&state.pool, &dir.canonical).await {
        Ok(ids) => repoint::refresh_after_change(&state, ids),
        Err(err) => {
            tracing::warn!(error = %crate::error::error_chain(&err), "finding folders to refresh after an upload failed");
        }
    }
    Ok((StatusCode::CREATED, Json(UploadResponse { files })))
}

/// 受け取り終えた作業用のファイル。drop で消す (置いた後は、もう無いので何もしない)。
struct ReceivedFile {
    temp: PathBuf,
    head: Vec<u8>,
    size: u64,
}

impl Drop for ReceivedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.temp);
    }
}

/// `field` を `dir` の中の作業用のファイルに書き込む。`max_bytes` を超えたら 413 (`None` なら上限なし)。
///
/// 作業用のファイルは置き先と同じフォルダーに作る。同じディスクの中なら、置くときに写し直さずに済むため。
/// 一覧に出ない名前で、無いときだけ作る (同じ名前の何かを書き換えないため)。
async fn receive_file(
    field: &mut axum::extract::multipart::Field<'_>,
    dir: &FsPath,
    max_bytes: Option<u64>,
) -> Result<ReceivedFile, AppError> {
    let temp = dir.join(format!(
        "{}{}.part",
        fsops::UPLOAD_TEMP_PREFIX,
        crate::api::random_hex::<8>()?
    ));
    let mut file = tokio::fs::File::create_new(&temp)
        .await
        .map_err(fsops::map_io_error)?;
    let written = write_field(field, &mut file, max_bytes).await;
    // 消す前に閉じる。Windows では開いたままのファイルを消せないため。
    drop(file);
    let mut received = ReceivedFile {
        temp,
        head: Vec::new(),
        size: 0,
    };
    (received.head, received.size) = written?;
    Ok(received)
}

/// `field` を `file` に書き込み、頭の `SNIFF_BYTES` と大きさを返す。
async fn write_field(
    field: &mut axum::extract::multipart::Field<'_>,
    file: &mut tokio::fs::File,
    max_bytes: Option<u64>,
) -> Result<(Vec<u8>, u64), AppError> {
    let mut head = Vec::with_capacity(SNIFF_BYTES);
    let mut size: u64 = 0;
    while let Some(chunk) = field.chunk().await.map_err(multipart_error_to_app_error)? {
        size += chunk.len() as u64;
        if max_bytes.is_some_and(|max| size > max) {
            return Err(AppError::FileTooLarge);
        }
        if head.len() < SNIFF_BYTES {
            let take = (SNIFF_BYTES - head.len()).min(chunk.len());
            head.extend_from_slice(&chunk[..take]);
        }
        file.write_all(&chunk).await.map_err(fsops::map_io_error)?;
    }
    file.flush().await.map_err(fsops::map_io_error)?;
    Ok((head, size))
}

/// 受け取ったファイルに名前を付けて置く。置いたファイルと大きさを返す。
async fn place_upload(
    state: &AppState,
    dir: &Resolved,
    name: &str,
    received: ReceivedFile,
) -> Result<(UploadedFile, u64), AppError> {
    let wanted = fsops::fixed_extension(name, &received.head);
    let extension_fixed = wanted.is_some();
    let wanted = wanted.unwrap_or_else(|| name.to_string());
    let lock = state.share_dir_locks.get(&dir.canonical);
    let dir_path = dir.canonical.clone();
    let size = received.size;
    let placed = run_blocking(move || {
        let _guard = lock
            .lock()
            .map_err(|_| AppError::Io(std::io::Error::other("the folder lock is poisoned")))?;
        let placed = fsops::place_new_file(&received.temp, &dir_path, &wanted)?;
        Ok::<_, AppError>((placed, wanted))
    })
    .await??;
    let (placed, wanted) = placed;
    Ok((
        UploadedFile {
            renamed: placed != wanted,
            name: placed,
            extension_fixed,
        },
        size,
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateShareFolderRequest {
    /// 作る場所 (フォルダー) の、共有からの相対パス。空文字列は共有そのもの。
    #[serde(default)]
    path: String,
    name: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SharePathResponse {
    /// 作った・名前を変えた後の場所の、共有からの相対パス。
    path: String,
}

/// 共有の中に新しいフォルダーを作る。
#[utoipa::path(
    post,
    path = "/shares/{id}/folders",
    params(("id" = i64, Path)),
    request_body = CreateShareFolderRequest,
    responses(
        (status = CREATED, body = SharePathResponse, description = "作った"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "この場所に足せない"),
        (status = 404, body = crate::error::ErrorResponse, description = "共有・フォルダーが無い、見られない"),
        (status = 409, body = crate::error::ErrorResponse, description = "共有に書き込めない (`share_read_only`)"),
        (status = 422, body = crate::error::ErrorResponse, description = "名前が使えない (`entryNameInvalid`)・同じ名前がある (`entryNameTaken`)"),
    )
)]
async fn create_share_folder(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<CreateShareFolderRequest>,
) -> Result<(StatusCode, Json<SharePathResponse>), AppError> {
    let new = resolve_parent_for_new(&state, &user, id, &payload.path, &payload.name).await?;
    require(&new.dir, ShareAccess::Add)?;
    let target = new.target.clone();
    run_blocking(move || fsops::create_dir_new(&target)).await??;
    let path = join_rel(&new.dir.rel, &new.name);
    record_or_warn(
        &state.pool,
        &OpRecord::new(
            ShareOp::Mkdir,
            &user,
            new.dir.root_id,
            &new.dir.root_name,
            vec![path.clone()],
        ),
    )
    .await;
    Ok((StatusCode::CREATED, Json(SharePathResponse { path })))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameShareEntryRequest {
    /// 名前を変えるものの、共有からの相対パス。
    path: String,
    /// 新しい名前。
    name: String,
}

/// 共有の中のファイル・フォルダーの名前を変える。大文字・小文字だけの変更もできる。
/// 共有そのものと、中に別の共有を含むフォルダーは変えられない。
#[utoipa::path(
    post,
    path = "/shares/{id}/rename",
    params(("id" = i64, Path)),
    request_body = RenameShareEntryRequest,
    responses(
        (status = OK, body = SharePathResponse, description = "名前を変えた"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "この場所で名前を変えられない"),
        (status = 404, body = crate::error::ErrorResponse, description = "共有・ファイルが無い、見られない"),
        (status = 409, body = crate::error::ErrorResponse, description = "共有に書き込めない・ほかのアプリが開いている"),
        (status = 422, body = crate::error::ErrorResponse, description = "名前が使えない・同じ名前がある・共有そのもの (`entryProtected`)"),
    )
)]
async fn rename_share_entry(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<RenameShareEntryRequest>,
) -> Result<Json<SharePathResponse>, AppError> {
    let entry = resolve_existing(&state, &user, id, &payload.path).await?;
    require(&entry, ShareAccess::All)?;
    ensure_not_protected(&state, &entry).await?;
    let name = super::paths::validate_name(&payload.name)?;
    let from = entry.canonical.clone();
    let to = run_blocking(move || fsops::rename_entry(&from, &name)).await??;
    let parent = parent_rel(&entry.rel);
    let new_name = to
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let path = join_rel(parent, new_name);
    repoint::apply(
        &state,
        &[repoint::Change::Moved {
            from: entry.canonical.clone(),
            to,
        }],
    )
    .await;
    record_or_warn(
        &state.pool,
        &OpRecord::new(
            ShareOp::Rename,
            &user,
            entry.root_id,
            &entry.root_name,
            vec![entry.rel.clone()],
        )
        .dest(LoggedPlace {
            root_id: entry.root_id,
            root_name: entry.root_name.clone(),
            path: path.clone(),
        }),
    )
    .await;
    Ok(Json(SharePathResponse { path }))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListOnHomeRequest {
    /// 載せるフォルダーかファイルの、共有からの相対パス。空文字列は共有そのもの。
    #[serde(default)]
    path: String,
    /// 公開範囲。省略時は既定値。
    #[serde(default)]
    visibility: Option<Visibility>,
}

/// 共有の中のフォルダーかファイルを、それを指すショートカットとしてホームに載せる。タイトルは名前から付ける。
/// 見られる場所なら載せられる (コンテンツの作成と同じ)。その場所を自分で戻せる非表示のコンテンツがあれば、
/// 新しく作らずにそれを戻す (→ docs/shares.md「ホームに載せる」)。
#[utoipa::path(
    post,
    path = "/shares/{id}/list-on-home",
    params(("id" = i64, Path)),
    request_body = ListOnHomeRequest,
    responses(
        (status = CREATED, body = AdminContentResponse, description = "載せた"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "共有・フォルダー・ファイルが無い、見られない"),
        (status = 422, body = crate::error::ErrorResponse, description = "公開範囲が合わない"),
    )
)]
async fn list_on_home(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<ListOnHomeRequest>,
) -> Result<(StatusCode, Json<AdminContentResponse>), AppError> {
    let dir = resolve_existing(&state, &user, id, &payload.path).await?;
    require(&dir, ShareAccess::Read)?;
    let path = crate::api::fs::path_to_string(dir.canonical.clone())?;
    let created =
        crate::api::contents::list_path_on_home(&state, &user, path, payload.visibility).await?;
    record_or_warn(
        &state.pool,
        &OpRecord::new(
            ShareOp::ListOnHome,
            &user,
            dir.root_id,
            &dir.root_name,
            vec![dir.rel.clone()],
        ),
    )
    .await;
    Ok((StatusCode::CREATED, Json(created)))
}

/// `max_upload_bytes` は1つのファイルの上限。要求の大きさの上限は、それに multipart の区切りの分を足す。
/// 上限なし (`None`) なら、要求の大きさの上限 (axum の既定は 2MB) をこの口でだけ外す。
/// ほかの口に掛からないよう、別の `OpenApiRouter` に分けてから merge する。
pub(super) fn router(max_upload_bytes: Option<u64>) -> OpenApiRouter<AppState> {
    let body_limit = match max_upload_bytes {
        Some(max) => DefaultBodyLimit::max(
            usize::try_from(max.saturating_add(UPLOAD_BODY_OVERHEAD_BYTES)).unwrap_or(usize::MAX),
        ),
        None => DefaultBodyLimit::disable(),
    };
    let upload_routes = OpenApiRouter::new()
        .routes(routes!(upload_to_share))
        .layer(body_limit);
    OpenApiRouter::new()
        .routes(routes!(create_share_folder))
        .routes(routes!(rename_share_entry))
        .routes(routes!(list_on_home))
        .merge(upload_routes)
}
