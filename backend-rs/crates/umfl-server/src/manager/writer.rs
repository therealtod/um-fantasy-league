//! Manager writes.
//!
//! Oracle: `ManagerRepository.save` as `SupabaseAuthenticationConverter` uses
//! it -- **the only place this application creates a manager.** Everything else
//! about a league is written through the Admin API; a manager row appears when
//! a new Supabase identity first presents a token, and never otherwise.

use sqlx::PgExecutor;
use uuid::Uuid;

use super::Manager;

/// Inserts a just-in-time provisioned manager.
///
/// `is_admin` is deliberately not a parameter: a provisioned manager is never
/// an admin, and the column is our own data set by hand. See
/// `SupabaseAuthenticationConverterTest`'s "a JIT-provisioned manager is never
/// an admin by default".
///
/// A unique-violation here is expected rather than exceptional -- two
/// concurrent first requests from the same new identity both miss the read and
/// both try to insert -- so the caller inspects the error instead of this
/// mapping it to an `ApiError`.
pub async fn insert(
    db: impl PgExecutor<'_>,
    handle: &str,
    display_name: &str,
    auth_user_id: Uuid,
) -> sqlx::Result<Manager> {
    sqlx::query_as!(
        Manager,
        r#"insert into managers (handle, display_name, auth_user_id, is_admin)
           values ($1, $2, $3, false)
           returning id, handle, display_name, auth_user_id, is_admin"#,
        handle,
        display_name,
        auth_user_id
    )
    .fetch_one(db)
    .await
}
