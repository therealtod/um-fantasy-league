package com.umfl.map

import org.springframework.jdbc.core.simple.JdbcClient
import org.springframework.stereotype.Repository
import java.sql.ResultSet

/**
 * Writes to `tournament_map`, the composite-keyed link table Spring Data JDBC
 * cannot map as an entity. There is no non-key column here, so the only write
 * is an idempotent add — there is no "re-price" to also cover.
 */
@Repository
class MapPoolAdminRepository(private val jdbcClient: JdbcClient) {

    fun addToPool(tournamentId: Long, mapId: Long) {
        jdbcClient.sql(
            "insert into tournament_map (tournament_id, map_id) values (:tournamentId, :mapId) on conflict do nothing"
        ).param("tournamentId", tournamentId).param("mapId", mapId).update()
    }

    /** The set of maps this tournament may record a match on — reused by [com.umfl.match.MatchResultPolicy]. */
    fun poolMapIds(tournamentId: Long): Set<Long> =
        jdbcClient.sql("select map_id from tournament_map where tournament_id = :tournamentId")
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
            from tournament_map tm
            join game_map gm on gm.id = tm.map_id
            where tm.tournament_id = :tournamentId
            order by gm.name
            """
        ).param("tournamentId", tournamentId).query(::mapGameMap).list()
}

private fun mapGameMap(rs: ResultSet, @Suppress("UNUSED_PARAMETER") rowNum: Int) =
    GameMap(id = rs.getLong("id"), name = rs.getString("name"))
