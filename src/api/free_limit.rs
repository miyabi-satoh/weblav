//! Free の個数の上限 (→ docs/pro.md「上限を数えて止める」)。
//!
//! 数えてから足すまでを直列化するのは呼び出し側の役目。コンテンツは `contents_write_lock`、
//! ユーザーは `users_write_lock` を持ったまま呼ぶこと。

use serde::Serialize;
use sqlx::SqliteExecutor;
use utoipa::ToSchema;

use crate::auth::Role;
use crate::error::AppError;
use crate::pro::Edition;

use super::contents::ContentType;

/// 上限のある対象。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum FreeLimitTarget {
    Admin,
    User,
    Archive,
    Folder,
    File,
    Link,
}

impl FreeLimitTarget {
    fn limit(self) -> i64 {
        match self {
            Self::Admin => 1,
            Self::User => 3,
            Self::Archive => 3,
            Self::Folder | Self::File | Self::Link => 10,
        }
    }
}

/// 上限のある対象の、今の件数と Free の上限。
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FreeLimitUsage {
    target: FreeLimitTarget,
    count: i64,
    limit: i64,
}

/// 上限のある対象ごとの今の件数 (管理画面の Pro の区画に出す → `api::pro`)。
pub(crate) async fn usage(db: &sqlx::SqlitePool) -> Result<Vec<FreeLimitUsage>, AppError> {
    let roles = sqlx::query!(
        r#"SELECT role as "role!: Role", COUNT(*) as "count!: i64" FROM users GROUP BY role"#
    )
    .fetch_all(db)
    .await?;
    let contents = sqlx::query!(
        r#"SELECT type as "content_type!: ContentType", COUNT(*) as "count!: i64" FROM contents GROUP BY type"#
    )
    .fetch_all(db)
    .await?;
    let role_count = |role: Role| roles.iter().find(|r| r.role == role).map_or(0, |r| r.count);
    let content_count = |content_type: ContentType| {
        contents
            .iter()
            .find(|c| c.content_type == content_type)
            .map_or(0, |c| c.count)
    };
    Ok([
        (FreeLimitTarget::Admin, role_count(Role::Admin)),
        (FreeLimitTarget::User, role_count(Role::User)),
        (
            FreeLimitTarget::Archive,
            content_count(ContentType::Archive),
        ),
        (FreeLimitTarget::Folder, content_count(ContentType::Folder)),
        (FreeLimitTarget::File, content_count(ContentType::File)),
        (FreeLimitTarget::Link, content_count(ContentType::Link)),
    ]
    .into_iter()
    .map(|(target, count)| FreeLimitUsage {
        target,
        count,
        limit: target.limit(),
    })
    .collect())
}

fn reject_if_reached(target: FreeLimitTarget, count: i64) -> Result<(), AppError> {
    let limit = target.limit();
    if count >= limit {
        return Err(AppError::FreeLimitReached { target, limit });
    }
    Ok(())
}

/// `content_type` のコンテンツをもう1つ足せるか。
pub(crate) async fn check_content(
    edition: Edition,
    db: impl SqliteExecutor<'_>,
    content_type: ContentType,
) -> Result<(), AppError> {
    let target = match content_type {
        ContentType::Archive => FreeLimitTarget::Archive,
        ContentType::Folder => FreeLimitTarget::Folder,
        ContentType::File => FreeLimitTarget::File,
        ContentType::Link => FreeLimitTarget::Link,
        ContentType::Group => return Ok(()),
    };
    if edition == Edition::Pro {
        return Ok(());
    }
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM contents WHERE type = ?"#,
        content_type
    )
    .fetch_one(db)
    .await?;
    reject_if_reached(target, count)
}

/// `role` のユーザーをもう1人足せるか。
pub(crate) async fn check_role(
    edition: Edition,
    db: impl SqliteExecutor<'_>,
    role: Role,
) -> Result<(), AppError> {
    if edition == Edition::Pro {
        return Ok(());
    }
    let target = match role {
        Role::Admin => FreeLimitTarget::Admin,
        Role::User => FreeLimitTarget::User,
    };
    let count = sqlx::query_scalar!(
        r#"SELECT COUNT(*) as "count!: i64" FROM users WHERE role = ?"#,
        role
    )
    .fetch_one(db)
    .await?;
    reject_if_reached(target, count)
}
