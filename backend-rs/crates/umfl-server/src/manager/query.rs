//! Manager reads.
//!
//! Every function takes an executor rather than the pool, so it composes inside
//! somebody's transaction or outside one.

use sqlx::PgExecutor;
use uuid::Uuid;

use super::Manager;

pub async fn find_by_id(db: impl PgExecutor<'_>, id: i64) -> sqlx::Result<Option<Manager>> {
    sqlx::query_as!(
        Manager,
        r#"select id, handle, display_name, auth_user_id, is_admin
           from managers where id = $1"#,
        id
    )
    .fetch_optional(db)
    .await
}

pub async fn find_by_handle(
    db: impl PgExecutor<'_>,
    handle: &str,
) -> sqlx::Result<Option<Manager>> {
    sqlx::query_as!(
        Manager,
        r#"select id, handle, display_name, auth_user_id, is_admin
           from managers where handle = $1"#,
        handle
    )
    .fetch_optional(db)
    .await
}

pub async fn find_by_auth_user_id(
    db: impl PgExecutor<'_>,
    auth_user_id: Uuid,
) -> sqlx::Result<Option<Manager>> {
    sqlx::query_as!(
        Manager,
        r#"select id, handle, display_name, auth_user_id, is_admin
           from managers where auth_user_id = $1"#,
        auth_user_id
    )
    .fetch_optional(db)
    .await
}
