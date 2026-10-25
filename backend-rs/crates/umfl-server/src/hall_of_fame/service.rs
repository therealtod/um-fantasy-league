//! The Hall of Fame is the completed tournaments' leaderboards, read live.
//!
//! Each board comes from [`crate::standings::service::board`] unchanged, so a
//! podium is exactly the top of what `/standings` shows for that tournament --
//! the cache-before-snapshot ordering and the REPEATABLE READ snapshot
//! documented there apply per board, and nothing here stores a winner. The
//! boards are folded one after another rather than concurrently: there are
//! only ever a handful of completed tournaments, and fanning out would put the
//! whole set's connections on the pool at once for no gain.

use umfl_domain::hall_of_fame::{self, CompletedTournament, HallOfFame};
use umfl_domain::tournament::TournamentStatus;

use crate::error::ApiResult;
use crate::standings::service as standings_service;
use crate::state::AppState;
use crate::tournament::query as tournament_query;

pub async fn hall_of_fame(state: &AppState) -> ApiResult<HallOfFame> {
    let completed =
        tournament_query::find_by_status_ordered(&state.pool, TournamentStatus::Completed).await?;

    let mut boards = Vec::with_capacity(completed.len());
    for tournament in completed {
        let tournament_id = tournament.id.expect("a loaded tournament has an id");
        let board = standings_service::board(state, tournament_id).await?;
        boards.push((
            CompletedTournament {
                tournament_id,
                name: tournament.name,
                format: tournament.format,
                start_date: tournament.start_date,
                end_date: tournament.end_date,
            },
            board,
        ));
    }

    Ok(hall_of_fame::build(&boards))
}
