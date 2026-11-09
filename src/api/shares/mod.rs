//! 共有 (→ `api::roots`) の中のファイルを、画面から扱うための API。
//!
//! 一般ユーザーが共有の中でできることは、共有ごとの既定 (`roots.editor_access`) と
//! ユーザーごとの例外 (`root_user_access`) で決まる。管理者は常にすべての操作ができる。
//! 共有が入れ子のときは、その場所を含む共有のうち一番強いものを使う
//! (外側で見られるものは、内側の共有の扱いに関わらず外側から辿れば見られるため)。

mod browse;
mod fsops;
mod jobs;
pub(crate) mod log;
mod paths;
pub(super) mod repoint;
mod transfer;
mod write;

pub use fsops::TrashPolicy;
pub use jobs::ShareJobs;

use std::collections::HashMap;
use std::path::Path as FsPath;

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::auth::AuthUser;
use crate::error::AppError;
use crate::state::AppState;

/// 共有の中でできること。弱い順に並べ、`Ord` で比べる (入れ子では強い方を採る)。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, ToSchema, sqlx::Type,
)]
#[serde(rename_all = "lowercase")]
#[sqlx(rename_all = "lowercase")]
pub enum ShareAccess {
    /// 見えない。
    None,
    /// 見る・ダウンロードする・ホームに載せる。
    Read,
    /// 見るに加えて、アップロード・新しいフォルダー・コピーで入れる。上書き・名前の変更・移動・削除はできない。
    Add,
    /// すべての操作。
    All,
}

/// あるユーザーの、共有ごとのできること。要求ごとにまとめて読む (共有は多くても数件)。
pub(super) struct AccessMap {
    admin: bool,
    by_root: HashMap<i64, ShareAccess>,
}

impl AccessMap {
    /// 削除済みの共有も含めて読む。判定に使う側が登録中のものだけを渡すため。
    pub(super) async fn load(pool: &sqlx::SqlitePool, user: &AuthUser) -> Result<Self, AppError> {
        if user.is_admin() {
            return Ok(Self {
                admin: true,
                by_root: HashMap::new(),
            });
        }
        let rows = sqlx::query!(
            r#"SELECT roots.id as "id!",
                      COALESCE(root_user_access.access, roots.editor_access) as "access!: ShareAccess"
               FROM roots
               LEFT JOIN root_user_access
                 ON root_user_access.root_id = roots.id AND root_user_access.user_id = ?"#,
            user.id
        )
        .fetch_all(pool)
        .await?;
        Ok(Self {
            admin: false,
            by_root: rows.into_iter().map(|row| (row.id, row.access)).collect(),
        })
    }

    /// 共有 `root_id` そのものに付いているできること。入れ子は見ない (→ `effective`)。
    pub(super) fn of_root(&self, root_id: i64) -> ShareAccess {
        if self.admin {
            return ShareAccess::All;
        }
        self.by_root
            .get(&root_id)
            .copied()
            .unwrap_or(ShareAccess::None)
    }

    #[cfg(test)]
    pub(super) fn for_test(entries: &[(i64, ShareAccess)]) -> Self {
        Self {
            admin: false,
            by_root: entries.iter().copied().collect(),
        }
    }

    /// ある場所を含む共有 (`containing`) のうち、一番強いもの。どれにも含まれなければ `None`。
    pub(super) fn effective(&self, containing: impl IntoIterator<Item = i64>) -> ShareAccess {
        containing
            .into_iter()
            .map(|root_id| self.of_root(root_id))
            .max()
            .unwrap_or(ShareAccess::None)
    }

    /// 共有 `share_path` の先頭でできること。入れ子なら、それを含む共有 (`shares` の `(id, 場所)`) のうち一番強いもの。
    pub(super) fn at_share<'a>(
        &self,
        shares: impl IntoIterator<Item = (i64, &'a str)>,
        share_path: &str,
    ) -> ShareAccess {
        self.effective(
            shares
                .into_iter()
                .filter(|(_, path)| super::roots::contains(path, share_path))
                .map(|(id, _)| id),
        )
    }
}

/// その場所で `needed` 以上のことができなければ 403。見られない場所は、解決の時点で 404 になっている。
fn require(resolved: &paths::Resolved, needed: ShareAccess) -> Result<(), AppError> {
    if resolved.access < needed {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

/// ログインしたユーザーに見える共有の1件。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShareResponse {
    pub id: i64,
    pub name: String,
    /// このユーザーが、この共有のいちばん上でできること。`none` のものは一覧に出さない。
    pub access: ShareAccess,
}

/// 見える共有の一覧。名前の順。
///
/// つながらない NAS の共有も出す (開いたときに、つながらないと伝える)。ファイルシステムには触らず、
/// 登録したパスの文字列で入れ子を判定する (→ `roots::contains`)。
#[utoipa::path(
    get,
    path = "/shares",
    responses(
        (status = OK, body = Vec<ShareResponse>, description = "見える共有の一覧"),
        (status = 401, body = crate::error::ErrorResponse, description = "未ログイン"),
    )
)]
async fn list_shares(
    user: AuthUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<ShareResponse>>, AppError> {
    let rows = sqlx::query!(
        r#"SELECT id as "id!", name, path FROM roots WHERE deleted_at IS NULL ORDER BY name COLLATE NOCASE"#
    )
    .fetch_all(&state.pool)
    .await?;
    let access = AccessMap::load(&state.pool, &user).await?;
    let shares = rows
        .iter()
        .filter_map(|row| {
            let access = access.at_share(
                rows.iter().map(|other| (other.id, other.path.as_str())),
                &row.path,
            );
            (access > ShareAccess::None).then(|| ShareResponse {
                id: row.id,
                name: row.name.clone(),
                access,
            })
        })
        .collect();
    Ok(Json(shares))
}

/// フォルダーのコンテンツの中の場所を、共有の中の場所として見たもの。フォルダーの画面から
/// 共有の操作へつなぐのに使う。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FolderShare {
    /// 共有の id。
    pub id: i64,
    pub name: String,
    /// 共有からの相対パス。実体の綴りを `/` でつなぐ。
    pub path: String,
    /// この場所で、見ている人ができること。
    pub access: ShareAccess,
    /// 消したものを OS のごみ箱へ送れる場所か。偽なら、消すと元に戻せない。
    pub trash: bool,
}

/// 実体のパス `canonical` (フォルダーのコンテンツの中で見ている場所) を、`user` が見られる共有の中の
/// 場所として返す。見られなければ `None`。
///
/// 共有は、その場所を含み、いちばん上で見られるもののうち最も外側を採る (→ `roots::containing_root`)。
pub(super) async fn folder_share(
    state: &AppState,
    user: &AuthUser,
    canonical: &FsPath,
) -> Result<Option<FolderShare>, AppError> {
    let roots = super::roots::load_roots(state).await?;
    let access = AccessMap::load(&state.pool, user).await?;
    let at = paths::effective_access(&access, &roots, canonical);
    if at < ShareAccess::Read {
        return Ok(None);
    }
    let Some(root) = roots
        .iter()
        .filter(|root| canonical.starts_with(&root.path))
        .filter(|root| paths::effective_access(&access, &roots, &root.path) >= ShareAccess::Read)
        .min_by_key(|root| root.path.components().count())
    else {
        return Ok(None);
    };
    let path = canonical.to_path_buf();
    let trash_policy = state.share_trash.clone();
    let trash = crate::error::run_blocking(move || trash_policy.available(&path)).await?;
    Ok(Some(FolderShare {
        id: root.id,
        name: root.name.clone(),
        path: super::fs::rel_of(&root.path, canonical)?,
        access: at,
        trash,
    }))
}

/// `max_upload_bytes` はアップロードの1つのファイルの上限 (→ `write::router`)。`None` なら上限なし。
pub(crate) fn router(max_upload_bytes: Option<u64>) -> OpenApiRouter<AppState> {
    OpenApiRouter::new()
        .routes(routes!(list_shares))
        .merge(browse::router())
        .merge(write::router(max_upload_bytes))
        .merge(transfer::router())
        .merge(log::router())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_is_ordered_from_none_to_all() {
        assert!(ShareAccess::None < ShareAccess::Read);
        assert!(ShareAccess::Read < ShareAccess::Add);
        assert!(ShareAccess::Add < ShareAccess::All);
    }

    /// 入れ子では、含む共有のうち一番強いものを採る。外側で見えれば、内側が見えない設定でも見える。
    #[test]
    fn the_effective_access_is_the_strongest_containing_share() {
        let access = AccessMap::for_test(&[(1, ShareAccess::Read), (2, ShareAccess::None)]);
        assert_eq!(access.effective([1, 2]), ShareAccess::Read);
        assert_eq!(access.effective([2]), ShareAccess::None);
        let access = AccessMap::for_test(&[(1, ShareAccess::Read), (2, ShareAccess::All)]);
        assert_eq!(access.effective([1, 2]), ShareAccess::All);
    }

    /// 共有の先頭でできることは、それを含む共有 (自身と外側) のうち一番強いもの。隣の共有は数えない。
    #[test]
    fn a_share_takes_the_strongest_of_the_shares_containing_it() {
        let sep = std::path::MAIN_SEPARATOR;
        let outer = format!("{sep}srv");
        let inner = format!("{sep}srv{sep}inner");
        let other = format!("{sep}other");
        let shares = [
            (1, outer.as_str()),
            (2, inner.as_str()),
            (3, other.as_str()),
        ];
        let access = AccessMap::for_test(&[
            (1, ShareAccess::None),
            (2, ShareAccess::Add),
            (3, ShareAccess::All),
        ]);
        assert_eq!(access.at_share(shares, &inner), ShareAccess::Add);
        assert_eq!(access.at_share(shares, &outer), ShareAccess::None);
        let access = AccessMap::for_test(&[(1, ShareAccess::All), (2, ShareAccess::None)]);
        assert_eq!(access.at_share(shares, &inner), ShareAccess::All);
    }

    #[test]
    fn a_place_outside_every_share_has_no_access() {
        let access = AccessMap::for_test(&[(1, ShareAccess::All)]);
        assert_eq!(access.effective([]), ShareAccess::None);
        // 知らない共有 (読んだ後に足された) も見えない扱い。
        assert_eq!(access.effective([9]), ShareAccess::None);
    }

    #[test]
    fn an_admin_can_do_everything_in_every_share() {
        let access = AccessMap {
            admin: true,
            by_root: HashMap::new(),
        };
        assert_eq!(access.of_root(1), ShareAccess::All);
    }

    fn user(id: i64, role: crate::auth::Role) -> AuthUser {
        AuthUser {
            id,
            username: format!("user{id}"),
            role,
            has_recovery_code: false,
            session: None,
        }
    }

    /// ユーザーごとの例外があればそれを、無ければ共有の既定を使う。
    #[sqlx::test]
    async fn a_user_exception_overrides_the_share_default(pool: sqlx::SqlitePool) {
        sqlx::query(
            "INSERT INTO users (id, username, password_hash) VALUES (1, 'taro', 'x'), (2, 'hanako', 'x');
             INSERT INTO roots (id, name, path, editor_access) VALUES (1, 'a', '/a', 'read'), (2, 'b', '/b', 'none');
             INSERT INTO root_user_access (root_id, user_id, access) VALUES (1, 1, 'all');",
        )
        .execute(&pool)
        .await
        .expect("土台を入れられなかった");

        let taro = AccessMap::load(&pool, &user(1, crate::auth::Role::User))
            .await
            .expect("読めなかった");
        assert_eq!(taro.of_root(1), ShareAccess::All);
        assert_eq!(taro.of_root(2), ShareAccess::None);
        let hanako = AccessMap::load(&pool, &user(2, crate::auth::Role::User))
            .await
            .expect("読めなかった");
        assert_eq!(hanako.of_root(1), ShareAccess::Read);
        let admin = AccessMap::load(&pool, &user(2, crate::auth::Role::Admin))
            .await
            .expect("読めなかった");
        assert_eq!(admin.of_root(2), ShareAccess::All);
    }

    #[test]
    fn access_is_written_in_lowercase() {
        assert_eq!(
            serde_json::to_string(&ShareAccess::Add).expect("書き出せなかった"),
            r#""add""#
        );
    }
}
