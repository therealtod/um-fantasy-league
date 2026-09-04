//! Reads recorded matches back out as whole [`MatchResult`]s.
//!
//! Six flat queries -- matches, participants, games, game-participants, bans,
//! picks -- grouped in memory, rather than one join. A match has N participants
//! each with their own draft, M games each with their own participants, and K
//! bans, so a single join would fan out to a ragged cross product that then has
//! to be de-duplicated anyway.
//!
//! Read-only by design: `admin_service` is the sole write path, via the
//! [`super::TournamentMatchWrite`] aggregate, so reads and writes cannot
//! silently drift out of step. That single-writer property is doubly
//! load-bearing -- it is the whole premise of [`super::cache`]'s invalidation
//! argument, since one writer announcing every write is what makes one signal
//! complete.
//!
//! The public standings path reaches these queries *through* that cache rather
//! than directly, so [`find_by_tournament`] is the load behind a miss, not a
//! per-request cost.

use indexmap::IndexMap;
use sqlx::{PgConnection, PgExecutor};
use umfl_domain::match_result::{
    BanResult, BanType, DraftedHeroResult, GameParticipantResult, GameResult,
    MatchParticipantResult, MatchResult,
};

/// A `tournament_matches` row before its children are attached.
struct MatchHeader {
    match_id: i64,
    tournament_id: i64,
    round: i32,
    played_at: chrono::DateTime<chrono::Utc>,
    external_link: String,
}

/// One match by id, or `None` -- the admin write endpoints' response shape.
pub async fn find_by_id(
    conn: &mut PgConnection,
    match_id: i64,
) -> sqlx::Result<Option<MatchResult>> {
    let headers = sqlx::query!(
        "select m.id, m.tournament_id, m.round, m.played_at, m.external_link
         from tournament_matches m where m.id = $1",
        match_id
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|r| MatchHeader {
        match_id: r.id,
        tournament_id: r.tournament_id,
        round: r.round,
        played_at: r.played_at,
        external_link: r.external_link,
    })
    .collect();
    Ok(assemble(conn, headers).await?.into_iter().next())
}

/// The id of the match in `tournament_id` already recorded against
/// `external_link`, if there is one.
///
/// `uq_tournament_match_external_link` makes that at most one row, so this is
/// the pre-check behind a rule rather than a warning: the importer shows it
/// before the admin fills anything in, and `admin_service` refuses the write.
/// Correcting a match still reuses its own URL legitimately -- that updates the
/// row in place and never meets the index -- so `correct` excludes the match
/// under correction from the result.
pub async fn find_id_by_external_link(
    db: impl PgExecutor<'_>,
    tournament_id: i64,
    external_link: &str,
) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar!(
        "select id from tournament_matches
         where tournament_id = $1 and external_link = $2",
        tournament_id,
        external_link
    )
    .fetch_optional(db)
    .await
}

/// Every recorded match in a tournament, oldest first -- the scoring fold's
/// input. Optionally narrowed to one `round`, which is the admin match list's
/// filter.
pub async fn find_by_tournament(
    conn: &mut PgConnection,
    tournament_id: i64,
    round: Option<i32>,
) -> sqlx::Result<Vec<MatchResult>> {
    let headers: Vec<MatchHeader> = sqlx::query!(
        "select m.id, m.tournament_id, m.round, m.played_at, m.external_link
         from tournament_matches m
         where m.tournament_id = $1
           and ($2::int is null or m.round = $2::int)
         order by m.played_at, m.id",
        tournament_id,
        round
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|r| MatchHeader {
        match_id: r.id,
        tournament_id: r.tournament_id,
        round: r.round,
        played_at: r.played_at,
        external_link: r.external_link,
    })
    .collect();
    assemble(conn, headers).await
}

/// The newest `limit` matches in a tournament, optionally narrowed to one
/// `round` -- the admin match list's page.
///
/// Newest first and bounded in SQL rather than afterwards: the admin list is
/// the one screen that wants the most recent results, while
/// [`find_by_tournament`] is the scoring fold's input, which wants all of them
/// oldest first. Same `played_at desc, id desc` tiebreak as the ticker, since
/// parallel tables in a round share a timestamp.
pub async fn find_by_tournament_newest_first(
    conn: &mut PgConnection,
    tournament_id: i64,
    round: Option<i32>,
    limit: i64,
) -> sqlx::Result<Vec<MatchResult>> {
    let headers: Vec<MatchHeader> = sqlx::query!(
        "select m.id, m.tournament_id, m.round, m.played_at, m.external_link
         from tournament_matches m
         where m.tournament_id = $1
           and ($2::int is null or m.round = $2::int)
         order by m.played_at desc, m.id desc
         limit $3",
        tournament_id,
        round,
        limit
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|r| MatchHeader {
        match_id: r.id,
        tournament_id: r.tournament_id,
        round: r.round,
        played_at: r.played_at,
        external_link: r.external_link,
    })
    .collect();
    assemble(conn, headers).await
}

/// The newest `limit` matches after `since_match_id` -- the ticker's page.
///
/// The polling key is the id, not `played_at`: parallel tables share a start
/// time, so `played_at` is not unique, while `id` is a monotonic `bigserial`
/// seeded in chronological order. Display order is still `played_at`.
///
/// **Not dead code, despite having no production caller.** The ticker slices
/// its page out of the list [`super::cache`] already holds rather than issuing
/// this query. This is the oracle in that comparison -- the authoritative
/// statement, in SQL, of what that slice is supposed to mean. Deleting it would
/// leave the in-memory derivation with nothing to check it against.
pub async fn find_by_tournament_since(
    conn: &mut PgConnection,
    tournament_id: i64,
    since_match_id: i64,
    limit: i64,
) -> sqlx::Result<Vec<MatchResult>> {
    let headers: Vec<MatchHeader> = sqlx::query!(
        "select m.id, m.tournament_id, m.round, m.played_at, m.external_link
         from tournament_matches m
         where m.tournament_id = $1
           and m.id > $2
         order by m.played_at desc, m.id desc
         limit $3",
        tournament_id,
        since_match_id,
        limit
    )
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|r| MatchHeader {
        match_id: r.id,
        tournament_id: r.tournament_id,
        round: r.round,
        played_at: r.played_at,
        external_link: r.external_link,
    })
    .collect();
    assemble(conn, headers).await
}

/// Attaches participants (with their drafts), games (with their own
/// participants) and bans to a page of match headers.
///
/// Every map below is an [`IndexMap`], iterated in the order the ordered
/// query filled it.
async fn assemble(
    conn: &mut PgConnection,
    headers: Vec<MatchHeader>,
) -> sqlx::Result<Vec<MatchResult>> {
    if headers.is_empty() {
        return Ok(Vec::new());
    }
    let match_ids: Vec<i64> = headers.iter().map(|h| h.match_id).collect();

    // Keyed by (match, side): a pick belongs to the side that drafted it,
    // which is what MatchParticipantResult hangs it off.
    let mut picks: IndexMap<(i64, i32), Vec<DraftedHeroResult>> = IndexMap::new();
    for row in sqlx::query!(
        "select hp.match_id, hp.side, hp.hero_id, h.name as hero_name
         from match_hero_picks hp
         join heroes h on h.id = hp.hero_id
         where hp.match_id = any($1)
         order by hp.match_id, hp.side, h.name",
        &match_ids
    )
    .fetch_all(&mut *conn)
    .await?
    {
        picks
            .entry((row.match_id, row.side))
            .or_default()
            .push(DraftedHeroResult {
                hero_id: row.hero_id,
                hero_name: row.hero_name,
            });
    }

    let mut participants: IndexMap<i64, Vec<MatchParticipantResult>> = IndexMap::new();
    for row in sqlx::query!(
        "select mp.match_id, mp.side, mp.player_label
         from match_participants mp
         where mp.match_id = any($1)
         order by mp.match_id, mp.side",
        &match_ids
    )
    .fetch_all(&mut *conn)
    .await?
    {
        participants
            .entry(row.match_id)
            .or_default()
            .push(MatchParticipantResult {
                side: row.side,
                player_label: row.player_label,
                drafted_heroes: picks
                    .swap_remove(&(row.match_id, row.side))
                    .unwrap_or_default(),
            });
    }

    // Already `order by mg.match_id, mg.game_number`, so games come back
    // grouped and sorted by game number within each match.
    let game_rows = sqlx::query!(
        "select mg.id, mg.match_id, mg.game_number, mg.map_id, gm.name as map_name
         from match_games mg
         join game_maps gm on gm.id = mg.map_id
         where mg.match_id = any($1)
         order by mg.match_id, mg.game_number",
        &match_ids
    )
    .fetch_all(&mut *conn)
    .await?;

    // Joined through match_games to recover match_id, which
    // match_game_participants does not itself carry.
    let mut game_participants: IndexMap<i64, Vec<GameParticipantResult>> = IndexMap::new();
    for row in sqlx::query!(
        "select mgp.game_id, mgp.side,
                mgp.hero_id, h.name as hero_name, mgp.health_remaining, mgp.is_winner
         from match_game_participants mgp
         join match_games mg on mg.id = mgp.game_id
         join heroes h on h.id = mgp.hero_id
         where mg.match_id = any($1)
         order by mgp.game_id, mgp.side",
        &match_ids
    )
    .fetch_all(&mut *conn)
    .await?
    {
        game_participants
            .entry(row.game_id)
            .or_default()
            .push(GameParticipantResult {
                side: row.side,
                hero_id: row.hero_id,
                hero_name: row.hero_name,
                health_remaining: row.health_remaining,
                is_winner: row.is_winner,
            });
    }

    let mut games: IndexMap<i64, Vec<GameResult>> = IndexMap::new();
    for row in game_rows {
        games.entry(row.match_id).or_default().push(GameResult {
            game_id: row.id,
            game_number: row.game_number,
            map_id: row.map_id,
            map_name: row.map_name,
            participants: game_participants.swap_remove(&row.id).unwrap_or_default(),
        });
    }

    let mut bans: IndexMap<i64, Vec<BanResult>> = IndexMap::new();
    for row in sqlx::query!(
        // `ban_type` is a `text` column with a CHECK, not a Postgres enum, so
        // it decodes as a String and is parsed here. An unparseable value
        // cannot reach this point -- the CHECK is the same three names.
        "select hb.match_id, hb.hero_id, h.name as hero_name, hb.ban_type, hb.side
         from hero_bans hb
         join heroes h on h.id = hb.hero_id
         where hb.match_id = any($1)
         order by hb.match_id, h.name",
        &match_ids
    )
    .fetch_all(&mut *conn)
    .await?
    {
        bans.entry(row.match_id).or_default().push(BanResult {
            hero_id: row.hero_id,
            hero_name: row.hero_name,
            ban_type: parse_ban_type(&row.ban_type),
            // `side` decodes as Option<i32> already: the column is nullable
            // for a pre-ban, and reading it as a plain i32 would report 0.
            side: row.side,
        });
    }

    Ok(headers
        .into_iter()
        .map(|h| MatchResult {
            match_id: h.match_id,
            tournament_id: h.tournament_id,
            round: h.round,
            played_at: h.played_at,
            external_link: h.external_link,
            participants: participants.swap_remove(&h.match_id).unwrap_or_default(),
            games: games.swap_remove(&h.match_id).unwrap_or_default(),
            bans: bans.swap_remove(&h.match_id).unwrap_or_default(),
        })
        .collect())
}

/// `BanType.valueOf(...)`, which throws on anything else. The CHECK constraint
/// on `hero_bans.ban_type` admits exactly these three, so the fallback is
/// unreachable and a `PRE_BAN` -- the one that scores neither ban metric -- is
/// the safe answer if the constraint is ever relaxed without this being
/// updated.
fn parse_ban_type(raw: &str) -> BanType {
    match raw {
        "SELF_BAN" => BanType::SelfBan,
        "OPPONENT_BAN" => BanType::OpponentBan,
        _ => BanType::PreBan,
    }
}
