//! コンテンツ(リンク/フォルダ/アップロードファイル/グループ)のCRUD API、および
//! folder/file/groupコンテンツの閲覧・配信API。
//!
//! `type = 'group'` は仮想フォルダで、`parent_id`(自己参照)により無制限にネストできる。
//! 親に指定できるのは`group`のみ(`folder`はFS上の実ディレクトリを指すコンテナであり、
//! その「子」はDB行ではなくFSのエントリなので、DB階層の親にはなれない)。循環参照は
//! `validate_parent`で拒否する。
//!
//! `file` の実体は `AppState::blobs_dir` 配下にSHA-256ハッシュ名で保存する
//! (content-addressed storage、重複排除)。作成・差し替えは専用のmultipartエンドポイント
//! (`POST /contents/upload` / `PUT /contents/{id}/upload`)で行う。JSONの
//! `create_content`/`update_content` は`file`以外を扱う(`file`が来たら422にする)。

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path as FsPath, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use axum::Json;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize, de::IntoDeserializer};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::error_detail::ValidationDetail;
use super::sort::{SortOrder, SortQuery, title_cmp};
use crate::auth::{AdminUser, AuthUser, Viewer};
use crate::config::create_owner_only_dir;
use crate::error::{AppError, AppJson, multipart_error_to_app_error, run_blocking};
use crate::state::AppState;

/// multipartの受付上限(`DefaultBodyLimit`)は、実際に許可したいファイルサイズ
/// (`AppState::max_upload_bytes`)に、テキストフィールド・multipartの境界文字列等の
/// オーバーヘッド分の余裕を追加した値にする。厳密な上限はハンドラ内でストリーミング中に
/// 別途チェックする(`stream_field_to_temp_blob`)ため、ここはあくまで「話にならないほど
/// 大きいリクエストを早期に切る」ためのバックストップ。
const UPLOAD_BODY_OVERHEAD_BYTES: u64 = 1024 * 1024;

/// `contents.type`。管理画面のレスポンスにもそのまま `type` として載せるため、
/// `Serialize`/`Deserialize`/`ToSchema` も併せて持つ(`auth::Role` と同じ書き方)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum ContentType {
    Link,
    Folder,
    File,
    Group,
    /// サーバー上のフォルダを索引し、属性軸で絞り込めるフラットな一覧として見せる。
    /// `Folder`(階層をそのまま辿る)とは性質も用途も違うため別の型にしてある
    /// (→ docs/archive.md)。
    Archive,
}

/// `contents.visibility`。公開範囲を表す (→ docs/access.md「公開範囲」)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum Visibility {
    /// 匿名を含む全員に見える。
    Public,
    /// ログイン済みの人に見える。
    Authenticated,
    /// 閲覧の場では作成者にだけ見える。
    Private,
    /// 閲覧の場には誰にも出ない。管理画面の一覧にだけ出る。
    Hidden,
}

/// 閲覧者のユーザーid。匿名なら `None`。
fn viewer_id(viewer: &Viewer) -> Option<i64> {
    match viewer {
        Viewer::User(user) => Some(user.id),
        Viewer::Anonymous => None,
    }
}

/// 閲覧の場の一覧に出すか (→ docs/access.md「匿名閲覧の受け口」)。
///
/// 対象ノード自身だけを見る。祖先のグループの判定は、呼び出し側が親を開けることを
/// 確かめて済ませる (→ docs/access.md「祖先のグループを辿る」)。
fn can_list(viewer: &Viewer, visibility: Visibility, created_by: Option<i64>) -> bool {
    match visibility {
        Visibility::Public => true,
        Visibility::Authenticated => viewer.is_authenticated(),
        Visibility::Private => viewer_id(viewer).is_some_and(|id| created_by == Some(id)),
        Visibility::Hidden => false,
    }
}

/// 単体の取得・配信を許すか (→ docs/access.md「匿名閲覧の受け口」)。
fn can_open(viewer: &Viewer, visibility: Visibility, created_by: Option<i64>) -> bool {
    if can_list(viewer, visibility, created_by) {
        return true;
    }
    match (visibility, viewer) {
        (Visibility::Hidden, Viewer::User(_)) => true,
        (Visibility::Private, Viewer::User(user)) => user.is_admin(),
        _ => false,
    }
}

/// `can_open` を満たさなければ、匿名には401、ログイン済みには404を返す
/// (→ docs/access.md「匿名閲覧の受け口」の表)。個別の閲覧・配信で使う。
fn ensure_openable(
    viewer: &Viewer,
    visibility: Visibility,
    created_by: Option<i64>,
) -> Result<(), AppError> {
    if can_open(viewer, visibility, created_by) {
        Ok(())
    } else if viewer.is_authenticated() {
        Err(AppError::NotFound)
    } else {
        Err(AppError::Unauthorized)
    }
}

/// 自分から祖先へ辿った1段分。
struct LineageRow {
    id: i64,
    title: String,
    visibility: Visibility,
    created_by: Option<i64>,
}

/// `id` 自身と、その祖先のグループ。先頭が自分で、ルートに近いものほど後ろに並ぶ。
///
/// 深さの上限は設けない (「無制限ネスト」の要件と矛盾するため)。`parent_id` を書き込む経路は
/// すべて、`contents_write_lock` の内側で `validate_parent` の循環の検出を通る。そのため循環は
/// DB に存在せず、辿れば必ずルートで終わる。DB を直接書き換えて循環を持ち込んだ場合は
/// 終わらないが、想定運用の外とする (`resolve_path` と同じ前提)。
async fn fetch_lineage(pool: &sqlx::SqlitePool, id: i64) -> Result<Vec<LineageRow>, AppError> {
    let rows = sqlx::query_as!(
        LineageRow,
        r#"WITH RECURSIVE lineage(id, parent_id, title, visibility, created_by, depth) AS (
               SELECT id, parent_id, title, visibility, created_by, 0 FROM contents WHERE id = ?1
               UNION ALL
               SELECT c.id, c.parent_id, c.title, c.visibility, c.created_by, l.depth + 1
               FROM contents c JOIN lineage l ON c.id = l.parent_id
           )
           SELECT id as "id!", title as "title!", visibility as "visibility!: Visibility",
                  created_by as "created_by?"
           FROM lineage ORDER BY depth"#,
        id,
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// 閲覧時に辿った、対象ノードとその祖先のグループ。
pub(super) struct Lineage(Vec<LineageRow>);

impl Lineage {
    /// 自分のタイトル。
    pub(super) fn own_title(&self) -> String {
        self.0
            .first()
            .map(|row| row.title.clone())
            .unwrap_or_default()
    }

    /// 自分を除く祖先を、ルートに近い順で返す。パンくずにそのまま使える形。
    pub(super) fn breadcrumbs(&self) -> Vec<GroupAncestor> {
        self.0
            .iter()
            .skip(1)
            .rev()
            .map(|row| GroupAncestor {
                id: row.id,
                title: row.title.clone(),
            })
            .collect()
    }

    /// 自分か祖先のどれかが `hidden` か。
    fn any_hidden(&self) -> bool {
        self.0
            .iter()
            .any(|row| row.visibility == Visibility::Hidden)
    }
}

/// 対象ノードと祖先のグループすべてに `can_open` を通す (→ docs/access.md「祖先のグループを辿る」)。
/// 単体で開く経路はすべてこれを呼ぶ。
///
/// `id` の実在と種別の確認は呼び出し側で先に済ませること。ファイルシステムに触る前に
/// 呼ぶこと (理由は `folder_root` と同じ)。
pub(super) async fn ensure_viewable(
    pool: &sqlx::SqlitePool,
    viewer: &Viewer,
    id: i64,
) -> Result<Lineage, AppError> {
    let rows = fetch_lineage(pool, id).await?;
    if rows.is_empty() {
        return Err(AppError::NotFound);
    }
    for row in &rows {
        ensure_openable(viewer, row.visibility, row.created_by)?;
    }
    Ok(Lineage(rows))
}

/// 見られる `link` コンテンツの URL。無い・見られない・リンクでないときは `None`
/// (リンクのカードの取り直しで、頼まれた id のうち見られるものだけを使うため)。
pub(super) async fn viewable_link_url(
    pool: &sqlx::SqlitePool,
    viewer: &Viewer,
    id: i64,
) -> Result<Option<String>, AppError> {
    let url = sqlx::query_scalar!(
        "SELECT url FROM contents WHERE id = ? AND type = ?",
        id,
        ContentType::Link
    )
    .fetch_optional(pool)
    .await?
    .flatten();
    let Some(url) = url else {
        return Ok(None);
    };
    match ensure_viewable(pool, viewer, id).await {
        Ok(_) => Ok(Some(url)),
        Err(AppError::NotFound | AppError::Unauthorized) => Ok(None),
        Err(err) => Err(err),
    }
}

/// 管理画面の一覧に出すか (→ docs/access.md「管理画面の一覧が `user` に見えること」)。他人の `private` は `admin` にだけ出す。
fn can_see_in_admin_list(user: &AuthUser, visibility: Visibility, created_by: Option<i64>) -> bool {
    visibility != Visibility::Private || user.is_admin() || created_by == Some(user.id)
}

/// 管理用の操作で、`user` から見て他人の `private` なら404を返す (→ docs/access.md「ロールと操作」)。
/// 管理画面の一覧に出ないものは、存在ごと隠す。
pub(super) fn ensure_manageable(
    user: &AuthUser,
    visibility: Visibility,
    created_by: Option<i64>,
) -> Result<(), AppError> {
    if can_see_in_admin_list(user, visibility, created_by) {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

/// 書き換える操作の判定 (→ docs/access.md「ロールと操作」)。`user` が書き換えられるのは自分が作ったものだけ。
/// 他人の `private` は `ensure_manageable` と同じく404で隠し、見えるものは403にする。
pub(super) fn ensure_editable(
    user: &AuthUser,
    visibility: Visibility,
    created_by: Option<i64>,
) -> Result<(), AppError> {
    ensure_manageable(user, visibility, created_by)?;
    if user.is_admin() || created_by == Some(user.id) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// folderコンテンツの `path` を取り出す。folderの行には書き込み時に必ず入れている。
fn folder_path(path: Option<String>) -> Result<String, AppError> {
    path.ok_or(AppError::DataIntegrity("folder content without a path"))
}

struct ContentRow {
    id: i64,
    content_type: ContentType,
    parent_id: Option<i64>,
    title: String,
    url: Option<String>,
    path: Option<String>,
    description: Option<String>,
    /// 「新しい順」の並べ替えに使う (→ docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。レスポンスには出さない。
    created_at: String,
    // `blob_hash`は選択しない: `ContentResponse`/`AdminContentResponse`のどちらも
    // 公開しない内部実装detail(delete/replace時のGCは別途ad-hocなクエリで取得する)。
    file_name: Option<String>,
    file_size: Option<i64>,
    visibility: Visibility,
    created_by: Option<i64>,
    /// `type = 'archive'` のみ。索引対象の拡張子をカンマ区切りで保持する。
    extensions: Option<String>,
    /// `type = 'archive'` のみ。表示タイトルの組み立てテンプレート (→ docs/archive.md「表示タイトル」)。
    title_template: Option<String>,
}

/// 一般の閲覧者(匿名を含む)向けのレスポンス。`path`(サーバー上の絶対パス)は
/// 一切含めない(一般の閲覧者にファイルシステムのレイアウトを漏らさないため)。type分岐が
/// フロントからも自然に扱えるよう、tagged enumにしてある。
///
/// 各バリアントの `private` は、公開範囲が `private` なら真。一覧に「本人のみ」の印を出すため
/// (→ docs/access.md「フロントエンドのガード反転」)。一覧に並ぶ `private` は必ず閲覧者自身のもの (`can_list`)。
#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentResponse {
    Link {
        id: i64,
        title: String,
        /// `url` の必須チェック(type=linkのみ扱う現状は常にSome)は書き込み側で行う。
        url: String,
        description: Option<String>,
        private: bool,
        /// カードに出すページの情報で、覚えているもの (→ docs/ui.md「リンクのカード」)。
        /// 一覧 (`list_contents`・`browse_group`) でだけ埋める。新しい情報は画面が取り直しを頼んで得る。
        preview: Option<super::link_preview::LinkPreview>,
    },
    Folder {
        id: i64,
        title: String,
        description: Option<String>,
        private: bool,
    },
    // 複数語のフィールドを持つのはこのバリアントだけ。enum に付けた `rename_all` は
    // バリアント名にしか効かないので、フィールドの camelCase はここで指定する。
    #[serde(rename_all = "camelCase")]
    File {
        id: i64,
        title: String,
        description: Option<String>,
        private: bool,
        /// ダウンロード時のファイル名。ダウンロードURL自体は`id`ベース
        /// (`/contents/{id}/download`)なので、これは表示・保存名のヒント用。
        file_name: String,
        file_size: i64,
        /// ページ内でプレビューする画像なら、その大きさ (→ docs/ui.md「画像のプレビュー」)。
        /// 一覧 (`list_contents`・`browse_group`) でだけ埋める。
        image: Option<super::thumbnails::ImageSize>,
        /// テキストのビューアで見せるファイルか (→ docs/ui.md「PDF・動画・テキストのビューア」)。
        /// `image` と同じく、一覧でだけ埋める。
        is_text: bool,
        /// 行に縮小画像を出してみるか (→ docs/ui.md「画像のプレビュー」)。一覧でだけ埋める。
        thumbnail: bool,
    },
    /// 仮想フォルダ。子一覧は含めない(別途 `GET /contents/{id}/group` で取得する)。
    Group {
        id: i64,
        title: String,
        description: Option<String>,
        private: bool,
    },
    /// 索引型。アイテム一覧は含めない(別途 `GET /contents/{id}/archive` で取得する)。
    /// `folder` と同じく `path` は出さない。
    Archive {
        id: i64,
        title: String,
        description: Option<String>,
        private: bool,
    },
}

impl TryFrom<ContentRow> for ContentResponse {
    type Error = AppError;

    fn try_from(row: ContentRow) -> Result<Self, Self::Error> {
        let private = row.visibility == Visibility::Private;
        // 万一必須カラムがNULLだった場合(将来型が増えてマイグレーションミス等)は
        // レスポンスを諦めて500にする方が、フロントに嘘の空文字列を返すより安全。
        match row.content_type {
            ContentType::Archive => Ok(ContentResponse::Archive {
                id: row.id,
                title: row.title,
                description: row.description,
                private,
            }),
            ContentType::Link => {
                let url = row
                    .url
                    .ok_or(AppError::DataIntegrity("link content without a url"))?;
                Ok(ContentResponse::Link {
                    id: row.id,
                    title: row.title,
                    url,
                    description: row.description,
                    private,
                    preview: None,
                })
            }
            ContentType::Folder => Ok(ContentResponse::Folder {
                id: row.id,
                title: row.title,
                description: row.description,
                private,
            }),
            ContentType::File => {
                let file_name = row
                    .file_name
                    .ok_or(AppError::DataIntegrity("file content without a file_name"))?;
                let file_size = row
                    .file_size
                    .ok_or(AppError::DataIntegrity("file content without a file_size"))?;
                Ok(ContentResponse::File {
                    id: row.id,
                    title: row.title,
                    description: row.description,
                    private,
                    file_name,
                    file_size,
                    image: None,
                    is_text: false,
                    thumbnail: false,
                })
            }
            ContentType::Group => Ok(ContentResponse::Group {
                id: row.id,
                title: row.title,
                description: row.description,
                private,
            }),
        }
    }
}

/// 管理画面向けのレスポンス。`path` (サーバー上の絶対パス)・公開範囲・作成者を含む。
/// `user` にも返す (→ docs/access.md「管理画面の一覧が `user` に見えること」)。見えるのは「公開できるフォルダ」の配下だけ
/// なので、`user` に見せたくないパスはそもそも登録されない (→ docs/folders.md「公開できるフォルダ」)。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AdminContentResponse {
    pub id: i64,
    #[serde(rename = "type")]
    pub content_type: ContentType,
    /// 親グループのid。`None`ならルート直下。
    pub parent_id: Option<i64>,
    pub title: String,
    pub url: Option<String>,
    pub path: Option<String>,
    /// `path` を含む「公開できるフォルダ」の名前。画面では起点のフルパスの代わりに出す
    /// (→ docs/folders.md「公開できるフォルダ」)。`path` が無いか、どの公開できるフォルダにも含まれなければ `None`。
    pub root_name: Option<String>,
    /// `root_name` のフォルダから先の相対パス (`/` 区切り)。フォルダそのものなら空文字列。
    /// `root_name` が `None` なら `None`。
    pub path_in_root: Option<String>,
    /// `root_name` の公開できるフォルダが削除済みか。画面では名前の代わりに
    /// 「存在しない公開フォルダ」と出す。`root_name` が `None` なら `false`。
    pub root_deleted: bool,
    pub description: Option<String>,
    /// `type = 'file'` の場合のみ`Some`。`blob_hash`(内部実装detail)は含めない。
    pub file_name: Option<String>,
    pub file_size: Option<i64>,
    pub visibility: Visibility,
    /// 作成者のユーザーid。`None`なら作成者なし (→ docs/access.md「公開範囲」)。
    pub created_by: Option<i64>,
    /// 作成者のユーザー名。管理画面の一覧の「作成者」に出す (→ docs/access.md「管理画面の一覧が `user` に見えること」)。
    pub created_by_username: Option<String>,
    /// `type = 'archive'` の場合のみ`Some`。索引対象の拡張子をカンマ区切りで持つ。
    /// 空なら全ファイルが対象。
    pub extensions: Option<String>,
    /// `type = 'archive'` の場合のみ`Some`。表示タイトルの組み立てテンプレート
    /// (→ docs/archive.md「表示タイトル」)。`None`ならファイル名をそのまま表示する。
    pub title_template: Option<String>,
}

impl AdminContentResponse {
    fn from_row(
        row: ContentRow,
        created_by_username: Option<String>,
        root_records: &[super::roots::RootRecord],
    ) -> Self {
        let location = row
            .path
            .as_deref()
            .and_then(|path| super::roots::locate(root_records, path));
        let (root_name, path_in_root, root_deleted) = match location {
            Some(location) => (
                Some(location.root_name),
                Some(location.path_in_root),
                location.root_deleted,
            ),
            None => (None, None, false),
        };
        Self {
            id: row.id,
            content_type: row.content_type,
            parent_id: row.parent_id,
            title: row.title,
            url: row.url,
            path: row.path,
            root_name,
            path_in_root,
            root_deleted,
            description: row.description,
            file_name: row.file_name,
            file_size: row.file_size,
            visibility: row.visibility,
            created_by: row.created_by,
            created_by_username,
            extensions: row.extensions,
            title_template: row.title_template,
        }
    }
}

/// 書き込みの応答を組み立てる。作成者名は `RETURNING` では `users` と JOIN できないため、
/// 行とは別に引く。
async fn admin_response(
    pool: &sqlx::SqlitePool,
    row: ContentRow,
) -> Result<AdminContentResponse, AppError> {
    let username = match row.created_by {
        Some(id) => {
            sqlx::query_scalar!("SELECT username FROM users WHERE id = ?", id)
                .fetch_optional(pool)
                .await?
        }
        None => None,
    };
    let root_records = super::roots::load_root_records(pool).await?;
    Ok(AdminContentResponse::from_row(row, username, &root_records))
}

/// `type` は作成時のみ指定する(更新では変更不可、`UpdateContentRequest` に無い)。
/// tagged enumではなくフラット構造にしているのは、url/pathの欠落をデシリアライズの
/// 失敗(400)ではなく、ハンドラの検証(422)で返すため。
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateContentRequest {
    #[serde(rename = "type")]
    pub content_type: ContentType,
    /// 親グループのid。省略/`null`ならルート直下に作成する。
    /// 検証(存在確認・`type = 'group'`確認)は`validate_parent`で行う。
    #[serde(default)]
    pub parent_id: Option<i64>,
    /// タイトル。省略・空なら自動で付ける (`resolve_create_title`)。`group` だけは必須。
    #[serde(default)]
    pub title: Option<String>,
    pub url: Option<String>,
    pub path: Option<String>,
    pub description: Option<String>,
    /// 公開範囲。省略時は既定値 (→ docs/access.md「ロールと操作」)。
    #[serde(default)]
    pub visibility: Option<Visibility>,
    /// `type = 'archive'` のみ。索引対象の拡張子をカンマ区切りで指定する。
    /// 省略・空なら全ファイルを索引する。
    #[serde(default)]
    pub extensions: Option<String>,
    /// `type = 'archive'` のみ。表示タイトルの組み立てテンプレート (→ docs/archive.md「表示タイトル」)。
    /// 作成時点では軸がまだ存在しないため、プレースホルダーを含む値は422になる。
    #[serde(default)]
    pub title_template: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct UpdateContentRequest {
    /// 親グループのid。省略/`null`ならルート直下へ移動する (完全上書き型PUTの一部
    /// として扱う)。
    #[serde(default)]
    pub parent_id: Option<i64>,
    pub title: String,
    pub url: Option<String>,
    pub path: Option<String>,
    pub description: Option<String>,
    /// 公開範囲。省略時は現在値のまま。変えてよいかは `resolve_visibility_for_update` で判定する
    /// (→ docs/access.md「ロールと操作」)。
    #[serde(default)]
    pub visibility: Option<Visibility>,
    /// `type = 'archive'` のみ。索引対象の拡張子をカンマ区切りで指定する。
    /// 省略・空なら全ファイルを索引する。
    #[serde(default)]
    pub extensions: Option<String>,
    /// `type = 'archive'` のみ。表示タイトルの組み立てテンプレート (→ docs/archive.md「表示タイトル」)。
    /// 保存時にプレースホルダーが実在する軸名であることを検証する。
    #[serde(default)]
    pub title_template: Option<String>,
}

/// タイトルの前後の空白を取り除いた上で検証する。空タイトルを弾く。
fn validate_title(title: &str) -> Result<String, AppError> {
    super::validate::trimmed_non_empty(title, TITLE_REQUIRED)
}

const TITLE_REQUIRED: &str = "title is required";

/// 作成時のタイトル。入力があればそれを使う。無ければ `derived` (種別ごとに決めた
/// 自動の値) を、同じ親の中の他のタイトルと重ならないよう連番を付けて使う。
/// 入力も `derived` も無ければ422 (`group` の作成が当たる)。
/// 比べるのは同じ親の兄弟だけ。一覧で並んで見えるものが区別できればよいため。
async fn resolve_create_title(
    pool: &sqlx::SqlitePool,
    parent_id: Option<i64>,
    requested: Option<&str>,
    derived: Option<String>,
) -> Result<String, AppError> {
    if let Some(title) = super::validate::trimmed_or_none(requested) {
        return Ok(title.to_string());
    }
    let Some(base) = super::validate::trimmed_or_none(derived.as_deref()) else {
        return Err(AppError::Validation(TITLE_REQUIRED.to_string()));
    };
    let siblings =
        sqlx::query_scalar!("SELECT title FROM contents WHERE parent_id IS ?", parent_id)
            .fetch_all(pool)
            .await?;
    let taken: std::collections::HashSet<String> =
        siblings.iter().map(|title| title.to_lowercase()).collect();
    Ok(super::validate::first_free_name(base, |candidate| {
        taken.contains(&candidate.to_lowercase())
    }))
}

/// フォルダの絶対パスから付ける自動のタイトル。末尾のフォルダ名で、ドライブの
/// ルートのように名前が無いときはパス全体。
fn title_from_path(path: &str) -> String {
    super::roots::default_name(FsPath::new(path))
}

/// アップロードしたファイル名から付ける自動のタイトル。拡張子を除いた名前で、
/// 除くと空になるとき (`.hidden` など) はファイル名のまま。
fn title_from_file_name(file_name: &str) -> String {
    FsPath::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or(file_name)
        .to_string()
}

/// リンクの URL を検証する。スキームが省かれている (`example.com` のように貼り付けた) ときは、
/// 相手を見て `https://` か `http://` を補う (→ `link_title::choose_scheme`)。
/// 補ったときは、そのとき取ったページの中身も返す (取れなければ空)。もう一度取りに行かないため。
/// `javascript:` のように他のスキームが明示されているときは補わず、そのまま弾く。
async fn resolve_url(
    url: Option<&str>,
) -> Result<(String, Option<super::link_title::LinkMetadata>), AppError> {
    let trimmed = url.unwrap_or_default().trim();
    // 空のときは補わない。スキームだけの壊れた URL を作らず、検証で弾く。
    if !trimmed.is_empty() && has_no_scheme(trimmed) {
        let (url, metadata) = super::link_title::choose_scheme(trimmed).await?;
        return Ok((validate_url(&url)?, Some(metadata)));
    }
    Ok((validate_url(trimmed)?, None))
}

/// URLの前後の空白を取り除いた上で検証する。空URLや `javascript:` 等の非http(s)スキームを
/// 弾く(URLはトップページで `<a href>` にそのまま埋め込むため、スキームの制限はXSS対策)。
fn validate_url(url: &str) -> Result<String, AppError> {
    let url = url.trim();
    // スキームは RFC 上 大文字小文字を区別しないので、`HTTPS://` なども通す (保存は元の綴りのまま)。
    let is_http = ["http://", "https://"].iter().any(|prefix| {
        url.get(..prefix.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(prefix))
    });
    if !is_http {
        return Err(AppError::Validation(
            "url must start with http:// or https://".to_string(),
        ));
    }
    Ok(url.to_string())
}

/// `scheme:` の形 (RFC 3986、`ALPHA *( ALPHA / DIGIT / "+" / "-" / "." )`) を持たないか。
/// 持たなければ、プロトコルが省略されているとみなせる。
/// `:` の直後が数字なら、スキームではなくポート (`nas.local:5000`) とみなす。
fn has_no_scheme(url: &str) -> bool {
    let Some(colon) = url.find(':') else {
        return true;
    };
    if url[colon + 1..].starts_with(|c: char| c.is_ascii_digit()) {
        return true;
    }
    let scheme = &url[..colon];
    match scheme.chars().next() {
        Some(first) if first.is_ascii_alphabetic() => !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')),
        _ => true,
    }
}

/// folderの対象パスを検証する。絶対パスであること・実在すること・ディレクトリであることを
/// 確認し、`std::fs::canonicalize` 済みの絶対パス文字列を返す(閲覧時にDBの値を再度
/// canonicalizeしてstarts_with比較するため、保存時点でも正規化しておく必要がある。
/// macOSの `/tmp` -> `/private/tmp` のようなシンボリックリンクを踏んでいても
/// 一貫した比較ができるようにするため)。
///
/// `std::fs::canonicalize` はブロッキングI/Oのため `spawn_blocking` に載せる。
async fn validate_folder_path(state: &AppState, path: Option<&str>) -> Result<String, AppError> {
    // 入力は選択UIが返した値をそのまま送る想定なので trim しない。Unix系では
    // 前後に空白を含むディレクトリ名が有効で、落とすと別のパスとして扱われる。
    let path = path.unwrap_or_default().to_string();
    super::validate::non_empty(&path, "path is required")?;

    // 検証は全てブロッキングI/Oなので1つの `spawn_blocking` にまとめる。
    // 「公開できるフォルダ」はその前に読む (`run_blocking` の中ではDBに触れない)。
    let roots = super::roots::load_roots(&state.pool).await?;
    let own_dirs = state.own_dirs.clone();
    let canonical = run_blocking(move || {
        super::fs::canonical_dir_within_roots(
            &path,
            &roots,
            &super::roots::OwnDirs::resolve(&own_dirs),
        )
    })
    .await??;

    super::fs::path_to_string(canonical)
}

/// 書き込み前に親子関係を検証する。公開範囲の組み合わせは検査しない (→ docs/access.md「親が外れるときの公開範囲」)。
///
/// `parent_id`が`None`ならルート直下として扱い、検査は行わない。`Some`の場合:
/// (1) 親が実在し`type = 'group'`であること、(2) `self_id`(更新対象自身のid。新規作成時は
/// `None`)が親自身とその祖先に含まれない(循環参照にならない)ことを確認する。
///
/// (2)は`fetch_lineage`で親から祖先へ辿って判定する。`self_id == parent_id`
/// (自分自身を親にする)も親自身が含まれるため同じ判定で弾ける。
///
/// 検証はSELECTだけで書き込みとアトミックではないため、呼び出し側は
/// `contents_write_lock`を保持したまま書き込みまでを行うこと
/// (→ `AppState::contents_write_lock`)。
async fn validate_parent(
    pool: &sqlx::SqlitePool,
    parent_id: Option<i64>,
    self_id: Option<i64>,
) -> Result<Option<i64>, AppError> {
    let Some(parent_id) = parent_id else {
        return Ok(None);
    };

    let parent = sqlx::query!(
        r#"SELECT type as "content_type: ContentType" FROM contents WHERE id = ?"#,
        parent_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Validation("parent group not found".to_string()))?;

    if parent.content_type != ContentType::Group {
        return Err(AppError::Validation("parent must be a group".to_string()));
    }

    if let Some(self_id) = self_id
        && fetch_lineage(pool, parent_id)
            .await?
            .iter()
            .any(|row| row.id == self_id)
    {
        return Err(AppError::Validation(
            "parent must not be the content itself or one of its descendants".to_string(),
        ));
    }

    Ok(Some(parent_id))
}

/// `group` は `private` にできない (→ docs/access.md「公開範囲」)。
fn reject_private_group(content_type: ContentType, visibility: Visibility) -> Result<(), AppError> {
    if content_type == ContentType::Group && visibility == Visibility::Private {
        return Err(AppError::ValidationDetailed {
            message: "a group cannot be private".to_string(),
            detail: ValidationDetail::GroupCannotBePrivate,
        });
    }
    Ok(())
}

/// 新規作成時の既定の公開範囲 (→ docs/access.md「公開範囲」)。
/// 安全側は既定を厳しくすることではなく、登録前の確認で担保する (→ docs/folders.md「登録前の確認」)。
const DEFAULT_VISIBILITY: Visibility = Visibility::Public;

/// 新規作成時に書き込む公開範囲を決める。`user` も選べる (→ docs/access.md「ロールと操作」)。
/// 作成なら作成者は操作した本人なので、`private` を選んでも制限は無い。
fn resolve_visibility_for_create(requested: Option<Visibility>) -> Visibility {
    requested.unwrap_or(DEFAULT_VISIBILITY)
}

/// 更新時に書き込む公開範囲を決める (→ docs/access.md「ロールと操作」)。
///
/// 値が変わらない更新は常に通す。完全上書き型のPUTでは、編集していない公開範囲も
/// そのまま送り返されてくるため (例: `admin` が他人の `private` のアーカイブの
/// 表示タイトルを保存する)。
fn resolve_visibility_for_update(
    user: &AuthUser,
    requested: Option<Visibility>,
    current: Visibility,
    created_by: Option<i64>,
) -> Result<Visibility, AppError> {
    let next = requested.unwrap_or(current);
    if next == current {
        return Ok(current);
    }
    let is_creator = created_by == Some(user.id);
    if current == Visibility::Private && !is_creator {
        return if user.is_admin() && next == Visibility::Hidden {
            Ok(next)
        } else {
            Err(AppError::Forbidden)
        };
    }
    if next == Visibility::Private && !is_creator {
        return Err(AppError::Forbidden);
    }
    Ok(next)
}

/// 閲覧する一覧を指定された順に並べる。同じ値なら `id` で決める (→ docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。
fn sort_rows(rows: &mut [ContentRow], order: SortOrder) {
    match order {
        SortOrder::Title => {
            rows.sort_by(|a, b| title_cmp(&a.title, &b.title).then(a.id.cmp(&b.id)))
        }
        // `created_at` はゼロ埋めした固定長のUTC文字列なので、辞書順が日時の順になる。
        SortOrder::New => {
            rows.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)))
        }
    }
}

/// コンテンツ一覧(トップページ用)。匿名でも呼べる。`path`は含めない。
/// ルート直下(`parent_id IS NULL`)のみを返す。group配下の子はここには出てこず、
/// `GET /contents/{id}/group` で個別に取得する(ドリルダウン)。
/// 公開範囲による絞り込みは `can_list` で行う(→ docs/access.md「匿名閲覧の受け口」)。一覧なので401は返さず、
/// 見えないものは単に並ばない。
#[utoipa::path(
    get,
    path = "/contents",
    params(SortQuery),
    responses(
        (status = OK, body = Vec<ContentResponse>),
    )
)]
async fn list_contents(
    viewer: Viewer,
    State(state): State<AppState>,
    Query(query): Query<SortQuery>,
) -> Result<Json<Vec<ContentResponse>>, AppError> {
    // 判定を `can_list` の1箇所に保つため、絞り込みは取得後に行う。
    // 1つの一覧は数十件規模なので、全件を読んでもコストは問題にならない。
    let mut rows = sqlx::query_as!(
        ContentRow,
        r#"SELECT id as "id!", type as "content_type: ContentType", parent_id, title, url, path,
                  description, created_at as "created_at!", file_name, file_size,
                  visibility as "visibility: Visibility", created_by, extensions, title_template
           FROM contents
           WHERE parent_id IS NULL"#,
    )
    .fetch_all(&state.pool)
    .await?;

    sort_rows(&mut rows, SortOrder::from_query(query.sort.as_deref()));

    let contents = rows
        .into_iter()
        .filter(|row| can_list(&viewer, row.visibility, row.created_by))
        .map(ContentResponse::try_from)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(with_previews(&state, contents).await?))
}

/// 管理画面向けの一覧。`path`・公開範囲・作成者を含む。
/// ログイン済みなら `user` も呼べるが、他人の `private` は `admin` にだけ返す
/// (→ docs/access.md「管理画面の一覧が `user` に見えること」)。
#[utoipa::path(
    get,
    path = "/admin/contents",
    responses(
        (status = OK, body = Vec<AdminContentResponse>),
        (status = 401, body = crate::error::ErrorResponse),
    )
)]
async fn list_admin_contents(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<AdminContentResponse>>, AppError> {
    let rows = sqlx::query_as!(
        ContentRow,
        r#"SELECT id as "id!", type as "content_type: ContentType", parent_id, title, url, path,
                  description, created_at as "created_at!", file_name, file_size,
                  visibility as "visibility: Visibility", created_by, extensions, title_template
           FROM contents
           ORDER BY id"#,
    )
    .fetch_all(&state.pool)
    .await?;

    let usernames: HashMap<i64, String> =
        sqlx::query!(r#"SELECT id as "id!", username FROM users"#)
            .fetch_all(&state.pool)
            .await?
            .into_iter()
            .map(|row| (row.id, row.username))
            .collect();
    let root_records = super::roots::load_root_records(&state.pool).await?;

    Ok(Json(
        rows.into_iter()
            .filter(|row| can_see_in_admin_list(&user, row.visibility, row.created_by))
            .map(|row| {
                let username = row.created_by.and_then(|id| usernames.get(&id).cloned());
                AdminContentResponse::from_row(row, username, &root_records)
            })
            .collect(),
    ))
}

#[utoipa::path(
    post,
    path = "/contents",
    request_body = CreateContentRequest,
    responses(
        (status = 201, body = AdminContentResponse),
        (status = 401, body = crate::error::ErrorResponse),
        (status = 403, body = crate::error::ErrorResponse),
        (status = 409, body = crate::error::ErrorResponse, description = "Free の上限に当たった"),
        (status = 422, body = crate::error::ErrorResponse),
    )
)]
async fn create_content(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<CreateContentRequest>,
) -> Result<(StatusCode, Json<AdminContentResponse>), AppError> {
    let visibility = resolve_visibility_for_create(payload.visibility);
    reject_private_group(payload.content_type, visibility)?;

    // 外へつなぐ前にも数える。上限に当たっているのに、リンク先へ接続してから断らないため。
    // 鍵の内側でもう一度数える (接続の間に別の要求が足すことがある)。
    if payload.content_type == ContentType::Link {
        super::free_limit::check_content(state.pro.edition(), &state.pool, ContentType::Link)
            .await?;
    }
    // スキームを決めるための接続とページの取得は数秒かかりうるので、書き込みのロックを取る前に済ませる。
    let (link_url, fetched) = if payload.content_type == ContentType::Link {
        let (url, fetched) = resolve_url(payload.url.as_deref()).await?;
        (Some(url), fetched)
    } else {
        (None, None)
    };
    // タイトル・説明とも未入力のときだけ取りに行く (どちらか片方でも1回のリクエストで済ませる)。
    // スキームを補ったときは、そのとき試した結果を使う。
    let needs_link_title = payload.content_type == ContentType::Link
        && super::validate::trimmed_or_none(payload.title.as_deref()).is_none();
    let needs_link_description = payload.content_type == ContentType::Link
        && super::validate::trimmed_or_none(payload.description.as_deref()).is_none();
    let (link_title, link_description) = if let Some(url) = link_url
        .as_deref()
        .filter(|_| needs_link_title || needs_link_description)
    {
        let metadata = match fetched {
            Some(metadata) => metadata,
            None => super::link_title::fetch_link_metadata(url).await?,
        };
        // ページのタイトルが無ければ、ファイルを指す URL はファイル名、ほかはホスト名にする。
        // ファイル名のほうが、ファイルの行として出たときに中身を見分けられるため (→ docs/ui.md「URL のファイル」)。
        let title = needs_link_title.then(|| {
            metadata.title.unwrap_or_else(|| {
                super::remote_file::relayed_file_name(url)
                    .unwrap_or_else(|| super::link_title::host_of(url))
            })
        });
        let description = needs_link_description
            .then_some(metadata.description)
            .flatten();
        (title, description)
    } else {
        (None, None)
    };

    // 親の検証からINSERTまでを直列化する (→ `AppState::contents_write_lock`)。
    let _write_guard = state.contents_write_lock.lock().await;
    let parent_id = validate_parent(&state.pool, payload.parent_id, None).await?;
    let (url, path, extensions, title_template) = match payload.content_type {
        ContentType::Link => (link_url, None, None, None),
        ContentType::Folder => (
            None,
            Some(validate_folder_path(&state, payload.path.as_deref()).await?),
            None,
            None,
        ),
        // fileはJSONではなくmultipart専用エンドポイント(upload_content)で作る。
        ContentType::File => {
            return Err(AppError::Validation(
                "use POST /contents/upload to create a file content".to_string(),
            ));
        }
        ContentType::Group => (None, None, None, None),
        // 登録先の検証は folder と同じ。索引するだけで、見せ方が違うだけのため。
        ContentType::Archive => (
            None,
            Some(validate_folder_path(&state, payload.path.as_deref()).await?),
            super::archive::normalize_extensions(payload.extensions.as_deref()),
            // 作成時点では軸が1つも存在しないため、プレースホルダーを含む値は
            // ここで422になる (→ docs/archive.md「表示タイトル」)。
            super::archive::validate_title_template(
                &state.pool,
                None,
                payload.title_template.as_deref(),
            )
            .await?,
        ),
    };
    super::free_limit::check_content(state.pro.edition(), &state.pool, payload.content_type)
        .await?;

    let derived_title = match payload.content_type {
        ContentType::Link => link_title,
        ContentType::Folder | ContentType::Archive => path.as_deref().map(title_from_path),
        ContentType::File | ContentType::Group => None,
    };
    let title = resolve_create_title(
        &state.pool,
        parent_id,
        payload.title.as_deref(),
        derived_title,
    )
    .await?;

    let content_type = payload.content_type;
    let description = link_description.or(payload.description);
    let row = sqlx::query_as!(
        ContentRow,
        r#"INSERT INTO contents (type, parent_id, title, url, path, description,
                                  visibility, extensions, title_template, created_by)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           RETURNING id as "id!", type as "content_type: ContentType", parent_id, title, url,
                     path, description, created_at as "created_at!", file_name, file_size,
                     visibility as "visibility: Visibility", created_by, extensions, title_template"#,
        content_type,
        parent_id,
        title,
        url,
        path,
        description,
        visibility,
        extensions,
        title_template,
        user.id,
    )
    .fetch_one(&state.pool)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(admin_response(&state.pool, row).await?),
    ))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkMetadataRequest {
    pub url: String,
}

/// `og:title`・`og:description` (無ければ `<title>`・`<meta name="description">`) から取った
/// タイトルと説明。取れなかったものは `None` (呼び出し元の判断に任せる。リンク作成時と違い
/// ホスト名へのフォールバックはしない。明示の再取得なので、取れなかったことがそのまま
/// 分かったほうがよい)。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinkMetadataResponse {
    pub title: Option<String>,
    pub description: Option<String>,
}

/// リンクのタイトル・説明を URL から取り直す。作成・編集どちらのフォームからも使う
/// (→ `src/api/link_title.rs`)。ログイン済みなら誰でも呼べる (コンテンツの作成・更新権限は
/// 問わない。取得するだけで何も書き込まないため)。
#[utoipa::path(
    post,
    path = "/contents/link-metadata",
    request_body = LinkMetadataRequest,
    responses(
        (status = 200, body = LinkMetadataResponse),
        (status = 401, body = crate::error::ErrorResponse),
        (status = 422, body = crate::error::ErrorResponse),
    )
)]
async fn fetch_link_metadata(
    _user: AuthUser,
    AppJson(payload): AppJson<LinkMetadataRequest>,
) -> Result<Json<LinkMetadataResponse>, AppError> {
    let (url, fetched) = resolve_url(Some(&payload.url)).await?;
    let metadata = match fetched {
        Some(metadata) => metadata,
        None => super::link_title::fetch_link_metadata(&url).await?,
    };
    Ok(Json(LinkMetadataResponse {
        title: metadata.title,
        description: metadata.description,
    }))
}

#[utoipa::path(
    put,
    path = "/contents/{id}",
    params(("id" = i64, Path)),
    request_body = UpdateContentRequest,
    responses(
        (status = OK, body = AdminContentResponse),
        (status = 401, body = crate::error::ErrorResponse),
        (status = 403, body = crate::error::ErrorResponse),
        (status = 404, body = crate::error::ErrorResponse),
        (status = 422, body = crate::error::ErrorResponse),
    )
)]
async fn update_content(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<UpdateContentRequest>,
) -> Result<Json<AdminContentResponse>, AppError> {
    // スキームを決めるための接続は数秒かかりうるので、書き込みのロックを取る前に済ませる。
    // 種別は更新できないので、ロックの外で読んでよい。link 以外では外へ接続しない。
    let is_link = sqlx::query_scalar!(
        r#"SELECT type as "content_type: ContentType" FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?
        == ContentType::Link;
    // 失敗はここでは返さず link の分岐で返す (権限の判定より先に検証の失敗を返さないため)。
    let link_url = if is_link {
        Some(resolve_url(payload.url.as_deref()).await)
    } else {
        None
    };

    // 親の付け替えの検証からUPDATEまでを直列化する (→ `AppState::contents_write_lock`)。
    let _write_guard = state.contents_write_lock.lock().await;

    // `type` は更新不可。既存の行のtypeに応じてurl/pathどちらを検証するか決める。
    // 公開範囲の現在値と作成者も同時に取る(変えてよいかの判定に要る)。
    // 取得をロックの内側で行うのは、現在値の読み取りとUPDATEの間に別リクエストが
    // 公開範囲を変えると、古い値に基づいて判定してしまうため。
    let existing = sqlx::query!(
        r#"SELECT type as "content_type: ContentType", path,
                  visibility as "visibility: Visibility", created_by
           FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    ensure_editable(&user, existing.visibility, existing.created_by)?;
    let existing_type = existing.content_type;
    // 入力の検証は、存在を隠す判定 (404) と権限の判定の後に行う。
    let title = validate_title(&payload.title)?;
    // `group` を `private` にする要求は誰の操作でも422。権限の判定 (403) より先に行う。
    reject_private_group(
        existing_type,
        payload.visibility.unwrap_or(existing.visibility),
    )?;
    let visibility = resolve_visibility_for_update(
        &user,
        payload.visibility,
        existing.visibility,
        existing.created_by,
    )?;
    // 走査中の `path` 変更を防ぐ (→ docs/archive.md「スキャン」)。取れなければ 409 を返し、
    // 待たない。待つと走査が終わるまで `contents_write_lock` を握り続けることになる。
    let _scan_guard = (existing_type == ContentType::Archive)
        .then(|| state.archive_scans.acquire(id))
        .transpose()?;

    let parent_id = validate_parent(&state.pool, payload.parent_id, Some(id)).await?;
    let (url, path, extensions, title_template) = match existing_type {
        ContentType::Link => (link_url.transpose()?.map(|(url, _)| url), None, None, None),
        ContentType::Folder => (
            None,
            Some(validate_folder_path(&state, payload.path.as_deref()).await?),
            None,
            None,
        ),
        // fileはJSONではなくmultipart専用エンドポイント(replace_content_upload)で更新する。
        ContentType::File => {
            return Err(AppError::Validation(
                "use PUT /contents/{id}/upload to update a file content".to_string(),
            ));
        }
        ContentType::Group => (None, None, None, None),
        ContentType::Archive => (
            None,
            Some(validate_folder_path(&state, payload.path.as_deref()).await?),
            super::archive::normalize_extensions(payload.extensions.as_deref()),
            super::archive::validate_title_template(
                &state.pool,
                Some(id),
                payload.title_template.as_deref(),
            )
            .await?,
        ),
    };

    // `path` を変えたら索引を捨てる (→ docs/archive.md「スキャン」)。`rel_path` は登録先を
    // 起点とする相対パスなので、起点が変われば同じ `rel_path` が別のファイルを指す。
    // 残すと変更前の公開フラグが無関係なファイルへ引き継がれる。
    // 拡張子の変更では消さない。索引対象であり続けたアイテムは公開フラグを保つ
    // (対象から外れた行は次の走査で消える)。
    let path_changed = existing_type == ContentType::Archive && existing.path != path;

    // `path` の更新と索引の削除は同じトランザクションに入れる (→ docs/archive.md「スキャン」)。
    // 分けると、削除だけが失敗したときに新しい `path` と旧索引の組み合わせが残り、
    // 次の走査で変更前の公開フラグが無関係なファイルへ引き継がれる。
    let mut tx = crate::db::begin_write(&state.pool).await?;

    // `updated_at` はSQLiteに`ON UPDATE`相当の機能が無いため、ここで明示的に更新する。
    let row = sqlx::query_as!(
        ContentRow,
        r#"UPDATE contents
           SET parent_id = ?, title = ?, url = ?, path = ?, description = ?,
               visibility = ?, extensions = ?, title_template = ?,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE id = ?
           RETURNING id as "id!", type as "content_type: ContentType", parent_id, title, url,
                     path, description, created_at as "created_at!", file_name, file_size,
                     visibility as "visibility: Visibility", created_by, extensions, title_template"#,
        parent_id,
        title,
        url,
        path,
        payload.description,
        visibility,
        extensions,
        title_template,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    if path_changed {
        sqlx::query!("DELETE FROM archive_items WHERE archive_id = ?", id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    Ok(Json(admin_response(&state.pool, row).await?))
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetCreatorRequest {
    /// 新しい作成者のユーザーid。
    pub created_by: i64,
}

/// 作成者の付け替え (→ docs/access.md「ロールと操作」)。`admin` だけ。
///
/// 作成者のいないものや、辞めた人のものを、引き継ぐ人が書き換えられるようにするため。
/// `private` は付け替えない (403)。本人しか見られないはずのものが、付け替えた先に見えてしまうため。
#[utoipa::path(
    put,
    path = "/contents/{id}/creator",
    params(("id" = i64, Path)),
    request_body = SetCreatorRequest,
    responses(
        (status = OK, body = AdminContentResponse),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外・private"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない"),
        (status = 422, body = crate::error::ErrorResponse, description = "存在しないユーザー"),
    )
)]
async fn set_creator(
    _admin: AdminUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<SetCreatorRequest>,
) -> Result<Json<AdminContentResponse>, AppError> {
    // 公開範囲の判定から書き込みまでを直列化する。判定の後に `private` へ変わっても書き込まないため。
    let _write_guard = state.contents_write_lock.lock().await;
    let visibility = sqlx::query_scalar!(
        r#"SELECT visibility as "visibility: Visibility" FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    if visibility == Visibility::Private {
        return Err(AppError::Forbidden);
    }
    let user_exists = sqlx::query_scalar!("SELECT 1 FROM users WHERE id = ?", payload.created_by)
        .fetch_optional(&state.pool)
        .await?
        .is_some();
    if !user_exists {
        return Err(AppError::Validation(
            "createdBy user does not exist".to_string(),
        ));
    }

    let row = sqlx::query_as!(
        ContentRow,
        r#"UPDATE contents
           SET created_by = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE id = ?
           RETURNING id as "id!", type as "content_type: ContentType", parent_id, title, url,
                     path, description, created_at as "created_at!", file_name, file_size,
                     visibility as "visibility: Visibility", created_by, extensions, title_template"#,
        payload.created_by,
        id,
    )
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(admin_response(&state.pool, row).await?))
}

#[utoipa::path(
    delete,
    path = "/contents/{id}",
    params(("id" = i64, Path)),
    responses(
        (status = 204),
        (status = 401, body = crate::error::ErrorResponse),
        (status = 403, body = crate::error::ErrorResponse),
        (status = 404, body = crate::error::ErrorResponse),
    )
)]
async fn delete_content(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    // 配下の昇格からDELETE〜GCまでを直列化する (→ `AppState::contents_write_lock`)。
    // typeによってロックの有無を変えない。
    let _write_guard = state.contents_write_lock.lock().await;

    // 公開範囲・作成者・typeをDELETEの前に取る。DELETE~RETURNINGで受け取ってからでは、
    // 他人の `private` (404で隠す対象) でも行が消えてしまう。typeは走査ロックの要否に使う。
    let existing = sqlx::query!(
        r#"SELECT type as "content_type: ContentType", visibility as "visibility: Visibility",
                  created_by
           FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    ensure_editable(&user, existing.visibility, existing.created_by)?;
    let existing_type = existing.content_type;

    // 走査中の削除を防ぐ (→ docs/archive.md「スキャン」)。走査の途中で行が消えると、
    // 同期が消えたアーカイブの索引を書き戻してしまう。update_content と同じく待たない。
    let _scan_guard = (existing_type == ContentType::Archive)
        .then(|| state.archive_scans.acquire(id))
        .transpose()?;

    // groupを削除する場合、直接の子はルート直下(parent_id = NULL)へ昇格させる
    // (孫以下は親=直接の子がそのまま生き残るので変更不要)。この
    // UPDATE(reparent)をDELETEより先にトランザクション内で行うことで、削除対象を
    // 親として参照する行が無い状態を作ってからDELETEする。こうすることで、
    // `contents.parent_id`の`ON DELETE CASCADE`が発火する前提(削除対象がまだ
    // 親として参照されている状態)自体を作らない
    // (詳細はmigrations/20260908070902_add_parent_id_index_to_contents.sqlのコメント参照)。
    // group以外の削除でもこのUPDATEは無害(該当行が無いだけ)なので、type分岐はしない。
    let mut tx = crate::db::begin_write(&state.pool).await?;

    sqlx::query!(
        r#"UPDATE contents SET parent_id = NULL,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE parent_id = ?"#,
        id
    )
    .execute(&mut *tx)
    .await?;

    // `type`/`blob_hash`をRETURNINGで受け取り、file行なら削除後にblobのGCを行う。
    let deleted = sqlx::query!(
        r#"DELETE FROM contents WHERE id = ?
           RETURNING type as "content_type: ContentType", blob_hash"#,
        id
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    tx.commit().await?;

    if deleted.content_type == ContentType::File
        && let Some(hash) = deleted.blob_hash
    {
        gc_blob_if_unreferenced(&state, &hash).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

/// `hash`を参照する行が(このDELETE/UPDATEを確定させた後で)他に残っていなければ、
/// 対応するblobファイルを削除する。呼び出し側は必ずDBの変更を確定させた**後**に呼ぶこと
/// (確定前に呼ぶと、削除/更新対象自身の古い参照がまだ残っていて「参照あり」に
/// 誤判定してしまう)。
///
/// あくまでベストエフォートの後始末なので、失敗しても呼び出し元(DELETE/PUT)を
/// 失敗させない(戻り値が無いのはそのため)。DBの変更は既に確定しているため、ここで
/// 5xxを返すと「成功したはずの操作が失敗したように見える」上に、失敗を見た管理者が
/// 同じDELETEを再試行すると今度は404になって余計混乱させる。ファイルが既に存在しない
/// 場合(孤立blobの手動削除・クラッシュ後の再実行等)や削除自体に失敗した場合
/// (Windowsでダウンロード配信中に共有違反になる等)は孤立blobとしてログに警告を残すだけ
/// にする(孤立blobは`AppState::blobs_dir`配下に残るだけの「無害」な状態であり、
/// 後から手動/別ツールで掃除できる)。
async fn gc_blob_if_unreferenced(state: &AppState, hash: &str) {
    if !is_valid_blob_hash(hash) {
        tracing::warn!(
            blob_hash = %hash,
            "blob_hash is not 64 hex digits; skipping GC (the database may have been modified directly)"
        );
        return;
    }

    let remaining = match sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!" FROM contents WHERE blob_hash = ?"#,
        hash
    )
    .fetch_one(&state.pool)
    .await
    {
        Ok(remaining) => remaining,
        Err(err) => {
            tracing::warn!(
                %err,
                blob_hash = %hash,
                "failed to count blob references; the blob is left orphaned"
            );
            return;
        }
    };

    if remaining != 0 {
        return;
    }

    let path = state.blobs_dir.join(hash);
    if let Err(err) = tokio::fs::remove_file(&path).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(
            %err,
            blob_hash = %hash,
            "failed to delete unreferenced blob; the blob is left orphaned"
        );
    }
}

// --- folder コンテンツ配下の閲覧・配信 ---

#[derive(Debug, Deserialize, IntoParams)]
struct BrowseQuery {
    /// folderの登録パスからの相対パス。省略時(空文字列)はルート。
    #[serde(default)]
    path: String,
    /// 並び順。`title` (既定、タイトル順) か `new` (新しい順)。
    /// ディレクトリ優先の並びは変えず、その中の順だけを切り替える (→ docs/ui.md「ホーム・グループ・フォルダ・アーカイブの並び順」)。
    /// 知らない値は既定として扱う。
    #[serde(default)]
    sort: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct FolderEntry {
    pub(super) name: String,
    pub(super) is_dir: bool,
    /// ディレクトリの場合は `None`。
    size: Option<u64>,
    /// UNIXエポックミリ秒。取得できない場合は `None`。
    modified_at: Option<i64>,
    /// ページ内でプレビューする画像なら、その大きさ (→ docs/ui.md「画像のプレビュー」)。
    image: Option<super::thumbnails::ImageSize>,
    /// テキストのビューアで見せるファイルか (→ docs/ui.md「PDF・動画・テキストのビューア」)。
    is_text: bool,
    /// 行に縮小画像を出してみるか (→ docs/ui.md「画像のプレビュー」)。
    thumbnail: bool,
}

/// 更新日時を UNIX エポックミリ秒で返す。取れなければ `None`。
pub(super) fn modified_at_millis(metadata: &std::fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct FolderBrowseResponse {
    folder_title: String,
    /// ルートに近い順の祖先グループ。自分自身は含まない。
    /// フォルダもグループの下に置けるため、閲覧側のパンくずで上へ戻れるようにする。
    ancestors: Vec<GroupAncestor>,
    path: String,
    entries: Vec<FolderEntry>,
}

/// `id` が指す `type = 'folder'` の行から、フォルダのタイトルと登録済みルートパス
/// (書き込み時にcanonicalize済み)を取得する。folder以外(link等)のidや存在しないidは
/// 404にする(browse/download はfolder専用のエンドポイントであるため)。
///
/// 公開範囲の判定もここで行う。**ファイルシステムに触る前に済ませること**が重要で、
/// 後回しにすると匿名の呼び出し元が401と404の差から`authenticated`なフォルダの中身の
/// 有無を探れてしまう。
pub(super) async fn folder_root(
    state: &AppState,
    viewer: &Viewer,
    id: i64,
) -> Result<(String, String, Lineage), AppError> {
    let row = sqlx::query!(
        r#"SELECT title, path FROM contents WHERE id = ? AND type = ?"#,
        id,
        ContentType::Folder
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let lineage = ensure_viewable(&state.pool, viewer, id).await?;
    Ok((row.title, folder_path(row.path)?, lineage))
}

/// パスの状態をファイルシステムに問い合わせる。
///
/// クローンをここで行うのは、呼び出し側が判定用の複製を持ち回らずに済むようにするため。
async fn check_path(path: &FsPath, check: fn(&FsPath) -> bool) -> Result<bool, AppError> {
    let path = path.to_path_buf();
    run_blocking(move || check(&path)).await
}

async fn path_is_dir(path: &FsPath) -> Result<bool, AppError> {
    check_path(path, FsPath::is_dir).await
}

async fn path_is_file(path: &FsPath) -> Result<bool, AppError> {
    check_path(path, FsPath::is_file).await
}

async fn path_exists(path: &FsPath) -> Result<bool, AppError> {
    check_path(path, FsPath::exists).await
}

/// 相対パスの各コンポーネントが通常の名前(`Component::Normal`)のみであることを確認する
/// 安価な一次フィルタ。`..`・ルート・(Windowsの)ドライブプレフィックス等を弾く。
///
/// Windows では `:` を含む名前も弾く。`a.mp3:stream` は代替データストリームを指し、
/// 一覧に出ないものを配信できてしまうため (→ docs/folders.md「公開できるフォルダ」)。
fn reject_traversal_components(rel: &str) -> Result<(), AppError> {
    for component in FsPath::new(rel).components() {
        match component {
            Component::Normal(name) if cfg!(windows) && name.to_string_lossy().contains(':') => {
                return Err(AppError::NotFound);
            }
            Component::Normal(_) => {}
            _ => return Err(AppError::NotFound),
        }
    }
    Ok(())
}

/// `root`(DB保存済み、canonicalize済みの絶対パス)配下の `rel` を解決する。
///
/// `root` 自体もリクエストのたびに再度 `canonicalize` してから比較する。DB保存値を
/// そのまま信用しない理由は、シンボリックリンクを含むrootパス配下に、更にシンボリックリンクで
/// root外を指すエントリがある場合、`candidate` 側だけをcanonicalizeして比較しても
/// 正しく判定できない(root自体の実体パスと揃えて比較する必要がある)ため。
/// `..`・絶対パス指定・シンボリックリンクによるroot外脱出のいずれも、この
/// canonicalize後のstarts_with比較で一括して弾ける。
///
/// 検証に失敗した場合は一律404にする(403にはしない: パスの存在有無を教えないため)。
///
/// 【既知の制限・運用上の前提(TOCTOU)】ここで得た canonicalize 済みパスは、その後
/// `browse_folder`/`download_content` がパス文字列として`read_dir`/`ServeFile`に
/// 再度渡す。検証(canonicalize)と実際のファイルオープンはアトミックではないため、
/// 理論上はその間に対象を書き換えられると検証をすり抜けられる(TOCTOU)。これを厳密に
/// 塞ぐには Unix の `openat` + `O_NOFOLLOW` でファイルディスクリプタを固定する再設計
/// (`ServeFile`も自前のストリーミング実装に置き換える)が要るが、今回のスコープ外とした。
/// **folderの登録先(および祖先ディレクトリ)には、weblavプロセス以外の信頼できない主体に
/// 書き込み権限を与えないこと。** 書き込み可能な第三者がいる共有フォルダはサポート対象外。
/// SMB/NFS共有・他のログインユーザーによる書き込み等でこの前提が崩れる場合は、この制限の
/// 解消(上記の再設計)が必須になる。なお`file`コンテンツ(アップロード)はこのTOCTOUの
/// 対象外: 実体は`blobs_dir`配下にweblavプロセス自身だけが書くcontent-addressedな
/// 領域にあり、folderのような「外部が管理する共有ディレクトリ」を覗くものではないため。
async fn resolve_path(state: &AppState, root: &str, rel: &str) -> Result<PathBuf, AppError> {
    reject_traversal_components(rel)?;

    let root = PathBuf::from(root);
    let candidate = root.join(rel);
    let own_dirs = state.own_dirs.clone();
    let roots = super::roots::load_roots(&state.pool).await?;

    let rel_owned = rel.to_string();
    let canonical_candidate = run_blocking(move || {
        let own_dirs = super::roots::OwnDirs::resolve(&own_dirs);
        let canonical_root = std::fs::canonicalize(&root).map_err(|_| AppError::NotFound)?;

        // 「公開できるフォルダ」の外は**配信の側でも**拒む。登録時の検証
        // (`validate_folder_path`) は書き込み経路にしか掛からないので、登録を
        // 消したあとや、登録済みのフォルダを削除したあとも配信され続ける
        // (→ docs/folders.md「公開できるフォルダ」)。
        //
        // **候補パスに触る前に判定する**。触ってから弾くと、候補が実在するかどうかで
        // 応答までの時間が変わり、存在を推し量る手がかりになる。
        // 存在を伏せるため 422 ではなく 404 を返す (閲覧者に見せる経路のため)。
        if !super::roots::is_within_roots(&roots, &canonical_root, &own_dirs) {
            return Err(AppError::NotFound);
        }

        let canonical_candidate =
            std::fs::canonicalize(&candidate).map_err(|_| AppError::NotFound)?;
        if !canonical_candidate.starts_with(&canonical_root) {
            return Err(AppError::NotFound);
        }
        // 一覧に出さないもの (ドット始まり・シンボリックリンク・隠し属性) は配信もしない。
        // 一覧に出ないだけでパスを知っていれば落とせる、という食い違いを残さないため。
        // weblav 自身の置き場 (設定とデータ) の中も同じ (公開できるフォルダが `C:\` のような祖先でもありうる)。
        // いずれも存在を伏せるため 404 (→ docs/folders.md「公開できるフォルダ」)。
        //
        // 名前は、要求の名前と実体の名前の両方で見る。Windows の 8.3 短縮名 (`ENV~1`) は
        // ドットで始まらないまま `.env` に解決されるため。リンクを見分けるには、辿る前の
        // 要求のパスが要る。
        if super::fs::has_hidden_component(&canonical_root, FsPath::new(&rel_owned))
            || super::fs::has_dot_component(&canonical_root, &canonical_candidate)
            || own_dirs.contains(&canonical_candidate)
        {
            return Err(AppError::NotFound);
        }
        Ok::<_, AppError>(canonical_candidate)
    })
    .await??;

    Ok(canonical_candidate)
}

/// `dir` 直下のエントリを列挙する。ドットファイル(`.DS_Store`等)と weblav 自身の置き場
/// (`own_dirs`。→ docs/folders.md「公開できるフォルダ」) は除外し、
/// ディレクトリ優先(固定)→`order`で選んだ順(タイトル順か新しい順)でソートする。
///
/// シンボリックリンクは一覧に表示しない(ポリシー)。もし表示してリンク先を
/// canonicalize検証しようとすると、`resolve_path`のroot判定ロジックを
/// エントリごとに再実装することになり複雑になる上、その検証からダウンロード等の
/// 実際のファイルオープンまでの間に対象が差し替えられ得るTOCTOU(Time-of-check
/// to time-of-use)の窓を広げてしまう。単純にリンクそのものを見せない方が安全。
/// これによりroot外を指すリンクの名前・サイズ・更新日時が一覧経由で漏れることも防げる。
fn read_entries(
    dir: &FsPath,
    own_dirs: &super::roots::OwnDirs,
    order: SortOrder,
) -> std::io::Result<Vec<FolderEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let Some((name, _)) = super::fs::visible_entry(&entry) else {
            continue;
        };
        // `dir` は実体パスで、リンクは上で外しているので、エントリのパスも実体パスのまま比べられる。
        if own_dirs.contains(&entry.path()) {
            continue;
        }
        // ここまで来ればシンボリックリンクではないので、`std::fs::metadata` を呼んでも
        // 実体(非リンク)のメタデータがそのまま返る。取得自体が失敗した場合
        // (権限エラー等)はそのエントリだけ読み飛ばす。
        if let Some(entry) = folder_entry(name, &entry.path()) {
            entries.push(entry);
        }
    }
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| match order {
            SortOrder::Title => title_cmp(&a.name, &b.name),
            // `None` (取得できなかった更新日時) は最後に回し、同じ値・同じ`None`同士は
            // タイトル順でタイブレークする。
            SortOrder::New => b
                .modified_at
                .cmp(&a.modified_at)
                .then_with(|| title_cmp(&a.name, &b.name)),
        })
    });
    Ok(entries)
}

/// 一覧の行の形にする。メタデータを読めなければ `None`。`path` はリンクでない実体のパス。
///
/// ブロッキングI/Oを行うため、呼び出し側は `spawn_blocking` の中で呼ぶこと。
pub(super) fn folder_entry(name: String, path: &FsPath) -> Option<FolderEntry> {
    let metadata = std::fs::metadata(path).ok()?;
    let is_dir = metadata.is_dir();
    let size = (!is_dir).then_some(metadata.len());
    let modified_at = modified_at_millis(&metadata);
    let preview = size.map(|size| super::thumbnails::file_preview(&name, path, size));
    Some(FolderEntry {
        name,
        is_dir,
        size,
        modified_at,
        image: preview.as_ref().and_then(|preview| preview.image),
        is_text: preview.as_ref().is_some_and(|preview| preview.is_text),
        thumbnail: preview.as_ref().is_some_and(|preview| preview.thumbnail),
    })
}

/// folder配下のファイル/サブディレクトリ一覧。公開範囲の判定は `folder_root` で行う。
#[utoipa::path(
    get,
    path = "/contents/{id}/browse",
    params(("id" = i64, Path), BrowseQuery),
    responses(
        (status = OK, body = FolderBrowseResponse),
        (status = 401, body = crate::error::ErrorResponse),
        (status = 404, body = crate::error::ErrorResponse),
    )
)]
async fn browse_folder(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<BrowseQuery>,
) -> Result<Json<FolderBrowseResponse>, AppError> {
    let (folder_title, root, lineage) = folder_root(&state, &viewer, id).await?;
    let target = resolve_path(&state, &root, &query.path).await?;

    if !path_is_dir(&target).await? {
        return Err(AppError::NotFound);
    }

    let own_dirs = state.own_dirs.clone();
    let order = SortOrder::from_query(query.sort.as_deref());
    let entries = run_blocking(move || {
        read_entries(&target, &super::roots::OwnDirs::resolve(&own_dirs), order)
    })
    .await??;

    Ok(Json(FolderBrowseResponse {
        folder_title,
        ancestors: lineage.breadcrumbs(),
        path: query.path,
        entries,
    }))
}

// --- group コンテンツ配下の閲覧 ---

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub(super) struct GroupAncestor {
    pub(super) id: i64,
    pub(super) title: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct GroupBrowseResponse {
    group_title: String,
    /// ルートに近い順(祖先→自分の直前の親まで)。自分自身は含まない。
    /// パンくず表示にそのまま使える。
    ancestors: Vec<GroupAncestor>,
    entries: Vec<ContentResponse>,
}

/// `id`が指す`type = 'group'`のコンテンツについて、タイトル・祖先パス・直下の子一覧を
/// 1回のリクエストで返す。並び順は `GET /contents` と同じく `sort` で受ける。
/// group以外のidや存在しないidは404、グループ自体を開けなければ401/404にする
/// (→ docs/access.md「匿名閲覧の受け口」)。
/// 子一覧は`GET /contents`と同じく `can_list` で絞る。
#[utoipa::path(
    get,
    path = "/contents/{id}/group",
    params(("id" = i64, Path), SortQuery),
    responses(
        (status = OK, body = GroupBrowseResponse),
        (status = 401, body = crate::error::ErrorResponse),
        (status = 404, body = crate::error::ErrorResponse),
    )
)]
async fn browse_group(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<SortQuery>,
) -> Result<Json<GroupBrowseResponse>, AppError> {
    let group = sqlx::query!(
        r#"SELECT title FROM contents WHERE id = ? AND type = ?"#,
        id,
        ContentType::Group
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let lineage = ensure_viewable(&state.pool, &viewer, id).await?;
    let group_title = group.title;
    let ancestors = lineage.breadcrumbs();

    // 絞り込みと並べ替えを取得後に行う理由は `list_contents` と同じ。
    let mut rows = sqlx::query_as!(
        ContentRow,
        r#"SELECT id as "id!", type as "content_type: ContentType", parent_id, title, url, path,
                  description, created_at as "created_at!", file_name, file_size,
                  visibility as "visibility: Visibility", created_by, extensions, title_template
           FROM contents
           WHERE parent_id = ?"#,
        id,
    )
    .fetch_all(&state.pool)
    .await?;

    sort_rows(&mut rows, SortOrder::from_query(query.sort.as_deref()));

    // 自分か祖先が `hidden` のグループを開けるのはログイン済みの人だけで、閲覧の場には
    // 並ばない。中身を確かめられるよう、子は `can_list` ではなく `can_open` で絞る
    // (→ docs/access.md「匿名閲覧の受け口」)。
    let inspecting_hidden = lineage.any_hidden();
    let entries = rows
        .into_iter()
        .filter(|row| {
            if inspecting_hidden {
                can_open(&viewer, row.visibility, row.created_by)
            } else {
                can_list(&viewer, row.visibility, row.created_by)
            }
        })
        .map(ContentResponse::try_from)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(GroupBrowseResponse {
        group_title,
        ancestors,
        entries: with_previews(&state, entries).await?,
    }))
}

/// 一覧に並ぶ file コンテンツに、ページ内で見せるための情報 (画像の大きさ・テキストか) を埋める。
/// `ContentRow` は `blob_hash` を持たないので、並んだ file コンテンツの分をまとめて引き直す。
async fn with_previews(
    state: &AppState,
    mut contents: Vec<ContentResponse>,
) -> Result<Vec<ContentResponse>, AppError> {
    let link_urls: Vec<String> = contents
        .iter()
        .filter_map(|content| match content {
            ContentResponse::Link { url, .. } => Some(url.clone()),
            _ => None,
        })
        .collect();
    let mut link_previews = super::link_preview::cached(&state.pool, &link_urls).await?;
    for content in &mut contents {
        if let ContentResponse::Link { url, preview, .. } = content {
            *preview = link_previews.remove(url);
        }
    }

    let file_ids: Vec<i64> = contents
        .iter()
        .filter_map(|content| match content {
            ContentResponse::File { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    let file_ids_json = serde_json::to_string(&file_ids).expect("数の並びは JSON にできる");
    let hashes: HashMap<i64, Option<String>> = sqlx::query!(
        r#"SELECT id as "id!", blob_hash FROM contents
           WHERE id IN (SELECT value FROM json_each(?)) AND type = ?"#,
        file_ids_json,
        ContentType::File
    )
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| (row.id, row.blob_hash))
    .collect();

    let mut targets = Vec::new();
    for (index, content) in contents.iter().enumerate() {
        let ContentResponse::File {
            id,
            file_name,
            file_size,
            ..
        } = content
        else {
            continue;
        };
        let hash = hashes
            .get(id)
            .cloned()
            .flatten()
            .ok_or(AppError::DataIntegrity("file content without a blob_hash"))?;
        targets.push((
            index,
            blob_path(state, *id, &hash)?,
            file_name.clone(),
            u64::try_from(*file_size).unwrap_or(u64::MAX),
        ));
    }
    if targets.is_empty() {
        return Ok(contents);
    }

    let previews = run_blocking(move || {
        targets
            .into_iter()
            .map(|(index, path, file_name, size)| {
                (
                    index,
                    super::thumbnails::file_preview(&file_name, &path, size),
                )
            })
            .collect::<Vec<_>>()
    })
    .await?;
    for (index, preview) in previews {
        if let ContentResponse::File {
            image,
            is_text,
            thumbnail,
            ..
        } = &mut contents[index]
        {
            *image = preview.image;
            *is_text = preview.is_text;
            *thumbnail = preview.thumbnail;
        }
    }
    Ok(contents)
}

/// 閲覧者がホームからたどって一覧で見られる行の id (→ docs/search.md「見える範囲」)。
///
/// 全行を受けてメモリ上で親子を辿り、行ごとに再帰 CTE を投げない。判定済みの祖先で打ち切るので、
/// 各行を辿るのは1回だけになる。循環が無い前提は `fetch_lineage` と同じ。
fn listable_ids(viewer: &Viewer, rows: &[ContentRow]) -> HashSet<i64> {
    let by_id: HashMap<i64, &ContentRow> = rows.iter().map(|row| (row.id, row)).collect();
    let mut known: HashMap<i64, bool> = HashMap::new();
    for row in rows {
        // 判定済みの祖先か、ルートまで登ってから、上から順に決める。
        let mut chain = Vec::new();
        let mut inherited = true;
        let mut current = Some(row.id);
        while let Some(id) = current {
            if let Some(&listable) = known.get(&id) {
                inherited = listable;
                break;
            }
            let Some(node) = by_id.get(&id) else {
                inherited = false;
                break;
            };
            chain.push(*node);
            current = node.parent_id;
        }
        for node in chain.into_iter().rev() {
            inherited = inherited && can_list(viewer, node.visibility, node.created_by);
            known.insert(node.id, inherited);
        }
    }
    known
        .into_iter()
        .filter_map(|(id, listable)| listable.then_some(id))
        .collect()
}

/// 検索の、コンテンツの区画の結果 (→ docs/search.md)。
pub(super) struct ContentSearch {
    pub(super) hits: Vec<super::search::SearchContentHit>,
    pub(super) truncated: bool,
    /// 閲覧者が一覧で見られるアーカイブ。アイテムの区画は、この中から探す。
    pub(super) archives: Vec<super::archive_items::SearchableArchive>,
    /// 閲覧者が一覧で見られるフォルダ。フォルダの中の区画は、この中から探す。
    pub(super) folders: Vec<super::folder_search::SearchableFolder>,
    /// 閲覧者が一覧で見られる、リンクの一覧のファイルの `file` コンテンツ。
    pub(super) links_files: Vec<super::folder_search::LinksFileContent>,
}

/// 閲覧者が一覧で見られるコンテンツを、タイトルと説明で探す。
pub(super) async fn search_contents(
    state: &AppState,
    viewer: &Viewer,
    terms: &super::search::Terms,
) -> Result<ContentSearch, AppError> {
    // 祖先をたどるため全行を読む。数千件の規模なら足りる。
    let rows = sqlx::query_as!(
        ContentRow,
        r#"SELECT id as "id!", type as "content_type: ContentType", parent_id, title, url, path,
                  description, created_at as "created_at!", file_name, file_size,
                  visibility as "visibility: Visibility", created_by, extensions, title_template
           FROM contents"#,
    )
    .fetch_all(&state.pool)
    .await?;

    let group_titles: HashMap<i64, String> = rows
        .iter()
        .filter(|row| row.content_type == ContentType::Group)
        .map(|row| (row.id, row.title.clone()))
        .collect();

    let listable = listable_ids(viewer, &rows);
    let rows: Vec<ContentRow> = rows
        .into_iter()
        .filter(|row| listable.contains(&row.id))
        .collect();

    // 照らすのは全行の文字列を揃える計算で、件数に比例して重くなる。
    let terms_owned = terms.clone();
    let (mut hits, archives, folders, links_files) = run_blocking(move || {
        let mut hits = Vec::new();
        let mut archives = Vec::new();
        let mut folders = Vec::new();
        let mut links_files = Vec::new();
        for row in rows {
            match row.content_type {
                ContentType::Archive => archives.push((
                    row.id,
                    row.title.clone(),
                    row.path.clone(),
                    row.title_template.clone(),
                )),
                ContentType::Folder => folders.push((row.id, row.title.clone(), row.path.clone())),
                ContentType::File
                    if row
                        .file_name
                        .as_deref()
                        .is_some_and(super::links_file::is_links_file_name) =>
                {
                    links_files.push(super::folder_search::LinksFileContent {
                        id: row.id,
                        title: row.title.clone(),
                    });
                }
                _ => {}
            }
            let title = super::search::normalize(&row.title);
            if terms_owned.matches(&[&title]) {
                hits.push((row, false));
            } else if let Some(description) = &row.description
                && terms_owned.matches(&[&title, &super::search::normalize(description)])
            {
                hits.push((row, true));
            }
        }
        (hits, archives, folders, links_files)
    })
    .await?;

    hits.sort_by(|(a, _), (b, _)| title_cmp(&a.title, &b.title).then(a.id.cmp(&b.id)));
    let truncated = hits.len() > super::search::RESULT_LIMIT;
    hits.truncate(super::search::RESULT_LIMIT);

    let mut placements = Vec::with_capacity(hits.len());
    let mut contents = Vec::with_capacity(hits.len());
    for (row, matched_in_description) in hits {
        let parent = row.parent_id.map(|id| GroupAncestor {
            id,
            title: group_titles.get(&id).cloned().unwrap_or_default(),
        });
        placements.push((parent, matched_in_description));
        contents.push(ContentResponse::try_from(row)?);
    }
    let contents = with_previews(state, contents).await?;

    let archives = archives
        .into_iter()
        .map(|(id, title, path, title_template)| {
            Ok(super::archive_items::SearchableArchive {
                id,
                title,
                path: path.ok_or(AppError::DataIntegrity("archive content without a path"))?,
                title_template,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let folders = folders
        .into_iter()
        .map(|(id, title, path)| {
            Ok(super::folder_search::SearchableFolder {
                id,
                title,
                path: folder_path(path)?,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    Ok(ContentSearch {
        hits: contents
            .into_iter()
            .zip(placements)
            .map(
                |(content, (parent, matched_in_description))| super::search::SearchContentHit {
                    content,
                    parent,
                    matched_in_description,
                },
            )
            .collect(),
        truncated,
        archives,
        folders,
        links_files,
    })
}

/// 同一オリジンでのインライン表示(ブラウザ内蔵ビューア)を許可するMIMEタイプかどうか。
/// `text/html`・`image/svg+xml` はスクリプト実行が可能なため、インライン表示は
/// セッションCookieを持つこのアプリのオリジン上でXSSの経路になり得る。ホワイトリストに
/// 無いものは安全側で強制ダウンロード(`attachment`)にする。
pub(super) fn is_inline_allowed(mime: &mime_guess::Mime) -> bool {
    if mime.essence_str() == "image/svg+xml" {
        return false;
    }
    matches!(
        mime.type_(),
        mime_guess::mime::IMAGE | mime_guess::mime::AUDIO | mime_guess::mime::VIDEO
    ) || mime.essence_str() == "application/pdf"
        || mime.essence_str() == mime_guess::mime::TEXT_PLAIN.essence_str()
}

/// `Content-Disposition` ヘッダーの値を組み立てる。ファイル名は日本語等の非ASCII文字を
/// 含み得るため、ASCII近似のフォールバック(`filename=`)と RFC 5987 のパーセントエンコード
/// (`filename*=UTF-8''...`)の両方を付与する(モダンなブラウザは後者を優先する)。
pub(super) fn content_disposition_header(disposition: &str, file_name: &str) -> HeaderValue {
    let ascii_fallback: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded = utf8_percent_encode(file_name, NON_ALPHANUMERIC);
    let value = format!("{disposition}; filename=\"{ascii_fallback}\"; filename*=UTF-8''{encoded}");
    HeaderValue::from_str(&value).unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// `target`を`ServeFile`(ストリーミング・Rangeリクエスト対応)で配信し、
/// `Content-Disposition`をホワイトリスト方式で上書きしたレスポンスを組み立てる。
/// folder配下のファイルとfileコンテンツ(blob)の両方から共通で使う。
/// `mime`は呼び出し側が決める(folderは実ファイルパスから、fileは保存済みファイル名から
/// 推測する。blobのパス自体は拡張子を持たないため、blobのパスから推測すると
/// 常に`application/octet-stream`になってしまう)。
async fn serve_file_response(
    target: &FsPath,
    file_name: &str,
    mime: mime_guess::Mime,
    request: Request,
) -> Response {
    // `ServeFile::call` は `Infallible`(常に成功、エラーはレスポンス自体に変換される)。
    let mut response = ServeFile::new_with_mime(target, &mime)
        .oneshot(request)
        .await
        .into_response();

    let disposition = if is_inline_allowed(&mime) {
        "inline"
    } else {
        "attachment"
    };
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        content_disposition_header(disposition, file_name),
    );

    response
}

/// 配信する実体と、その名前 (mime と `Content-Disposition` に使う)。
pub(super) struct ServedFile {
    pub(super) path: PathBuf,
    pub(super) file_name: String,
}

impl ServedFile {
    pub(super) async fn download(self, request: Request) -> Response {
        let mime = mime_guess::from_path(&self.file_name).first_or_octet_stream();
        serve_file_response(&self.path, &self.file_name, mime, request).await
    }

    pub(super) async fn thumbnail(
        self,
        state: &AppState,
        request_headers: &HeaderMap,
    ) -> Result<Response, AppError> {
        super::thumbnails::thumbnail_response(
            &state.thumbnails,
            self.path,
            &self.file_name,
            request_headers,
        )
        .await
    }
}

/// `root` 配下の `rel` を `resolve_path` で解決する。ファイルでなければ404。
/// folder配下のファイルとアーカイブアイテムの両方が、実体そのもの以外に
/// 手がかりを持たない (DBにfile_nameを別途保存していない) ため共有する。
pub(super) async fn resolve_file_under(
    state: &AppState,
    root: &str,
    rel: &str,
) -> Result<ServedFile, AppError> {
    let target = resolve_path(state, root, rel).await?;
    if !path_is_file(&target).await? {
        return Err(AppError::NotFound);
    }

    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("download")
        .to_string();
    Ok(ServedFile {
        path: target,
        file_name,
    })
}

/// folder配下のファイル、またはfileコンテンツの実体(blob)を配信する。
/// そのコンテンツを開けるなら (`ensure_openable`) ダウンロードできる。
/// `query.path`はfolderの場合のみ使う(fileは`id`だけで実体が一意に決まる)。
#[utoipa::path(
    get,
    path = "/contents/{id}/download",
    params(("id" = i64, Path), BrowseQuery),
    responses(
        (status = OK),
        (status = 401, body = crate::error::ErrorResponse),
        (status = 404, body = crate::error::ErrorResponse),
    )
)]
async fn download_content(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<BrowseQuery>,
    request: Request,
) -> Result<Response, AppError> {
    let file = content_file(&state, &viewer, id, &query.path).await?;
    Ok(file.download(request).await)
}

/// `download_content` と同じ実体の縮小画像 (→ docs/ui.md「画像のプレビュー」)。
#[utoipa::path(
    get,
    path = "/contents/{id}/thumbnail",
    params(("id" = i64, Path), BrowseQuery),
    responses(
        (status = OK, description = "縮小画像 (JPEG か PNG)"),
        (status = 304, description = "ETag が一致した"),
        (status = 401, body = crate::error::ErrorResponse, description = "閲覧にログインが必要"),
        (status = 404, body = crate::error::ErrorResponse, description = "見えない・存在しない、またはプレビューしない画像"),
    )
)]
async fn thumbnail_content(
    viewer: Viewer,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<BrowseQuery>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let file = content_file(&state, &viewer, id, &query.path).await?;
    file.thumbnail(&state, &headers).await
}

/// `id` のコンテンツが指す実体。folder は `rel` で配下のファイルを、file は blob を指す。
pub(super) async fn content_file(
    state: &AppState,
    viewer: &Viewer,
    id: i64,
    rel: &str,
) -> Result<ServedFile, AppError> {
    let row = sqlx::query!(
        r#"SELECT type as "content_type: ContentType", path, blob_hash, file_name
           FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    // 公開範囲の判定はファイルシステムに触る前に行う(理由は`folder_root`のコメント)。
    ensure_viewable(&state.pool, viewer, id).await?;

    match row.content_type {
        // アーカイブのアイテムは専用の配信エンドポイント
        // (`GET /contents/{id}/items/{item_id}/download`、→ archive_items::download_item)
        // で扱う。
        ContentType::Link | ContentType::Group | ContentType::Archive => Err(AppError::NotFound),
        ContentType::Folder => {
            let root = folder_path(row.path)?;
            resolve_file_under(state, &root, rel).await
        }
        ContentType::File => {
            let hash = row
                .blob_hash
                .ok_or(AppError::DataIntegrity("file content without a blob_hash"))?;
            let file_name = row
                .file_name
                .ok_or(AppError::DataIntegrity("file content without a file_name"))?;
            let target = blob_path(state, id, &hash)?;

            if !path_is_file(&target).await? {
                // rowはあるのに実体が無い(想定外)。管理者の手動操作・バグ等の可能性が
                // あるため、パストラバーサル対策の404とは別にログに残す。
                tracing::error!(
                    content_id = id,
                    blob_hash = %hash,
                    "blob for file content not found"
                );
                return Err(AppError::NotFound);
            }

            // mimeはblobのパス(拡張子を持たない)ではなく、保存済みの元ファイル名から推測する。
            Ok(ServedFile {
                path: target,
                file_name,
            })
        }
    }
}

/// file コンテンツの実体 (blob) のパス。
fn blob_path(state: &AppState, id: i64, hash: &str) -> Result<PathBuf, AppError> {
    // DBの`blob_hash`をファイルパスに結合する前の多層防御(`is_valid_blob_hash`
    // 参照)。通常のAPI経路では常に正当な値のはずなので、ここに来るのは
    // DBが直接改変された場合のみ。
    if !is_valid_blob_hash(hash) {
        tracing::error!(
            content_id = id,
            blob_hash = %hash,
            "blob_hash is not 64 hex digits (the database may have been modified directly)"
        );
        return Err(AppError::NotFound);
    }
    Ok(state.blobs_dir.join(hash))
}

/// アップロード一時ファイルの名前を作る(プロセスID+単調カウンタ+ナノ秒時刻で
/// プロセス内外の衝突を避ける)。
static TEMP_BLOB_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_blob_file_name() -> String {
    let seq = TEMP_BLOB_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    format!("{pid}-{nanos}-{seq}.tmp")
}

/// `blob_hash`が想定形式(SHA-256を`hex_encode`した64桁の小文字16進文字列)かどうかを
/// 検証する。通常のAPI経路(`commit_blob`が書き込む値)では常にこの形式になるはずだが、
/// DBの値をファイルパスに結合する前の多層防御として、DBから読んだ`blob_hash`を使う
/// 箇所(GC・ダウンロード)ではこれで検証してから使う。DBが直接改変され
/// `../../etc/passwd`等の不正な値が入っていた場合でも、`blobs_dir`外を指すパスの
/// 組み立てに使わせない。
pub(super) fn is_valid_blob_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// multipartの`file`フィールド名から保存用のファイル名を作る。ブラウザによっては
/// パス区切りを含む値を送ることがあるため、basenameだけを取り出す。空になった場合は
/// "download" にフォールバックする。
fn sanitize_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    if base.is_empty() {
        "download".to_string()
    } else {
        base.to_string()
    }
}

/// 一時ファイル(`blobs_dir/tmp/`配下)へのパスを持ち、`commit_blob`で確定(rename)する前に
/// エラー等で処理が中断した場合、Dropで削除する。`commit_blob`が成功したら`committed`を
/// `true`にしてDropでの削除をスキップする。
struct TempBlobGuard {
    path: PathBuf,
    committed: bool,
}

impl TempBlobGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }
}

impl Drop for TempBlobGuard {
    fn drop(&mut self) {
        // `Drop` は同期関数のため await できない。削除自体の完了を待つ相手もいないので、
        // `spawn_blocking` に投げて tokio ランタイムを止めないようにするだけで十分
        // (fire-and-forget、失敗しても一時ファイルが残るだけで実害はない)。
        if !self.committed {
            let path = std::mem::take(&mut self.path);
            tokio::task::spawn_blocking(move || {
                let _ = std::fs::remove_file(&path);
            });
        }
    }
}

/// multipartの`file`フィールドを`blobs_dir/tmp/`配下の一時ファイルにストリーミングで
/// 書き込みながらSHA-256ハッシュとサイズを計算する。`max_bytes`を超えたら
/// `AppError::FileTooLarge`にする(一時ファイルは`TempBlobGuard`のDropで削除される)。
/// クライアントが申告するサイズは信用せず、実際に書き込んだバイト数だけを使う。
async fn stream_field_to_temp_blob(
    field: &mut axum::extract::multipart::Field<'_>,
    blobs_dir: &FsPath,
    max_bytes: u64,
) -> Result<(TempBlobGuard, String, i64), AppError> {
    // `create_owner_only_dir`は対象ディレクトリ自体しか0700にしない(`create_dir_all`で
    // 暗黙に作られる親ディレクトリの権限は変えない)。`db::connect`と同じ理由で、
    // `blobs_dir`自身にも明示的に呼んでおく(`tmp_dir`だけだと`blobs_dir`がumask依存の
    // 権限になってしまう)。ブロッキングI/Oなので1つの `spawn_blocking` にまとめる。
    let blobs_dir_owned = blobs_dir.to_path_buf();
    let tmp_dir = run_blocking(move || -> std::io::Result<PathBuf> {
        create_owner_only_dir(&blobs_dir_owned)?;
        let tmp_dir = blobs_dir_owned.join("tmp");
        create_owner_only_dir(&tmp_dir)?;
        Ok(tmp_dir)
    })
    .await??;

    let tmp_path = tmp_dir.join(temp_blob_file_name());
    let mut file = tokio::fs::File::create(&tmp_path).await?;
    let guard = TempBlobGuard::new(tmp_path);

    let mut hasher = Sha256::new();
    let mut size: u64 = 0;
    while let Some(chunk) = field.chunk().await.map_err(multipart_error_to_app_error)? {
        size += chunk.len() as u64;
        if size > max_bytes {
            return Err(AppError::FileTooLarge);
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);

    let hash = super::hex_encode(&hasher.finalize());
    let size = i64::try_from(size).unwrap_or(i64::MAX);
    Ok((guard, hash, size))
}

/// 一時ファイルを`blobs_dir/<hash>`にリネームしてコミットする(確定後は`guard`のDropで
/// 削除されないようにする)。Unix/Windowsとも通常`rename`は既存の宛先を上書きするだけで
/// 完了する(重複排除: 既に同じハッシュ=同じ内容のblobがあっても実害は無い)。
///
/// それでも`rename`が失敗した場合(典型的にはWindowsで、宛先blobが別の同時ダウンロード
/// 配信によって開かれている等の共有違反)、宛先が既に存在するかどうかで判定する:
/// 存在するなら「重複排除により既に用意されている」とみなし一時ファイル側
/// (guardのDropで削除される)を諦めるだけにする。存在しないなら本当の失敗としてエラーを
/// 返す。
async fn commit_blob(
    mut guard: TempBlobGuard,
    blobs_dir: &FsPath,
    hash: &str,
) -> Result<(), AppError> {
    let dest = blobs_dir.join(hash);
    let rename_err = match tokio::fs::rename(&guard.path, &dest).await {
        Ok(()) => {
            guard.committed = true;
            return Ok(());
        }
        Err(err) => err,
    };

    let dest_exists = path_exists(&dest).await?;
    if dest_exists {
        Ok(())
    } else {
        Err(AppError::Io(rename_err))
    }
}

/// アップロードmultipartから収集したフィールド値。フィールドの到着順はクライアント次第
/// (`collect_upload_fields`参照)なので、検証は全フィールドを読み終えた後にまとめて行う。
#[derive(Default)]
struct UploadFields {
    title: Option<String>,
    description: Option<String>,
    parent_id: Option<i64>,
    visibility: Option<Visibility>,
    file: Option<(TempBlobGuard, String, i64, String)>,
}

/// multipartのテキストフィールド1つあたりの上限(バイト)。title/description等の
/// メタデータは通常数百文字程度で十分なはずなので、`file`フィールド用のストリーミング
/// サイズチェック(`stream_field_to_temp_blob`)とは別に、テキスト側にも独立した上限を
/// 設ける。これが無いと、`file`を伴わない巨大な`description`だけでも
/// `DefaultBodyLimit`のバックストップ(約`max_upload_bytes`+1MiB)近くまで
/// メモリに読み込まれてしまう(ログインした人だけの操作だが、事故防止のため)。
const MAX_TEXT_FIELD_BYTES: usize = 64 * 1024;

/// multipartのテキストフィールドを、`max_bytes`を超えたら早期に拒否しながら
/// チャンク単位で読む。`Field::text()`は全量を一括でメモリに読み込んでしまうため、
/// サイズを検証する前に大きなバッファを確保してしまう問題を避けるため、
/// 自前でチャンクを積み上げながらその都度サイズを確認する。
async fn read_text_field(
    field: &mut axum::extract::multipart::Field<'_>,
    max_bytes: usize,
) -> Result<String, AppError> {
    let mut buf = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(multipart_error_to_app_error)? {
        // 累積してから上限判定すると、1回の`chunk()`が既に上限を大幅に超える大きさを
        // 返した場合に、判定前にその全量を`buf`へコピーしてしまう(multipartライブラリの
        // チャンク分割ポリシーはこちらで制御できないため、単一チャンクが大きいことは
        // 想定しておく必要がある)。`extend_from_slice`より先にサイズを確認することで、
        // 上限超過が確定した時点で即座に拒否し、`buf`側への無駄なコピーを避ける。
        if buf.len().saturating_add(chunk.len()) > max_bytes {
            return Err(AppError::Validation("value is too long".to_string()));
        }
        buf.extend_from_slice(&chunk);
    }
    String::from_utf8(buf).map_err(|_| AppError::Validation("value is not valid UTF-8".to_string()))
}

/// multipartのフィールドを名前で振り分けて収集する。フィールドの順序に依存しない
/// (`file`が先に来ても後に来ても正しく扱える)。未知のフィールド名は読み飛ばす
/// (将来フロント側にフィールドが増えても本APIが壊れないようにするため)。
async fn collect_upload_fields(
    multipart: &mut Multipart,
    blobs_dir: &FsPath,
    max_bytes: u64,
) -> Result<UploadFields, AppError> {
    let mut fields = UploadFields::default();
    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(multipart_error_to_app_error)?
    {
        match field.name() {
            Some("title") => {
                fields.title = Some(read_text_field(&mut field, MAX_TEXT_FIELD_BYTES).await?);
            }
            Some("description") => {
                let text = read_text_field(&mut field, MAX_TEXT_FIELD_BYTES).await?;
                fields.description = (!text.is_empty()).then_some(text);
            }
            Some("parentId") => {
                let text = read_text_field(&mut field, MAX_TEXT_FIELD_BYTES).await?;
                let text = text.trim();
                if !text.is_empty() {
                    let value = text.parse::<i64>().map_err(|_| {
                        AppError::Validation("parentId must be a number".to_string())
                    })?;
                    fields.parent_id = Some(value);
                }
            }
            Some("visibility") => {
                let text = read_text_field(&mut field, MAX_TEXT_FIELD_BYTES).await?;
                let text = text.trim();
                if !text.is_empty() {
                    // 値の対応はJSONと同じ`Visibility`のserdeの定義に任せる。
                    let value = Visibility::deserialize(text.into_deserializer()).map_err(
                        |_: serde::de::value::Error| {
                            AppError::Validation(
                                "visibility is not one of the allowed values".to_string(),
                            )
                        },
                    )?;
                    fields.visibility = Some(value);
                }
            }
            Some("file") => {
                let file_name = sanitize_file_name(field.file_name().unwrap_or("download"));
                let (guard, hash, size) =
                    stream_field_to_temp_blob(&mut field, blobs_dir, max_bytes).await?;
                fields.file = Some((guard, hash, size, file_name));
            }
            _ => {
                while field
                    .chunk()
                    .await
                    .map_err(multipart_error_to_app_error)?
                    .is_some()
                {}
            }
        }
    }
    Ok(fields)
}

/// utoipaのOpenAPIドキュメント用のダミー構造体。実際のリクエストは
/// `axum::extract::Multipart` で受け取るため、この構造体自体はデシリアライズに使わない
/// (`multipart/form-data`のスキーマ表現としてutoipaに渡すためだけのもの)。
#[derive(Debug, Deserialize, ToSchema)]
#[allow(dead_code)]
struct UploadContentRequest {
    /// 省略・空ならファイル名 (拡張子を除く) にする。
    title: Option<String>,
    description: Option<String>,
    /// 親グループのid。省略/空ならルート直下に作成する。
    #[schema(value_type = Option<i64>)]
    parent_id: Option<i64>,
    /// 公開範囲。省略/空なら既定値。
    visibility: Option<Visibility>,
    #[schema(value_type = String, format = Binary)]
    file: Vec<u8>,
}

/// ファイルコンテンツを新規作成する(multipart)。`file`は必須、`title`/`description`は省略可
/// (`title`を省くとファイル名から付ける)。
#[utoipa::path(
    post,
    path = "/contents/upload",
    request_body(content = UploadContentRequest, content_type = "multipart/form-data"),
    responses(
        (status = 201, body = AdminContentResponse),
        (status = 401, body = crate::error::ErrorResponse),
        (status = 409, body = crate::error::ErrorResponse, description = "Free の上限に当たった"),
        (status = 413, body = crate::error::ErrorResponse),
        (status = 422, body = crate::error::ErrorResponse),
    )
)]
async fn upload_content(
    user: AuthUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<AdminContentResponse>), AppError> {
    // 受け取る前にも確かめる。上限に当たっているのに、大きいファイルを受け取り切ってから断らないため。
    // 受け取る間に別の要求が足すことがあるので、鍵の内側でもう一度数える。
    super::free_limit::check_content(state.pro.edition(), &state.pool, ContentType::File).await?;
    let fields =
        collect_upload_fields(&mut multipart, &state.blobs_dir, state.max_upload_bytes).await?;

    let (guard, hash, size, file_name) = fields
        .file
        .ok_or_else(|| AppError::Validation("file field is required".to_string()))?;

    // 親の検証〜commit_blob〜INSERTを直列化する (→ `AppState::contents_write_lock`)。
    // ストリーミング受信は済んでいるので、ロック区間には含めない。
    let visibility = resolve_visibility_for_create(fields.visibility);
    let _write_guard = state.contents_write_lock.lock().await;
    super::free_limit::check_content(state.pro.edition(), &state.pool, ContentType::File).await?;
    let parent_id = validate_parent(&state.pool, fields.parent_id, None).await?;
    let title = resolve_create_title(
        &state.pool,
        parent_id,
        fields.title.as_deref(),
        Some(title_from_file_name(&file_name)),
    )
    .await?;

    // DBへの書き込みより先にblobを確定させる: 万一この後のINSERTが失敗しても、
    // 参照する行が無い孤立blobが残るだけで実害が無い(逆順だと、実体を指せない行が
    // 残ってしまう)。
    commit_blob(guard, &state.blobs_dir, &hash).await?;

    let content_type = ContentType::File;
    let description = fields.description;

    let row = sqlx::query_as!(
        ContentRow,
        r#"INSERT INTO contents (type, parent_id, title, description, visibility,
                                  blob_hash, file_name, file_size, created_by)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
           RETURNING id as "id!", type as "content_type: ContentType", parent_id, title, url,
                     path, description, created_at as "created_at!", file_name, file_size,
                     visibility as "visibility: Visibility", created_by, extensions, title_template"#,
        content_type,
        parent_id,
        title,
        description,
        visibility,
        hash,
        file_name,
        size,
        user.id,
    )
    .fetch_one(&state.pool)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(admin_response(&state.pool, row).await?),
    ))
}

/// utoipaのOpenAPIドキュメント用のダミー構造体(`UploadContentRequest`と同様、
/// 実際のリクエストは`axum::extract::Multipart`で受け取る)。`file`は差し替え時のみ
/// 指定する(省略時は既存のファイルを維持する)。
#[derive(Debug, Deserialize, ToSchema)]
#[allow(dead_code)]
struct ReplaceContentUploadRequest {
    title: String,
    description: Option<String>,
    /// 親グループのid。省略/空ならルート直下へ移動する。
    #[schema(value_type = Option<i64>)]
    parent_id: Option<i64>,
    /// 公開範囲。省略/空なら現在値のまま。
    visibility: Option<Visibility>,
    // `value_type = String`だけだとOpenAPI上`file`が必須になってしまう
    // (`value_type`指定は元のOptionのnullable/required判定を上書きしてしまうため)。
    // `Option<String>`を明示して省略可であることをスキーマに正しく反映させる。
    #[schema(value_type = Option<String>, format = Binary)]
    file: Option<Vec<u8>>,
}

/// ファイルコンテンツを更新する(multipart)。`file`を指定した場合のみ実体を差し替える
/// (指定しなければ既存のblobを維持する)。`type = 'file'` 以外のidに対しては422にする
/// (link/folderの更新は既存の`PUT /contents/{id}`(JSON)を使う)。
#[utoipa::path(
    put,
    path = "/contents/{id}/upload",
    params(("id" = i64, Path)),
    request_body(content = ReplaceContentUploadRequest, content_type = "multipart/form-data"),
    responses(
        (status = OK, body = AdminContentResponse),
        (status = 401, body = crate::error::ErrorResponse),
        (status = 403, body = crate::error::ErrorResponse),
        (status = 404, body = crate::error::ErrorResponse),
        (status = 413, body = crate::error::ErrorResponse),
        (status = 422, body = crate::error::ErrorResponse),
    )
)]
async fn replace_content_upload(
    user: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> Result<Json<AdminContentResponse>, AppError> {
    // 事前チェック(型のみ。404/422の早期判定用): ロックは取らない。ロックを取ったまま
    // 大きいファイルのストリーミング受信を待つと、他のblob操作(GC含む)を不必要に
    // ブロックしてしまうため。
    let existing = sqlx::query!(
        r#"SELECT type as "content_type: ContentType", visibility as "visibility: Visibility",
                  created_by
           FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    // 他人の `private` なら、アップロードを受け取る前に404にする。
    ensure_editable(&user, existing.visibility, existing.created_by)?;
    if existing.content_type != ContentType::File {
        return Err(AppError::Validation(
            "this content is not a file".to_string(),
        ));
    }

    let fields =
        collect_upload_fields(&mut multipart, &state.blobs_dir, state.max_upload_bytes).await?;
    let description = fields.description;

    // commit_blob〜UPDATE〜GCを直列化する (→ `AppState::contents_write_lock`)。
    // 受信の間にこの行が削除・変更されうるので、ロック取得後に`existing`を取り直し、
    // 公開範囲の判定にもその値を使う。
    let _write_guard = state.contents_write_lock.lock().await;

    let existing = sqlx::query!(
        r#"SELECT type as "content_type: ContentType", blob_hash, file_name, file_size,
                  visibility as "visibility: Visibility", created_by
           FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    // 事前チェックの後に公開範囲が変わっている場合に備え、取り直した値で判定し直す。
    // 存在を隠す判定 (404) は、入力の検証 (422) より先に行う。
    ensure_editable(&user, existing.visibility, existing.created_by)?;
    if existing.content_type != ContentType::File {
        return Err(AppError::Validation(
            "this content is not a file".to_string(),
        ));
    }
    let title = validate_title(fields.title.as_deref().unwrap_or_default())?;
    let visibility = resolve_visibility_for_update(
        &user,
        fields.visibility,
        existing.visibility,
        existing.created_by,
    )?;
    let parent_id = validate_parent(&state.pool, fields.parent_id, Some(id)).await?;

    let old_hash = existing.blob_hash;

    let (hash, file_name, size) = match fields.file {
        Some((guard, hash, size, file_name)) => {
            commit_blob(guard, &state.blobs_dir, &hash).await?;
            (hash, file_name, size)
        }
        None => {
            let hash = old_hash
                .clone()
                .ok_or(AppError::DataIntegrity("file content without a blob_hash"))?;
            let file_name = existing
                .file_name
                .ok_or(AppError::DataIntegrity("file content without a file_name"))?;
            let size = existing
                .file_size
                .ok_or(AppError::DataIntegrity("file content without a file_size"))?;
            (hash, file_name, size)
        }
    };

    // `updated_at` はSQLiteに`ON UPDATE`相当の機能が無いため、ここで明示的に更新する。
    let row = sqlx::query_as!(
        ContentRow,
        r#"UPDATE contents
           SET parent_id = ?, title = ?, description = ?, blob_hash = ?,
               file_name = ?, file_size = ?, visibility = ?,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE id = ?
           RETURNING id as "id!", type as "content_type: ContentType", parent_id, title, url,
                     path, description, created_at as "created_at!", file_name, file_size,
                     visibility as "visibility: Visibility", created_by, extensions, title_template"#,
        parent_id,
        title,
        description,
        hash,
        file_name,
        size,
        visibility,
        id,
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    // 古いblobを新しいものに差し替えた場合、UPDATEを確定させた**後**に参照が無くなったかを
    // 確認する(確定前に確認すると、この行自体が持つ古い参照がまだ残っていて「参照あり」に
    // 誤判定してしまう)。ハッシュが変わらない場合(同一内容の再アップロード、あるいは
    // `file`未指定で既存を維持)は何もしない。
    if let Some(old_hash) = old_hash
        && old_hash != hash
    {
        gc_blob_if_unreferenced(&state, &old_hash).await;
    }

    Ok(Json(admin_response(&state.pool, row).await?))
}

/// `max_upload_bytes`は許可したい実際のファイルサイズ(`AppState::max_upload_bytes`)。
/// アップロード系エンドポイントにのみ`DefaultBodyLimit`を適用するため、別の
/// `OpenApiRouter`として組み立ててから`merge`する(`OpenApiRouter::route_layer`は
/// 呼び出し時点でルーターに登録済みの全ルートに掛かってしまうため、他のcontentsルートを
/// 巻き込まないようにこの順序にしている)。
pub(crate) fn router(max_upload_bytes: u64) -> OpenApiRouter<AppState> {
    let upload_body_limit = max_upload_bytes.saturating_add(UPLOAD_BODY_OVERHEAD_BYTES);
    let upload_body_limit = usize::try_from(upload_body_limit).unwrap_or(usize::MAX);

    let upload_routes = OpenApiRouter::new()
        .routes(routes!(upload_content))
        .routes(routes!(replace_content_upload))
        .layer(DefaultBodyLimit::max(upload_body_limit));

    // ブラウザが直接開くルートだけ、エラーを画面へのリダイレクトに変える
    // (→ `api::browser`)。他のルートを巻き込まないよう、アップロード系と同じく
    // 別の `OpenApiRouter` に分けてから merge する。
    let download_routes = OpenApiRouter::new()
        .routes(routes!(download_content))
        .layer(axum::middleware::from_fn(
            super::browser::redirect_errors_for_browsers,
        ));

    OpenApiRouter::new()
        .routes(routes!(list_contents, create_content))
        .routes(routes!(update_content, delete_content))
        .routes(routes!(set_creator))
        .routes(routes!(list_admin_contents))
        .routes(routes!(fetch_link_metadata))
        .routes(routes!(browse_folder))
        .routes(routes!(browse_group))
        .routes(routes!(thumbnail_content))
        .merge(download_routes)
        .merge(upload_routes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_validation<T>(result: &Result<T, AppError>) -> bool {
        matches!(result, Err(AppError::Validation(_)))
    }

    /// 空・空白だけの URL と、http(s) 以外のスキームは弾く。スキームの大文字小文字は問わず、綴りは保つ。
    #[test]
    fn validate_url_accepts_only_http_and_https() {
        assert_eq!(
            validate_url(" https://example.com/x ").expect("通るはず"),
            "https://example.com/x"
        );
        assert_eq!(
            validate_url("HTTPS://example.com/x").expect("通るはず"),
            "HTTPS://example.com/x"
        );
        assert_eq!(
            validate_url("http://192.168.0.10").expect("通るはず"),
            "http://192.168.0.10"
        );
        for url in [
            "",
            "   ",
            "javascript:alert(1)",
            "ftp://example.com",
            "https:/x",
        ] {
            assert!(is_validation(&validate_url(url)), "{url:?}");
        }
    }

    /// `:` の直後の数字はポートとみなし、スキームではない。
    #[test]
    fn has_no_scheme_tells_a_port_from_a_scheme() {
        for url in [
            "example.com",
            "192.168.0.10/wiki",
            "nas.local:5000",
            "1abc:x",
        ] {
            assert!(has_no_scheme(url), "{url}");
        }
        for url in [
            "https://example.com",
            "javascript:alert(1)",
            "mailto:a@example.com",
        ] {
            assert!(!has_no_scheme(url), "{url}");
        }
    }

    /// スキームを省いた LAN の相手には、接続せずに `http://` を補う。空の URL には補わずに弾く。
    /// 公開アドレスの相手 (https・http を試す) の決め方は `link_title` の単体テストで見る。
    #[tokio::test]
    async fn resolve_url_prefixes_lan_hosts_and_rejects_empty_or_other_schemes() {
        for (input, expected) in [
            ("192.168.0.10/wiki", "http://192.168.0.10/wiki"),
            ("nas.local:5000", "http://nas.local:5000"),
            (" HTTPS://example.com/x ", "HTTPS://example.com/x"),
        ] {
            let (url, _) = resolve_url(Some(input)).await.expect("通るはず");
            assert_eq!(url, expected, "{input}");
        }
        for input in [None, Some(""), Some("   "), Some("javascript:alert(1)")] {
            assert!(is_validation(&resolve_url(input).await), "{input:?}");
        }
    }

    fn viewer(id: i64, role: crate::auth::Role) -> Viewer {
        Viewer::User(AuthUser {
            id,
            username: format!("user{id}"),
            role,
            has_recovery_code: false,
        })
    }

    fn content_row(
        id: i64,
        content_type: ContentType,
        parent_id: Option<i64>,
        visibility: Visibility,
        created_by: Option<i64>,
    ) -> ContentRow {
        ContentRow {
            id,
            content_type,
            parent_id,
            title: format!("row{id}"),
            url: None,
            path: None,
            description: None,
            created_at: String::new(),
            file_name: None,
            file_size: None,
            visibility,
            created_by,
            extensions: None,
            title_template: None,
        }
    }

    fn sorted(ids: HashSet<i64>) -> Vec<i64> {
        let mut ids: Vec<i64> = ids.into_iter().collect();
        ids.sort();
        ids
    }

    /// 検索で拾えるのは、自分と祖先のグループすべてが一覧に出るものだけ。
    /// `hidden` のグループの中は、ログイン済みの人にも、作成者の `private` でも拾わない。
    #[test]
    fn listable_ids_requires_every_ancestor_to_be_listable() {
        use crate::auth::Role;
        use ContentType::{Group, Link};
        use Visibility::{Authenticated, Hidden, Private, Public};
        let rows = vec![
            content_row(1, Group, None, Public, None),
            content_row(2, Link, Some(1), Public, None),
            content_row(3, Group, Some(1), Authenticated, None),
            content_row(4, Link, Some(3), Public, None),
            content_row(5, Group, None, Hidden, None),
            content_row(6, Link, Some(5), Public, None),
            content_row(7, Link, Some(5), Private, Some(10)),
            content_row(8, Link, Some(1), Private, Some(10)),
            content_row(9, Link, Some(3), Hidden, None),
        ];

        assert_eq!(sorted(listable_ids(&Viewer::Anonymous, &rows)), [1, 2]);
        assert_eq!(
            sorted(listable_ids(&viewer(10, Role::User), &rows)),
            [1, 2, 3, 4, 8]
        );
        // 他人の `private` は、`admin` にも出さない。
        assert_eq!(
            sorted(listable_ids(&viewer(11, Role::Admin), &rows)),
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn title_from_file_name_drops_the_extension_unless_nothing_is_left() {
        assert_eq!(title_from_file_name("問題.pdf"), "問題");
        assert_eq!(title_from_file_name("a.tar.gz"), "a.tar");
        assert_eq!(title_from_file_name(".hidden"), ".hidden");
        assert_eq!(title_from_file_name("README"), "README");
    }

    async fn insert_group(pool: &sqlx::SqlitePool, title: &str, parent_id: Option<i64>) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO contents (type, parent_id, title, visibility) \
             VALUES ('group', ?, ?, 'public') RETURNING id",
        )
        .bind(parent_id)
        .bind(title)
        .fetch_one(pool)
        .await
        .expect("グループを入れられなかった")
    }

    async fn insert_link(pool: &sqlx::SqlitePool, title: &str, parent_id: Option<i64>) -> i64 {
        sqlx::query_scalar(
            "INSERT INTO contents (type, parent_id, title, url, visibility) \
             VALUES ('link', ?, ?, 'https://example.com', 'public') RETURNING id",
        )
        .bind(parent_id)
        .bind(title)
        .fetch_one(pool)
        .await
        .expect("リンクを入れられなかった")
    }

    /// 見られるリンクの URL だけを返し、見られない・リンクでない・無い id は `None` にする
    /// (リンクのカードの取り直しで、頼まれた id を黙って飛ばすため)。
    #[sqlx::test]
    async fn viewable_link_url_returns_only_links_the_viewer_can_see(pool: sqlx::SqlitePool) {
        let public = insert_link(&pool, "公開", None).await;
        let members_group = insert_group(&pool, "ログインした人", None).await;
        sqlx::query("UPDATE contents SET visibility = 'authenticated' WHERE id = ?")
            .bind(members_group)
            .execute(&pool)
            .await
            .expect("公開範囲を変えられるはず");
        let in_members_group = insert_link(&pool, "中のリンク", Some(members_group)).await;

        let cases = [
            ("見られるリンク", public, Some("https://example.com")),
            ("見られないグループの中のリンク", in_members_group, None),
            ("リンクでない", members_group, None),
            ("無い id", 9999, None),
        ];
        for (label, id, expected) in cases {
            let url = viewable_link_url(&pool, &Viewer::Anonymous, id)
                .await
                .expect("引けるはず");
            assert_eq!(url.as_deref(), expected, "{label}");
        }
    }

    /// 入力があれば前後の空白を落として使う。無ければ自動の値に、同じ親の兄弟と重ならない連番を付ける
    /// (大文字小文字は同じとみなす)。どちらも無ければ 422。
    #[sqlx::test]
    async fn resolve_create_title_numbers_the_derived_title_among_siblings(pool: sqlx::SqlitePool) {
        let group = insert_group(&pool, "G", None).await;
        insert_link(&pool, "問題", None).await;
        insert_link(&pool, "問題 (1)", None).await;
        insert_link(&pool, "REPORT", None).await;
        let derived = |title: &str| Some(title.to_string());

        let cases = [
            ("入力を使う", None, Some("  教材 "), derived("問題"), "教材"),
            ("連番を付ける", None, None, derived("問題"), "問題 (2)"),
            (
                "空白だけの入力は無いとみなす",
                None,
                Some("  "),
                derived("問題"),
                "問題 (2)",
            ),
            (
                "大文字小文字は同じとみなす",
                None,
                None,
                derived("report"),
                "report (1)",
            ),
            (
                "ほかの親の兄弟は数えない",
                Some(group),
                None,
                derived("問題"),
                "問題",
            ),
        ];
        for (label, parent_id, requested, derived, expected) in cases {
            let title = resolve_create_title(&pool, parent_id, requested, derived)
                .await
                .unwrap_or_else(|error| panic!("{label}: {error:?}"));
            assert_eq!(title, expected, "{label}");
        }

        for requested in [None, Some(""), Some("   ")] {
            let result = resolve_create_title(&pool, None, requested, None).await;
            assert!(is_validation(&result), "{requested:?}");
        }
    }

    /// 親はグループに限る。自分自身と、何階層下の子孫も親にできない (循環になる)。
    #[sqlx::test]
    async fn validate_parent_rejects_non_groups_and_cycles(pool: sqlx::SqlitePool) {
        // 祖先を辿る深さに上限があると、深い子孫を親にする循環を見落とす。
        const DEPTH: usize = 60;
        let mut chain = vec![insert_group(&pool, "root", None).await];
        for i in 1..DEPTH {
            let parent = chain.last().copied();
            chain.push(insert_group(&pool, &format!("group-{i}"), parent).await);
        }
        let root = chain[0];
        let unrelated = insert_group(&pool, "無関係", None).await;
        let link = insert_link(&pool, "リンク", None).await;

        assert_eq!(
            validate_parent(&pool, None, Some(root))
                .await
                .expect("通るはず"),
            None
        );
        assert_eq!(
            validate_parent(&pool, Some(unrelated), Some(root))
                .await
                .expect("通るはず"),
            Some(unrelated)
        );
        // 作るときは自分がまだ無いので、循環は調べない。
        assert_eq!(
            validate_parent(&pool, Some(root), None)
                .await
                .expect("通るはず"),
            Some(root)
        );

        let cases = [
            ("無い親", 99_999, None),
            ("グループでない親", link, None),
            ("自分自身", root, Some(root)),
            ("直接の子", chain[1], Some(root)),
            ("孫", chain[2], Some(root)),
            ("50階層より下の子孫", chain[DEPTH - 1], Some(root)),
        ];
        for (label, parent_id, self_id) in cases {
            let result = validate_parent(&pool, Some(parent_id), self_id).await;
            assert!(is_validation(&result), "{label}: {result:?}");
        }
    }
}
