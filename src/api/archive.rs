//! アーカイブ (索引型コンテンツ) の走査と索引の同期 (→ docs/archive.md「スキャン」)。
//!
//! `contents.path` 配下を再帰的に走査し、見つかったファイルを `archive_items` に
//! 同期する。軸の値・表示タイトルの導出もここに置き、`archive_axes`・`archive_items`
//! から使う。

use std::collections::{HashMap, HashSet};
use std::path::Path as FsPath;

use axum::Json;
use axum::extract::{Path, State};
use serde::Serialize;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::error::{AppError, run_blocking};
use crate::state::AppState;

use super::archive_axes::{AxisMatchPosition, AxisSource};
use super::contents::ContentType;
use super::error_detail::ValidationDetail;

/// 保存用に `extensions` を正規化する。`fs::parse_extensions` と同じ規則で解釈し、
/// カンマ区切りの文字列に戻す。空なら `None` (全ファイルが対象)。
///
/// 保存時点で正規化しておくのは、管理画面が入力した見た目のまま (`.MP3, pdf`) を
/// 持ち回ると、走査のたびに解釈がぶれる余地を残すため。
pub(super) fn normalize_extensions(raw: Option<&str>) -> Option<String> {
    Some(super::fs::parse_extensions(raw)?.join(","))
}

/// `rel_path` の `dir_level` 段目のディレクトリ名を返す (→ docs/archive.md「軸の定義」)。
/// 最後の要素はファイル名なので対象外。階層が足りなければ `None` (「未設定」)。
///
/// 軸の値の辞書 (→ docs/archive.md「軸の値の辞書と導出」) と表示タイトルの導出の両方がこの定義を共有する必要がある
/// ため、純粋関数として切り出してある。
pub(super) fn dir_level_value(rel_path: &str, dir_level: i64) -> Option<&str> {
    let dir_level = usize::try_from(dir_level).ok()?;
    if dir_level == 0 {
        return None;
    }
    let parts: Vec<&str> = rel_path.split('/').collect();
    (parts.len() > dir_level).then(|| parts[dir_level - 1])
}

/// 拡張子を除いたファイル名。`filename_word` 軸の照合 (→ docs/archive.md「軸の定義」) と、
/// ファイル名によく出る語の集計が同じ取り方をするために共有する。
pub(super) fn file_stem(rel_path: &str) -> &str {
    FsPath::new(rel_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
}

/// アーカイブの全アイテムの `rel_path`。並びは保証しない。
pub(super) async fn load_item_rel_paths(
    pool: &sqlx::SqlitePool,
    archive_id: i64,
) -> Result<Vec<String>, AppError> {
    let rel_paths = sqlx::query_scalar!(
        "SELECT rel_path FROM archive_items WHERE archive_id = ?",
        archive_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rel_paths)
}

/// `dir_level` 軸の階層番号。保存時に必須にしてあるので、無ければデータの不整合。
pub(super) fn require_dir_level(dir_level: Option<i64>) -> Result<i64, AppError> {
    dir_level.ok_or(AppError::DataIntegrity("dir_level axis without dir_level"))
}

/// 軸名をテンプレート中のプレースホルダーの書式 (`{軸名}`) にする (→ docs/archive.md「表示タイトル」)。
pub(super) fn placeholder(name: &str) -> String {
    format!("{{{name}}}")
}

/// 軸ではなく、アイテム自身から値が決まる予約プレースホルダー (→ docs/archive.md「表示タイトル」)。
/// 拡張子を除いたファイル名に展開する。
///
/// 軸名は利用者が自由に付けられるので、表示言語によらず一意になるASCIIの名前にする。
/// この名前の軸は作れない (`archive_axes::validate_axis_name`)。
pub(super) const FILE_NAME_PLACEHOLDER: &str = "fileName";

/// テンプレートが参照するプレースホルダーの名前の集合。テンプレートが無い・解析できない
/// ときは空 (どちらもタイトルはファイル名になり、軸を1つも使わないため)。
pub(super) fn template_placeholder_names(template: Option<&str>) -> HashSet<&str> {
    template
        .and_then(|template| parse_placeholders(template).ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// 表示タイトルのテンプレート (`{軸名}` を含む文字列) からプレースホルダーの名前を
/// 取り出す。ネスト・閉じ忘れ・空の `{}` は422にする (→ docs/archive.md「表示タイトル」)。
fn parse_placeholders(template: &str) -> Result<Vec<&str>, AppError> {
    let mut placeholders = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after_open = &rest[start + 1..];
        let end = after_open
            .find('}')
            .ok_or_else(|| AppError::Validation("unclosed { in the title template".to_string()))?;
        let name = &after_open[..end];
        if name.is_empty() {
            return Err(AppError::Validation(
                "empty {} in the title template".to_string(),
            ));
        }
        if name.contains('{') {
            return Err(AppError::Validation(
                "nested { in the title template".to_string(),
            ));
        }
        placeholders.push(name);
        rest = &after_open[end + 1..];
    }
    if rest.contains('}') {
        return Err(AppError::Validation(
            "unmatched } in the title template".to_string(),
        ));
    }
    Ok(placeholders)
}

/// `title_template` の保存時検証 (→ docs/archive.md「表示タイトル」)。空白のみ・空文字列は
/// `None` に正規化する (「ファイル名をそのまま表示する」の状態と同一に扱うため)。
///
/// `archive_id` が `None` のとき (コンテンツ作成時、軸がまだ1つも存在しえない) は
/// 実在する軸名の集合を空として扱う。プレースホルダーを含む値は常に422になる。
pub(super) async fn validate_title_template(
    pool: &sqlx::SqlitePool,
    archive_id: Option<i64>,
    template: Option<&str>,
) -> Result<Option<String>, AppError> {
    // テンプレートが無ければ軸名を読まずに済ませる (コンテンツの作成・更新のたびに通るため)。
    if super::validate::trimmed_or_none(template).is_none() {
        return Ok(None);
    }
    let axis_names: HashSet<String> = match archive_id {
        Some(archive_id) => sqlx::query_scalar!(
            "SELECT name FROM archive_axes WHERE archive_id = ?",
            archive_id
        )
        .fetch_all(pool)
        .await?
        .into_iter()
        .collect(),
        None => HashSet::new(),
    };
    check_title_template(template, |name| axis_names.contains(name))
}

/// `validate_title_template` のうち、軸名の集合を受け取って確かめる部分。
/// 保存前の軸の定義 (外部の AI の答えの取り込み) でも同じ決まりで確かめるために分けている。
pub(super) fn check_title_template(
    template: Option<&str>,
    axis_exists: impl Fn(&str) -> bool,
) -> Result<Option<String>, AppError> {
    let Some(template) = super::validate::trimmed_or_none(template) else {
        return Ok(None);
    };

    let placeholders = parse_placeholders(template)?;
    if let Some(unknown) = placeholders
        .iter()
        .find(|name| **name != FILE_NAME_PLACEHOLDER && !axis_exists(name))
    {
        return Err(AppError::ValidationDetailed {
            message: format!("unknown axis '{unknown}' in the title template"),
            detail: ValidationDetail::TemplateUnknownAxis {
                name: unknown.to_string(),
            },
        });
    }

    Ok(Some(template.to_string()))
}

/// アーカイブの基本情報。走査・軸・アイテムの各処理が必要とする分をまとめて
/// 1回のSELECTで取る。
pub(super) struct ArchiveMeta {
    pub title: String,
    pub path: String,
    pub extensions: Option<String>,
    pub visibility: super::contents::Visibility,
    pub created_by: Option<i64>,
    pub title_template: Option<String>,
}

/// `id` が指す `type = 'archive'` の行を取得する。存在しない・archiveでなければ404。
pub(super) async fn load_archive(
    pool: &sqlx::SqlitePool,
    id: i64,
) -> Result<ArchiveMeta, AppError> {
    let row = sqlx::query!(
        r#"SELECT type as "content_type: ContentType", title, path, extensions,
                  visibility as "visibility: super::contents::Visibility", created_by,
                  title_template
           FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    if row.content_type != ContentType::Archive {
        return Err(AppError::NotFound);
    }
    let path = row
        .path
        .ok_or(AppError::DataIntegrity("archive content without a path"))?;
    Ok(ArchiveMeta {
        title: row.title,
        path,
        extensions: row.extensions,
        visibility: row.visibility,
        created_by: row.created_by,
        title_template: row.title_template,
    })
}

/// 管理用の操作向けに `load_archive` する。`user` から見て他人の `private` なら404
/// (→ docs/access.md「ロールと操作」)。
pub(super) async fn load_manageable_archive(
    pool: &sqlx::SqlitePool,
    user: &AuthUser,
    id: i64,
) -> Result<ArchiveMeta, AppError> {
    let meta = load_archive(pool, id).await?;
    super::contents::ensure_manageable(user, meta.visibility, meta.created_by)?;
    Ok(meta)
}

/// 書き換える操作向けに `load_archive` する。`user` が書き換えられるのは自分が作ったもの
/// だけ (→ `contents::ensure_editable`)。
pub(super) async fn load_editable_archive(
    pool: &sqlx::SqlitePool,
    user: &AuthUser,
    id: i64,
) -> Result<ArchiveMeta, AppError> {
    let meta = load_archive(pool, id).await?;
    super::contents::ensure_editable(user, meta.visibility, meta.created_by)?;
    Ok(meta)
}

/// `load_editable_archive` の、存在確認だけで足りる呼び出し元向け。
pub(super) async fn ensure_editable_archive(
    pool: &sqlx::SqlitePool,
    user: &AuthUser,
    id: i64,
) -> Result<(), AppError> {
    load_editable_archive(pool, user, id).await?;
    Ok(())
}

/// `load_manageable_archive` の、存在確認だけで足りる呼び出し元向け。
pub(super) async fn ensure_manageable_archive(
    pool: &sqlx::SqlitePool,
    user: &AuthUser,
    id: i64,
) -> Result<(), AppError> {
    load_manageable_archive(pool, user, id).await?;
    Ok(())
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RescanResponse {
    /// 新しく索引に加えたアイテム数。非公開から始まる。
    added: usize,
    /// 索引から取り除いたアイテム数。実体が消えたもの、または拡張子の対象から
    /// 外れたもの。
    removed: usize,
    /// 同期後の索引の総数。
    total: usize,
}

/// アーカイブの再スキャン。
///
/// 権限は `AuthUser`。`user` は自分が作ったものだけ (→ docs/access.md「ロールと操作」)。
#[utoipa::path(
    post,
    path = "/contents/{id}/rescan",
    params(("id" = i64, Path)),
    responses(
        (status = OK, body = RescanResponse, description = "同期した件数"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "`user` から見て他人が作ったもの"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない・アーカイブでない"),
        (status = 409, body = crate::error::ErrorResponse, description = "同じアーカイブを走査中"),
        (status = 422, body = crate::error::ErrorResponse, description = "登録先が見つからない・読めない場所がある・大きすぎる"),
    )
)]
async fn rescan(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<RescanResponse>, AppError> {
    // 他人の `private` かどうかは排他を取る前に確かめる。後にすると、走査中かどうか (409)
    // の差から存在を探れてしまう。
    ensure_editable_archive(&state.pool, &user, id).await?;

    // 走査中の更新・削除と直列化する (→ docs/archive.md「スキャン」)。二重クリックで同じ
    // 走査が二重に走るのを防ぐと同時に、走査中に `path` が変わるのも防ぐ。
    let _scan_guard = state.archive_scans.acquire(id)?;

    // 排他を取る前の確認の後に公開範囲が変わっていた場合に備え、取り直した値で判定し直す。
    // 更新は走査中なら409になるので、ここから先は公開範囲と path は変わらない。
    let archive = load_editable_archive(&state.pool, &user, id).await?;
    let extensions = super::fs::parse_extensions(archive.extensions.as_deref());

    let scan_root = archive.path.clone();
    let own_dirs = state.own_dirs.clone();
    let roots = super::roots::load_roots(&state.pool).await?;
    let result = run_blocking(move || {
        let own_dirs = super::roots::OwnDirs::resolve(&own_dirs);
        // 無いことを伝えても、任意のパスの有無を探る道具にはならない (見るのは登録済みのパスだけ)。
        // ルートの外を1つの文言に寄せる `canonical_dir_within_roots` より先に見分ける。
        if std::fs::metadata(&scan_root)
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
        {
            return Err(AppError::ValidationDetailed {
                message: "the archive folder was not found".to_string(),
                detail: ValidationDetail::ArchiveFolderMissing,
            });
        }
        // 「公開できるフォルダ」の外は再走査でも拒む (理由は `contents::resolve_path` と同じ)。
        let root = super::fs::canonical_dir_within_roots(&scan_root, &roots, &own_dirs)?;
        Ok::<_, AppError>(super::fs::scan_files(
            &root,
            extensions.as_deref(),
            super::fs::SCAN_LIMIT,
            super::fs::ITEM_LIMIT,
            &own_dirs,
        ))
    })
    .await??;

    if result.truncated {
        return Err(AppError::Validation("too many files to index".to_string()));
    }
    // 読めなかった分は走査の結果から欠けているので、同期すると公開フラグごと消える。
    if let Some(path) = result.unreadable {
        return Err(AppError::ValidationDetailed {
            message: "some folders could not be read".to_string(),
            detail: ValidationDetail::ArchiveFolderUnreadable { path },
        });
    }

    sync_items(&state, id, &archive.path, &result.rel_paths).await
}

/// 走査結果を `archive_items` へ反映する。
///
/// 走査開始時と `path` が変わっていないことを、DB更新と同じトランザクションで
/// 確認する。排他 (`archive_scans`) の取り漏らしに対する最終防御
/// (→ docs/archive.md「スキャン」)。
async fn sync_items(
    state: &AppState,
    id: i64,
    scanned_path: &str,
    rel_paths: &[String],
) -> Result<Json<RescanResponse>, AppError> {
    let mut tx = crate::db::begin_write(&state.pool).await?;

    let current_path = sqlx::query_scalar!("SELECT path FROM contents WHERE id = ?", id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
    if current_path.as_deref() != Some(scanned_path) {
        return Err(AppError::Conflict(
            "the target directory changed during the scan".to_string(),
        ));
    }

    let existing: HashSet<String> = sqlx::query_scalar!(
        "SELECT rel_path FROM archive_items WHERE archive_id = ?",
        id
    )
    .fetch_all(&mut *tx)
    .await?
    .into_iter()
    .collect();
    let found: HashSet<&str> = rel_paths.iter().map(String::as_str).collect();

    let mut added = 0;
    for rel_path in rel_paths {
        if existing.contains(rel_path.as_str()) {
            continue;
        }
        // 新規は必ず非公開から始まる (→ docs/archive.md「アイテムの公開」)。
        sqlx::query!(
            "INSERT INTO archive_items (archive_id, rel_path, published) VALUES (?, ?, 0)",
            id,
            rel_path
        )
        .execute(&mut *tx)
        .await?;
        added += 1;
    }

    let mut removed = 0;
    for rel_path in existing.iter().filter(|rel| !found.contains(rel.as_str())) {
        // 墓標は残さない。行を消すことが「一度対象から外れたら公開フラグを
        // 復元しない」の実装になる (→ docs/archive.md「スキャン」)。
        sqlx::query!(
            "DELETE FROM archive_items WHERE archive_id = ? AND rel_path = ?",
            id,
            rel_path
        )
        .execute(&mut *tx)
        .await?;
        removed += 1;
    }

    tx.commit().await?;

    Ok(Json(RescanResponse {
        added,
        removed,
        total: rel_paths.len(),
    }))
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::new().routes(routes!(rescan))
}

// --- 軸の値の導出と表示タイトルの組み立て (→ docs/archive.md「軸の値の辞書と導出」・「表示タイトル」)。
// アイテムの一覧・配信 (`archive_items`) と軸の値の辞書 (`archive_axes`) の
// 両方から使う。

/// アイテムの並び順・選択肢の並びの比較キー (→ docs/archive.md「軸の値の辞書と導出」)。
/// 軸1本ぶんの値を表す。宣言順が3段の優先順位そのもの
/// (表示名が辞書にある → 辞書に無い生の値 → 未設定)。派生 `Ord` に任せることで、
/// 段の判定と各段内の比較を別々に書かずに済む。
///
/// 表示名が同じ値は同じキーになる。選択肢をまとめる単位と並べる単位を揃えるため。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ValueKey {
    /// 表示名が一致する辞書の行がある。その行たちの最小 `position`、同値なら表示名の昇順。
    Dict(i64, String),
    /// 表示名が一致する辞書の行が無い生の値。生の値の昇順 (`dir_level` 軸のみ発生。
    /// `filename_word` 軸は辞書の行そのものが値の集合なのでこの段は無い)。
    Raw(String),
    /// 未設定。常に最後。
    Unset,
}

/// 軸1本・アイテム1件ぶんの導出結果。
#[derive(Debug, Clone)]
pub(super) struct DerivedAxisValue {
    /// 表示名。選択肢をまとめる単位で、絞り込みクエリの値もこれと比べる (→ docs/archive.md「エンドポイント一覧」)。
    pub display: String,
    pub key: ValueKey,
}

/// 値の辞書 (`archive_axis_values`) の1行。
pub(super) struct AxisDictRow {
    pub raw_value: String,
    pub display_name: Option<String>,
    pub position: i64,
    /// 照合語をファイル名のどこで照合するか。`filename_word` 軸だけが使う (→ docs/archive.md「軸の定義」)。
    pub match_position: AxisMatchPosition,
}

impl AxisDictRow {
    /// 辞書で付けた表示名。空文字は付けていないものとみなす (→ docs/archive.md「軸の値の辞書と導出」)。
    fn own_display_name(&self) -> Option<&str> {
        self.display_name.as_deref().filter(|name| !name.is_empty())
    }

    /// この行の値の表示名。付けていなければ生の値。
    fn display(&self) -> &str {
        self.own_display_name().unwrap_or(&self.raw_value)
    }
}

/// 表示名ごとの、軸の辞書の全行での最小 `position`。
struct DisplayPositions(HashMap<String, i64>);

impl DisplayPositions {
    fn new(rows: &[AxisDictRow]) -> Self {
        let mut positions: HashMap<String, i64> = HashMap::new();
        for row in rows {
            positions
                .entry(row.display().to_string())
                .and_modify(|position| *position = (*position).min(row.position))
                .or_insert(row.position);
        }
        Self(positions)
    }

    /// 表示名の比較キー。一致する行が無ければ「辞書に無い生の値」の段に入る
    /// (その場合、表示名は生の値そのもの)。
    fn key(&self, display: &str) -> ValueKey {
        match self.0.get(display) {
            Some(position) => ValueKey::Dict(*position, display.to_string()),
            None => ValueKey::Raw(display.to_string()),
        }
    }
}

/// 軸1本ぶんの値の辞書を `position` 順で読む。
pub(super) async fn load_axis_dict(
    pool: &sqlx::SqlitePool,
    axis_id: i64,
) -> Result<Vec<AxisDictRow>, AppError> {
    let rows = sqlx::query_as!(
        AxisDictRow,
        r#"SELECT raw_value, display_name, position,
                  match_position as "match_position: AxisMatchPosition"
           FROM archive_axis_values WHERE axis_id = ? ORDER BY position, id"#,
        axis_id
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// `dir_level` 軸の値の辞書。生の値で引く。
pub(super) struct DirLevelDict {
    /// 生の値 → 辞書で付けた表示名 (付けていなければ `None`)。
    display_names: HashMap<String, Option<String>>,
    positions: DisplayPositions,
}

impl DirLevelDict {
    pub(super) fn new(rows: Vec<AxisDictRow>) -> Self {
        let positions = DisplayPositions::new(&rows);
        let display_names = rows
            .iter()
            .map(|row| {
                (
                    row.raw_value.clone(),
                    row.own_display_name().map(str::to_string),
                )
            })
            .collect();
        Self {
            display_names,
            positions,
        }
    }

    /// 生の値の比較キーと、辞書で付けた表示名を返す。表示名を付けていなければ
    /// 生の値を表示名として比較キーを決める。
    pub(super) fn lookup(&self, raw: &str) -> (ValueKey, Option<&str>) {
        let display_name = self.display_names.get(raw).and_then(Option::as_deref);
        (
            self.positions.key(display_name.unwrap_or(raw)),
            display_name,
        )
    }
}

/// 軸1本ぶんの、導出に使う定義と値の辞書。
pub(super) enum AxisDict {
    DirLevel {
        dir_level: i64,
        values: DirLevelDict,
    },
    FilenameWord {
        /// 辞書の行そのものが照合語リスト。`position` 順 (読み込み時にソート済み)。
        entries: Vec<FilenameWordEntry>,
    },
}

pub(super) struct FilenameWordEntry {
    /// 大文字小文字を無視した部分一致で使う、小文字化済みの照合語。
    word_lower: String,
    /// 照合語の文字数。最長一致で比べる (→ docs/archive.md「軸の定義」)。
    len: usize,
    match_position: AxisMatchPosition,
    display: String,
    key: ValueKey,
}

/// 照合語リストを導出用に組み立てる。`rows` はリストの順 (`position`, `id`)。
fn filename_word_entries(rows: Vec<AxisDictRow>) -> Vec<FilenameWordEntry> {
    let positions = DisplayPositions::new(&rows);
    rows.iter()
        .map(|row| FilenameWordEntry {
            word_lower: row.raw_value.to_lowercase(),
            len: row.raw_value.chars().count(),
            match_position: row.match_position,
            display: row.display().to_string(),
            key: positions.key(row.display()),
        })
        .collect()
}

impl AxisDict {
    /// 軸の定義と辞書の行 (リストの順) から組み立てる。保存済みの辞書でも、保存前の
    /// 辞書 (値の辞書のプレビュー) でも、同じ組み立てを通して導出を食い違わせない。
    pub(super) fn new(
        source: AxisSource,
        dir_level: Option<i64>,
        rows: Vec<AxisDictRow>,
    ) -> Result<Self, AppError> {
        Ok(match source {
            AxisSource::DirLevel => Self::DirLevel {
                dir_level: require_dir_level(dir_level)?,
                values: DirLevelDict::new(rows),
            },
            AxisSource::FilenameWord => Self::FilenameWord {
                entries: filename_word_entries(rows),
            },
        })
    }
}

/// 軸1本の定義と値の辞書。導出に必要な分だけを持つ (`archive_axes::AxisResponse`
/// とは別に持つ: あちらはAPIレスポンス、こちらは導出計算専用の内部表現)。
pub(super) struct AxisIndex {
    pub name: String,
    /// 閲覧側の絞り込みに出すか (→ docs/archive.md「軸の定義」)。出さなくても並び順と表示タイトルには使う。
    pub filterable: bool,
    /// 値が無くても表示タイトルを組み立てるか (→ docs/archive.md「表示タイトル」)。
    optional_in_title: bool,
    dict: AxisDict,
}

impl AxisIndex {
    /// 保存前の軸の定義から組み立てる (外部の AI の答えの取り込みの予告)。保存済みのものは `load_axis_index` で読む。
    pub(super) fn new(
        name: String,
        filterable: bool,
        optional_in_title: bool,
        dict: AxisDict,
    ) -> Self {
        Self {
            name,
            filterable,
            optional_in_title,
            dict,
        }
    }
}

/// アーカイブの軸定義と値の辞書を、導出計算に使える形でまとめて読み込む。
/// `position` 順 (アイテムの既定の並び順の軸の優先順位、→ docs/archive.md「エンドポイント一覧」) で返す。
pub(super) async fn load_axis_index(
    pool: &sqlx::SqlitePool,
    archive_id: i64,
) -> Result<Vec<AxisIndex>, AppError> {
    struct AxisRow {
        id: i64,
        name: String,
        source: AxisSource,
        dir_level: Option<i64>,
        optional_in_title: bool,
        filterable: bool,
    }
    let axes = sqlx::query_as!(
        AxisRow,
        r#"SELECT id as "id!", name, source as "source: AxisSource",
                  dir_level,
                  optional_in_title as "optional_in_title: bool",
                  filterable as "filterable: bool"
           FROM archive_axes WHERE archive_id = ? ORDER BY position, id"#,
        archive_id
    )
    .fetch_all(pool)
    .await?;

    let mut result = Vec::with_capacity(axes.len());
    for axis in axes {
        let dict_rows = load_axis_dict(pool, axis.id).await?;
        let dict = AxisDict::new(axis.source, axis.dir_level, dict_rows)?;

        result.push(AxisIndex {
            name: axis.name,
            filterable: axis.filterable,
            optional_in_title: axis.optional_in_title,
            dict,
        });
    }
    Ok(result)
}

/// 照合語が、指定の位置でファイル名 (拡張子を除き小文字化済み) に現れるか (→ docs/archive.md「軸の定義」)。
fn word_matches(stem: &str, word: &str, match_position: AxisMatchPosition) -> bool {
    match match_position {
        AxisMatchPosition::Anywhere => stem.contains(word),
        AxisMatchPosition::End => stem.ends_with(word),
        // 直前が英数字でない位置。文字種の変わり目は含めない (`1kyu` の `kyu` を語の頭にしないため)。
        // `match_indices` は重ならない出現しか返さず、`xa_a_a` の `a_a` のように語の途中の出現に
        // 語の頭の出現が重なると見落とすので、出現の開始位置をすべて見る。
        AxisMatchPosition::WordStart => stem
            .char_indices()
            .filter(|(index, _)| stem[*index..].starts_with(word))
            .any(|(index, _)| {
                stem[..index]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !c.is_ascii_alphanumeric())
            }),
    }
}

/// 軸1本・アイテム1件の値を導出する (→ docs/archive.md「軸の定義」・「軸の値の辞書と導出」のアルゴリズム)。
pub(super) fn derive_axis_value(dict: &AxisDict, rel_path: &str) -> Option<DerivedAxisValue> {
    match dict {
        AxisDict::DirLevel { dir_level, values } => {
            let raw = dir_level_value(rel_path, *dir_level)?;
            let (key, display_name) = values.lookup(raw);
            Some(DerivedAxisValue {
                display: display_name.unwrap_or(raw).to_string(),
                key,
            })
        }
        AxisDict::FilenameWord { entries } => {
            let (_, entry) = match_filename_word(entries, rel_path)?;
            Some(DerivedAxisValue {
                display: entry.display.clone(),
                key: entry.key.clone(),
            })
        }
    }
}

/// ファイル名に当たる照合語の行と、リストの中での位置。
fn match_filename_word<'a>(
    entries: &'a [FilenameWordEntry],
    rel_path: &str,
) -> Option<(usize, &'a FilenameWordEntry)> {
    let stem = file_stem(rel_path).to_lowercase();
    // 位置で候補を絞ってから最長一致。`min_by_key` は同値なら先の要素を返すので、
    // 同じ長さはリストの上が残る。
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| word_matches(&stem, &entry.word_lower, entry.match_position))
        .min_by_key(|(_, entry)| std::cmp::Reverse(entry.len))
}

/// アイテムの値が決まるか、決まるならどの辞書の行でか (値の辞書の予告用)。
pub(super) enum AxisRowMatch {
    Unset,
    /// `dir_level` 軸で値が決まる。辞書に無い生の値もあるので、行は特定しない。
    DirLevel,
    /// `filename_word` 軸で、値を決めた辞書の行の位置 (辞書の行の順)。
    FilenameWordRow(usize),
}

/// `derive_axis_value` と同じ照合で、値が決まるかと、決めた行を返す。
/// 予告でアイテムごとに照合を1回で済ませるため、導出結果の組み立ては省く。
pub(super) fn match_axis_row(dict: &AxisDict, rel_path: &str) -> AxisRowMatch {
    match dict {
        AxisDict::DirLevel { dir_level, .. } => match dir_level_value(rel_path, *dir_level) {
            Some(_) => AxisRowMatch::DirLevel,
            None => AxisRowMatch::Unset,
        },
        AxisDict::FilenameWord { entries } => match match_filename_word(entries, rel_path) {
            Some((index, _)) => AxisRowMatch::FilenameWordRow(index),
            None => AxisRowMatch::Unset,
        },
    }
}

/// アイテム1件の導出結果。
pub(super) struct DerivedItem {
    pub title: String,
    /// `rel_path` の最後の要素。階層は出さない方針だが、ページ内
    /// プレイヤーへの振り分け (→ docs/ui.md「音声のページ内プレイヤー」) が拡張子で決まるため、
    /// 組み立て済みタイトルとは別に持たせる。
    pub file_name: String,
    /// `axes` と同じ順・同じ本数。`None` は「未設定」。
    pub axis_values: Vec<Option<DerivedAxisValue>>,
    /// タイトルをテンプレートで組み立てたか。ファイル名に落ちた行では、テンプレートの軸も
    /// タイトルで読めないので、行の2段目に出す (→ docs/ui.md「アーカイブの一覧画面」)。
    pub title_from_template: bool,
}

impl DerivedItem {
    /// アイテムの既定の並び順の比較キー (→ docs/archive.md「エンドポイント一覧」)。
    pub(super) fn sort_key(&self) -> Vec<ValueKey> {
        self.axis_values
            .iter()
            .map(|value| value.as_ref().map_or(ValueKey::Unset, |v| v.key.clone()))
            .collect()
    }
}

/// テンプレートのプレースホルダーを表示値で置き換えて組み立てる。
///
/// `parse_placeholders` と同じ走査で1回だけ組み立てる。全プレースホルダーを
/// 逐次 `String::replace` すると、ある軸の表示値がたまたま `{別軸名}` という
/// 文字列を含んでいた場合に、その値の中身まで後続の置換対象になってしまう
/// (`String::replace` は置換後の文字列全体を毎回再スキャンするため)。値は
/// 一度だけ、それ以上展開されない形で埋め込む必要がある。
///
/// `template` はプレースホルダーが全て `display_by_name` に存在することを
/// 呼び出し側が確認済みである前提 (`derive_item` 参照)。
fn assemble_title(template: &str, display_by_name: &HashMap<&str, &str>) -> String {
    let mut result = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        let after_open = &rest[start + 1..];
        // `parse_placeholders` が既に検証済みなので、閉じ括弧は必ず見つかる。
        let end = after_open
            .find('}')
            .expect("a validated template must have a closing brace");
        let name = &after_open[..end];
        result.push_str(display_by_name[name]);
        rest = &after_open[end + 1..];
    }
    result.push_str(rest);
    result
}

/// テンプレートが軸を1つ以上参照していて、そのどれにも値が無いか。
/// 軸を参照しない (`{fileName}` だけの) テンプレートは該当しない。
fn all_axis_placeholders_unset(
    placeholders: &[&str],
    axes: &[AxisIndex],
    axes_with_value: &HashSet<&str>,
) -> bool {
    let mut axis_placeholders = placeholders
        .iter()
        .filter(|name| axes.iter().any(|axis| axis.name == **name))
        .peekable();
    axis_placeholders.peek().is_some()
        && axis_placeholders.all(|name| !axes_with_value.contains(name))
}

/// アイテム1件を導出する。`axes` は `load_axis_index` の結果 (`position` 順)。
///
/// タイトルの組み立ては、テンプレート中の軸が1つでも「未設定」なら行わず
/// ファイル名をそのまま返す (→ docs/archive.md「表示タイトル」)。ただし値が無くても組み立てる軸は、
/// 未設定の部分を空にする。テンプレート中の軸がすべて未設定ならファイル名を返す。プレースホルダーが指す軸が
/// (通常は起こらないが) 見つからない場合、テンプレート自体が解析できない場合
/// (保存時に検証済みのため通常は起こらない) も同様にファイル名へ落とす。
/// テンプレートが原因でこの関数がエラーになることは無い。
pub(super) fn derive_item(
    rel_path: &str,
    axes: &[AxisIndex],
    template: Option<&str>,
) -> DerivedItem {
    let file_name = rel_path.rsplit('/').next().unwrap_or(rel_path).to_string();

    let axis_values: Vec<Option<DerivedAxisValue>> = axes
        .iter()
        .map(|axis| derive_axis_value(&axis.dict, rel_path))
        .collect();

    // 拡張子を除いたファイル名。軸が全て埋まっていても同じタイトルに潰れる
    // (同じフォルダに複数のファイルがある) ケースを、テンプレート側で区別できるようにする。
    let file_stem = file_name
        .rsplit_once('.')
        .map_or(file_name.as_str(), |(stem, _)| stem);

    let mut display_by_name: HashMap<&str, &str> = axes
        .iter()
        .zip(&axis_values)
        .filter_map(|(axis, value)| match value {
            Some(value) => Some((axis.name.as_str(), value.display.as_str())),
            None => axis.optional_in_title.then_some((axis.name.as_str(), "")),
        })
        .collect();
    // テンプレート中の軸がすべて未設定のときに、固定文字だけのタイトルを出さないために使う。
    let axes_with_value: HashSet<&str> = axes
        .iter()
        .zip(&axis_values)
        .filter(|(_, value)| value.is_some())
        .map(|(axis, _)| axis.name.as_str())
        .collect();
    // 予約プレースホルダー。同名の軸は作れないので、軸の値とはぶつからない。
    display_by_name.insert(FILE_NAME_PLACEHOLDER, file_stem);

    let assembled = match template.and_then(|t| parse_placeholders(t).ok().map(|p| (t, p))) {
        Some((template, placeholders))
            if placeholders
                .iter()
                .all(|name| display_by_name.contains_key(name))
                && !all_axis_placeholders_unset(&placeholders, axes, &axes_with_value) =>
        {
            Some(assemble_title(template, &display_by_name))
        }
        _ => None,
    };
    let title_from_template = assembled.is_some();
    let title = assembled.unwrap_or_else(|| file_name.clone());

    DerivedItem {
        title,
        file_name,
        axis_values,
        title_from_template,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_extensions_returns_a_canonical_list() {
        assert_eq!(
            normalize_extensions(Some(".MP3, pdf ,,")),
            Some("mp3,pdf".to_string())
        );
        assert_eq!(normalize_extensions(Some(" , ")), None);
        assert_eq!(normalize_extensions(None), None);
    }

    #[test]
    fn dir_level_value_skips_the_filename_and_missing_levels() {
        assert_eq!(dir_level_value("2024/第1回/listening.mp3", 1), Some("2024"));
        assert_eq!(
            dir_level_value("2024/第1回/listening.mp3", 2),
            Some("第1回")
        );
        // 最後の要素はファイル名なので対象外。
        assert_eq!(dir_level_value("2024/第1回/listening.mp3", 3), None);
        // 階層が足りない。
        assert_eq!(dir_level_value("listening.mp3", 1), None);
        assert_eq!(dir_level_value("listening.mp3", 0), None);
    }

    #[test]
    fn parse_placeholders_extracts_names_and_rejects_malformed_templates() {
        assert_eq!(
            parse_placeholders("固定文字のみ").unwrap(),
            Vec::<&str>::new()
        );
        assert_eq!(
            parse_placeholders("{科目}（{資料種別}）").unwrap(),
            vec!["科目", "資料種別"]
        );
        assert!(parse_placeholders("{未閉鎖").is_err());
        assert!(parse_placeholders("対応しない}").is_err());
        assert!(parse_placeholders("{{入れ子}}").is_err());
        assert!(parse_placeholders("{}").is_err());
    }

    /// 空白だけは未設定。プレースホルダーは実在する軸名か `fileName` に限る。形の誤りは `parse_placeholders` で見る。
    #[test]
    fn check_title_template_requires_known_axes() {
        let axis_exists = |name: &str| name == "科目";

        assert_eq!(
            check_title_template(None, axis_exists).expect("通るはず"),
            None
        );
        assert_eq!(
            check_title_template(Some("  "), axis_exists).expect("通るはず"),
            None
        );
        assert_eq!(
            check_title_template(Some(" {科目} {fileName} "), axis_exists)
                .expect("通るはず")
                .as_deref(),
            Some("{科目} {fileName}")
        );
        assert!(matches!(
            check_title_template(Some("{教科}"), axis_exists),
            Err(AppError::ValidationDetailed {
                detail: ValidationDetail::TemplateUnknownAxis { name },
                ..
            }) if name == "教科"
        ));
        assert!(matches!(
            check_title_template(Some("{科目"), axis_exists),
            Err(AppError::Validation(_))
        ));
    }

    /// 3段の優先順位 (辞書あり → 辞書なしの生の値 → 未設定) が
    /// 派生 `Ord` の宣言順・フィールド順どおりになることを確認する (→ docs/archive.md「軸の値の辞書と導出」)。
    #[test]
    fn value_key_orders_dict_before_raw_before_unset() {
        let mut keys = vec![
            ValueKey::Unset,
            ValueKey::Raw("b".to_string()),
            ValueKey::Dict(1, "国語".to_string()),
            ValueKey::Raw("a".to_string()),
            ValueKey::Dict(0, "数学".to_string()),
        ];
        keys.sort();
        assert_eq!(
            keys,
            vec![
                ValueKey::Dict(0, "数学".to_string()),
                ValueKey::Dict(1, "国語".to_string()),
                ValueKey::Raw("a".to_string()),
                ValueKey::Raw("b".to_string()),
                ValueKey::Unset,
            ]
        );
    }

    fn dict_rows(dict: &[(&str, Option<&str>, i64)]) -> Vec<AxisDictRow> {
        dict.iter()
            .map(|(raw, display, position)| AxisDictRow {
                raw_value: raw.to_string(),
                display_name: display.map(str::to_string),
                position: *position,
                match_position: AxisMatchPosition::Anywhere,
            })
            .collect()
    }

    fn dir_level_axis(name: &str, dir_level: i64, dict: &[(&str, Option<&str>, i64)]) -> AxisIndex {
        AxisIndex {
            name: name.to_string(),
            filterable: true,
            optional_in_title: false,
            dict: AxisDict::DirLevel {
                dir_level,
                values: DirLevelDict::new(dict_rows(dict)),
            },
        }
    }

    /// `words` はリストの順 (`position`, `id`) に並べて渡す。
    fn filename_word_axis(name: &str, words: &[(&str, Option<&str>, i64)]) -> AxisIndex {
        filename_word_axis_at(name, AxisMatchPosition::Anywhere, words)
    }

    fn filename_word_axis_at(
        name: &str,
        match_position: AxisMatchPosition,
        words: &[(&str, Option<&str>, i64)],
    ) -> AxisIndex {
        AxisIndex {
            name: name.to_string(),
            filterable: true,
            optional_in_title: false,
            dict: AxisDict::FilenameWord {
                entries: filename_word_entries(
                    dict_rows(words)
                        .into_iter()
                        .map(|row| AxisDictRow {
                            match_position,
                            ..row
                        })
                        .collect(),
                ),
            },
        }
    }

    fn optional_in_title(axis: AxisIndex) -> AxisIndex {
        AxisIndex {
            optional_in_title: true,
            ..axis
        }
    }

    /// 語の頭はファイル名の先頭か直前が英数字でない位置、末尾は拡張子を除いたファイル名の末尾
    /// (→ docs/archive.md「軸の定義」)。
    #[test]
    fn derive_axis_value_filename_word_respects_the_match_position() {
        let axis = filename_word_axis_at(
            "課程",
            AxisMatchPosition::WordStart,
            &[("kyuu", Some("旧"), 0)],
        );
        assert!(derive_axis_value(&axis.dict, "r7/2025_op_13_kyuunihonshiB.pdf").is_some());
        assert!(derive_axis_value(&axis.dict, "kyuusugaku1.pdf").is_some());
        assert!(derive_axis_value(&axis.dict, "【kyuu】数学.pdf").is_some());
        // 英数字の途中からは始まらない。文字種の変わり目 (数字→英字) も語の頭にしない。
        assert!(derive_axis_value(&axis.dict, "2025_op_02_chirisougouchiritankyuu.pdf").is_none());
        assert!(derive_axis_value(&axis.dict, "2025-2-1kyuu.pdf").is_none());
        // 語の途中の出現があっても、別の語の頭に現れれば一致する。
        assert!(derive_axis_value(&axis.dict, "tankyuu_kyuu.pdf").is_some());
        // 語の途中の出現と語の頭の出現が重なっても、語の頭の方を見落とさない。
        let axis = filename_word_axis_at(
            "記号",
            AxisMatchPosition::WordStart,
            &[("a_a", Some("A"), 0)],
        );
        assert!(derive_axis_value(&axis.dict, "xa_a_a.pdf").is_some());

        // 位置で候補を絞ってから最長一致を採る。長い語でも位置が合わなければ候補にならない。
        let axis = filename_word_axis_at(
            "表紙",
            AxisMatchPosition::End,
            &[("hyoshi", Some("表紙"), 0), ("shi", Some("誤り"), 1)],
        );
        let matched = derive_axis_value(&axis.dict, "2024_or_00_kokugohyoshi.pdf")
            .expect("値が導出されなかった");
        assert_eq!(matched.display, "表紙");
        let matched =
            derive_axis_value(&axis.dict, "2024_hyoshi_shi.pdf").expect("値が導出されなかった");
        assert_eq!(matched.display, "誤り");
        assert!(derive_axis_value(&axis.dict, "2024_hyoshi_kokugo.pdf").is_none());
    }

    /// 照合する位置は照合語の行ごとに持つ。同じ軸の中で、行ごとに違う位置で照合する (→ docs/archive.md「軸の定義」)。
    #[test]
    fn derive_axis_value_filename_word_uses_each_row_match_position() {
        let row = |raw: &str, display: &str, position: i64, match_position| AxisDictRow {
            raw_value: raw.to_string(),
            display_name: Some(display.to_string()),
            position,
            match_position,
        };
        let dict = AxisDict::FilenameWord {
            entries: filename_word_entries(vec![
                row("kyuu", "旧", 0, AxisMatchPosition::WordStart),
                row("hyoshi", "表紙", 1, AxisMatchPosition::End),
                row("kaisetsu", "解説", 2, AxisMatchPosition::Anywhere),
            ]),
        };
        let display = |rel_path| derive_axis_value(&dict, rel_path).map(|value| value.display);

        assert_eq!(display("2025_kyuu_sugaku.pdf").as_deref(), Some("旧"));
        assert_eq!(display("2025_tankyuu.pdf"), None);
        assert_eq!(display("2025_kokugohyoshi.pdf").as_deref(), Some("表紙"));
        assert_eq!(display("2025_hyoshi_kokugo.pdf"), None);
        assert_eq!(display("2025_sugakukaisetsu2.pdf").as_deref(), Some("解説"));
    }

    /// 値が無くても組み立てる軸は、未設定の部分を空にする。テンプレート中の軸が
    /// すべて未設定ならファイル名に落ちる (→ docs/archive.md「表示タイトル」)。
    #[test]
    fn derive_item_blanks_unset_axes_that_are_optional_in_title() {
        let axes = vec![
            optional_in_title(filename_word_axis_at(
                "課程",
                AxisMatchPosition::WordStart,
                &[("kyuu", Some("旧"), 0)],
            )),
            filename_word_axis("科目", &[("sugaku1", Some("数学Ⅰ"), 0)]),
            optional_in_title(filename_word_axis_at(
                "表紙",
                AxisMatchPosition::End,
                &[("hyoshi", Some("（表紙）"), 0)],
            )),
        ];
        let template = Some("{課程}{科目}{表紙}");

        let item = derive_item("2025_op_00_kyuusugaku1hyoshi.pdf", &axes, template);
        assert_eq!(item.title, "旧数学Ⅰ（表紙）");

        let item = derive_item("2025_op_29_sugaku1.pdf", &axes, template);
        assert_eq!(item.title, "数学Ⅰ");
        assert!(item.title_from_template);

        // 値が無いと組み立てない軸 (科目) が未設定なら、これまでどおりファイル名。
        let item = derive_item("2025_op_00_kyuukokugohyoshi.pdf", &axes, template);
        assert_eq!(item.title, "2025_op_00_kyuukokugohyoshi.pdf");

        // テンプレート中の軸がすべて未設定なら、固定文字だけのタイトルにせずファイル名。
        let item = derive_item("2025_op_29_sugaku1.pdf", &axes, Some("{課程}の{表紙}"));
        assert_eq!(item.title, "2025_op_29_sugaku1.pdf");
        assert!(!item.title_from_template);
    }

    #[test]
    fn derive_axis_value_dir_level_prefers_dict_then_falls_back_to_raw_or_unset() {
        let axis = dir_level_axis("年度", 1, &[("2024", Some("2024年度"), 0)]);

        let dicted =
            derive_axis_value(&axis.dict, "2024/listening.mp3").expect("値が導出されなかった");
        assert_eq!(dicted.display, "2024年度");
        assert_eq!(dicted.key, ValueKey::Dict(0, "2024年度".to_string()));

        // 辞書に無い生の値は、そのまま表示し「辞書なしの生の値」段に入る。
        let undicted =
            derive_axis_value(&axis.dict, "2023/listening.mp3").expect("値が導出されなかった");
        assert_eq!(undicted.display, "2023");
        assert_eq!(undicted.key, ValueKey::Raw("2023".to_string()));

        // 階層が足りなければ未設定。
        assert!(derive_axis_value(&axis.dict, "listening.mp3").is_none());
    }

    /// 比較キーは表示名単位で、辞書の全行のうちその表示名を持つ行の最小 `position`
    /// (→ docs/archive.md「軸の値の辞書と導出」)。
    #[test]
    fn derive_axis_value_dir_level_keys_by_display_name() {
        let axis = dir_level_axis(
            "年度",
            1,
            &[
                ("R6", Some("令和6年度"), 1),
                ("2024", Some("令和6年度"), 3),
                ("別名", Some("2023"), 5),
                ("2022", Some(""), 2),
            ],
        );

        // 表示名が同じ行のうち、最小の position を採る。
        let value = derive_axis_value(&axis.dict, "2024/a.mp3").expect("値が導出されなかった");
        assert_eq!(value.key, ValueKey::Dict(1, "令和6年度".to_string()));

        // 自分の行が無くても、生の値が別の行の表示名と一致すれば辞書の段に入る。
        let value = derive_axis_value(&axis.dict, "2023/a.mp3").expect("値が導出されなかった");
        assert_eq!(value.display, "2023");
        assert_eq!(value.key, ValueKey::Dict(5, "2023".to_string()));

        // 空文字の表示名は付けていないものとみなす。
        let value = derive_axis_value(&axis.dict, "2022/a.mp3").expect("値が導出されなかった");
        assert_eq!(value.display, "2022");
        assert_eq!(value.key, ValueKey::Dict(2, "2022".to_string()));
    }

    /// 部分一致・大文字小文字を区別しない・複数一致時は最長の語を採用
    /// (→ docs/archive.md「軸の定義」)。
    #[test]
    fn derive_axis_value_filename_word_prefers_the_longest_match() {
        // 短い語がリストの上にあっても、長い語が勝つ。
        let axis = filename_word_axis("科目", &[("数学", None, 0), ("数学I", None, 1)]);

        let matched = derive_axis_value(&axis.dict, "2024/2024_数学I_問題.pdf")
            .expect("値が導出されなかった");
        assert_eq!(matched.display, "数学I");

        let matched =
            derive_axis_value(&axis.dict, "2024/2024_数学_問題.pdf").expect("値が導出されなかった");
        assert_eq!(matched.display, "数学");

        assert!(derive_axis_value(&axis.dict, "2024/2024_英語_問題.pdf").is_none());

        // 大文字小文字を区別しない (→ docs/archive.md「軸の定義」)。
        let axis = filename_word_axis("種別", &[("Listening", None, 0)]);
        let matched =
            derive_axis_value(&axis.dict, "2024/01_LISTENING.mp3").expect("値が導出されなかった");
        assert_eq!(matched.display, "Listening");
    }

    /// 同じ長さの語が複数一致したら、リストの上 (position が同じなら先に並ぶ行) を採る。
    #[test]
    fn derive_axis_value_filename_word_breaks_ties_by_list_order() {
        let axis = filename_word_axis("種別", &[("ab", None, 0), ("bc", None, 1)]);
        let matched = derive_axis_value(&axis.dict, "abc.mp3").expect("値が導出されなかった");
        assert_eq!(matched.display, "ab");

        let axis = filename_word_axis("種別", &[("bc", None, 0), ("ab", None, 0)]);
        let matched = derive_axis_value(&axis.dict, "abc.mp3").expect("値が導出されなかった");
        assert_eq!(matched.display, "bc");
    }

    /// 表示名が同じ照合語は、同じ比較キー (その表示名の最小 position) になる。
    #[test]
    fn derive_axis_value_filename_word_shares_the_key_among_the_same_display_name() {
        let axis = filename_word_axis(
            "教科",
            &[
                ("化学", Some("理科"), 0),
                ("国語", None, 1),
                ("物理", Some("理科"), 2),
            ],
        );

        let value = derive_axis_value(&axis.dict, "2024_物理.pdf").expect("値が導出されなかった");
        assert_eq!(value.display, "理科");
        assert_eq!(value.key, ValueKey::Dict(0, "理科".to_string()));
    }

    /// 予約プレースホルダーは軸が無くても使えて、拡張子を除いたファイル名になる。
    #[test]
    fn derive_item_expands_the_file_name_placeholder() {
        let axes = vec![dir_level_axis("年度", 1, &[])];

        let item = derive_item("2024/part1.mp3", &axes, Some("{年度}年度 {fileName}"));
        assert_eq!(item.title, "2024年度 part1");

        // 拡張子が無いファイル名はそのまま。
        let item = derive_item("2024/README", &axes, Some("{fileName}"));
        assert_eq!(item.title, "README");

        // 軸が「未設定」ならテンプレートを使わずファイル名へ落ちる規則は変わらない。
        let item = derive_item("part1.mp3", &axes, Some("{年度} {fileName}"));
        assert_eq!(item.title, "part1.mp3");
    }

    #[test]
    fn derive_item_falls_back_to_file_name_when_any_placeholder_axis_is_unset() {
        let axes = vec![
            dir_level_axis("年度", 1, &[]),
            filename_word_axis("科目", &[("国語", None, 0)]),
        ];

        // 両方設定済み: 組み立てる。
        let item = derive_item("2024/2024_国語.pdf", &axes, Some("{年度}年度の{科目}"));
        assert_eq!(item.title, "2024年度の国語");
        assert_eq!(item.file_name, "2024_国語.pdf");

        // 科目が未設定(どの語にも一致しない): ファイル名にフォールバックする。
        let item = derive_item("2024/2024_英語.pdf", &axes, Some("{年度}年度の{科目}"));
        assert_eq!(item.title, "2024_英語.pdf");

        // テンプレートが無い: ファイル名をそのまま表示する。
        let item = derive_item("2024/2024_国語.pdf", &axes, None);
        assert_eq!(item.title, "2024_国語.pdf");
    }

    /// 軸の表示値がたまたま `{別軸名}` という文字列を含んでいても、それ自体は
    /// プレースホルダーとして再展開されない。逐次 `String::replace` で組み立てると
    /// この値の中身まで書き換わってしまう (→ `assemble_title` のコメント)。
    #[test]
    fn derive_item_does_not_re_expand_placeholder_like_display_values() {
        let axes = vec![
            filename_word_axis("A", &[("a", Some("{B}"), 0)]),
            filename_word_axis("B", &[("b", Some("b"), 0)]),
        ];

        let item = derive_item("a_b.mp3", &axes, Some("{A}{B}"));
        assert_eq!(item.title, "{B}b");
    }
}
