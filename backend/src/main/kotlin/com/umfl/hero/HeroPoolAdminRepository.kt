package com.umfl.hero

import org.springframework.jdbc.core.simple.JdbcClient
import org.springframework.stereotype.Repository

/**
 * Writes to `tournament_heroes`, the composite-keyed link table Spring Data JDBC
 * cannot map as an entity (see the note in `V1__core_schema.sql`).
 *
 * There is no separate "add to pool" and "re-price": the row's only non-key
 * column is `cost`, so an upsert covers both — a hero not yet in the pool gets
 * added at the given cost, one already there gets re-priced.
 */
@Repository
class HeroPoolAdminRepository(private val jdbcClient: JdbcClient) {

    fun upsertCost(tournamentId: Long, heroId: Long, cost: Int) {
        jdbcClient.sql(
            """
            insert into tournament_heroes (tournament_id, hero_id, cost)
            values (:tournamentId, :heroId, :cost)
            on conflict (tournament_id, hero_id) do update set cost = excluded.cost
            """
        ).param("tournamentId", tournamentId).param("heroId", heroId).param("cost", cost).update()
    }

    /**
     * Same upsert as [upsertCost], batched into a single multi-row INSERT so N
     * additions cost one round trip instead of N.
     */
    fun upsertCosts(tournamentId: Long, entries: List<HeroPoolEntryInput>) {
        if (entries.isEmpty()) return
        val valuesSql = entries.indices.joinToString(", ") { "(:tournamentId, :heroId$it, :cost$it)" }
        val statement = entries.foldIndexed(
            jdbcClient.sql(
                """
                insert into tournament_heroes (tournament_id, hero_id, cost)
                values $valuesSql
                on conflict (tournament_id, hero_id) do update set cost = excluded.cost
                """
            ).param("tournamentId", tournamentId)
        ) { i, spec, entry -> spec.param("heroId$i", entry.heroId).param("cost$i", entry.cost) }
        statement.update()
    }

    /**
     * Removes [heroId] from [tournamentId]'s pool. Returns false if it wasn't
     * there to begin with.
     *
     * Nothing in the schema stops this — `entry_slots.hero_id` references
     * `heroes(id)` directly, never `tournament_heroes` — so a roster still
     * holding this hero doesn't break. It re-prices to 0 on its next read via
     * the `coalesce` in [HeroQueryRepository.findRosterHeroes], the same
     * "cost is never snapshotted" behaviour that already applies when a hero
     * is merely re-priced rather than removed. That is deliberate, not an
     * oversight: see `RosterPolicy`'s "no cost snapshot" invariant.
     */
    fun removeFromPool(tournamentId: Long, heroId: Long): Boolean {
        val rowsDeleted = jdbcClient.sql(
            "delete from tournament_heroes where tournament_id = :tournamentId and hero_id = :heroId"
        ).param("tournamentId", tournamentId).param("heroId", heroId).update()
        return rowsDeleted > 0
    }
}
