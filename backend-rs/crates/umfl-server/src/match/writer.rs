//! Writes a recorded match as a whole aggregate.
//!
//! Oracle: `match/TournamentMatch.kt`'s `@MappedCollection`s, saved through
//! `TournamentMatchRepository`.
//!
//! Spring Data JDBC's `save` on a root owning child collections inserts the
//! root then its children, or updates the root and **deletes and reinserts**
//! every child -- it does not diff. [`insert`] and [`update`] reproduce exactly
//! that, for the same reason `scoring::writer` and `tournament::writer` do: an
//! in-place child update would have to reconcile
//! `match_game_participant`'s `unique (game_id, hero_id)` and `hero_ban`'s
//! composite key mid-statement, where a wholesale replace never meets them.
//!
//! Deleting the children is one statement per table rather than a cascade from
//! the root, because the root survives an [`update`]; `delete` *does* lean on
//! the schema's `on delete cascade`, which is what the Kotlin's
//! `repository.delete(root)` does too.
//!
//! Every function takes the connection: each is more than one statement, and
//! all of them belong to somebody's transaction (PORTING.md §7).

use sqlx::PgConnection;

use super::{MatchGameWrite, TournamentMatchWrite};

/// Inserts a match and every child, returning the generated root id.
pub async fn insert(conn: &mut PgConnection, m: &TournamentMatchWrite) -> sqlx::Result<i64> {
    let id = sqlx::query_scalar!(
        "insert into tournament_match (tournament_id, round, played_at, external_link)
         values ($1, $2, $3, $4) returning id",
        m.tournament_id,
        m.round,
        m.played_at,
        m.external_link
    )
    .fetch_one(&mut *conn)
    .await?;

    insert_children(conn, id, m).await?;
    Ok(id)
}

/// Updates the root row, then replaces every child collection wholesale.
///
/// # Panics
///
/// On a match with no id. Unreachable -- the only caller loaded it first --
/// and it is the `requireNotNull` the Kotlin service makes at the same point.
pub async fn update(conn: &mut PgConnection, m: &TournamentMatchWrite) -> sqlx::Result<()> {
    let id = m.id.expect("a loaded match has an id");

    sqlx::query!(
        "update tournament_match
            set tournament_id = $2, round = $3, played_at = $4, external_link = $5
          where id = $1",
        id,
        m.tournament_id,
        m.round,
        m.played_at,
        m.external_link
    )
    .execute(&mut *conn)
    .await?;

    // `match_game_participant` hangs off `match_game`, not off the match, so it
    // goes first and by hand -- deleting the games would cascade it away, but
    // only after this statement has already named the rows.
    sqlx::query!(
        "delete from match_game_participant
         where game_id in (select id from match_game where match_id = $1)",
        id
    )
    .execute(&mut *conn)
    .await?;
    // Four literal statements rather than a loop over table names: a query
    // built by `format!` is unchecked by `sqlx`, and these are exactly the
    // tables the aggregate owns.
    sqlx::query!("delete from match_game where match_id = $1", id)
        .execute(&mut *conn)
        .await?;
    sqlx::query!("delete from match_participant where match_id = $1", id)
        .execute(&mut *conn)
        .await?;
    sqlx::query!("delete from hero_ban where match_id = $1", id)
        .execute(&mut *conn)
        .await?;
    sqlx::query!("delete from match_hero_pick where match_id = $1", id)
        .execute(&mut *conn)
        .await?;

    insert_children(conn, id, m).await
}

/// Removes a match and, by `on delete cascade`, every row that hangs off it.
pub async fn delete(conn: &mut PgConnection, match_id: i64) -> sqlx::Result<()> {
    sqlx::query!("delete from tournament_match where id = $1", match_id)
        .execute(conn)
        .await?;
    Ok(())
}

async fn insert_children(
    conn: &mut PgConnection,
    match_id: i64,
    m: &TournamentMatchWrite,
) -> sqlx::Result<()> {
    insert_participants(conn, match_id, m).await?;
    for game in &m.games {
        insert_game(conn, match_id, m.tournament_id, game).await?;
    }
    insert_bans(conn, match_id, m).await?;
    insert_picks(conn, match_id, m).await
}

/// `side` is the participant's **list position**, which is the `keyColumn`
/// idiom the aggregate uses in place of an explicit field.
async fn insert_participants(
    conn: &mut PgConnection,
    match_id: i64,
    m: &TournamentMatchWrite,
) -> sqlx::Result<()> {
    if m.participants.is_empty() {
        return Ok(());
    }
    let sides: Vec<i32> = (0..m.participants.len() as i32).collect();
    let labels: Vec<Option<String>> = m
        .participants
        .iter()
        .map(|p| p.player_label.clone())
        .collect();

    sqlx::query!(
        "insert into match_participant (match_id, side, player_label)
         select $1, s, l from unnest($2::integer[], $3::text[]) as t(s, l)",
        match_id,
        &sides,
        &labels as &[Option<String>]
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// One game and its own two participants. The game's id is only known after
/// its insert, which is why this is a statement pair per game rather than one
/// `unnest` over the whole series.
///
/// `tournament_id` is denormalised off the root purely so `match_game` can
/// carry the composite "map is in this tournament's pool" foreign key --
/// nothing besides construction reads it, and `match_game_of_match` pins the
/// copy to its parent so the two cannot drift.
async fn insert_game(
    conn: &mut PgConnection,
    match_id: i64,
    tournament_id: i64,
    game: &MatchGameWrite,
) -> sqlx::Result<()> {
    let game_id = sqlx::query_scalar!(
        "insert into match_game (match_id, tournament_id, game_number, map_id)
         values ($1, $2, $3, $4) returning id",
        match_id,
        tournament_id,
        game.game_number,
        game.map_id
    )
    .fetch_one(&mut *conn)
    .await?;

    if game.participants.is_empty() {
        return Ok(());
    }
    let sides: Vec<i32> = (0..game.participants.len() as i32).collect();
    let hero_ids: Vec<i64> = game.participants.iter().map(|p| p.hero_id).collect();
    let health: Vec<i32> = game
        .participants
        .iter()
        .map(|p| p.health_remaining)
        .collect();
    let winners: Vec<bool> = game.participants.iter().map(|p| p.is_winner).collect();

    sqlx::query!(
        "insert into match_game_participant (game_id, side, hero_id, health_remaining, is_winner)
         select $1, s, h, r, w
         from unnest($2::integer[], $3::bigint[], $4::integer[], $5::boolean[]) as t(s, h, r, w)",
        game_id,
        &sides,
        &hero_ids,
        &health,
        &winners
    )
    .execute(conn)
    .await?;
    Ok(())
}

async fn insert_bans(
    conn: &mut PgConnection,
    match_id: i64,
    m: &TournamentMatchWrite,
) -> sqlx::Result<()> {
    if m.bans.is_empty() {
        return Ok(());
    }
    let hero_ids: Vec<i64> = m.bans.iter().map(|b| b.hero_id).collect();
    let types: Vec<String> = m
        .bans
        .iter()
        .map(|b| b.ban_type.as_str().to_owned())
        .collect();
    let sides: Vec<Option<i32>> = m.bans.iter().map(|b| b.side).collect();

    sqlx::query!(
        "insert into hero_ban (match_id, hero_id, ban_type, side)
         select $1, h, t, s
         from unnest($2::bigint[], $3::text[], $4::integer[]) as t(h, t, s)",
        match_id,
        &hero_ids,
        &types,
        &sides as &[Option<i32>]
    )
    .execute(conn)
    .await?;
    Ok(())
}

async fn insert_picks(
    conn: &mut PgConnection,
    match_id: i64,
    m: &TournamentMatchWrite,
) -> sqlx::Result<()> {
    if m.picks.is_empty() {
        return Ok(());
    }
    let sides: Vec<i32> = m.picks.iter().map(|p| p.side).collect();
    let hero_ids: Vec<i64> = m.picks.iter().map(|p| p.hero_id).collect();

    sqlx::query!(
        "insert into match_hero_pick (match_id, side, hero_id)
         select $1, s, h from unnest($2::integer[], $3::bigint[]) as t(s, h)",
        match_id,
        &sides,
        &hero_ids
    )
    .execute(conn)
    .await?;
    Ok(())
}
