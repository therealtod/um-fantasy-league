package com.umfl.match

import org.springframework.data.repository.CrudRepository
import org.springframework.stereotype.Repository

/**
 * Writes recorded matches as whole aggregates. Every save replaces the
 * entire `participants`/`games`/`bans` collections — including each game's
 * own nested `participants` — the same "delete and reinsert the child rows"
 * semantics [com.umfl.tournament.TournamentEntryRepository] already has for
 * `entry_slots`.
 */
@Repository
interface TournamentMatchRepository : CrudRepository<TournamentMatch, Long>
