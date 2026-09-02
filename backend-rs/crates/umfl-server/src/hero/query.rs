//! The hero pool of one tournament.
//!
//! Oracle: `hero/HeroQueryRepository.kt`.
//!
//! Everything here is keyed by `tournament_id` rather than by a season, because
//! cost is per tournament: the same hero is a bargain at one event and a
//! premium pick at the next. A hero absent from `tournament_heroes` is simply not
//! in that tournament's pool, which is what makes `UNKNOWN_HERO` a real check
//! rather than an existence test.

use serde::Deserialize;
use sqlx::PgExecutor;

/// Sort options for the Roster Builder grid.
///
/// The Kotlin holds the ORDER BY fragments on the enum constants and splices
/// the chosen one into the SQL, because a sort key cannot be parameterised. The
/// whitelist survives as the enum; the splice does not, because `query_as!`
/// checks a *literal* against the schema. So each arm carries its own checked
/// query instead — strictly stronger than the Kotlin, where a fragment was only
/// ever proofread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HeroSort {
    /// `@RequestParam(defaultValue = "COST")`.
    #[default]
    Cost,
    Name,
}

#[derive(Debug, Clone, Default)]
pub struct HeroFilter {
    pub search: Option<String>,
    pub sort: HeroSort,
}

/// A hero as the Roster Builder sees it: identity plus *this* tournament's
/// price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeroView {
    pub id: i64,
    pub name: String,
    pub image_url: Option<String>,
    pub cost: i32,
}

pub async fn find_by_tournament(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    filter: &HeroFilter,
) -> sqlx::Result<Vec<HeroView>> {
    // `isNullOrBlank` decided whether the clause was appended at all; a null
    // parameter is the same decision expressed inside one checked statement.
    let search = filter
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("%{}%", escape_like(s)));

    match filter.sort {
        HeroSort::Cost => {
            sqlx::query_as!(
                HeroView,
                r#"select h.id, h.name, h.image_url, th.cost
                   from tournament_heroes th
                   join heroes h on h.id = th.hero_id
                   where th.tournament_id = $1
                     and ($2::text is null or h.name ilike $2 escape '\')
                   order by th.cost desc, h.name asc"#,
                tournament_id,
                search.as_deref()
            )
            .fetch_all(db)
            .await
        }
        HeroSort::Name => {
            sqlx::query_as!(
                HeroView,
                r#"select h.id, h.name, h.image_url, th.cost
                   from tournament_heroes th
                   join heroes h on h.id = th.hero_id
                   where th.tournament_id = $1
                     and ($2::text is null or h.name ilike $2 escape '\')
                   order by h.name asc"#,
                tournament_id,
                search.as_deref()
            )
            .fetch_all(db)
            .await
        }
    }
}

/// Priced identity for ids that must be **in** this tournament's pool.
///
/// Drops an id that is not, which is what turns
/// [`crate::tournament::service`]'s pick resolution into an `UNKNOWN_HERO`
/// violation naming exactly the ids the pool does not carry.
pub async fn find_by_ids(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    ids: &[i64],
) -> sqlx::Result<Vec<HeroView>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as!(
        HeroView,
        r#"select h.id, h.name, h.image_url, th.cost
           from tournament_heroes th
           join heroes h on h.id = th.hero_id
           where th.tournament_id = $1 and h.id = any($2)
           order by h.name"#,
        tournament_id,
        ids
    )
    .fetch_all(db)
    .await
}

/// Identity for ids already committed to a roster (an `entry_slots`), priced by
/// this tournament's *current* pool — cost 0 if the hero has since left it.
///
/// Unlike [`find_by_ids`], this never drops an id: a locked or in-progress
/// roster must keep reporting every slot it holds rather than silently
/// shrinking the roster and its spend.
pub async fn find_roster_heroes(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    ids: &[i64],
) -> sqlx::Result<Vec<HeroView>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as!(
        HeroView,
        // The `!` overrides are the outer join's doing, not the schema's:
        // `heroes` is the preserved side, so `h.id`/`h.name` are as non-null
        // here as anywhere, but sqlx marks every column of a query carrying a
        // LEFT JOIN as nullable. `cost` genuinely can be null, and `coalesce`
        // is what the Kotlin used to make it 0.
        r#"select h.id as "id!", h.name as "name!", h.image_url,
                  coalesce(th.cost, 0) as "cost!"
           from heroes h
           left join tournament_heroes th
               on th.tournament_id = $1 and th.hero_id = h.id
           where h.id = any($2)"#,
        tournament_id,
        ids
    )
    .fetch_all(db)
    .await
}

// ---------------------------------------------------------------------------
// The catalogue -- oracle: `hero/HeroRepository.kt` (Spring Data JDBC's
// `CrudRepository` derived queries), used by the admin half.
// ---------------------------------------------------------------------------

/// `findAll()`, ordered by id for the same reason `map::query::find_all` is:
/// Spring Data JDBC's own order is merely insertion order until the first
/// `UPDATE`, and this makes that the guarantee rather than the observation.
pub async fn find_all(db: impl PgExecutor<'_>) -> sqlx::Result<Vec<super::Hero>> {
    let rows = sqlx::query!("select id, name, image_url from heroes order by id")
        .fetch_all(db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|r| super::Hero {
            id: Some(r.id),
            name: r.name,
            image_url: r.image_url,
        })
        .collect())
}

/// `findByName` -- `heroes.name` is `unique`, so this is at most one row.
pub async fn find_by_name(
    db: impl PgExecutor<'_>,
    name: &str,
) -> sqlx::Result<Option<super::Hero>> {
    let row = sqlx::query!(
        "select id, name, image_url from heroes where name = $1",
        name
    )
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| super::Hero {
        id: Some(r.id),
        name: r.name,
        image_url: r.image_url,
    }))
}

/// `findById`.
pub async fn find_by_id(db: impl PgExecutor<'_>, id: i64) -> sqlx::Result<Option<super::Hero>> {
    let row = sqlx::query!("select id, name, image_url from heroes where id = $1", id)
        .fetch_optional(db)
        .await?;
    Ok(row.map(|r| super::Hero {
        id: Some(r.id),
        name: r.name,
        image_url: r.image_url,
    }))
}

/// `findAllById` -- the batch add's existence check, and how it names the ids
/// that do not exist. Catalogue-wide, unlike [`find_by_ids`] above: a hero's
/// *identity* is not tournament-scoped, only its price is.
pub async fn find_all_by_id(
    db: impl PgExecutor<'_>,
    ids: &[i64],
) -> sqlx::Result<Vec<super::Hero>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query!(
        "select id, name, image_url from heroes where id = any($1) order by id",
        ids
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| super::Hero {
            id: Some(r.id),
            name: r.name,
            image_url: r.image_url,
        })
        .collect())
}

/// Escapes ILIKE metacharacters so a search for `_` or `%` matches those
/// characters literally.
fn escape_like(raw: &str) -> String {
    raw.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_metacharacters_are_escaped_before_they_reach_the_pattern() {
        assert_eq!(escape_like("50%_off"), "50\\%\\_off");
        assert_eq!(escape_like(r"a\b"), r"a\\b");
    }

    /// `@RequestParam(required = false, defaultValue = "COST")`.
    #[test]
    fn sort_defaults_to_cost_and_reads_the_kotlin_constant_names() {
        assert_eq!(HeroSort::default(), HeroSort::Cost);
        assert_eq!(
            serde_json::from_str::<HeroSort>(r#""NAME""#).unwrap(),
            HeroSort::Name
        );
        assert!(serde_json::from_str::<HeroSort>(r#""name""#).is_err());
    }
}
