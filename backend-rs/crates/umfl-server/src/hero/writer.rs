//! Hero catalogue writes.
//!
//! Oracle: `HeroRepository.save` (Spring Data JDBC's `CrudRepository`), which
//! inserts when the `@Id` is null and updates when it is not.
//!
//! `heroes` owns no child collection -- cost is tournament-scoped and lives in
//! `tournament_hero` (see `pool_admin.rs`), not here -- so there is none of the
//! delete-and-reinsert cascade `scoring::writer` and `tournament::writer` have
//! to reproduce.

use sqlx::PgExecutor;

/// Inserts a hero, returning the generated id.
pub async fn insert(
    db: impl PgExecutor<'_>,
    name: &str,
    image_url: Option<&str>,
) -> sqlx::Result<i64> {
    sqlx::query_scalar!(
        "insert into heroes (name, image_url) values ($1, $2) returning id",
        name,
        image_url
    )
    .fetch_one(db)
    .await
}

/// Updates an existing hero's name and artwork.
pub async fn update(
    db: impl PgExecutor<'_>,
    hero_id: i64,
    name: &str,
    image_url: Option<&str>,
) -> sqlx::Result<()> {
    sqlx::query!(
        "update heroes set name = $1, image_url = $2 where id = $3",
        name,
        image_url,
        hero_id
    )
    .execute(db)
    .await?;
    Ok(())
}
