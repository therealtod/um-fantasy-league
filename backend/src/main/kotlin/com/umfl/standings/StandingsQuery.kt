package com.umfl.standings

import org.springframework.jdbc.core.simple.JdbcClient
import org.springframework.stereotype.Repository

/** One hero on a roster, with the price this tournament charges for it. */
data class RosterHero(
    val slotIndex: Int,
    val heroId: Long,
    val name: String,
    val cost: Int,
)

/** One entry's roster, as the leaderboard needs it. Points are added by the service. */
data class EntryRoster(
    val entryId: Long,
    val managerId: Long,
    val handle: String,
    val displayName: String,
    val creditGrant: Int,
    val heroes: List<RosterHero>,
) {
    val spent: Int
        get() = heroes.sumOf { it.cost }
}

/**
 * The read side of the Live Standings screen: who entered, and what they drafted.
 *
 * Scoring is deliberately *not* in this query. Points are folded in Kotlin from
 * `tournament_matches` (see [StandingsService]) because the coefficients live in
 * `scoring_coefficients` and each (hero, match) pair is then priced exactly once,
 * no matter how many rosters hold that hero — cheaper than the join's fan-out,
 * and impossible to get inconsistent with the ticker.
 */
@Repository
class StandingsQuery(private val jdbcClient: JdbcClient) {

    /**
     * Every entry in the tournament with its roster, ordered by entry then slot.
     *
     * Note the **left join** onto `entry_slots`: an entry with no picks yet is
     * still an entry and still belongs on the board. An inner join silently
     * drops it.
     */
    fun rosters(tournamentId: Long): List<EntryRoster> {
        val rows = jdbcClient
            .sql(ROSTERS_SQL)
            .param("tournamentId", tournamentId)
            .query { rs, _ ->
                val heroId = (rs.getObject("hero_id") as? Number)?.toLong()
                RosterRow(
                    entryId = rs.getLong("entry_id"),
                    managerId = rs.getLong("manager_id"),
                    handle = rs.getString("handle"),
                    displayName = rs.getString("display_name"),
                    creditGrant = rs.getInt("credit_grant"),
                    hero = heroId?.let {
                        RosterHero(
                            slotIndex = rs.getInt("slot_index"),
                            heroId = it,
                            name = rs.getString("hero_name"),
                            // 0 only if a rostered hero left the pool: report it, don't crash.
                            cost = rs.getInt("hero_cost"),
                        )
                    },
                )
            }
            .list()

        return rows
            .groupBy { it.entryId }
            .map { (_, group) ->
                val first = group.first()
                EntryRoster(
                    entryId = first.entryId,
                    managerId = first.managerId,
                    handle = first.handle,
                    displayName = first.displayName,
                    creditGrant = first.creditGrant,
                    heroes = group.mapNotNull { it.hero },
                )
            }
    }

    private data class RosterRow(
        val entryId: Long,
        val managerId: Long,
        val handle: String,
        val displayName: String,
        val creditGrant: Int,
        val hero: RosterHero?,
    )

    private companion object {
        const val ROSTERS_SQL = """
            select e.id            as entry_id,
                   mg.id           as manager_id,
                   mg.handle,
                   mg.display_name,
                   e.credit_grant,
                   es.slot_index,
                   h.id            as hero_id,
                   h.name          as hero_name,
                   th.cost         as hero_cost
            from tournament_entries e
                join managers mg on mg.id = e.manager_id
                left join entry_slots es on es.entry_id = e.id
                left join heroes h on h.id = es.hero_id
                left join tournament_heroes th
                    on th.tournament_id = e.tournament_id and th.hero_id = es.hero_id
            where e.tournament_id = :tournamentId
            order by e.id, es.slot_index
        """
    }
}
