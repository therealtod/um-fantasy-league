package com.umfl.map

import org.springframework.jdbc.core.simple.JdbcClient
import org.springframework.stereotype.Repository
import java.sql.ResultSet

/**
 * Writes to `tournament_maps`, the composite-keyed link table Spring Data JDBC
 * cannot map as an entity. There is no non-key column here, so the only write
 * is an idempotent add — there is no "re-price" to also cover.
 */
@Repository
class MapPoolAdminRepository(private val jdbcClient: JdbcClient) {

    fun addToPool(tournamentId: Long, mapId: Long) {
        jdbcClient.sql(
            "insert into tournament_maps (tournament_id, map_id) values (:tournamentId, :mapId) on conflict do nothing"
        ).param("tournamentId", tournamentId).param("mapId", mapId).update()
    }

    /** Same idempotent add as [addToPool], batched into one multi-row INSERT. */
    fun addToPoolBatch(tournamentId: Long, mapIds: Collection<Long>) {
        if (mapIds.isEmpty()) return
        val ids = mapIds.toList()
        val valuesSql = ids.indices.joinToString(", ") { "(:tournamentId, :mapId$it)" }
        val statement = ids.foldIndexed(
            jdbcClient.sql(
                "insert into tournament_maps (tournament_id, map_id) values $valuesSql on conflict do nothing"
            ).param("tournamentId", tournamentId)
        ) { i, spec, mapId -> spec.param("mapId$i", mapId) }
        statement.update()
    }

    /** The set of maps this tournament may record a match on — reused by [com.umfl.match.MatchResultPolicy]. */
    fun poolMapIds(tournamentId: Long): Set<Long> =
        jdbcClient.sql("select map_id from tournament_maps where tournament_id = :tournamentId")
            .param("tournamentId", tournamentId)
            .query(Long::class.java)
            .list()
            .filterNotNull()
            .toSet()

    /** The maps already in this tournament's pool, identity included — feeds the admin Map Pool wizard. */
    fun poolMaps(tournamentId: Long): List<GameMap> =
        jdbcClient.sql(
            """
            select gm.id, gm.name
            from tournament_maps tm
            join game_maps gm on gm.id = tm.map_id
            where tm.tournament_id = :tournamentId
            order by gm.name
            """
        ).param("tournamentId", tournamentId).query(::mapGameMap).list()

    /**
     * True when this tournament has a recorded game on this map. Unlike hero
     * removal, this one *is* blocked by the schema: `match_games` carries a
     * composite FK onto `(tournament_id, map_id)` here, so deleting the pool
     * row out from under a recorded game would fail as a raw
     * `DataIntegrityViolationException`. The caller checks this first so that
     * surfaces as an explicit [com.umfl.common.ConflictException] instead.
     */
    fun hasRecordedMatch(tournamentId: Long, mapId: Long): Boolean =
        jdbcClient.sql(
            "select exists(select 1 from match_games where tournament_id = :tournamentId and map_id = :mapId)"
        ).param("tournamentId", tournamentId).param("mapId", mapId).query(Boolean::class.java).single()

    /** Removes [mapId] from [tournamentId]'s pool. Returns false if it wasn't there to begin with. */
    fun removeFromPool(tournamentId: Long, mapId: Long): Boolean {
        val rowsDeleted = jdbcClient.sql(
            "delete from tournament_maps where tournament_id = :tournamentId and map_id = :mapId"
        ).param("tournamentId", tournamentId).param("mapId", mapId).update()
        return rowsDeleted > 0
    }

    /**
     * Checks `match_game_map_in_pool` now, in the caller's transaction, then
     * restores its deferral.
     *
     * That FK is DEFERRABLE INITIALLY DEFERRED for the sake of tournament
     * deletes (see the schema comment), which means a pool row deleted out
     * from under a recorded game does *not* fail the `delete` statement — it
     * fails at commit, long after the service that could name the map and the
     * tournament has returned. `set constraints ... immediate` fires the
     * pending check on the spot, so [removeFromPool]'s caller sees the
     * violation as a `DataIntegrityViolationException` it can still translate.
     */
    fun checkMapInPoolNow() {
        jdbcClient.sql("set constraints match_game_map_in_pool immediate").update()
        jdbcClient.sql("set constraints match_game_map_in_pool deferred").update()
    }
}

private fun mapGameMap(rs: ResultSet, @Suppress("UNUSED_PARAMETER") rowNum: Int) =
    GameMap(id = rs.getLong("id"), name = rs.getString("name"))
