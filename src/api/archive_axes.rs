//! アーカイブの属性軸の定義と、軸の値の辞書 (→ docs/archive.md「軸の定義」・「軸の値の辞書と導出」)。
//!
//! アイテムごとの属性値はどこにも保存しない。`archive_items.rel_path` から
//! 読み出し時に導出する (→ `archive::dir_level_value`)。この2エンドポイント群は
//! その導出に使う「軸そのものの定義」と「値の表示名・並び順の上書き」を管理する。

use std::collections::HashSet;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use sqlx::{SqliteConnection, SqliteExecutor, SqlitePool};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::archive::AxisRowMatch;
use super::error_detail::ValidationDetail;
use crate::auth::AuthUser;
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

/// `archive_axes.source`。2値のみで正規表現は受け付けない (→ docs/archive.md「軸の定義」)。
///
/// DB上は `dir_level` / `filename_word` の snake_case (マイグレーションのコメント参照)。
/// JSONはプロジェクト全体の慣例 (camelCase) に合わせるため、表現をそれぞれ別に持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "camelCase")]
#[sqlx(rename_all = "snake_case")]
pub enum AxisSource {
    /// アーカイブの登録先フォルダーを起点にした第N階層のディレクトリ名。
    DirLevel,
    /// 拡張子を除いたファイル名に対する部分一致。
    FilenameWord,
}

/// `archive_axis_values.match_position`。照合語をファイル名のどこで照合するか (→ docs/archive.md「軸の定義」)。
/// `filename_word` 軸の行だけが `anywhere` 以外を持てる。
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type,
)]
#[serde(rename_all = "camelCase")]
#[sqlx(rename_all = "snake_case")]
pub enum AxisMatchPosition {
    /// ファイル名のどこでもよい。
    #[default]
    Anywhere,
    /// 語の頭。ファイル名の先頭か、直前が英数字でない位置。
    WordStart,
    /// 拡張子を除いたファイル名の末尾。
    End,
}

/// 軸名の前後の空白を落とし、非空・`{` `}` を含まないことを検証する
/// (→ docs/archive.md「軸の定義」)。プレースホルダーとして参照するための制約。
fn validate_axis_name(name: &str) -> Result<String, AppError> {
    let name = super::validate::trimmed_non_empty(name, "axis name is required")?;
    if name.contains('{') || name.contains('}') {
        return Err(AppError::ValidationDetailed {
            message: "axis name must not contain { or }".to_string(),
            detail: ValidationDetail::AxisNameHasBraces,
        });
    }
    // 予約プレースホルダーと同じ名前の軸を作れると、テンプレートでどちらを指すのか
    // 決まらなくなる (→ docs/archive.md「表示タイトル」)。
    if name == super::archive::FILE_NAME_PLACEHOLDER {
        return Err(AppError::ValidationDetailed {
            message: format!("axis name '{name}' is reserved"),
            detail: ValidationDetail::AxisNameReserved { name },
        });
    }
    // `sort` は並び順のクエリキーで予約している (→ docs/archive.md「エンドポイント一覧」, docs/ui.md「ホーム・グループ・フォルダー・アーカイブの並び順」)。
    // 同名の軸を許すと、その軸の絞り込みクエリが常に並び順として消費され、
    // 絞り込めなくなる (`build_archive_view` が `"sort"` キーを軸名より先に取り出すため)。
    // `filter` は画面のページ内の絞り込みのクエリキーで、画面がサーバーへ送らずに取り除くので、同名の軸は絞り込めない
    // (→ docs/ui.md「一覧の絞り込み」)。
    if name == "sort" || name == "filter" {
        return Err(AppError::ValidationDetailed {
            message: format!("axis name '{name}' is reserved"),
            detail: ValidationDetail::AxisNameReserved { name },
        });
    }
    Ok(name)
}

/// `source` と `dir_level` の組み合わせを検証する。`dir_level` 軸には1始まりの
/// 階層番号が必須で、`filename_word` 軸には持たせない (無関係な値が紛れ込むのを防ぐ)。
fn validate_axis_source(source: AxisSource, dir_level: Option<i64>) -> Result<(), AppError> {
    match source {
        AxisSource::DirLevel if dir_level.is_none_or(|level| level < 1) => Err(
            AppError::Validation("dirLevel axis requires dirLevel >= 1".to_string()),
        ),
        AxisSource::FilenameWord if dir_level.is_some() => Err(AppError::Validation(
            "filenameWord axis must not have dirLevel".to_string(),
        )),
        _ => Ok(()),
    }
}

fn axis_name_taken(name: String) -> AppError {
    AppError::ValidationDetailed {
        message: format!("axis name '{name}' is already taken"),
        detail: ValidationDetail::AxisNameTaken { name },
    }
}

/// 同じアーカイブ内で軸名が重複していないことを確認する (→ docs/archive.md「軸の定義」)。
/// `exclude_axis_id` は更新時、自分自身を重複判定から除くために使う。
async fn ensure_axis_name_available(
    pool: &SqlitePool,
    archive_id: i64,
    name: &str,
    exclude_axis_id: Option<i64>,
) -> Result<(), AppError> {
    // `exclude_axis_id` が `None` なら `id IS NOT NULL` になり、全行が対象になる。
    let exists = sqlx::query_scalar!(
        r#"SELECT EXISTS(
           SELECT 1 FROM archive_axes WHERE archive_id = ? AND name = ? AND id IS NOT ?
       ) as "exists!: bool""#,
        archive_id,
        name,
        exclude_axis_id
    )
    .fetch_one(pool)
    .await?;
    if exists {
        return Err(axis_name_taken(name.to_string()));
    }
    Ok(())
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AxisResponse {
    id: i64,
    name: String,
    source: AxisSource,
    dir_level: Option<i64>,
    /// 値が無くても表示タイトルを組み立てるか (→ docs/archive.md「表示タイトル」)。
    optional_in_title: bool,
    /// 閲覧側の絞り込みに出すか (→ docs/archive.md「軸の定義」)。
    filterable: bool,
    position: i64,
}

fn default_filterable() -> bool {
    true
}

/// 軸の追加と変更で共通の要求ボディ。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AxisRequest {
    name: String,
    source: AxisSource,
    #[serde(default)]
    dir_level: Option<i64>,
    #[serde(default)]
    optional_in_title: bool,
    #[serde(default = "default_filterable")]
    #[schema(default = true)]
    filterable: bool,
    #[serde(default)]
    position: i64,
}

/// 軸の一覧。並び替えUIでの左からの順番 (`position`) 順に返す。
#[utoipa::path(
    get,
    path = "/contents/{id}/axes",
    params(("id" = i64, Path)),
    responses(
        (status = OK, body = Vec<AxisResponse>, description = "軸の一覧"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・アーカイブでない"),
    )
)]
async fn list_axes(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<AxisResponse>>, AppError> {
    super::archive::ensure_manageable_archive(&state.pool, &user, id).await?;

    let rows = sqlx::query_as!(
        AxisResponse,
        r#"SELECT id as "id!", name, source as "source: AxisSource", dir_level,
                  optional_in_title as "optional_in_title: bool",
                  filterable as "filterable: bool", position
           FROM archive_axes WHERE archive_id = ? ORDER BY position, id"#,
        id
    )
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(rows))
}

/// 軸の追加。`user` は自分が作ったアーカイブだけ (→ docs/access.md「ロールと操作」)。
#[utoipa::path(
    post,
    path = "/contents/{id}/axes",
    params(("id" = i64, Path)),
    request_body = AxisRequest,
    responses(
        (status = 201, body = AxisResponse, description = "作成した軸"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "`user` から見て他人が作ったもの"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・アーカイブでない"),
        (status = 422, body = crate::error::ErrorResponse, description = "軸名が空・重複・使えない文字を含む、階層番号の指定が不正"),
    )
)]
async fn create_axis(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<AxisRequest>,
) -> Result<(StatusCode, Json<AxisResponse>), AppError> {
    // アーカイブの存在確認からINSERTまでを直列化する (→ `AppState::contents_write_lock`)。
    // この鍵は`title_template`のリネーム時の書き換え (→ update_axis) とも共有する。
    let _write_guard = state.contents_write_lock.lock().await;
    super::archive::ensure_editable_archive(&state.pool, &user, id).await?;
    // 入力の検証は存在の判定 (404) の後に行う。軸 API とコンテンツの更新で順を揃える。
    let name = validate_axis_name(&payload.name)?;
    validate_axis_source(payload.source, payload.dir_level)?;
    ensure_axis_name_available(&state.pool, id, &name, None).await?;

    let row = insert_axis(
        &state.pool,
        id,
        NewAxis {
            name: &name,
            source: payload.source,
            dir_level: payload.dir_level,
            optional_in_title: payload.optional_in_title,
            filterable: payload.filterable,
            position: payload.position,
        },
    )
    .await?;

    Ok((StatusCode::CREATED, Json(row)))
}

/// 追加する軸。検証を済ませた値を渡す。
struct NewAxis<'a> {
    name: &'a str,
    source: AxisSource,
    dir_level: Option<i64>,
    optional_in_title: bool,
    filterable: bool,
    position: i64,
}

async fn insert_axis(
    db: impl SqliteExecutor<'_>,
    archive_id: i64,
    axis: NewAxis<'_>,
) -> Result<AxisResponse, AppError> {
    let row = sqlx::query_as!(
        AxisResponse,
        r#"INSERT INTO archive_axes
               (archive_id, name, source, dir_level, optional_in_title, filterable, position)
           VALUES (?, ?, ?, ?, ?, ?, ?)
           RETURNING id as "id!", name, source as "source: AxisSource", dir_level,
                     optional_in_title as "optional_in_title: bool",
                     filterable as "filterable: bool", position"#,
        archive_id,
        axis.name,
        axis.source,
        axis.dir_level,
        axis.optional_in_title,
        axis.filterable,
        axis.position,
    )
    .fetch_one(db)
    .await?;
    Ok(row)
}

/// 軸 `axis_id` の値の辞書に `rows` を足す。今の行を消すのは呼び出し側。
async fn insert_axis_values(
    conn: &mut SqliteConnection,
    axis_id: i64,
    rows: &[super::archive::AxisDictRow],
) -> Result<(), AppError> {
    for row in rows {
        sqlx::query!(
            "INSERT INTO archive_axis_values (axis_id, raw_value, display_name, position, match_position)
             VALUES (?, ?, ?, ?, ?)",
            axis_id,
            row.raw_value,
            row.display_name,
            row.position,
            row.match_position,
        )
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// 軸の名前・抽出元・階層番号・表示タイトルでの扱い・並び順の変更。権限は `create_axis` と同じ。
///
/// 名前を変えた場合、`contents.title_template` 中の該当プレースホルダーを
/// 同一トランザクションで書き換える (→ docs/archive.md「表示タイトル」)。別々のコミットに
/// 分けると、書き換え漏れの状態 (旧名のまま残ったプレースホルダー) が観測されうる。
#[utoipa::path(
    put,
    path = "/contents/{id}/axes/{axis_id}",
    params(("id" = i64, Path), ("axis_id" = i64, Path)),
    request_body = AxisRequest,
    responses(
        (status = OK, body = AxisResponse, description = "更新後の軸"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "`user` から見て他人が作ったもの"),
        (status = 404, body = crate::error::ErrorResponse, description = "所属しない・存在しない軸"),
        (status = 422, body = crate::error::ErrorResponse, description = "軸名が空・重複・使えない文字を含む、階層番号の指定が不正"),
    )
)]
async fn update_axis(
    user: AuthUser,
    State(state): State<AppState>,
    Path((id, axis_id)): Path<(i64, i64)>,
    AppJson(payload): AppJson<AxisRequest>,
) -> Result<Json<AxisResponse>, AppError> {
    let _write_guard = state.contents_write_lock.lock().await;
    super::archive::ensure_editable_archive(&state.pool, &user, id).await?;

    // 所属条件 (`WHERE id = ? AND archive_id = ?`) を通ったものだけを対象にする
    // (→ docs/archive.md「エンドポイント一覧」)。別アーカイブの軸idを渡されても404にする。
    let existing = fetch_axis(&state.pool, id, axis_id).await?;
    let existing_name = existing.name;

    // 入力の検証は存在の判定 (404) の後に行う (→ create_axis)。
    let name = validate_axis_name(&payload.name)?;
    validate_axis_source(payload.source, payload.dir_level)?;
    ensure_axis_name_available(&state.pool, id, &name, Some(axis_id)).await?;

    let mut tx = crate::db::begin_write(&state.pool).await?;

    let row = sqlx::query_as!(
        AxisResponse,
        r#"UPDATE archive_axes
           SET name = ?, source = ?, dir_level = ?, optional_in_title = ?,
               filterable = ?, position = ?
           WHERE id = ? AND archive_id = ?
           RETURNING id as "id!", name, source as "source: AxisSource", dir_level,
                     optional_in_title as "optional_in_title: bool",
                     filterable as "filterable: bool", position"#,
        name,
        payload.source,
        payload.dir_level,
        payload.optional_in_title,
        payload.filterable,
        payload.position,
        axis_id,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    // 抽出元か階層番号が変わると、辞書の行は別の値の集まりに向けたものになる。
    // 残すと表示名が照合語に化けたり並び順に効いたりするため消す (→ docs/archive.md「軸の値の辞書と導出」)。
    if payload.source != existing.source || payload.dir_level != existing.dir_level {
        sqlx::query!("DELETE FROM archive_axis_values WHERE axis_id = ?", axis_id)
            .execute(&mut *tx)
            .await?;
    }

    if name != existing_name {
        // `{` `}` は軸名に使えないため、置換対象は一意に決まる (→ validate_axis_name)。
        let old_placeholder = super::archive::placeholder(&existing_name);
        let new_placeholder = super::archive::placeholder(&name);
        sqlx::query!(
            "UPDATE contents SET title_template = REPLACE(title_template, ?, ?) WHERE id = ?",
            old_placeholder,
            new_placeholder,
            id,
        )
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(Json(row))
}

/// 軸の削除。権限は `create_axis` と同じ。`archive_axis_values` は `ON DELETE CASCADE` で
/// 一緒に消える。表示タイトルのテンプレートがこの軸を参照している場合は422で拒否する
/// (→ docs/archive.md「表示タイトル」)。
#[utoipa::path(
    delete,
    path = "/contents/{id}/axes/{axis_id}",
    params(("id" = i64, Path), ("axis_id" = i64, Path)),
    responses(
        (status = 204, description = "削除した"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "`user` から見て他人が作ったもの"),
        (status = 404, body = crate::error::ErrorResponse, description = "所属しない・存在しない軸"),
        (status = 422, body = crate::error::ErrorResponse, description = "テンプレートで使用中"),
    )
)]
async fn delete_axis(
    user: AuthUser,
    State(state): State<AppState>,
    Path((id, axis_id)): Path<(i64, i64)>,
) -> Result<StatusCode, AppError> {
    let _write_guard = state.contents_write_lock.lock().await;
    super::archive::ensure_editable_archive(&state.pool, &user, id).await?;

    let name = fetch_axis(&state.pool, id, axis_id).await?.name;

    let template = sqlx::query_scalar!("SELECT title_template FROM contents WHERE id = ?", id)
        .fetch_optional(&state.pool)
        .await?
        .flatten();
    let placeholder = super::archive::placeholder(&name);
    if template.is_some_and(|template| template.contains(&placeholder)) {
        return Err(AppError::ValidationDetailed {
            message: format!("axis '{name}' is used by the title template"),
            detail: ValidationDetail::AxisInUseByTemplate { name },
        });
    }

    sqlx::query!(
        "DELETE FROM archive_axes WHERE id = ? AND archive_id = ?",
        axis_id,
        id
    )
    .execute(&state.pool)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

struct AxisInfo {
    name: String,
    source: AxisSource,
    dir_level: Option<i64>,
}

/// 所属条件を通した軸の定義を取得する。存在しない・所属しない軸は404
/// (→ docs/archive.md「エンドポイント一覧」)。別アーカイブの軸idを渡されても404にする。
async fn fetch_axis(
    pool: &SqlitePool,
    archive_id: i64,
    axis_id: i64,
) -> Result<AxisInfo, AppError> {
    sqlx::query_as!(
        AxisInfo,
        r#"SELECT name, source as "source: AxisSource", dir_level
           FROM archive_axes WHERE id = ? AND archive_id = ?"#,
        axis_id,
        archive_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AxisValueResponse {
    raw_value: String,
    display_name: Option<String>,
    /// 照合語をファイル名のどこで照合するか (→ docs/archive.md「軸の定義」)。階層の軸では常に `anywhere`。
    match_position: AxisMatchPosition,
}

/// 軸の値の辞書。`dir_level` 軸では、現在のアイテムに現れる生の値と辞書を
/// 突き合わせて返す (→ docs/archive.md「軸の値の辞書と導出」)。`filename_word` 軸は辞書の行そのものが
/// 照合語リストなので、辞書をそのまま返す。
#[utoipa::path(
    get,
    path = "/contents/{id}/axes/{axis_id}/values",
    params(("id" = i64, Path), ("axis_id" = i64, Path)),
    responses(
        (status = OK, body = Vec<AxisValueResponse>, description = "軸の値の一覧"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "所属しない・存在しない軸"),
    )
)]
async fn list_axis_values(
    user: AuthUser,
    State(state): State<AppState>,
    Path((id, axis_id)): Path<(i64, i64)>,
) -> Result<Json<Vec<AxisValueResponse>>, AppError> {
    super::archive::ensure_manageable_archive(&state.pool, &user, id).await?;
    let axis = fetch_axis(&state.pool, id, axis_id).await?;
    let dict = super::archive::load_axis_dict(&state.pool, axis_id).await?;

    let values = match axis.source {
        // 辞書行そのものが照合語リストなので、そのまま返す (→ docs/archive.md「軸の値の辞書と導出」)。
        AxisSource::FilenameWord => dict
            .into_iter()
            .map(|row| AxisValueResponse {
                raw_value: row.raw_value,
                display_name: row.display_name,
                match_position: row.match_position,
            })
            .collect(),
        AxisSource::DirLevel => {
            let dir_level = super::archive::require_dir_level(axis.dir_level)?;

            let rel_paths = super::archive::load_item_rel_paths(&state.pool, id).await?;
            // 走査と並べ替えはアイテム数に比例するので、非同期のワーカーを塞がない (→ docs/archive.md「軸の設定を助ける表示」)。
            run_blocking(move || {
                let raw_values: HashSet<&str> = rel_paths
                    .iter()
                    .filter_map(|rel_path| super::archive::dir_level_value(rel_path, dir_level))
                    .collect();

                // 並び順はアイテムの並び替えと同じ `ValueKey` の順序 (→ docs/archive.md「軸の値の辞書と導出」)。
                let dict = super::archive::DirLevelDict::new(dict);
                let mut entries: Vec<(super::archive::ValueKey, AxisValueResponse)> = raw_values
                    .into_iter()
                    .map(|raw_value| {
                        let (key, display_name) = dict.lookup(raw_value);
                        let value = AxisValueResponse {
                            raw_value: raw_value.to_string(),
                            display_name: display_name.map(str::to_string),
                            match_position: AxisMatchPosition::Anywhere,
                        };
                        (key, value)
                    })
                    .collect();
                // 表示名が同じ値は比較キーも同じになるので、生の値で順を決める。
                entries.sort_by(|a, b| {
                    a.0.cmp(&b.0)
                        .then_with(|| a.1.raw_value.cmp(&b.1.raw_value))
                });

                entries.into_iter().map(|(_, value)| value).collect()
            })
            .await?
        }
    };

    Ok(Json(values))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct AxisValueInput {
    raw_value: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    match_position: AxisMatchPosition,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceAxisValuesRequest {
    values: Vec<AxisValueInput>,
}

/// 軸の値の辞書の一括更新。追加・削除・表示名・並び順をまとめて送る
/// (→ docs/archive.md「エンドポイント一覧」)。送られてきた配列の順番をそのまま `position` にする
/// (行を1件ずつ動かすAPIを持たない代わりに、D&Dで並べ替えたリスト全体を送る運用)。
///
/// 既存の辞書行を全削除してから入れ直す。生の値が現在のアイテムから消えて
/// 二度と現れなくなった辞書行も、次のこの呼び出しで自然に失われる (`GET` の
/// 表示にはそもそも出てこないため、実害はない → docs/archive.md「軸の値の辞書と導出」)。
#[utoipa::path(
    put,
    path = "/contents/{id}/axes/{axis_id}/values",
    params(("id" = i64, Path), ("axis_id" = i64, Path)),
    request_body = ReplaceAxisValuesRequest,
    responses(
        (status = OK, body = Vec<AxisValueResponse>, description = "更新後の値の辞書"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "`user` から見て他人が作ったもの"),
        (status = 404, body = crate::error::ErrorResponse, description = "所属しない・存在しない軸"),
        (status = 422, body = crate::error::ErrorResponse, description = "値が空・重複・上限超え、階層の軸に anywhere 以外の照合位置"),
    )
)]
async fn replace_axis_values(
    user: AuthUser,
    State(state): State<AppState>,
    Path((id, axis_id)): Path<(i64, i64)>,
    AppJson(payload): AppJson<ReplaceAxisValuesRequest>,
) -> Result<Json<Vec<AxisValueResponse>>, AppError> {
    // 軸の削除と直列化する (`contents_write_lock` のドキュメント参照)。これが無いと、
    // 所属確認からINSERTまでの間に軸が削除され、外部キー制約違反 (500) になりうる。
    // 公開範囲の判定も鍵の内側で行い、判定の後に他人の`private`へ変わっても書き込まない。
    let _write_guard = state.contents_write_lock.lock().await;
    super::archive::ensure_editable_archive(&state.pool, &user, id).await?;
    let axis = fetch_axis(&state.pool, id, axis_id).await?;
    let rows = normalize_axis_values(axis.source, payload.values)?;

    let mut tx = crate::db::begin_write(&state.pool).await?;
    sqlx::query!("DELETE FROM archive_axis_values WHERE axis_id = ?", axis_id)
        .execute(&mut *tx)
        .await?;
    insert_axis_values(&mut tx, axis_id, &rows).await?;
    tx.commit().await?;

    Ok(Json(
        rows.into_iter()
            .map(|row| AxisValueResponse {
                raw_value: row.raw_value,
                display_name: row.display_name,
                match_position: row.match_position,
            })
            .collect(),
    ))
}

/// ファイル名の語の軸の、照合語の行数の上限。階層の軸には掛けない (→ docs/archive.md「軸の設定を助ける表示」)。
const FILENAME_WORD_VALUES_LIMIT: usize = 1_000;

/// 値の辞書の要求ボディを検証し、保存する形の行にする。一括更新とプレビューで共有し、
/// プレビューで通った辞書が保存でも同じ行になるようにする。
///
/// 送られてきた配列の順番をそのまま `position` にする。照合する位置は `filename_word` 軸の行だけが
/// `anywhere` 以外を持てる (無関係な値が紛れ込むのを防ぐ)。
fn normalize_axis_values(
    source: AxisSource,
    values: Vec<AxisValueInput>,
) -> Result<Vec<super::archive::AxisDictRow>, AppError> {
    if source == AxisSource::FilenameWord && values.len() > FILENAME_WORD_VALUES_LIMIT {
        return Err(AppError::Validation(format!(
            "too many values (limit {FILENAME_WORD_VALUES_LIMIT})"
        )));
    }
    let mut seen = HashSet::new();
    let mut rows = Vec::with_capacity(values.len());
    for (position, entry) in values.into_iter().enumerate() {
        let raw_value =
            super::validate::trimmed_non_empty(&entry.raw_value, "value must not be empty")?;
        if !seen.insert(raw_value.clone()) {
            return Err(AppError::Validation(format!(
                "duplicate value '{raw_value}'"
            )));
        }
        if source == AxisSource::DirLevel && entry.match_position != AxisMatchPosition::Anywhere {
            return Err(AppError::Validation(
                "dirLevel axis values must use matchPosition anywhere".to_string(),
            ));
        }
        let display_name =
            super::validate::trimmed_or_none(entry.display_name.as_deref()).map(str::to_string);
        rows.push(super::archive::AxisDictRow {
            raw_value,
            display_name,
            position: i64::try_from(position).expect("要求ボディの件数がi64に収まらない"),
            match_position: entry.match_position,
        });
    }
    Ok(rows)
}

/// 未設定のアイテムや、辞書の行ごとに当たるアイテムの例として返す件数。
/// どう照合語を足せばよいか、語が何を指すかの見当が付けば足りる。
const EXAMPLE_LIMIT: usize = 5;

/// 例として返すアイテム。管理画面から開けるよう `id` も返す。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AxisValuesPreviewItem {
    id: i64,
    rel_path: String,
}

/// 辞書の1行で値が決まるアイテム (→ docs/archive.md「軸の定義」)。
#[derive(Debug, Default, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AxisValuesPreviewRow {
    /// この行で値が決まるアイテム数。ほかの行のほうが長く一致したアイテムは数えない。
    matched: usize,
    /// その `rel_path` 昇順の最大5件。
    examples: Vec<AxisValuesPreviewItem>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AxisValuesPreviewResponse {
    /// アーカイブの全アイテム数。`matched + unset` と等しい。
    item_count: usize,
    /// 値が決まったアイテム数。
    matched: usize,
    /// 未設定のアイテム数。
    unset: usize,
    /// 未設定のアイテムの `rel_path`。昇順で最大5件。
    unset_examples: Vec<String>,
    /// 送られてきた辞書の行ごとの結果。行と同じ順・同じ数。
    /// `filename_word` 軸だけが返し、`dir_level` 軸では空。
    rows: Vec<AxisValuesPreviewRow>,
}

/// 保存前の値の辞書で、全アイテムの軸の値を導出した結果の集計。書き込みはしない。
///
/// 軸の定義は保存済みのものを使い、辞書だけを送られてきたものに差し替える。
/// 導出は表示と同じ `archive::derive_axis_value` を通すので、保存後の結果と食い違わない。
#[utoipa::path(
    post,
    path = "/contents/{id}/axes/{axis_id}/values/preview",
    params(("id" = i64, Path), ("axis_id" = i64, Path)),
    request_body = ReplaceAxisValuesRequest,
    responses(
        (status = OK, body = AxisValuesPreviewResponse, description = "一致した件数・未設定の件数と例、辞書の行ごとの件数と例"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 404, body = crate::error::ErrorResponse, description = "所属しない・存在しない軸"),
        (status = 422, body = crate::error::ErrorResponse, description = "値が空・重複・上限超え、階層の軸に anywhere 以外の照合位置"),
    )
)]
async fn preview_axis_values(
    user: AuthUser,
    State(state): State<AppState>,
    Path((id, axis_id)): Path<(i64, i64)>,
    AppJson(payload): AppJson<ReplaceAxisValuesRequest>,
) -> Result<Json<AxisValuesPreviewResponse>, AppError> {
    super::archive::ensure_manageable_archive(&state.pool, &user, id).await?;
    let axis = fetch_axis(&state.pool, id, axis_id).await?;
    let rows = normalize_axis_values(axis.source, payload.values)?;
    let row_count = match axis.source {
        AxisSource::FilenameWord => rows.len(),
        AxisSource::DirLevel => 0,
    };
    let dict = super::archive::AxisDict::new(axis.source, axis.dir_level, rows)?;

    let mut items = sqlx::query_as!(
        AxisValuesPreviewItem,
        r#"SELECT id as "id!", rel_path FROM archive_items WHERE archive_id = ?"#,
        id
    )
    .fetch_all(&state.pool)
    .await?;
    // 例を `rel_path` の昇順で取るため、先に並べておく。
    items.sort_unstable_by(|a, b| a.rel_path.cmp(&b.rel_path));
    // 非同期のワーカーを塞がない (→ docs/archive.md「軸の設定を助ける表示」)。
    let response = run_blocking(move || {
        let item_count = items.len();
        let mut unset_examples = Vec::new();
        let mut unset = 0;
        let mut rows: Vec<AxisValuesPreviewRow> =
            std::iter::repeat_with(AxisValuesPreviewRow::default)
                .take(row_count)
                .collect();
        for item in items {
            match super::archive::match_axis_row(&dict, &item.rel_path) {
                AxisRowMatch::Unset => {
                    unset += 1;
                    if unset_examples.len() < EXAMPLE_LIMIT {
                        unset_examples.push(item.rel_path);
                    }
                }
                AxisRowMatch::DirLevel => {}
                AxisRowMatch::FilenameWordRow(index) => {
                    let row = &mut rows[index];
                    row.matched += 1;
                    if row.examples.len() < EXAMPLE_LIMIT {
                        row.examples.push(item);
                    }
                }
            }
        }

        AxisValuesPreviewResponse {
            item_count,
            matched: item_count - unset,
            unset,
            unset_examples,
            rows,
        }
    })
    .await?;
    Ok(Json(response))
}

/// 外部の AI が考えた軸1本ぶんの定義 (→ docs/archive.md「軸の定義を外部の AI で作る」)。
/// 形は軸の追加・値の辞書の更新の要求ボディを1つにまとめたもの。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportAxisInput {
    name: String,
    source: AxisSource,
    #[serde(default)]
    dir_level: Option<i64>,
    #[serde(default)]
    optional_in_title: bool,
    #[serde(default = "default_filterable")]
    #[schema(default = true)]
    filterable: bool,
    #[serde(default)]
    values: Vec<AxisValueInput>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportAxesRequest {
    axes: Vec<ImportAxisInput>,
    #[serde(default)]
    title_template: Option<String>,
}

/// 確かめ終えた、取り込む軸1本。
struct ImportAxis {
    name: String,
    source: AxisSource,
    dir_level: Option<i64>,
    optional_in_title: bool,
    filterable: bool,
    rows: Vec<super::archive::AxisDictRow>,
}

/// 取り込む定義を、軸の追加・値の辞書の更新・テンプレートの保存と同じ決まりで確かめる。
/// 予告と取り込みで共有し、予告で通った定義が取り込みでも通るようにする。
fn validate_import(
    payload: ImportAxesRequest,
) -> Result<(Vec<ImportAxis>, Option<String>), AppError> {
    let mut names = HashSet::new();
    let mut axes = Vec::with_capacity(payload.axes.len());
    for axis in payload.axes {
        let name = validate_axis_name(&axis.name)?;
        if !names.insert(name.clone()) {
            return Err(axis_name_taken(name));
        }
        // 値が無いまま返る決まり (階層の番号・値の空や重複) は、どの軸かを添えて返す。
        let in_axis = |error: AppError| match error {
            AppError::Validation(message) => AppError::ValidationDetailed {
                message: format!("axis '{name}': {message}"),
                detail: ValidationDetail::ImportAxisInvalid { name: name.clone() },
            },
            other => other,
        };
        validate_axis_source(axis.source, axis.dir_level).map_err(in_axis)?;
        let rows = normalize_axis_values(axis.source, axis.values).map_err(in_axis)?;
        axes.push(ImportAxis {
            name,
            source: axis.source,
            dir_level: axis.dir_level,
            optional_in_title: axis.optional_in_title,
            filterable: axis.filterable,
            rows,
        });
    }
    let template =
        super::archive::check_title_template(payload.title_template.as_deref(), |name| {
            names.contains(name)
        })?;
    Ok((axes, template))
}

/// 予告で表示タイトルと値を見せるアイテムの数。決まりが思ったとおりに当たるかの見当が付けば足りる。
const IMPORT_EXAMPLE_LIMIT: usize = 10;

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewAxis {
    name: String,
    /// 値が決まったアイテム数。
    matched: usize,
    /// 未設定のアイテム数。
    unset: usize,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewItem {
    rel_path: String,
    /// 取り込んだ後の表示タイトル。
    title: String,
    /// 軸ごとの値の表示名。`axes` と同じ順。`None` は未設定。
    values: Vec<Option<String>>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewResponse {
    item_count: usize,
    axes: Vec<ImportPreviewAxis>,
    /// `rel_path` の順に、全体から間を空けて選んだアイテム。別のフォルダーのものも入るように。
    examples: Vec<ImportPreviewItem>,
    /// 取り込むと置き換わる、今の軸か表示タイトルのテンプレートがあるか。
    replaces_existing: bool,
}

/// 外部の AI が考えた軸の定義を、取り込む前に全アイテムで導出して見せる。書き込みはしない。
/// 導出は表示と同じ `archive::derive_item` を通すので、取り込んだ後の結果と食い違わない。
#[utoipa::path(
    post,
    path = "/contents/{id}/axes/import/preview",
    params(("id" = i64, Path)),
    request_body = ImportAxesRequest,
    responses(
        (status = OK, body = ImportPreviewResponse, description = "軸ごとの件数と、アイテムの例の表示タイトル・値"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "`user` から見て他人が作ったもの"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・アーカイブでない"),
        (status = 422, body = crate::error::ErrorResponse, description = "軸名・階層番号・値・テンプレートが決まりに合わない"),
    )
)]
async fn preview_import_axes(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<ImportAxesRequest>,
) -> Result<Json<ImportPreviewResponse>, AppError> {
    super::archive::ensure_editable_archive(&state.pool, &user, id).await?;
    let (axes, template) = validate_import(payload)?;
    let index = axes
        .into_iter()
        .map(|axis| {
            let dict = super::archive::AxisDict::new(axis.source, axis.dir_level, axis.rows)?;
            Ok(super::archive::AxisIndex::new(
                axis.name,
                axis.filterable,
                axis.optional_in_title,
                dict,
            ))
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let replaces_existing = sqlx::query_scalar!(
        r#"SELECT (EXISTS(SELECT 1 FROM archive_axes WHERE archive_id = ?1)
                   OR title_template IS NOT NULL) as "replaces!: bool"
           FROM contents WHERE id = ?1"#,
        id
    )
    .fetch_one(&state.pool)
    .await?;
    let mut rel_paths = sqlx::query_scalar!(
        "SELECT rel_path FROM archive_items WHERE archive_id = ?",
        id
    )
    .fetch_all(&state.pool)
    .await?;
    rel_paths.sort_unstable();

    // 件数に比例して重くなるので、非同期のワーカーを塞がない。
    let response = run_blocking(move || {
        let item_count = rel_paths.len();
        let mut counts = vec![0usize; index.len()];
        // 全体から間を空けて、ちょうど上限の件数を選ぶ (画面のプロンプトのパスの選び方と同じ)。
        let example_positions: HashSet<usize> = (0..IMPORT_EXAMPLE_LIMIT.min(item_count))
            .map(|i| i * item_count / IMPORT_EXAMPLE_LIMIT.min(item_count))
            .collect();
        let mut examples = Vec::new();
        for (position, rel_path) in rel_paths.iter().enumerate() {
            let derived = super::archive::derive_item(rel_path, &index, template.as_deref());
            for (count, value) in counts.iter_mut().zip(&derived.axis_values) {
                if value.is_some() {
                    *count += 1;
                }
            }
            if example_positions.contains(&position) {
                examples.push(ImportPreviewItem {
                    rel_path: rel_path.clone(),
                    title: derived.title,
                    values: derived
                        .axis_values
                        .into_iter()
                        .map(|value| value.map(|value| value.display))
                        .collect(),
                });
            }
        }
        ImportPreviewResponse {
            item_count,
            axes: index
                .iter()
                .zip(counts)
                .map(|(axis, matched)| ImportPreviewAxis {
                    name: axis.name.clone(),
                    matched,
                    unset: item_count - matched,
                })
                .collect(),
            examples,
            replaces_existing,
        }
    })
    .await?;
    Ok(Json(response))
}

/// 外部の AI が考えた軸の定義で、今の軸・値の辞書・表示タイトルのテンプレートをまとめて置き換える。
/// 権限は `create_axis` と同じ。1つのトランザクションで行い、途中の状態を見せない。
#[utoipa::path(
    put,
    path = "/contents/{id}/axes",
    params(("id" = i64, Path)),
    request_body = ImportAxesRequest,
    responses(
        (status = OK, body = Vec<AxisResponse>, description = "置き換えた後の軸の一覧"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "`user` から見て他人が作ったもの"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・アーカイブでない"),
        (status = 422, body = crate::error::ErrorResponse, description = "軸名・階層番号・値・テンプレートが決まりに合わない"),
    )
)]
async fn replace_axes(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<ImportAxesRequest>,
) -> Result<Json<Vec<AxisResponse>>, AppError> {
    let _write_guard = state.contents_write_lock.lock().await;
    super::archive::ensure_editable_archive(&state.pool, &user, id).await?;
    let (axes, template) = validate_import(payload)?;

    let mut tx = crate::db::begin_write(&state.pool).await?;
    // 値の辞書は `ON DELETE CASCADE` で一緒に消える。
    sqlx::query!("DELETE FROM archive_axes WHERE archive_id = ?", id)
        .execute(&mut *tx)
        .await?;
    let mut created = Vec::with_capacity(axes.len());
    for (position, axis) in axes.into_iter().enumerate() {
        let position = i64::try_from(position).expect("軸の数がi64に収まらない");
        let row = insert_axis(
            &mut *tx,
            id,
            NewAxis {
                name: &axis.name,
                source: axis.source,
                dir_level: axis.dir_level,
                optional_in_title: axis.optional_in_title,
                filterable: axis.filterable,
                position,
            },
        )
        .await?;
        insert_axis_values(&mut tx, row.id, &axis.rows).await?;
        created.push(row);
    }
    sqlx::query!(
        "UPDATE contents SET title_template = ? WHERE id = ?",
        template,
        id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok(Json(created))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_axes, create_axis, replace_axes))
        .routes(routes!(preview_import_axes))
        .routes(routes!(update_axis, delete_axis))
        .routes(routes!(list_axis_values, replace_axis_values))
        .routes(routes!(preview_axis_values))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(json: &str) -> Result<(Vec<ImportAxis>, Option<String>), AppError> {
        validate_import(serde_json::from_str(json).expect("invalid ImportAxesRequest json"))
    }

    fn detail(json: &str) -> ValidationDetail {
        match validate(json) {
            Err(AppError::ValidationDetailed { detail, .. }) => detail,
            Err(other) => panic!("expected a detailed validation error, got {other:?}"),
            Ok(_) => panic!("expected the definition to be rejected"),
        }
    }

    fn is_validation<T>(result: &Result<T, AppError>) -> bool {
        matches!(
            result,
            Err(AppError::Validation(_) | AppError::ValidationDetailed { .. })
        )
    }

    /// 前後の空白を落とす。空・`{` `}` を含む名前・予約した名前 (`fileName`・`sort`・`filter`) は弾く。
    #[test]
    fn validate_axis_name_rejects_names_unusable_as_placeholders() {
        assert_eq!(validate_axis_name(" 科目 ").expect("通るはず"), "科目");
        for name in [
            "", "  ", "{科目}", "科{目", "科目}", "fileName", "sort", "filter",
        ] {
            assert!(is_validation(&validate_axis_name(name)), "{name:?}");
        }
        assert!(matches!(
            validate_axis_name("{科目}"),
            Err(AppError::ValidationDetailed {
                detail: ValidationDetail::AxisNameHasBraces,
                ..
            })
        ));
        assert!(matches!(
            validate_axis_name("sort"),
            Err(AppError::ValidationDetailed {
                detail: ValidationDetail::AxisNameReserved { .. },
                ..
            })
        ));
    }

    /// 階層の軸には 1 以上の階層番号が要り、ファイル名の語の軸には持たせない。
    #[test]
    fn validate_axis_source_matches_the_source_and_dir_level() {
        assert!(validate_axis_source(AxisSource::DirLevel, Some(1)).is_ok());
        assert!(validate_axis_source(AxisSource::FilenameWord, None).is_ok());
        for (source, dir_level) in [
            (AxisSource::DirLevel, None),
            (AxisSource::DirLevel, Some(0)),
            (AxisSource::FilenameWord, Some(1)),
        ] {
            assert!(
                is_validation(&validate_axis_source(source, dir_level)),
                "{source:?} {dir_level:?}"
            );
        }
    }

    fn values(raw_values: &[&str]) -> Vec<AxisValueInput> {
        raw_values
            .iter()
            .map(|raw| AxisValueInput {
                raw_value: raw.to_string(),
                display_name: None,
                match_position: AxisMatchPosition::Anywhere,
            })
            .collect()
    }

    /// 送られた順を `position` にし、生の値と表示名の前後の空白を落とす。空の表示名は付けていないものとみなす。
    #[test]
    fn normalize_axis_values_trims_and_keeps_the_order() {
        let input = vec![
            AxisValueInput {
                raw_value: " listening ".to_string(),
                display_name: Some(" リスニング ".to_string()),
                match_position: AxisMatchPosition::End,
            },
            AxisValueInput {
                raw_value: "reading".to_string(),
                display_name: Some("  ".to_string()),
                match_position: AxisMatchPosition::Anywhere,
            },
        ];

        let rows = normalize_axis_values(AxisSource::FilenameWord, input).expect("通るはず");

        let got: Vec<_> = rows
            .iter()
            .map(|row| {
                (
                    row.raw_value.as_str(),
                    row.display_name.as_deref(),
                    row.position,
                    row.match_position,
                )
            })
            .collect();
        assert_eq!(
            got,
            vec![
                ("listening", Some("リスニング"), 0, AxisMatchPosition::End),
                ("reading", None, 1, AxisMatchPosition::Anywhere),
            ]
        );
    }

    /// 空の値・前後の空白を落とすと重なる値は弾く。UNIQUE 制約の違反 (500) にさせないため、書く前に見る。
    #[test]
    fn normalize_axis_values_rejects_blank_and_duplicate_values() {
        for raw_values in [
            &["  "][..],
            &["listening", "listening"],
            &["listening", " listening "],
        ] {
            let result = normalize_axis_values(AxisSource::FilenameWord, values(raw_values));
            assert!(is_validation(&result), "{raw_values:?}");
        }
    }

    /// 照合語の行数の上限はファイル名の語の軸だけに掛ける。
    #[test]
    fn normalize_axis_values_limits_only_filename_word_axes() {
        let words = |count: usize| -> Vec<String> { (0..count).map(|i| format!("w{i}")).collect() };
        let input = |count: usize| {
            let words = words(count);
            values(&words.iter().map(String::as_str).collect::<Vec<_>>())
        };

        assert!(
            normalize_axis_values(AxisSource::FilenameWord, input(FILENAME_WORD_VALUES_LIMIT))
                .is_ok()
        );
        assert!(is_validation(&normalize_axis_values(
            AxisSource::FilenameWord,
            input(FILENAME_WORD_VALUES_LIMIT + 1)
        )));
        assert!(
            normalize_axis_values(AxisSource::DirLevel, input(FILENAME_WORD_VALUES_LIMIT + 1))
                .is_ok()
        );
    }

    /// 照合する位置は、ファイル名の語の軸の行だけが `anywhere` 以外を持てる。
    #[test]
    fn normalize_axis_values_keeps_dir_level_rows_matching_anywhere() {
        let input = |match_position| {
            vec![AxisValueInput {
                raw_value: "2024".to_string(),
                display_name: None,
                match_position,
            }]
        };

        assert!(
            normalize_axis_values(AxisSource::DirLevel, input(AxisMatchPosition::Anywhere)).is_ok()
        );
        assert!(is_validation(&normalize_axis_values(
            AxisSource::DirLevel,
            input(AxisMatchPosition::WordStart)
        )));
        assert!(
            normalize_axis_values(
                AxisSource::FilenameWord,
                input(AxisMatchPosition::WordStart)
            )
            .is_ok()
        );
    }

    #[test]
    fn validate_import_names_the_axis_whose_definition_is_invalid() {
        let detail = detail(r#"{"axes":[{"name":"年度","source":"dirLevel"}]}"#);
        assert!(
            matches!(&detail, ValidationDetail::ImportAxisInvalid { name } if name == "年度"),
            "{detail:?}"
        );
    }

    #[test]
    fn validate_import_rejects_duplicate_axis_names() {
        let detail = detail(
            r#"{"axes":[{"name":"科目","source":"filenameWord"},{"name":"科目","source":"filenameWord"}]}"#,
        );
        assert!(
            matches!(&detail, ValidationDetail::AxisNameTaken { name } if name == "科目"),
            "{detail:?}"
        );
    }

    #[test]
    fn validate_import_rejects_templates_that_use_unknown_axes() {
        let detail = detail(
            r#"{"axes":[{"name":"科目","source":"filenameWord"}],"titleTemplate":"{教科}"}"#,
        );
        assert!(
            matches!(&detail, ValidationDetail::TemplateUnknownAxis { name } if name == "教科"),
            "{detail:?}"
        );
    }

    #[test]
    fn validate_import_accepts_axes_referenced_by_the_template() {
        let (axes, template) = validate(
            r#"{"axes":[{"name":"年度","source":"dirLevel","dirLevel":1}],"titleTemplate":"{年度}"}"#,
        )
        .expect("a valid definition should pass");
        assert_eq!(axes.len(), 1);
        assert_eq!(template.as_deref(), Some("{年度}"));
    }
}
