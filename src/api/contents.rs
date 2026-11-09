//! コンテンツ(リンク/フォルダー/ショートカット/グループ)のCRUD API、および
//! folder/shortcut/groupコンテンツの閲覧・配信API。
//!
//! `type = 'group'` は仮想フォルダーで、`parent_id`(自己参照)により無制限にネストできる。
//! 親に指定できるのは`group`のみ(`folder`はFS上の実ディレクトリを指すコンテナであり、
//! その「子」はDB行ではなくFSのエントリなので、DB階層の親にはなれない)。循環参照は
//! `validate_parent`で拒否する。
//!
//! `shortcut` は、共有の中の1つのファイルを `path` で指す (→ docs/folders.md「ショートカット」)。
//! 閲覧の場にはファイルの行 (`ContentResponse::File`) として返す。

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path as FsPath, PathBuf};

use axum::Json;
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Deserialize, Serialize};
use tower::ServiceExt;
use tower_http::services::ServeFile;
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use super::error_detail::ValidationDetail;
use super::file_filter::{FileFilter, normalize_patterns};
use super::schedule::{AutoMode, AutoRefresh, parse_auto_refresh};
use super::search::FieldKind;
use super::sort::{SortOrder, SortQuery, title_cmp};
use crate::auth::{AuthUser, ConfirmedAdmin, Viewer};
use crate::error::{AppError, AppJson, run_blocking};
use crate::state::AppState;

/// `contents.type`。管理画面のレスポンスにもそのまま `type` として載せるため、
/// `Serialize`/`Deserialize`/`ToSchema` も併せて持つ(`auth::Role` と同じ書き方)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, sqlx::Type)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum ContentType {
    Link,
    Folder,
    Group,
    /// 共有の中のファイルを指すショートカット。フォルダーを指すものは `Folder` のまま
    /// (ライブラリに切り替えられるのはフォルダーだけなので、一覧の表示の読み手を増やさないため)。
    Shortcut,
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

    /// `viewer` が自分を書き換えられるか (判定は `ensure_editable` と同じ)。
    pub(super) fn editable_by(&self, viewer: &Viewer) -> bool {
        match (viewer, self.0.first()) {
            (Viewer::User(user), Some(own)) => {
                ensure_editable(user, own.visibility, own.created_by).is_ok()
            }
            _ => false,
        }
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

    /// 自分か祖先のどれかが、`viewer` の閲覧の場の一覧に出ないものか (`hidden` か、他人の `private`)。
    fn any_unlisted(&self, viewer: &Viewer) -> bool {
        self.0
            .iter()
            .any(|row| !can_list(viewer, row.visibility, row.created_by))
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

/// 管理画面の一覧に出すか (→ docs/access.md「管理用の一覧が `user` に見えること」)。他人の `private` は `admin` にだけ出す。
pub(super) fn can_see_in_admin_list(
    user: &AuthUser,
    visibility: Visibility,
    created_by: Option<i64>,
) -> bool {
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
    if is_creator_or_admin(user, created_by) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

/// 書き換える操作の前の `ensure_editable`。他人が作ったもの (作成者がいないものを含む) を
/// admin が変えるときは、パスワードの確かめ直しも求める (→ docs/access.md「操作の前のパスワードの確認」)。
pub(super) async fn ensure_changeable(
    user: &AuthUser,
    visibility: Visibility,
    created_by: Option<i64>,
) -> Result<(), AppError> {
    ensure_editable(user, visibility, created_by)?;
    if created_by != Some(user.id) {
        user.require_confirmation().await?;
    }
    Ok(())
}

/// 書き換えられる人か。作成者と admin (→ docs/access.md「ロールと操作」)。
fn is_creator_or_admin(user: &AuthUser, created_by: Option<i64>) -> bool {
    user.is_admin() || created_by == Some(user.id)
}

/// folderコンテンツの `path` を取り出す。folderの行には書き込み時に必ず入れている。
pub(super) fn folder_path(path: Option<String>) -> Result<String, AppError> {
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
    /// 「新しい順」の並べ替えに使う (→ docs/ui.md「ホーム・グループ・フォルダー・アーカイブの並び順」)。レスポンスには出さない。
    created_at: String,
    visibility: Visibility,
    created_by: Option<i64>,
    /// `type = 'folder'` で見せるファイルのパターン。1行に1つ (→ `file_filter`)。
    include_patterns: Option<String>,
    /// `type = 'folder'` で隠すファイル・フォルダーのパターン。1行に1つ。対象より優先する。
    exclude_patterns: Option<String>,
    /// `type = 'folder'` の一覧の表示の、表示タイトルの組み立てテンプレート (→ docs/archive.md「表示タイトル」)。
    title_template: Option<String>,
    /// `type = 'folder'` で、一覧の表示 (索引して軸で絞り込む) を使うか。
    list_view: bool,
    auto_rescan_mode: AutoMode,
    auto_rescan_hours: Option<i64>,
    auto_rescan_time: Option<String>,
}

/// 一般の閲覧者(匿名を含む)向けのレスポンス。`path`(サーバー上の絶対パス)は
/// 一切含めない(一般の閲覧者にファイルシステムのレイアウトを漏らさないため)。type分岐が
/// フロントからも自然に扱えるよう、tagged enumにしてある。
///
/// 各バリアントの `private` は、公開範囲が `private` なら真。一覧に「作成者のみ」の印を出すため
/// (→ docs/access.md「フロントエンドのガード反転」)。一覧に並ぶ `private` は必ず閲覧者自身のもの (`can_list`)。
///
/// 各バリアントの `can_edit` は、閲覧者がそのコンテンツを書き換えられるか (`admin` か作成者)。一覧の行から
/// 編集・公開範囲の変更を出すかを画面が決めるのに使う。匿名には常に偽。
///
/// enum に付けた `rename_all` はバリアント名にしか効かないので、フィールドの camelCase はバリアントごとに指定する。
#[derive(Debug, Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ContentResponse {
    #[serde(rename_all = "camelCase")]
    Link {
        id: i64,
        title: String,
        /// `url` の必須チェック(type=linkのみ扱う現状は常にSome)は書き込み側で行う。
        url: String,
        description: Option<String>,
        private: bool,
        can_edit: bool,
        /// カードに出すページの情報で、覚えているもの (→ docs/ui.md「リンクのカード」)。
        /// 一覧 (`list_contents`・`browse_group`) でだけ埋める。新しい情報は画面が取り直しを頼んで得る。
        preview: Option<super::link_preview::LinkPreview>,
    },
    #[serde(rename_all = "camelCase")]
    Folder {
        id: i64,
        title: String,
        description: Option<String>,
        private: bool,
        can_edit: bool,
        /// 一覧の表示 (軸で絞り込む平らな一覧) を使うか。使うなら、閲覧の入口はそちらを開く。
        list_view: bool,
    },
    /// 共有の中のファイルを指すショートカット (`ContentType::Shortcut`)。閲覧の場では、フォルダーの中のファイルと同じく
    /// ファイルとして見せる (フォルダーを指すものが `Folder` で返るのと同じく、指す先の種類で返す)。
    #[serde(rename_all = "camelCase")]
    File {
        id: i64,
        title: String,
        description: Option<String>,
        private: bool,
        can_edit: bool,
        /// 先のファイルの名前。ダウンロードURL自体は`id`ベース
        /// (`/contents/{id}/download`)なので、これは表示・保存名のヒント用。
        file_name: String,
        /// 先のファイルの大きさ。一覧でだけ埋める (ほかは0)。
        file_size: i64,
        /// ページ内でプレビューする画像なら、その大きさ (→ docs/ui.md「画像のプレビュー」)。
        /// 一覧 (`list_contents`・`browse_group`) でだけ埋める。
        image: Option<super::thumbnails::ImageSize>,
        /// テキストのビューアーで見せるファイルか (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
        /// `image` と同じく、一覧でだけ埋める。
        is_text: bool,
        /// 行に縮小画像を出してみるか (→ docs/ui.md「画像のプレビュー」)。一覧でだけ埋める。
        thumbnail: bool,
        /// ショートカットの先のファイルが無い。行は残し、開くと「見つかりません」になる。
        /// つながらない NAS は無いと決められないので、偽のまま。一覧でだけ埋める。
        missing: bool,
    },
    /// 仮想フォルダー。子一覧は含めない(別途 `GET /contents/{id}/group` で取得する)。
    #[serde(rename_all = "camelCase")]
    Group {
        id: i64,
        title: String,
        description: Option<String>,
        private: bool,
        can_edit: bool,
    },
}

/// 閲覧者が書き換えられるか (`ensure_editable` と同じ判定)。一覧に並ぶものは閲覧者に見えるものだけなので、
/// 存在を隠す判定 (`ensure_manageable`) は要らない。
fn can_edit(viewer: &Viewer, created_by: Option<i64>) -> bool {
    match viewer {
        Viewer::User(user) => is_creator_or_admin(user, created_by),
        Viewer::Anonymous => false,
    }
}

impl ContentResponse {
    fn from_row(row: ContentRow, viewer: &Viewer) -> Result<Self, AppError> {
        let private = row.visibility == Visibility::Private;
        let can_edit = can_edit(viewer, row.created_by);
        // 万一必須カラムがNULLだった場合(将来型が増えてマイグレーションミス等)は
        // レスポンスを諦めて500にする方が、フロントに嘘の空文字列を返すより安全。
        match row.content_type {
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
                    can_edit,
                    preview: None,
                })
            }
            ContentType::Folder => Ok(ContentResponse::Folder {
                id: row.id,
                title: row.title,
                description: row.description,
                private,
                can_edit,
                list_view: row.list_view,
            }),
            // 大きさとプレビューは `with_previews` で埋める。
            ContentType::Shortcut => {
                let path = row
                    .path
                    .ok_or(AppError::DataIntegrity("shortcut content without a path"))?;
                Ok(ContentResponse::File {
                    id: row.id,
                    title: row.title,
                    description: row.description,
                    private,
                    can_edit,
                    file_name: shortcut_file_name(&path),
                    file_size: 0,
                    image: None,
                    is_text: false,
                    thumbnail: false,
                    missing: false,
                })
            }
            ContentType::Group => Ok(ContentResponse::Group {
                id: row.id,
                title: row.title,
                description: row.description,
                private,
                can_edit,
            }),
        }
    }
}

/// ショートカットの先のファイル名。パスの最後の要素。
fn shortcut_file_name(path: &str) -> String {
    super::fs::file_name_lossy(FsPath::new(path)).unwrap_or_default()
}

/// 管理画面向けのレスポンス。`path` (サーバー上の絶対パス)・公開範囲・作成者を含む。
/// `user` にも返す (→ docs/access.md「管理用の一覧が `user` に見えること」)。見えるのは「共有」の配下だけ
/// なので、`user` に見せたくないパスはそもそも登録されない (→ docs/folders.md「共有」)。
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
    /// `path` を含む「共有」の名前。画面では起点のフルパスの代わりに出す
    /// (→ docs/folders.md「共有」)。`path` が無いか、どの共有にも含まれなければ `None`。
    pub root_name: Option<String>,
    /// `root_name` のフォルダーから先の相対パス (`/` 区切り)。フォルダーそのものなら空文字列。
    /// `root_name` が `None` なら `None`。
    pub path_in_root: Option<String>,
    /// `root_name` の共有が削除済みか。画面では名前の代わりに
    /// 「(見つからない共有)」と出す。`root_name` が `None` なら `false`。
    pub root_deleted: bool,
    pub description: Option<String>,
    /// `type = 'shortcut'` の場合のみ`Some`。先のファイルの名前。
    pub file_name: Option<String>,
    pub visibility: Visibility,
    /// 作成者のユーザーid。`None`なら作成者なし (→ docs/access.md「公開範囲」)。
    pub created_by: Option<i64>,
    /// 作成者のユーザー名。管理画面の一覧の「作成者」に出す (→ docs/access.md「管理用の一覧が `user` に見えること」)。
    pub created_by_username: Option<String>,
    /// `type = 'folder'` で見せるファイルのパターン。1行に1つ。`None` なら全ファイルが対象。
    pub include_patterns: Option<String>,
    /// `type = 'folder'` で隠すファイル・フォルダーのパターン。1行に1つ。対象より優先する。
    pub exclude_patterns: Option<String>,
    /// `type = 'folder'` の一覧の表示の、表示タイトルの組み立てテンプレート
    /// (→ docs/archive.md「表示タイトル」)。`None`ならファイル名をそのまま表示する。
    pub title_template: Option<String>,
    /// `type = 'folder'` で、一覧の表示 (索引して軸で絞り込む) を使うか。ほかの種類は `false`。
    pub list_view: bool,
    /// 一覧の表示を使うフォルダーを、自動で再スキャンする時刻 (→ docs/archive.md「スキャン」)。
    pub auto_rescan: AutoRefresh,
    /// 一覧の表示を使うフォルダーの場合のみ`Some`。アイテムの数。
    pub archive_items: Option<super::archive_items::ArchiveItemCounts>,
}

impl AdminContentResponse {
    fn from_row(
        row: ContentRow,
        created_by_username: Option<String>,
        root_records: &[super::roots::RootRecord],
        archive_items: Option<super::archive_items::ArchiveItemCounts>,
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
        let file_name = match (row.content_type, row.path.as_deref()) {
            (ContentType::Shortcut, Some(path)) => Some(shortcut_file_name(path)),
            _ => None,
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
            file_name,
            visibility: row.visibility,
            created_by: row.created_by,
            created_by_username,
            include_patterns: row.include_patterns,
            exclude_patterns: row.exclude_patterns,
            title_template: row.title_template,
            list_view: row.list_view,
            auto_rescan: AutoRefresh::from_columns(
                row.auto_rescan_mode,
                row.auto_rescan_hours,
                row.auto_rescan_time,
            ),
            archive_items,
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
    let archive_items = if row.list_view {
        Some(super::archive_items::item_counts_of(pool, row.id).await?)
    } else {
        None
    };
    Ok(AdminContentResponse::from_row(
        row,
        username,
        &root_records,
        archive_items,
    ))
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
    /// `type = 'folder'` のみ。見せるファイルのパターン。1行に1つ。省略・空なら全ファイルが対象。
    #[serde(default)]
    pub include_patterns: Option<String>,
    /// `type = 'folder'` のみ。隠すファイル・フォルダーのパターン。1行に1つ。対象より優先する。
    #[serde(default)]
    pub exclude_patterns: Option<String>,
    /// `type = 'folder'` のみ。一覧の表示の、表示タイトルの組み立てテンプレート (→ docs/archive.md「表示タイトル」)。
    /// 保存時にプレースホルダーが実在する軸名であることを検証する。
    #[serde(default)]
    pub title_template: Option<String>,
    /// `type = 'folder'` のみ。一覧の表示を使うか。省略時は現在値のまま。
    #[serde(default)]
    pub list_view: Option<bool>,
    /// `type = 'folder'` のみ。自動で再スキャンする時刻。省略時は現在値のまま。
    #[serde(default)]
    pub auto_rescan: Option<AutoRefresh>,
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

/// フォルダーの絶対パスから付ける自動のタイトル。末尾のフォルダー名で、ドライブの
/// ルートのように名前が無いときはパス全体。
fn title_from_path(path: &str) -> String {
    super::roots::default_name(FsPath::new(path))
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
///
/// 選べるのは `user` が中を見られる共有の中だけ (→ `api::shares`)。ただし今の登録先 (`current`) の
/// ままなら、見られなくなった後でも通す。ほかの項目を直すたびに登録先を選び直させないため。
async fn validate_folder_path(
    state: &AppState,
    user: &AuthUser,
    path: Option<&str>,
    current: Option<&str>,
) -> Result<String, AppError> {
    // 入力は選択UIが返した値をそのまま送る想定なので trim しない。Unix系では
    // 前後に空白を含むディレクトリ名が有効で、落とすと別のパスとして扱われる。
    let path = path.unwrap_or_default().to_string();
    super::validate::non_empty(&path, "path is required")?;

    // 検証は全てブロッキングI/Oなので1つの `spawn_blocking` にまとめる。
    // 「共有」はその前に読む (`run_blocking` の中ではDBに触れない)。
    super::roots::ensure_reachable(state, &path).await?;
    let roots = if current == Some(path.as_str()) {
        super::roots::load_roots(state).await?
    } else {
        super::roots::load_roots_for(state, user).await?
    };
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

/// ショートカットの先のファイルを検証し、実体のパスを返す。見られる共有の扱いは `validate_folder_path` と同じ。
/// 今の先 (`current`) のままなら、ファイルが無くなった後でも通す。場所を選び直すまで、題などを直せるように。
async fn validate_shortcut_path(
    state: &AppState,
    user: &AuthUser,
    path: Option<&str>,
    current: Option<&str>,
) -> Result<String, AppError> {
    let path = path.unwrap_or_default().to_string();
    super::validate::non_empty(&path, "path is required")?;
    if current == Some(path.as_str()) {
        return Ok(path);
    }
    super::roots::ensure_reachable(state, &path).await?;
    let roots = super::roots::load_roots_for(state, user).await?;
    let own_dirs = state.own_dirs.clone();
    let canonical = run_blocking(move || {
        super::fs::canonical_file_within_roots(
            &path,
            &roots,
            &super::roots::OwnDirs::resolve(&own_dirs),
        )
    })
    .await??;
    super::fs::path_to_string(canonical)
}

/// 親に指定されたグループ。中に置けるものの判定に、公開範囲と作成者を使う (→ `ensure_fits_parent`)。
#[derive(Debug)]
struct ParentGroup {
    id: i64,
    visibility: Visibility,
    created_by: Option<i64>,
}

/// 書き込み前に親子関係を検証する。公開範囲の組み合わせは検査しない (→ docs/access.md「親が外れるときの公開範囲」)。
/// 「作成者のみ」のグループに置けるかは、公開範囲が決まってから `ensure_fits_parent` で確かめる。
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
) -> Result<Option<ParentGroup>, AppError> {
    let Some(parent_id) = parent_id else {
        return Ok(None);
    };

    let parent = sqlx::query!(
        r#"SELECT type as "content_type: ContentType", visibility as "visibility: Visibility",
                  created_by
           FROM contents WHERE id = ?"#,
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

    Ok(Some(ParentGroup {
        id: parent_id,
        visibility: parent.visibility,
        created_by: parent.created_by,
    }))
}

/// 「作成者のみ」のグループに置けるのは、グループの作成者の「作成者のみ」のコンテンツだけ
/// (→ docs/access.md「作成者のみのグループ」)。管理者でも例外にしない。
fn ensure_fits_parent(
    parent: Option<&ParentGroup>,
    visibility: Visibility,
    created_by: Option<i64>,
) -> Result<(), AppError> {
    match parent {
        Some(parent)
            if parent.visibility == Visibility::Private
                && (visibility != Visibility::Private || created_by != parent.created_by) =>
        {
            Err(AppError::ValidationDetailed {
                message:
                    "only the group creator's private contents can be placed in a private group"
                        .to_string(),
                detail: ValidationDetail::PrivateGroupNeedsOwnPrivate,
            })
        }
        _ => Ok(()),
    }
}

/// グループを「作成者のみ」にできるのは、直下の子がどれもグループの作成者の「作成者のみ」のときだけ
/// (→ docs/access.md「作成者のみのグループ」)。孫より下は、子が「作成者のみ」なら同じ決まりで揃っている。
async fn ensure_children_fit_private_group(
    pool: &sqlx::SqlitePool,
    id: i64,
    created_by: Option<i64>,
) -> Result<(), AppError> {
    let mismatched = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM contents
               WHERE parent_id = ?1 AND (visibility != ?2 OR created_by IS NOT ?3)
           ) as "mismatched!: bool""#,
        id,
        Visibility::Private,
        created_by,
    )
    .fetch_one(pool)
    .await?;
    if mismatched {
        return Err(AppError::ValidationDetailed {
            message: "a group can be private only when all its children are the creator's private contents"
                .to_string(),
            detail: ValidationDetail::GroupChildrenNotOwnPrivate,
        });
    }
    Ok(())
}

/// 新規作成時の既定の公開範囲 (→ docs/access.md「公開範囲」)。
/// 安全側は既定を厳しくすることではなく、登録前の確認で担保する (→ docs/folders.md「登録前の確認」)。
const DEFAULT_VISIBILITY: Visibility = Visibility::Public;

/// 新規作成時に書き込む公開範囲を決める。`user` も選べる (→ docs/access.md「ロールと操作」)。
/// 作成なら作成者は操作した本人なので、`private` を選んでも制限は無い。
/// 指定が無く、親が「作成者のみ」のグループなら「作成者のみ」にする。中にはそれしか置けないため。
fn resolve_visibility_for_create(
    requested: Option<Visibility>,
    parent: Option<&ParentGroup>,
) -> Visibility {
    requested.unwrap_or_else(|| match parent {
        Some(parent) if parent.visibility == Visibility::Private => Visibility::Private,
        _ => DEFAULT_VISIBILITY,
    })
}

/// 更新時に書き込む公開範囲を決める (→ docs/access.md「ロールと操作」)。
///
/// 値が変わらない更新は常に通す。完全上書き型のPUTでは、編集していない公開範囲も
/// そのまま送り返されてくるため (例: 作成者のいない `private` が残っていても表示タイトルを保存できる)。
/// 書き換えてよい人かは、呼び出し側が `ensure_editable` で確かめている。
fn resolve_visibility_for_update(
    requested: Option<Visibility>,
    current: Visibility,
    created_by: Option<i64>,
) -> Result<Visibility, AppError> {
    let next = requested.unwrap_or(current);
    // 作成者のいない `private` は、閲覧の場の一覧に並ぶ相手がいなくなるので作らない。
    if next != current && next == Visibility::Private && created_by.is_none() {
        return Err(AppError::Forbidden);
    }
    Ok(next)
}

/// 閲覧する一覧を指定された順に並べる。同じ値なら `id` で決める (→ docs/ui.md「ホーム・グループ・フォルダー・アーカイブの並び順」)。
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
                  description, created_at as "created_at!",
                  visibility as "visibility: Visibility", created_by, include_patterns, exclude_patterns, title_template,
                  list_view as "list_view: bool",
                  auto_rescan_mode as "auto_rescan_mode: AutoMode", auto_rescan_hours, auto_rescan_time
           FROM contents
           WHERE parent_id IS NULL"#,
    )
    .fetch_all(&state.pool)
    .await?;

    sort_rows(&mut rows, SortOrder::from_query(query.sort.as_deref()));

    let contents = rows
        .into_iter()
        .filter(|row| can_list(&viewer, row.visibility, row.created_by))
        .map(|row| ContentResponse::from_row(row, &viewer))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(with_previews(&state, contents).await?))
}

/// 管理画面向けの一覧。`path`・公開範囲・作成者を含む。
/// ログイン済みなら `user` も呼べるが、他人の `private` は `admin` にだけ返す
/// (→ docs/access.md「管理用の一覧が `user` に見えること」)。
#[utoipa::path(
    get,
    path = "/contents/manage",
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
                  description, created_at as "created_at!",
                  visibility as "visibility: Visibility", created_by, include_patterns, exclude_patterns, title_template,
                  list_view as "list_view: bool",
                  auto_rescan_mode as "auto_rescan_mode: AutoMode", auto_rescan_hours, auto_rescan_time
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
    let archive_item_counts = super::archive_items::item_counts(&state).await?;

    Ok(Json(
        rows.into_iter()
            .filter(|row| can_see_in_admin_list(&user, row.visibility, row.created_by))
            .map(|row| {
                let username = row.created_by.and_then(|id| usernames.get(&id).cloned());
                let archive_items = row.list_view.then(|| {
                    archive_item_counts
                        .get(&row.id)
                        .copied()
                        .unwrap_or_default()
                });
                AdminContentResponse::from_row(row, username, &root_records, archive_items)
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
        (status = 422, body = crate::error::ErrorResponse),
    )
)]
async fn create_content(
    user: AuthUser,
    State(state): State<AppState>,
    AppJson(payload): AppJson<CreateContentRequest>,
) -> Result<(StatusCode, Json<AdminContentResponse>), AppError> {
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

    let created = insert_content(
        &state,
        &user,
        NewContent {
            content_type: payload.content_type,
            parent_id: payload.parent_id,
            title: payload.title,
            link_title,
            url: link_url,
            path: payload.path,
            description: link_description.or(payload.description),
            visibility: payload.visibility,
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(created)))
}

/// 作るコンテンツ。リンクの URL とページから取ったタイトルは、取りに行った後のもの。
struct NewContent {
    content_type: ContentType,
    parent_id: Option<i64>,
    title: Option<String>,
    /// リンクのページから取ったタイトル。`title` が無いときに使う。
    link_title: Option<String>,
    url: Option<String>,
    /// フォルダーの登録先かショートカットの先 (入力のまま。検証はここで行う)。
    path: Option<String>,
    description: Option<String>,
    visibility: Option<Visibility>,
}

/// フォルダーを指すコンテンツか、ファイルを指すショートカットを、ルート直下に作る。共有の画面の「ホームに載せる」から使う。
pub(super) async fn create_path_content(
    state: &AppState,
    user: &AuthUser,
    content_type: ContentType,
    path: String,
    visibility: Option<Visibility>,
) -> Result<AdminContentResponse, AppError> {
    insert_content(
        state,
        user,
        NewContent {
            content_type,
            parent_id: None,
            title: None,
            link_title: None,
            url: None,
            path: Some(path),
            description: None,
            visibility,
        },
    )
    .await
}

/// 共有の中のフォルダーかファイルをホームに載せる。閲覧者が戻せる非表示のコンテンツがその場所にあれば、それを戻す。
/// 新しく作ると、対象と除外のパターンが空の2つ目のコンテンツができ、外していたファイルが見えてしまうため。
pub(super) async fn list_path_on_home(
    state: &AppState,
    user: &AuthUser,
    path: String,
    visibility: Option<Visibility>,
) -> Result<AdminContentResponse, AppError> {
    let content_type = if path_is_file(FsPath::new(&path)).await? {
        ContentType::Shortcut
    } else {
        ContentType::Folder
    };
    // 管理者が「作成者のみ」で載せるときは、自分のコンテンツを新しく作る。他人のものを戻すと、
    // 管理者の閲覧の場の一覧に並ばず、載せたことにならないため。
    let any_creator =
        user.is_admin() && visibility.is_some_and(|visibility| visibility != Visibility::Private);
    let hidden = sqlx::query_scalar!(
        r#"SELECT id as "id!" FROM contents
           WHERE type = ? AND path = ? AND visibility = ? AND (? OR created_by = ?)
           ORDER BY id LIMIT 1"#,
        content_type,
        path,
        Visibility::Hidden,
        any_creator,
        user.id,
    )
    .fetch_optional(&state.pool)
    .await?;
    let Some(id) = hidden else {
        return create_path_content(state, user, content_type, path, visibility).await;
    };
    {
        let _write_guard = state.contents_write_lock.lock().await;
        let existing = fetch_visibility_state(&state.pool, id).await?;
        let target = visibility.unwrap_or(restore_target(existing.visibility_before_hidden));
        change_visibility(&state.pool, user, id, &existing, target).await?;
    }
    let row = sqlx::query_as!(
        ContentRow,
        r#"SELECT id as "id!", type as "content_type: ContentType", parent_id, title, url,
                  path, description, created_at as "created_at!",
                  visibility as "visibility: Visibility", created_by, include_patterns, exclude_patterns, title_template,
                  list_view as "list_view: bool",
                  auto_rescan_mode as "auto_rescan_mode: AutoMode", auto_rescan_hours, auto_rescan_time
           FROM contents WHERE id = ?"#,
        id,
    )
    .fetch_one(&state.pool)
    .await?;
    admin_response(&state.pool, row).await
}

/// 作成の、検証して書き込む部分。時間のかかる取得 (リンクのページ) は、呼び出し側が先に済ませておく。
async fn insert_content(
    state: &AppState,
    user: &AuthUser,
    new: NewContent,
) -> Result<AdminContentResponse, AppError> {
    // 親の検証からINSERTまでを直列化する (→ `AppState::contents_write_lock`)。
    let _write_guard = state.contents_write_lock.lock().await;
    let parent = validate_parent(&state.pool, new.parent_id, None).await?;
    let visibility = resolve_visibility_for_create(new.visibility, parent.as_ref());
    ensure_fits_parent(parent.as_ref(), visibility, Some(user.id))?;
    let parent_id = parent.map(|parent| parent.id);
    let (url, path) = match new.content_type {
        ContentType::Link => (new.url, None),
        ContentType::Folder => (
            None,
            Some(validate_folder_path(state, user, new.path.as_deref(), None).await?),
        ),
        ContentType::Shortcut => (
            None,
            Some(validate_shortcut_path(state, user, new.path.as_deref(), None).await?),
        ),
        ContentType::Group => (None, None),
    };
    let derived_title = match new.content_type {
        ContentType::Link => new.link_title,
        ContentType::Folder | ContentType::Shortcut => path.as_deref().map(title_from_path),
        ContentType::Group => None,
    };
    let title =
        resolve_create_title(&state.pool, parent_id, new.title.as_deref(), derived_title).await?;

    let content_type = new.content_type;
    let description = new.description;
    let row = sqlx::query_as!(
        ContentRow,
        r#"INSERT INTO contents (type, parent_id, title, url, path, description,
                                  visibility, created_by)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?)
           RETURNING id as "id!", type as "content_type: ContentType", parent_id, title, url,
                     path, description, created_at as "created_at!",
                     visibility as "visibility: Visibility", created_by, include_patterns, exclude_patterns, title_template,
                     list_view as "list_view: bool",
                     auto_rescan_mode as "auto_rescan_mode: AutoMode", auto_rescan_hours, auto_rescan_time"#,
        content_type,
        parent_id,
        title,
        url,
        path,
        description,
        visibility,
        user.id,
    )
    .fetch_one(&state.pool)
    .await?;

    admin_response(&state.pool, row).await
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
                  visibility as "visibility: Visibility", created_by,
                  list_view as "list_view: bool",
                  auto_rescan_mode as "auto_rescan_mode: AutoMode", auto_rescan_hours, auto_rescan_time
           FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    ensure_changeable(&user, existing.visibility, existing.created_by).await?;
    let existing_type = existing.content_type;
    // 入力の検証は、存在を隠す判定 (404) と権限の判定の後に行う。
    let title = validate_title(&payload.title)?;
    let visibility = resolve_visibility_for_update(
        payload.visibility,
        existing.visibility,
        existing.created_by,
    )?;
    // 走査中の `path` 変更を防ぐ (→ docs/archive.md「スキャン」)。取れなければ 409 を返し、
    // 待たない。待つと走査が終わるまで `contents_write_lock` を握り続けることになる。
    let _scan_guard = (existing_type == ContentType::Folder)
        .then(|| state.archive_scans.acquire(id))
        .transpose()?;

    let parent = validate_parent(&state.pool, payload.parent_id, Some(id)).await?;
    ensure_fits_parent(parent.as_ref(), visibility, existing.created_by)?;
    // すでに「作成者のみ」のグループの子は、置くときに確かめてあるので、変えるときだけ確かめる。
    if existing_type == ContentType::Group
        && visibility == Visibility::Private
        && existing.visibility != Visibility::Private
    {
        ensure_children_fit_private_group(&state.pool, id, existing.created_by).await?;
    }
    let parent_id = parent.map(|parent| parent.id);
    if existing_type == ContentType::Folder {
        // 正しくないパターンを保存しない。閲覧やスキャンのたびに失敗することになるため。
        FileFilter::parse(
            payload.include_patterns.as_deref(),
            payload.exclude_patterns.as_deref(),
        )?;
    }
    let (url, path, include_patterns, exclude_patterns, title_template) = match existing_type {
        ContentType::Link => (
            link_url.transpose()?.map(|(url, _)| url),
            None,
            None,
            None,
            None,
        ),
        ContentType::Folder => (
            None,
            Some(
                validate_folder_path(
                    &state,
                    &user,
                    payload.path.as_deref(),
                    existing.path.as_deref(),
                )
                .await?,
            ),
            normalize_patterns(payload.include_patterns.as_deref()),
            normalize_patterns(payload.exclude_patterns.as_deref()),
            super::archive::validate_title_template(
                &state.pool,
                Some(id),
                payload.title_template.as_deref(),
            )
            .await?,
        ),
        ContentType::Shortcut => (
            None,
            Some(
                validate_shortcut_path(
                    &state,
                    &user,
                    payload.path.as_deref(),
                    existing.path.as_deref(),
                )
                .await?,
            ),
            None,
            None,
            None,
        ),
        ContentType::Group => (None, None, None, None, None),
    };
    let is_folder = existing_type == ContentType::Folder;
    let list_view = is_folder && payload.list_view.unwrap_or(existing.list_view);
    let auto_rescan = match payload.auto_rescan {
        Some(auto) => parse_auto_refresh(auto)?,
        None => AutoRefresh::from_columns(
            existing.auto_rescan_mode,
            existing.auto_rescan_hours,
            existing.auto_rescan_time,
        ),
    };
    let auto_rescan_hours = auto_rescan.hours.map(i64::from);

    // `path` を変えたら、索引を捨てる (→ docs/archive.md「スキャン」)。
    // 登録先を起点とする相対パスなので、起点が変われば同じパスが別のファイルを指す。
    // パターンの変更では消さない (外れた行も索引に残し、読むときに外す)。
    let path_changed = is_folder && existing.path != path;

    // `path` の更新と索引の削除は同じトランザクションに入れる (→ docs/archive.md「スキャン」)。
    // 分けると、削除だけが失敗したときに新しい `path` と旧索引の組み合わせが残り、
    // 次の走査で前の場所の見つけた日時などが無関係なファイルへ引き継がれる。
    let mut tx = crate::db::begin_write(&state.pool).await?;

    // `updated_at` はSQLiteに`ON UPDATE`相当の機能が無いため、ここで明示的に更新する。
    let row = sqlx::query_as!(
        ContentRow,
        r#"UPDATE contents
           SET parent_id = ?, title = ?, url = ?, path = ?, description = ?,
               visibility = ?, include_patterns = ?, exclude_patterns = ?, title_template = ?,
               list_view = ?,
               auto_rescan_mode = ?, auto_rescan_hours = ?, auto_rescan_time = ?,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE id = ?
           RETURNING id as "id!", type as "content_type: ContentType", parent_id, title, url,
                     path, description, created_at as "created_at!",
                     visibility as "visibility: Visibility", created_by, include_patterns, exclude_patterns, title_template,
                     list_view as "list_view: bool",
                     auto_rescan_mode as "auto_rescan_mode: AutoMode", auto_rescan_hours, auto_rescan_time"#,
        parent_id,
        title,
        url,
        path,
        payload.description,
        visibility,
        include_patterns,
        exclude_patterns,
        title_template,
        list_view,
        auto_rescan.mode,
        auto_rescan_hours,
        auto_rescan.time,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    if path_changed {
        sqlx::query!("DELETE FROM archive_items WHERE archive_id = ?", id)
            .execute(&mut *tx)
            .await?;
        sqlx::query!("DELETE FROM archive_dirs WHERE archive_id = ?", id)
            .execute(&mut *tx)
            .await?;
        // 新しい場所はまだ読んでいないので、前の場所のスキャンの時刻を残さない。保存の後のスキャンが
        // 失敗したときに、自動の再スキャンが前の場所の時刻から数えて遅れないため。
        sqlx::query!(
            "UPDATE contents SET last_scanned_at = NULL WHERE id = ?",
            id
        )
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
#[utoipa::path(
    put,
    path = "/contents/{id}/creator",
    params(("id" = i64, Path)),
    request_body = SetCreatorRequest,
    responses(
        (status = OK, body = AdminContentResponse),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
        (status = 403, body = crate::error::ErrorResponse, description = "admin以外"),
        (status = 404, body = crate::error::ErrorResponse, description = "存在しない"),
        (status = 422, body = crate::error::ErrorResponse, description = "存在しないユーザーか、作成者のみのグループの決まりに合わない"),
    )
)]
async fn set_creator(
    _admin: ConfirmedAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    AppJson(payload): AppJson<SetCreatorRequest>,
) -> Result<Json<AdminContentResponse>, AppError> {
    // 「作成者のみ」のグループの決まりの判定から書き込みまでを直列化する。
    let _write_guard = state.contents_write_lock.lock().await;
    let existing = fetch_visibility_state(&state.pool, id).await?;
    let user_exists = sqlx::query_scalar!("SELECT 1 FROM users WHERE id = ?", payload.created_by)
        .fetch_optional(&state.pool)
        .await?
        .is_some();
    if !user_exists {
        return Err(AppError::Validation(
            "createdBy user does not exist".to_string(),
        ));
    }
    if existing.visibility == Visibility::Private {
        let parent = validate_parent(&state.pool, existing.parent_id, None).await?;
        ensure_fits_parent(
            parent.as_ref(),
            Visibility::Private,
            Some(payload.created_by),
        )?;
        if existing.content_type == ContentType::Group {
            ensure_children_fit_private_group(&state.pool, id, Some(payload.created_by)).await?;
        }
    }

    let row = sqlx::query_as!(
        ContentRow,
        r#"UPDATE contents
           SET created_by = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE id = ?
           RETURNING id as "id!", type as "content_type: ContentType", parent_id, title, url,
                     path, description, created_at as "created_at!",
                     visibility as "visibility: Visibility", created_by, include_patterns, exclude_patterns, title_template,
                     list_view as "list_view: bool",
                     auto_rescan_mode as "auto_rescan_mode: AutoMode", auto_rescan_hours, auto_rescan_time"#,
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
    ensure_changeable(&user, existing.visibility, existing.created_by).await?;
    let existing_type = existing.content_type;

    // 走査中の削除を防ぐ (→ docs/archive.md「スキャン」)。走査の途中で行が消えると、
    // 同期が消えたアーカイブの索引を書き戻してしまう。update_content と同じく待たない。
    let _scan_guard = (existing_type == ContentType::Folder)
        .then(|| state.archive_scans.acquire(id))
        .transpose()?;

    // groupを削除する場合、直接の子はルート直下(parent_id = NULL)へ昇格させる
    // (孫以下は親=直接の子がそのまま生き残るので変更不要)。この
    // UPDATE(reparent)をDELETEより先にトランザクション内で行うことで、削除対象を
    // 親として参照する行が無い状態を作ってからDELETEする。こうすることで、
    // `contents.parent_id`の`ON DELETE CASCADE`が発火する前提(削除対象がまだ
    // 親として参照されている状態)自体を作らない。
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

    let deleted = sqlx::query!("DELETE FROM contents WHERE id = ?", id)
        .execute(&mut *tx)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

// --- folder コンテンツ配下の閲覧・配信 ---

#[derive(Debug, Deserialize, IntoParams)]
struct BrowseQuery {
    /// folderの登録パスからの相対パス。省略時(空文字列)はルート。
    #[serde(default)]
    path: String,
    /// 並び順。`title` (既定、タイトル順) か `new` (新しい順)。
    /// ディレクトリ優先の並びは変えず、その中の順だけを切り替える (→ docs/ui.md「ホーム・グループ・フォルダー・アーカイブの並び順」)。
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
    /// テキストのビューアーで見せるファイルか (→ docs/ui.md「PDF・動画・テキストのビューアー」)。
    is_text: bool,
    /// 行に縮小画像を出してみるか (→ docs/ui.md「画像のプレビュー」)。
    thumbnail: bool,
}

impl FolderEntry {
    /// 一覧の表示の索引から作るファイルの行。検索の結果の行は2段目に場所を出し、大きさと日時を出さないので、それらは持たない。
    pub(super) fn indexed_file(
        name: String,
        image: Option<super::thumbnails::ImageSize>,
        is_text: bool,
        thumbnail: bool,
    ) -> Self {
        Self {
            name,
            is_dir: false,
            size: None,
            modified_at: None,
            image,
            is_text,
            thumbnail,
        }
    }
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
    /// フォルダーもグループの下に置けるため、閲覧側のパンくずで上へ戻れるようにする。
    ancestors: Vec<GroupAncestor>,
    path: String,
    entries: Vec<FolderEntry>,
    /// 見ている人がこのコンテンツを書き換えられるか。画面は真のとき、編集ページへの導線を出す。
    can_edit: bool,
    /// 一覧の表示 (軸で絞り込む平らな一覧) も使うか。画面は真のとき、表示の切り替えを出す。
    list_view: bool,
    /// 見ている場所を、共有の中の場所として見たもの (→ `api::shares`)。ログインしていて、その共有を
    /// 見られるときだけ。画面は、ここからファイルの操作へつなぐ。
    share: Option<super::shares::FolderShare>,
}

/// `id` が指す `type = 'folder'` の行から、フォルダーのタイトルと登録済みルートパス
/// (書き込み時にcanonicalize済み)を取得する。folder以外(link等)のidや存在しないidは
/// 404にする(browse/download はfolder専用のエンドポイントであるため)。
///
/// 公開範囲の判定もここで行う。**ファイルシステムに触る前に済ませること**が重要で、
/// 後回しにすると匿名の呼び出し元が401と404の差から`authenticated`なフォルダーの中身の
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

async fn uses_list_view(pool: &sqlx::SqlitePool, id: i64) -> Result<bool, AppError> {
    Ok(sqlx::query_scalar!(
        r#"SELECT list_view as "list_view: bool" FROM contents WHERE id = ?"#,
        id
    )
    .fetch_one(pool)
    .await?)
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

/// 相対パスの各コンポーネントが通常の名前(`Component::Normal`)のみであることを確認する
/// 安価な一次フィルタ。`..`・ルート・(Windowsの)ドライブプレフィックス等を弾く。
///
/// Windows では `:` を含む名前も弾く。`a.mp3:stream` は代替データストリームを指し、
/// 一覧に出ないものを配信できてしまうため (→ docs/folders.md「共有」)。
pub(super) fn reject_traversal_components(rel: &str) -> Result<(), AppError> {
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
/// **folderの登録先(および祖先ディレクトリ)に書き込めるのは、信頼できる人に限ること。**
/// NAS の共有を登録するときは、その共有に書き込める人もここに入る (→ docs/folders.md「NAS と外付けのディスク」)。
/// 信頼できない人が書き込める場所を公開する必要が出たら、この制限の解消(上記の再設計)が
/// 必須になる。
///
/// `filter_of` はフォルダーのコンテンツの id で、そのフォルダーの絞り込みで外れるもの
/// (→ `file_filter`) も、一覧に出さないものと同じく 404 にする。登録先そのものは開ける。
/// ホームなどの入口に載っていて存在は伏せられないので、開けないエラーにせず、見せるものが無いと伝えるため。
/// 絞り込みを先に確かめた口と、パターンを持たないショートカットは `None` を渡す。
pub(super) async fn resolve_path(
    state: &AppState,
    root: &str,
    rel: &str,
    filter_of: Option<i64>,
) -> Result<PathBuf, AppError> {
    reject_traversal_components(rel)?;
    let filter = match filter_of {
        Some(id) => Some(FileFilter::load(&state.pool, id).await?),
        None => None,
    };

    super::roots::ensure_reachable(state, root).await?;
    let root = PathBuf::from(root);
    let own_dirs = state.own_dirs.clone();
    let roots = super::roots::load_roots(state).await?;

    let rel_owned = rel.to_string();
    let canonical_candidate = run_blocking(move || {
        let own_dirs = super::roots::OwnDirs::resolve(&own_dirs);
        let canonical_root = std::fs::canonicalize(&root).map_err(|_| AppError::NotFound)?;

        // 「共有」の外は**配信の側でも**拒む。登録時の検証
        // (`validate_folder_path`) は書き込み経路にしか掛からないので、登録を
        // 消したあとや、登録済みのフォルダーを削除したあとも配信され続ける
        // (→ docs/folders.md「共有」)。
        //
        // **候補パスに触る前に判定する**。触ってから弾くと、候補が実在するかどうかで
        // 応答までの時間が変わり、存在を推し量る手がかりになる。
        // 存在を伏せるため 422 ではなく 404 を返す (閲覧者に見せる経路のため)。
        if !super::roots::is_within_roots(&roots, &canonical_root, &own_dirs) {
            return Err(AppError::NotFound);
        }

        let canonical_candidate =
            super::fs::resolve_within(&canonical_root, &rel_owned, &own_dirs)?;
        if let Some(filter) = &filter {
            let rel = super::fs::rel_of(&canonical_root, &canonical_candidate)?;
            let shown = if canonical_candidate.is_dir() {
                filter.allows_dir(&rel)
            } else {
                filter.allows_file(&rel)
            };
            if !shown {
                return Err(AppError::NotFound);
            }
        }
        Ok::<_, AppError>(canonical_candidate)
    })
    .await??;

    Ok(canonical_candidate)
}

/// `dir` 直下のエントリを列挙する。ドットファイル(`.DS_Store`等)と weblav 自身の置き場
/// (`own_dirs`。→ docs/folders.md「共有」) は除外し、
/// ディレクトリ優先(固定)→`order`で選んだ順(タイトル順か新しい順)でソートする。
///
/// シンボリックリンクは一覧に表示しない(ポリシー)。もし表示してリンク先を
/// canonicalize検証しようとすると、`resolve_path`のroot判定ロジックを
/// エントリごとに再実装することになり複雑になる上、その検証からダウンロード等の
/// 実際のファイルオープンまでの間に対象が差し替えられ得るTOCTOU(Time-of-check
/// to time-of-use)の窓を広げてしまう。単純にリンクそのものを見せない方が安全。
/// これによりroot外を指すリンクの名前・サイズ・更新日時が一覧経由で漏れることも防げる。
///
/// `dir_rel` は `dir` の登録先からの相対パス。`filter` で外れるものを並べない。
/// サブフォルダーは、中に当てはまるファイルが無くても並べる (→ docs/folders.md「見せるものの絞り込み」)。
/// 共有の中を見るとき (→ `api::shares`) は絞り込みが無いので `None` を渡す。
pub(super) fn read_entries(
    dir: &FsPath,
    dir_rel: &str,
    own_dirs: &super::roots::OwnDirs,
    filter: Option<&FileFilter>,
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
        let rel = super::fs::join_rel(dir_rel, &name);
        // ここまで来ればシンボリックリンクではないので、`std::fs::metadata` を呼んでも
        // 実体(非リンク)のメタデータがそのまま返る。取得自体が失敗した場合
        // (権限エラー等)はそのエントリだけ読み飛ばす。
        if let Some(entry) = folder_entry(name, &entry.path()) {
            let shown = filter.is_none_or(|filter| {
                if entry.is_dir {
                    filter.allows_dir(&rel)
                } else {
                    filter.allows_file(&rel)
                }
            });
            if shown {
                entries.push(entry);
            }
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
    let can_edit = lineage.editable_by(&viewer);
    let target = resolve_path(&state, &root, &query.path, Some(id)).await?;

    if !path_is_dir(&target).await? {
        return Err(AppError::NotFound);
    }

    let own_dirs = state.own_dirs.clone();
    let order = SortOrder::from_query(query.sort.as_deref());
    let filter = FileFilter::load(&state.pool, id).await?;
    let dir = target.clone();
    let entries = run_blocking(move || {
        let target = dir;
        // 登録先は `resolve_path` で読めることを確かめてある。
        let root = std::fs::canonicalize(&root).map_err(|_| AppError::NotFound)?;
        let dir_rel = super::fs::rel_of(&root, &target)?;
        Ok::<_, AppError>(read_entries(
            &target,
            &dir_rel,
            &super::roots::OwnDirs::resolve(&own_dirs),
            Some(&filter),
            order,
        )?)
    })
    .await??;

    let share = match &viewer {
        Viewer::User(user) => super::shares::folder_share(&state, user, &target).await?,
        Viewer::Anonymous => None,
    };
    Ok(Json(FolderBrowseResponse {
        folder_title,
        ancestors: lineage.breadcrumbs(),
        can_edit,
        list_view: uses_list_view(&state.pool, id).await?,
        path: query.path,
        entries,
        share,
    }))
}

// --- group コンテンツ配下の閲覧 ---

#[derive(Debug, Clone, Serialize, ToSchema)]
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
    /// 見ている人がこのコンテンツを書き換えられるか。画面は真のとき、編集ページへの導線を出す。
    can_edit: bool,
    /// 見ている人がこの中にコンテンツを足せるか。画面は真のとき、見出しの段に「追加」を出す。
    /// ログインしていて、グループが他人の「作成者のみ」でないとき (→ docs/access.md「作成者のみのグループ」)。
    can_add: bool,
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
        r#"SELECT title, visibility as "visibility: Visibility", created_by
           FROM contents WHERE id = ? AND type = ?"#,
        id,
        ContentType::Group
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    let lineage = ensure_viewable(&state.pool, &viewer, id).await?;
    let group_title = group.title;
    let ancestors = lineage.breadcrumbs();
    let can_edit = lineage.editable_by(&viewer);
    let can_add = viewer_id(&viewer).is_some_and(|user_id| {
        group.visibility != Visibility::Private || group.created_by == Some(user_id)
    });

    // 絞り込みと並べ替えを取得後に行う理由は `list_contents` と同じ。
    let mut rows = sqlx::query_as!(
        ContentRow,
        r#"SELECT id as "id!", type as "content_type: ContentType", parent_id, title, url, path,
                  description, created_at as "created_at!",
                  visibility as "visibility: Visibility", created_by, include_patterns, exclude_patterns, title_template,
                  list_view as "list_view: bool",
                  auto_rescan_mode as "auto_rescan_mode: AutoMode", auto_rescan_hours, auto_rescan_time
           FROM contents
           WHERE parent_id = ?"#,
        id,
    )
    .fetch_all(&state.pool)
    .await?;

    sort_rows(&mut rows, SortOrder::from_query(query.sort.as_deref()));

    // 自分か祖先が閲覧の場に並ばないグループ (`hidden` か、管理者が開いた他人の `private`) は、
    // 管理画面から中身を確かめに開いたもの。子は `can_list` ではなく `can_open` で絞る
    // (→ docs/access.md「匿名閲覧の受け口」)。
    let inspecting = lineage.any_unlisted(&viewer);
    let entries = rows
        .into_iter()
        .filter(|row| {
            if inspecting {
                can_open(&viewer, row.visibility, row.created_by)
            } else {
                can_list(&viewer, row.visibility, row.created_by)
            }
        })
        .map(|row| ContentResponse::from_row(row, &viewer))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(GroupBrowseResponse {
        group_title,
        ancestors,
        can_edit,
        can_add,
        entries: with_previews(&state, entries).await?,
    }))
}

/// 一覧に並ぶショートカットに、ページ内で見せるための情報 (大きさ・画像の大きさ・テキストか) を埋める。
/// `ContentResponse` は `path` を持たないので、並んだ分をまとめて引き直す。
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
    let paths: HashMap<i64, String> = sqlx::query!(
        r#"SELECT id as "id!", path as "path!" FROM contents
           WHERE id IN (SELECT value FROM json_each(?)) AND type = 'shortcut' AND path IS NOT NULL"#,
        file_ids_json,
    )
    .fetch_all(&state.pool)
    .await?
    .into_iter()
    .map(|row| (row.id, row.path))
    .collect();

    // つながらない NAS のショートカットは、一覧全体を待たせずに飛ばす (検索と同じ)。
    let shortcut_paths: Vec<&str> = paths.values().map(String::as_str).collect();
    let reachable: HashMap<String, bool> = shortcut_paths
        .iter()
        .map(|path| path.to_string())
        .zip(
            super::roots::reachable_all(state, &shortcut_paths, crate::folder_reach::OTHERS_WAIT)
                .await?,
        )
        .collect();

    // 先のファイル。先が無いショートカットは、読むものが無いので別に持つ。
    let mut targets = Vec::new();
    let mut missing_rows = Vec::new();
    for (index, content) in contents.iter().enumerate() {
        let ContentResponse::File { id, file_name, .. } = content else {
            continue;
        };
        let Some(path) = paths.get(id) else {
            continue;
        };
        if !reachable.get(path).copied().unwrap_or(false) {
            continue;
        }
        match resolve_shortcut_file(state, path).await {
            Ok(file) => targets.push((index, file.path, file_name.clone())),
            Err(AppError::NotFound) => missing_rows.push(index),
            Err(err) => return Err(err),
        }
    }
    for index in missing_rows {
        if let ContentResponse::File { missing, .. } = &mut contents[index] {
            *missing = true;
        }
    }
    if targets.is_empty() {
        return Ok(contents);
    }

    let previews = run_blocking(move || {
        targets
            .into_iter()
            .map(|(index, path, file_name)| {
                let size = std::fs::metadata(&path)
                    .map(|metadata| metadata.len())
                    .unwrap_or_default();
                let preview = super::thumbnails::file_preview(&file_name, &path, size);
                (index, size, preview)
            })
            .collect::<Vec<_>>()
    })
    .await?;
    for (index, size, preview) in previews {
        if let ContentResponse::File {
            file_size,
            image,
            is_text,
            thumbnail,
            ..
        } = &mut contents[index]
        {
            *file_size = i64::try_from(size).unwrap_or(i64::MAX);
            *image = preview.image;
            *is_text = preview.is_text;
            *thumbnail = preview.thumbnail;
        }
    }
    Ok(contents)
}

/// ショートカットの先のファイルを、フォルダーの中のファイルと同じ判定で解決する
/// (共有の外・一覧に出さないもの・シンボリックリンクは 404)。先が消えても 404。
async fn resolve_shortcut_file(state: &AppState, path: &str) -> Result<ServedFile, AppError> {
    let path = FsPath::new(path);
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(AppError::NotFound);
    };
    let (Some(parent), Some(name)) = (parent.to_str(), name.to_str()) else {
        return Err(AppError::NotFound);
    };
    resolve_file_under(state, parent, name, None).await
}

/// 閲覧者がホームからたどって一覧で見られる行の id (→ docs/search.md「見える範囲」)。
/// `start` を渡すと、そのグループからたどって見られる子孫だけにする (`start` 自身は含めない)。
/// `start` と祖先を開けることは、呼び出し側が確かめる。
///
/// 全行を受けてメモリ上で親子を辿り、行ごとに再帰 CTE を投げない。判定済みの祖先で打ち切るので、
/// 各行を辿るのは1回だけになる。循環が無い前提は `fetch_lineage` と同じ。
fn listable_ids(viewer: &Viewer, rows: &[ContentRow], start: Option<i64>) -> HashSet<i64> {
    let by_id: HashMap<i64, &ContentRow> = rows.iter().map(|row| (row.id, row)).collect();
    let mut known: HashMap<i64, bool> = HashMap::new();
    if let Some(start) = start {
        known.insert(start, true);
    }
    for row in rows {
        // 判定済みの祖先か、ルートまで登ってから、上から順に決める。
        // `start` を通らずにルートへ着いたものは、範囲の外。
        let mut chain = Vec::new();
        let mut inherited = start.is_none();
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
        .filter_map(|(id, listable)| (listable && Some(id) != start).then_some(id))
        .collect()
}

/// 検索を絞る場所 (→ docs/search.md「範囲」)。
pub(super) struct SearchScope {
    id: i64,
    content_type: ContentType,
    /// フォルダーの中の階層 (登録パスからの相対パス)。フォルダー全体なら空。
    path: String,
    /// 範囲のコンテンツのタイトル。フォルダーの中の階層との組み立ては画面がする。
    pub(super) title: String,
}

/// 範囲にできる種類と階層の形か。階層を持てるのはフォルダーだけで、空の区切り (`a//b`・末尾の `/`) は
/// 受けない。`resolve_path` は区切りを均して通すが、範囲の名前と結果のパスがずれるため。
fn check_scope_target(content_type: ContentType, path: &str) -> Result<(), AppError> {
    let container = matches!(content_type, ContentType::Group | ContentType::Folder);
    let path_ok = path.is_empty()
        || (content_type == ContentType::Folder && path.split('/').all(|part| !part.is_empty()));
    if container && path_ok {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

/// 範囲に指定されたグループ・フォルダーを、開けることを確かめて引く。
/// 開けなければ、その画面を開くのと同じく 401/404 にする。ほかの種類と、無い階層は 404。
pub(super) async fn search_scope(
    state: &AppState,
    viewer: &Viewer,
    id: i64,
    path: &str,
) -> Result<SearchScope, AppError> {
    let row = sqlx::query!(
        r#"SELECT type as "content_type: ContentType", title, path FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    check_scope_target(row.content_type, path)?;
    // ファイルシステムに触る前に確かめる (理由は `folder_root` と同じ)。
    ensure_viewable(&state.pool, viewer, id).await?;
    // フォルダーは、階層が無くても登録先を確かめる。消えた登録先を範囲にすると、どの語でも空になるため。
    if row.content_type == ContentType::Folder {
        let target = resolve_path(state, &folder_path(row.path)?, path, Some(id)).await?;
        if !path_is_dir(&target).await? {
            return Err(AppError::NotFound);
        }
    }
    Ok(SearchScope {
        id,
        content_type: row.content_type,
        path: path.to_string(),
        title: row.title,
    })
}

/// `parent_id` から上のグループを、ルートに近い順に並べる。`groups` は id からタイトルと親。
fn group_lineage(
    groups: &HashMap<i64, (String, Option<i64>)>,
    parent_id: Option<i64>,
) -> Vec<GroupAncestor> {
    let mut lineage = Vec::new();
    let mut current = parent_id;
    // 親子が輪になった壊れたデータでも止まるよう、グループの数で打ち切る。
    while let Some(id) = current
        && lineage.len() < groups.len()
    {
        let Some((title, parent_id)) = groups.get(&id) else {
            break;
        };
        lineage.push(GroupAncestor {
            id,
            title: title.clone(),
        });
        current = *parent_id;
    }
    lineage.reverse();
    lineage
}

/// 検索の、コンテンツの区画の結果 (→ docs/search.md)。
pub(super) struct ContentSearch {
    pub(super) hits: Vec<super::search::SearchContentHit>,
    pub(super) truncated: bool,
    /// 閲覧者が一覧で見られるアーカイブ。アイテムの区画は、この中から探す。
    pub(super) archives: Vec<super::archive_items::SearchableArchive>,
    /// 閲覧者が一覧で見られるフォルダー。フォルダーの中の区画は、この中から探す。
    pub(super) folders: Vec<super::folder_search::SearchableFolder>,
    /// 閲覧者が一覧で見られる、リンクの一覧のファイルを指すショートカット。
    pub(super) links_files: Vec<super::folder_search::LinksFileContent>,
}

/// 閲覧者が一覧で見られるコンテンツを、タイトルと説明で探す。
/// `scope` がグループならその子孫だけを探す。フォルダー・アーカイブなら、コンテンツは探さず、
/// 中を探す対象をそれ1つにする。
pub(super) async fn search_contents(
    state: &AppState,
    viewer: &Viewer,
    terms: &super::search::Terms,
    scope: Option<&SearchScope>,
) -> Result<ContentSearch, AppError> {
    // 祖先をたどるため全行を読む。数千件の規模なら足りる。
    let rows = sqlx::query_as!(
        ContentRow,
        r#"SELECT id as "id!", type as "content_type: ContentType", parent_id, title, url, path,
                  description, created_at as "created_at!",
                  visibility as "visibility: Visibility", created_by, include_patterns, exclude_patterns, title_template,
                  list_view as "list_view: bool",
                  auto_rescan_mode as "auto_rescan_mode: AutoMode", auto_rescan_hours, auto_rescan_time
           FROM contents"#,
    )
    .fetch_all(&state.pool)
    .await?;

    // 当たりの場所に出す、上のグループを辿るための題と親。
    let groups: HashMap<i64, (String, Option<i64>)> = rows
        .iter()
        .filter(|row| row.content_type == ContentType::Group)
        .map(|row| (row.id, (row.title.clone(), row.parent_id)))
        .collect();

    let (rows, match_contents) = match scope {
        Some(scope) if scope.content_type != ContentType::Group => (
            rows.into_iter()
                .filter(|row| row.id == scope.id)
                .collect::<Vec<_>>(),
            false,
        ),
        _ => {
            let listable = listable_ids(viewer, &rows, scope.map(|scope| scope.id));
            let rows = rows
                .into_iter()
                .filter(|row| listable.contains(&row.id))
                .collect();
            (rows, true)
        }
    };
    // フォルダーの中の階層に絞るときの、その階層。
    let folder_sub_path = scope
        .filter(|scope| scope.content_type == ContentType::Folder)
        .map(|scope| scope.path.clone())
        .unwrap_or_default();

    let scoped_to_sub_path = !folder_sub_path.is_empty();
    // 照らすのは全行の文字列を揃える計算で、件数に比例して重くなる。
    let mut terms_owned = terms.clone();
    let (mut hits, archives, folders, links_files, groups) = run_blocking(move || {
        let mut hits = Vec::new();
        let mut archives = Vec::new();
        let mut folders = Vec::new();
        let mut links_files = Vec::new();
        for row in rows {
            match row.content_type {
                // 一覧の表示を使うフォルダーは、索引から探す。フォルダーの表示の中の階層に絞るときは、
                // その階層を辿る (索引は階層で絞らないため)。
                ContentType::Folder if row.list_view && !scoped_to_sub_path => archives.push((
                    row.id,
                    row.title.clone(),
                    row.path.clone(),
                    row.title_template.clone(),
                    group_lineage(&groups, row.parent_id),
                )),
                ContentType::Folder => folders.push((
                    row.id,
                    row.title.clone(),
                    row.path.clone(),
                    group_lineage(&groups, row.parent_id),
                )),
                ContentType::Shortcut
                    if row
                        .path
                        .as_deref()
                        .is_some_and(super::links_file::is_links_file_name) =>
                {
                    links_files.push(super::folder_search::LinksFileContent {
                        id: row.id,
                        title: row.title.clone(),
                        ancestors: group_lineage(&groups, row.parent_id),
                    });
                }
                _ => {}
            }
            if !match_contents {
                continue;
            }
            let title = super::search::normalize(&row.title);
            let description = row.description.as_deref().map(super::search::normalize);
            let places: Vec<String> = group_lineage(&groups, row.parent_id)
                .iter()
                .map(|group| super::search::normalize(&group.title))
                .collect();
            let mut fields = vec![(FieldKind::Own, title.as_str())];
            fields.extend(description.as_deref().map(|text| (FieldKind::Detail, text)));
            fields.extend(places.iter().map(|text| (FieldKind::Place, text.as_str())));
            if let Some(found) = terms_owned.score(&fields) {
                hits.push((row, found));
            }
        }
        (hits, archives, folders, links_files, groups)
    })
    .await?;

    hits.sort_by(|(a, a_found), (b, b_found)| {
        a_found
            .score
            .cmp(&b_found.score)
            .then_with(|| title_cmp(&a.title, &b.title))
            .then(a.id.cmp(&b.id))
    });
    let truncated = hits.len() > super::search::RESULT_LIMIT;
    hits.truncate(super::search::RESULT_LIMIT);

    let mut placements = Vec::with_capacity(hits.len());
    let mut contents = Vec::with_capacity(hits.len());
    for (row, found) in hits {
        placements.push((group_lineage(&groups, row.parent_id), found.in_detail));
        contents.push(ContentResponse::from_row(row, viewer)?);
    }
    let contents = with_previews(state, contents).await?;

    let archives = archives
        .into_iter()
        .map(|(id, title, path, _title_template, ancestors)| {
            Ok(super::archive_items::SearchableArchive {
                id,
                title,
                path: path.ok_or(AppError::DataIntegrity("archive content without a path"))?,
                ancestors,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;
    let folders = folders
        .into_iter()
        .map(|(id, title, path, ancestors)| {
            Ok(super::folder_search::SearchableFolder {
                id,
                title,
                path: folder_path(path)?,
                sub_path: folder_sub_path.clone(),
                ancestors,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    Ok(ContentSearch {
        hits: contents
            .into_iter()
            .zip(placements)
            .map(
                |(content, (ancestors, matched_in_description))| super::search::SearchContentHit {
                    content,
                    ancestors,
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

/// 同一オリジンでのインライン表示(ブラウザー内蔵ビューアー)を許可するMIMEタイプかどうか。
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
/// (`filename*=UTF-8''...`)の両方を付与する(モダンなブラウザーは後者を優先する)。
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
/// `mime`は呼び出し側が決める。
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
    // 共有フォルダーのファイルは同じ URL のまま差し替わる。指定が無いとブラウザーは更新日時から
    // 決めた間は問い合わせずに古い中身を出すので、毎回 ETag で確かめさせる (変わっていなければ 304)。
    // 閲覧の権限を確かめて出すものなので、共有のキャッシュには置かせない。
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-cache"),
    );

    response
}

/// 配信する実体と、その名前 (mime と `Content-Disposition` に使う)。
pub(super) struct ServedFile {
    pub(super) path: PathBuf,
    pub(super) file_name: String,
}

impl ServedFile {
    /// ファイル `path` を配信するもの。ファイルでなければ 404。名前が UTF-8 でなければ `download` と名乗る。
    pub(super) async fn from_file(path: PathBuf) -> Result<Self, AppError> {
        if !path_is_file(&path).await? {
            return Err(AppError::NotFound);
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("download")
            .to_string();
        Ok(Self { path, file_name })
    }

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
/// `filter_of` は `resolve_path` と同じ。
pub(super) async fn resolve_file_under(
    state: &AppState,
    root: &str,
    rel: &str,
    filter_of: Option<i64>,
) -> Result<ServedFile, AppError> {
    let target = resolve_path(state, root, rel, filter_of).await?;
    ServedFile::from_file(target).await
}

/// folder配下のファイル、またはショートカットの先のファイルを配信する。
/// そのコンテンツを開けるなら (`ensure_openable`) ダウンロードできる。
/// `query.path`はfolderの場合のみ使う(shortcutは`id`だけで先が一意に決まる)。
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

/// `id` のコンテンツが指す実体。folder は `rel` で配下のファイルを、shortcut は先のファイルを指す。
pub(super) async fn content_file(
    state: &AppState,
    viewer: &Viewer,
    id: i64,
    rel: &str,
) -> Result<ServedFile, AppError> {
    let row = sqlx::query!(
        r#"SELECT type as "content_type: ContentType", path
           FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(&state.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    // 公開範囲の判定はファイルシステムに触る前に行う(理由は`folder_root`のコメント)。
    ensure_viewable(&state.pool, viewer, id).await?;

    match row.content_type {
        ContentType::Link | ContentType::Group => Err(AppError::NotFound),
        ContentType::Folder => {
            let root = folder_path(row.path)?;
            resolve_file_under(state, &root, rel, Some(id)).await
        }
        ContentType::Shortcut => {
            let path = row
                .path
                .ok_or(AppError::DataIntegrity("shortcut content without a path"))?;
            resolve_shortcut_file(state, &path).await
        }
    }
}

/// 非表示から戻すときの公開範囲。覚えていなければ、ログインした人に見える `authenticated` にする
/// (`public` に戻すと、隠していたものを匿名にまで見せてしまうため)。
fn restore_target(before_hidden: Option<Visibility>) -> Visibility {
    match before_hidden {
        Some(Visibility::Hidden) | None => Visibility::Authenticated,
        Some(visibility) => visibility,
    }
}

struct VisibilityState {
    parent_id: Option<i64>,
    content_type: ContentType,
    visibility: Visibility,
    visibility_before_hidden: Option<Visibility>,
    created_by: Option<i64>,
}

async fn fetch_visibility_state(
    pool: &sqlx::SqlitePool,
    id: i64,
) -> Result<VisibilityState, AppError> {
    sqlx::query_as!(
        VisibilityState,
        r#"SELECT parent_id, type as "content_type: ContentType",
                  visibility as "visibility: Visibility",
                  visibility_before_hidden as "visibility_before_hidden: Visibility",
                  created_by
           FROM contents WHERE id = ?"#,
        id
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)
}

/// 公開範囲を `requested` に変える。判定は `update_content` と同じ。
/// 呼び出し側は `contents_write_lock` を握ったまま `existing` を読んでから呼ぶこと。
async fn change_visibility(
    pool: &sqlx::SqlitePool,
    user: &AuthUser,
    id: i64,
    existing: &VisibilityState,
    requested: Visibility,
) -> Result<Visibility, AppError> {
    ensure_changeable(user, existing.visibility, existing.created_by).await?;
    let visibility =
        resolve_visibility_for_update(Some(requested), existing.visibility, existing.created_by)?;
    if visibility == existing.visibility {
        return Ok(visibility);
    }
    let parent = validate_parent(pool, existing.parent_id, None).await?;
    ensure_fits_parent(parent.as_ref(), visibility, existing.created_by)?;
    if existing.content_type == ContentType::Group && visibility == Visibility::Private {
        ensure_children_fit_private_group(pool, id, existing.created_by).await?;
    }
    // 非表示にする前の公開範囲は、DB のトリガーが覚える (→ `contents.visibility_before_hidden`)。
    sqlx::query!(
        r#"UPDATE contents SET visibility = ?,
               updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
           WHERE id = ?"#,
        visibility,
        id,
    )
    .execute(pool)
    .await?;
    Ok(visibility)
}

/// 非表示のコンテンツの一覧。`admin` には全部、ほかの人には自分が作成者のものだけを返す。
/// 閲覧の場のどこにも並ばないので、後から探して開けるようにするため。
/// 行の形と並び順はホームやグループの一覧と同じにし、画面も同じ一覧の部品で出す (→ docs/ui.md「UI 全般」)。
#[utoipa::path(
    get,
    path = "/contents/hidden",
    params(SortQuery),
    responses(
        (status = OK, body = Vec<ContentResponse>),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
    )
)]
async fn list_hidden_contents(
    user: AuthUser,
    State(state): State<AppState>,
    Query(query): Query<SortQuery>,
) -> Result<Json<Vec<ContentResponse>>, AppError> {
    let is_admin = user.is_admin();
    let mut rows = sqlx::query_as!(
        ContentRow,
        r#"SELECT id as "id!", type as "content_type: ContentType", parent_id, title, url, path,
                  description, created_at as "created_at!",
                  visibility as "visibility: Visibility", created_by, include_patterns, exclude_patterns, title_template,
                  list_view as "list_view: bool",
                  auto_rescan_mode as "auto_rescan_mode: AutoMode", auto_rescan_hours, auto_rescan_time
           FROM contents
           WHERE visibility = ?1 AND (?2 OR created_by = ?3)"#,
        Visibility::Hidden,
        is_admin,
        user.id,
    )
    .fetch_all(&state.pool)
    .await?;
    sort_rows(&mut rows, SortOrder::from_query(query.sort.as_deref()));

    let viewer = Viewer::User(user);
    let entries = rows
        .into_iter()
        .map(|row| ContentResponse::from_row(row, &viewer))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(with_previews(&state, entries).await?))
}

/// `list_hidden_contents` が返す件数。ユーザーメニューに「非表示のコンテンツ (N)」と出すため。
pub(crate) async fn hidden_content_count(
    pool: &sqlx::SqlitePool,
    user: &AuthUser,
) -> Result<i64, AppError> {
    let is_admin = user.is_admin();
    Ok(sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM contents
           WHERE visibility = ?1 AND (?2 OR created_by = ?3)"#,
        Visibility::Hidden,
        is_admin,
        user.id,
    )
    .fetch_one(pool)
    .await?)
}

pub(crate) fn router() -> OpenApiRouter<AppState> {
    // ブラウザーが直接開くルートだけ、エラーを画面へのリダイレクトに変える
    // (→ `api::browser`)。他のルートを巻き込まないよう、別の `OpenApiRouter` に分けてから merge する。
    let download_routes = OpenApiRouter::new()
        .routes(routes!(download_content))
        .layer(axum::middleware::from_fn(
            super::browser::redirect_errors_for_browsers,
        ));

    OpenApiRouter::new()
        .routes(routes!(list_contents, create_content))
        .routes(routes!(update_content, delete_content))
        .routes(routes!(set_creator))
        .routes(routes!(list_hidden_contents))
        .routes(routes!(list_admin_contents))
        .routes(routes!(fetch_link_metadata))
        .routes(routes!(browse_folder))
        .routes(routes!(browse_group))
        .routes(routes!(thumbnail_content))
        .merge(download_routes)
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
            session: None,
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
            visibility,
            created_by,
            include_patterns: None,
            exclude_patterns: None,
            title_template: None,
            list_view: false,
            auto_rescan_mode: AutoMode::Off,
            auto_rescan_hours: None,
            auto_rescan_time: None,
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
    fn group_lineage_lists_the_groups_above_from_the_root() {
        let groups = HashMap::from([
            (1, ("教材".to_string(), None)),
            (2, ("2024".to_string(), Some(1))),
        ]);
        let titles = |parent_id| {
            group_lineage(&groups, parent_id)
                .into_iter()
                .map(|group| (group.id, group.title))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            titles(Some(2)),
            [(1, "教材".to_string()), (2, "2024".to_string())]
        );
        assert_eq!(titles(Some(1)), [(1, "教材".to_string())]);
        assert!(titles(None).is_empty(), "ルート直下は空");
    }

    #[test]
    fn group_lineage_stops_on_a_parent_cycle() {
        let groups = HashMap::from([
            (1, ("a".to_string(), Some(2))),
            (2, ("b".to_string(), Some(1))),
        ]);
        assert_eq!(group_lineage(&groups, Some(1)).len(), 2);
    }

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

        assert_eq!(
            sorted(listable_ids(&Viewer::Anonymous, &rows, None)),
            [1, 2]
        );
        assert_eq!(
            sorted(listable_ids(&viewer(10, Role::User), &rows, None)),
            [1, 2, 3, 4, 8]
        );
        // 他人の `private` は、`admin` にも出さない。
        assert_eq!(
            sorted(listable_ids(&viewer(11, Role::Admin), &rows, None)),
            [1, 2, 3, 4]
        );
    }

    /// 範囲のグループから探すときは、その子孫のうち、範囲からたどって一覧に出るものだけ。
    /// 範囲自身と祖先は、開けることを呼び出し側が確かめるので問わない (`hidden` のグループの中でも探せる)。
    #[test]
    fn listable_ids_from_a_start_group_covers_only_its_listable_descendants() {
        use ContentType::{Group, Link};
        use Visibility::{Authenticated, Hidden, Public};
        let rows = vec![
            content_row(1, Group, None, Hidden, None),
            content_row(2, Link, Some(1), Public, None),
            content_row(3, Group, Some(1), Public, None),
            content_row(4, Link, Some(3), Public, None),
            content_row(5, Group, Some(1), Authenticated, None),
            content_row(6, Link, Some(5), Public, None),
            content_row(7, Link, None, Public, None),
        ];

        assert_eq!(
            sorted(listable_ids(&Viewer::Anonymous, &rows, Some(1))),
            [2, 3, 4]
        );
        assert_eq!(
            sorted(listable_ids(&Viewer::Anonymous, &rows, Some(3))),
            [4]
        );
    }

    #[test]
    fn check_scope_target_accepts_only_containers_and_clean_folder_levels() {
        use ContentType::{Folder, Group, Link, Shortcut};
        for (content_type, path, ok) in [
            (Group, "", true),
            (Folder, "", true),
            (Folder, "2024/リスニング", true),
            (Link, "", false),
            (Shortcut, "", false),
            (Group, "2024", false),
            (Folder, "2024/", false),
            (Folder, "/2024", false),
            (Folder, "2024//a", false),
        ] {
            assert_eq!(
                check_scope_target(content_type, path).is_ok(),
                ok,
                "{content_type:?} {path:?}"
            );
        }
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

    fn parent_group(visibility: Visibility, created_by: Option<i64>) -> ParentGroup {
        ParentGroup {
            id: 1,
            visibility,
            created_by,
        }
    }

    /// 「作成者のみ」のグループに置けるのは、グループの作成者の「作成者のみ」だけ。
    /// ほかのグループとルート直下には、公開範囲と作成者によらず置ける。
    #[test]
    fn ensure_fits_parent_admits_only_creators_private_into_private_group() {
        let private = parent_group(Visibility::Private, Some(1));
        let public = parent_group(Visibility::Public, Some(1));
        let cases = [
            (
                "作成者の作成者のみ",
                Some(&private),
                Visibility::Private,
                Some(1),
                true,
            ),
            (
                "作成者の非表示",
                Some(&private),
                Visibility::Hidden,
                Some(1),
                false,
            ),
            (
                "作成者の誰でも",
                Some(&private),
                Visibility::Public,
                Some(1),
                false,
            ),
            (
                "ほかの人の作成者のみ",
                Some(&private),
                Visibility::Private,
                Some(2),
                false,
            ),
            (
                "作成者のいない行",
                Some(&private),
                Visibility::Private,
                None,
                false,
            ),
            (
                "誰でものグループ",
                Some(&public),
                Visibility::Public,
                Some(2),
                true,
            ),
            ("ルート直下", None, Visibility::Public, Some(2), true),
        ];
        for (label, parent, visibility, created_by, fits) in cases {
            let result = ensure_fits_parent(parent, visibility, created_by);
            assert_eq!(result.is_ok(), fits, "{label}: {result:?}");
        }
    }

    /// 公開範囲の指定が無いときだけ、「作成者のみ」のグループの中なら「作成者のみ」にする。
    #[test]
    fn resolve_visibility_for_create_defaults_to_private_inside_private_group() {
        let private = parent_group(Visibility::Private, Some(1));
        let public = parent_group(Visibility::Public, Some(1));
        assert_eq!(
            resolve_visibility_for_create(None, Some(&private)),
            Visibility::Private
        );
        assert_eq!(
            resolve_visibility_for_create(Some(Visibility::Public), Some(&private)),
            Visibility::Public
        );
        assert_eq!(
            resolve_visibility_for_create(None, Some(&public)),
            DEFAULT_VISIBILITY
        );
        assert_eq!(
            resolve_visibility_for_create(None, None),
            DEFAULT_VISIBILITY
        );
    }

    /// 直下の子がどれも作成者の「作成者のみ」なら、グループを「作成者のみ」にできる。
    /// 子が無いときも通す。
    #[sqlx::test]
    async fn children_fit_private_group_only_when_all_are_creators_private(pool: sqlx::SqlitePool) {
        let owner: i64 = sqlx::query_scalar(
            "INSERT INTO users (username, password_hash) VALUES ('alice', 'x') RETURNING id",
        )
        .fetch_one(&pool)
        .await
        .expect("ユーザーを入れられなかった");
        let group = insert_group(&pool, "私の関心", None).await;
        ensure_children_fit_private_group(&pool, group, Some(owner))
            .await
            .expect("子が無ければ通るはず");

        let child = insert_link(&pool, "記事", Some(group)).await;
        let set = |visibility: &'static str, created_by: Option<i64>| {
            let pool = pool.clone();
            async move {
                sqlx::query("UPDATE contents SET visibility = ?, created_by = ? WHERE id = ?")
                    .bind(visibility)
                    .bind(created_by)
                    .bind(child)
                    .execute(&pool)
                    .await
                    .expect("子を書き換えられなかった");
            }
        };
        let cases = [
            ("作成者の作成者のみ", "private", Some(owner), true),
            ("作成者の誰でも", "public", Some(owner), false),
            ("作成者のいない作成者のみ", "private", None, false),
        ];
        for (label, visibility, created_by, fits) in cases {
            set(visibility, created_by).await;
            let result = ensure_children_fit_private_group(&pool, group, Some(owner)).await;
            assert_eq!(result.is_ok(), fits, "{label}: {result:?}");
        }
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
                .expect("通るはず")
                .map(|parent| parent.id),
            None
        );
        assert_eq!(
            validate_parent(&pool, Some(unrelated), Some(root))
                .await
                .expect("通るはず")
                .map(|parent| parent.id),
            Some(unrelated)
        );
        // 作るときは自分がまだ無いので、循環は調べない。
        assert_eq!(
            validate_parent(&pool, Some(root), None)
                .await
                .expect("通るはず")
                .map(|parent| parent.id),
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
