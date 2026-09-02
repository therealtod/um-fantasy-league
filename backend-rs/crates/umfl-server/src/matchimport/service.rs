//! Turns a Tabletop League match URL into a reviewable draft of a match result.
//!
//! Oracle: `matchimport/MatchImportService.kt`.
//!
//! **This writes nothing.** It scrapes, resolves names to ids, and hands back a
//! [`MatchImportPreview`]; the admin reviews it in the match wizard and saves
//! through the existing record endpoint. That keeps `match_policy`,
//! `match::admin_service` and the standings push on exactly one path, so an
//! imported match is validated identically to a hand-typed one and this module
//! never needs to know the result rules.
//!
//! Nothing here fails on an unresolvable name. A missing hero or an out-of-pool
//! board comes back in [`MatchImportPreview::unresolved`] with the source's own
//! spelling, because the admin is the one who can tell whether it is a
//! catalogue gap, a pool they forgot to populate, or the source site renaming
//! something. Failing the whole import would only make one odd name cost them
//! the other twenty-odd it got right.

use indexmap::{IndexMap, IndexSet};
use umfl_domain::DomainError;
use umfl_domain::match_result::BanType;
use umfl_domain::name_resolver::NameResolver;
use umfl_domain::scraped_timestamps;

use crate::error::ApiResult;
use crate::map::query as map_query;
use crate::r#match::query as match_query;
use crate::state::AppState;
use crate::tournament::service::require_tournament;

use super::query;
use super::scraper::{ScrapedMatch, ScrapedSide, validate_source_url};
use super::{
    ImportedBan, ImportedGame, ImportedGameParticipant, ImportedParticipant, MatchImportPreview,
    UnresolvedKind, UnresolvedName, UnresolvedReason,
};

/// Scrapes `source_url` and resolves it against this tournament's catalogue and
/// board pool.
///
/// **Deliberately not transactional.** The scrape is an outbound HTTP call that
/// can hold a socket open for up to 90 seconds, and a transaction would pin one
/// of ten pool connections for the same span -- ten concurrent imports, or one
/// hung scraper plus ordinary traffic, would starve the pool for everyone else.
/// The scrape happens first and the resolution reads after are independent
/// lookups with no need of one snapshot, so nothing here wants a transaction at
/// all. PORTING.md §7 states the same invariant from the other side: the single
/// outbound HTTP call must not hold a database connection.
pub async fn preview(
    state: &AppState,
    tournament_id: i64,
    source_url: &str,
) -> ApiResult<MatchImportPreview> {
    require_tournament(&state.pool, tournament_id).await?;

    let trimmed_url = source_url.trim();
    if let Some(problem) = validate_source_url(trimmed_url) {
        return Err(DomainError::conflict(problem).into());
    }

    let scraped = state.scraper.scrape_match(trimmed_url).await?;
    let (Some(side_a), Some(side_b)) = (scraped.side_a.clone(), scraped.side_b.clone()) else {
        return Err(DomainError::conflict(
            "The scraper could not read both sides of that match. \
             It may not be a completed match, or the source site's markup may have changed.",
        )
        .into());
    };

    let heroes = NameResolver::new(query::hero_names(&state.pool).await?);
    let maps = NameResolver::new(query::map_names(&state.pool).await?);
    let pool_map_ids = map_query::pool_map_ids(&state.pool, tournament_id).await?;

    let mut resolution = Resolution {
        heroes,
        maps,
        pool_map_ids,
        // Collected as the resolution runs, then handed back in order: one hero
        // missing from the catalogue can appear as a pick, a game participant
        // and a ban, and the admin needs to be told about it once. `IndexSet`
        // is Kotlin's `LinkedHashSet` (PORTING.md §4.2).
        unresolved: IndexSet::new(),
    };

    let participants = [&side_a, &side_b]
        .into_iter()
        .map(|side| ImportedParticipant {
            player_label: side
                .player_label
                .as_deref()
                .map(str::trim)
                .filter(|label| !label.is_empty())
                .map(str::to_owned),
            drafted_hero_ids: distinct(
                side.picks
                    .iter()
                    .filter_map(|name| resolution.hero(Some(name.as_str())))
                    .collect(),
            ),
        })
        .collect();

    let mut sorted_games = scraped.games.clone();
    sorted_games.sort_by_key(|game| game.game_index.unwrap_or(0));
    let games = sorted_games
        .iter()
        .enumerate()
        .map(|(index, game)| ImportedGame {
            // The source's own index is trusted when present, but games must be
            // a dense 1..N run (`GAME_NUMBERS_NOT_SEQUENTIAL`) -- so position in
            // the sorted list is the fallback, not a null that cannot be saved.
            game_number: game.game_index.unwrap_or(index as i32 + 1),
            map_id: resolution.map(game.map_name.as_deref()),
            map_name: game.map_name.clone(),
            participants: [game.side_a.as_ref(), game.side_b.as_ref()]
                .into_iter()
                .map(|game_side| ImportedGameParticipant {
                    hero_id: resolution.hero(game_side.and_then(|s| s.hero_name.as_deref())),
                    hero_name: game_side.and_then(|s| s.hero_name.clone()),
                    health_remaining: game_side.and_then(|s| s.health).unwrap_or(0),
                    is_winner: game_side.is_some_and(|s| s.is_winner),
                })
                .collect(),
        })
        .collect();

    let bans = collect_bans(&mut resolution, &scraped, &side_a, &side_b);

    Ok(MatchImportPreview {
        source_url: trimmed_url.to_owned(),
        round_name: scraped.round_name.clone(),
        series_format: scraped.series_format.clone(),
        played_at: scraped_timestamps::parse(scraped.played_at_raw.as_deref()),
        played_at_raw: scraped.played_at_raw.clone(),
        already_imported_match_id: match_query::find_id_by_external_link(
            &state.pool,
            tournament_id,
            trimmed_url,
        )
        .await?,
        participants,
        games,
        bans,
        unresolved: resolution.unresolved.into_iter().collect(),
    })
}

/// Both sides' typed bans and the shared pre-ban pool, flattened into one list
/// as `hero_bans` stores them -- but the side survives. The source already
/// groups a typed ban under the side that owned the hero, and `hero_bans.side`
/// has somewhere to put it; a pre-ban precedes side assignment and so carries
/// none.
///
/// The table is keyed `(match_id, hero_id)`, so the same hero cannot be banned
/// twice in one series even if both sides struck it -- de-duplicating here
/// matches `DUPLICATE_BAN`'s view and avoids handing the admin a draft that
/// cannot be saved. That the survivor keeps the *first* side rather than
/// merging the two is inherent to that key, not a choice made here.
fn collect_bans(
    resolution: &mut Resolution,
    scraped: &ScrapedMatch,
    side_a: &ScrapedSide,
    side_b: &ScrapedSide,
) -> Vec<ImportedBan> {
    let mut bans = Vec::new();
    for (side, scraped_side) in [side_a, side_b].into_iter().enumerate() {
        for ban in &scraped_side.bans {
            let ban_type = parse_ban_type(ban.ban_type.as_deref());
            bans.push(ImportedBan {
                hero_id: resolution.hero(ban.hero_name.as_deref()),
                hero_name: ban.hero_name.clone(),
                ban_type,
                // A source filing a pre-ban under a side is mistaken about what
                // a pre-ban is; dropping the side keeps `BAN_SIDE_INVALID` from
                // firing on the import.
                side: (ban_type != BanType::PreBan).then_some(side as i32),
            });
        }
    }
    for hero_name in &scraped.pre_bans {
        bans.push(ImportedBan {
            hero_id: resolution.hero(Some(hero_name.as_str())),
            hero_name: Some(hero_name.clone()),
            ban_type: BanType::PreBan,
            side: None,
        });
    }

    // Kotlin's `distinctBy { it.heroId ?: it.heroName }`: the id when it
    // resolved, the source's spelling when it did not.
    let mut seen: IndexMap<(Option<i64>, Option<String>), ImportedBan> = IndexMap::new();
    for ban in bans {
        let key = match ban.hero_id {
            Some(id) => (Some(id), None),
            None => (None, ban.hero_name.clone()),
        };
        seen.entry(key).or_insert(ban);
    }
    seen.into_values().collect()
}

/// The scraper already emits this repo's own vocabulary, so this is a lookup
/// rather than a translation. An unrecognised value falls back to `PRE_BAN`:
/// the hero was definitely banned (it came out of the draft card), and
/// `PRE_BAN` is the type that scores neither ban metric, so an unknown label
/// cannot silently pay out points it should not.
fn parse_ban_type(raw: Option<&str>) -> BanType {
    match raw
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_uppercase()
        .as_str()
    {
        "SELF_BAN" => BanType::SelfBan,
        "OPPONENT_BAN" => BanType::OpponentBan,
        _ => BanType::PreBan,
    }
}

fn distinct(ids: Vec<i64>) -> Vec<i64> {
    ids.into_iter()
        .collect::<IndexSet<_>>()
        .into_iter()
        .collect()
}

/// The two name lookups plus the running list of what they could not resolve.
///
/// A struct rather than two closures because the Kotlin's `resolveHero` /
/// `resolveMap` both *mutate* the shared `unresolved` set as they go, which two
/// borrowing closures cannot do.
struct Resolution {
    heroes: NameResolver,
    maps: NameResolver,
    pool_map_ids: Vec<i64>,
    unresolved: IndexSet<UnresolvedName>,
}

impl Resolution {
    fn hero(&mut self, name: Option<&str>) -> Option<i64> {
        let name = name.map(str::trim).filter(|n| !n.is_empty())?;
        match self.heroes.resolve(Some(name)) {
            Some(id) => Some(id),
            None => {
                self.unresolved.insert(UnresolvedName {
                    kind: UnresolvedKind::Hero,
                    source_name: name.to_owned(),
                    reason: UnresolvedReason::UnknownHero,
                    map_id: None,
                    message: format!(
                        "No hero named \"{name}\" exists. Add it under Heroes, then import again."
                    ),
                });
                None
            }
        }
    }

    fn map(&mut self, name: Option<&str>) -> Option<i64> {
        let name = name.map(str::trim).filter(|n| !n.is_empty())?;
        let Some(map_id) = self.maps.resolve(Some(name)) else {
            self.unresolved.insert(UnresolvedName {
                kind: UnresolvedKind::Map,
                source_name: name.to_owned(),
                reason: UnresolvedReason::UnknownMap,
                map_id: None,
                message: format!(
                    "No board named \"{name}\" exists. Add it under Maps, then import again."
                ),
            });
            return None;
        };
        // The one that fires in practice: `match_games` carries a composite
        // foreign key onto `tournament_maps`, so a board this league knows about
        // but has not added to *this* tournament's pool cannot be recorded
        // against it. Heroes have no equivalent constraint.
        if !self.pool_map_ids.contains(&map_id) {
            self.unresolved.insert(UnresolvedName {
                kind: UnresolvedKind::Map,
                source_name: name.to_owned(),
                reason: UnresolvedReason::MapNotInPool,
                map_id: Some(map_id),
                message: format!(
                    "\"{name}\" is not in this tournament's board pool, \
                     so a game cannot be recorded on it."
                ),
            });
            return None;
        }
        Some(map_id)
    }
}
