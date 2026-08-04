package com.umfl.match

import org.springframework.jdbc.core.simple.JdbcClient
import org.springframework.stereotype.Repository
import java.sql.ResultSet

/**
 * Reads recorded matches back out as whole [MatchResult]s.
 *
 * Three flat queries — matches, participants, bans — grouped in Kotlin, rather
 * than one join. A match has N participants *and* M bans, so a single join
 * would fan out to N×M ragged rows that then have to be de-duplicated anyway.
 *
 * Read-only by design: the admin API (`com.umfl.match.AdminMatchService`) is
 * the sole write path, via the `TournamentMatch` aggregate — this class never
 * writes, so reads and writes cannot silently drift out of step.
 */
@Repository
class MatchResultQuery(private val jdbcClient: JdbcClient) {

    /** One match by id, or null if it does not exist — the admin write endpoints' response shape. */
    fun findById(matchId: Long): MatchResult? =
        assemble(
            jdbcClient
                .sql("$SELECT_MATCH where m.id = :matchId")
                .param("matchId", matchId)
                .query(::mapMatchRow)
                .list()
        ).singleOrNull()

    /**
     * Every recorded match in a tournament, oldest first — the scoring fold's input.
     * Optionally narrowed to one [round] — the admin match list's filter.
     */
    fun findByTournament(tournamentId: Long, round: Int? = null): List<MatchResult> =
        assemble(
            jdbcClient
                .sql(
                    """
                    $SELECT_MATCH
                    where m.tournament_id = :tournamentId
                      and (cast(:round as int) is null or m.round = cast(:round as int))
                    order by m.played_at, m.id
                    """
                )
                .param("tournamentId", tournamentId)
                .param("round", round)
                .query(::mapMatchRow)
                .list()
        )

    /**
     * The newest [limit] matches after [sinceMatchId] — the ticker's page.
     *
     * The polling key is the id, not `played_at`: parallel tables share a start
     * time, so `played_at` is not unique, while `id` is a monotonic bigserial
     * seeded in chronological order. Display order is still `played_at`.
     */
    fun findByTournamentSince(tournamentId: Long, sinceMatchId: Long, limit: Int): List<MatchResult> =
        assemble(
            jdbcClient
                .sql(
                    """
                    $SELECT_MATCH
                    where m.tournament_id = :tournamentId
                      and m.id > :sinceMatchId
                    order by m.played_at desc, m.id desc
                    limit :limit
                    """
                )
                .param("tournamentId", tournamentId)
                .param("sinceMatchId", sinceMatchId)
                .param("limit", limit)
                .query(::mapMatchRow)
                .list()
        )

    /** Attaches participants and bans to a page of match headers, preserving its order. */
    private fun assemble(headers: List<MatchHeader>): List<MatchResult> {
        if (headers.isEmpty()) return emptyList()
        val matchIds = headers.map { it.matchId }

        val participants = jdbcClient
            .sql(SELECT_PARTICIPANTS)
            .param("matchIds", matchIds)
            .query { rs, _ ->
                rs.getLong("match_id") to ParticipantResult(
                    participantId = rs.getLong("id"),
                    playerLabel = rs.getString("player_label"),
                    heroId = rs.getLong("hero_id"),
                    heroName = rs.getString("hero_name"),
                    healthRemaining = rs.getInt("health_remaining"),
                    isWinner = rs.getBoolean("is_winner"),
                )
            }
            .list()
            .groupBy({ it.first }, { it.second })

        val bans = jdbcClient
            .sql(SELECT_BANS)
            .param("matchIds", matchIds)
            .query { rs, _ ->
                rs.getLong("match_id") to BanResult(
                    heroId = rs.getLong("hero_id"),
                    heroName = rs.getString("hero_name"),
                )
            }
            .list()
            .groupBy({ it.first }, { it.second })

        return headers.map { header ->
            MatchResult(
                matchId = header.matchId,
                tournamentId = header.tournamentId,
                round = header.round,
                mapId = header.mapId,
                mapName = header.mapName,
                playedAt = header.playedAt,
                participants = participants[header.matchId].orEmpty(),
                bans = bans[header.matchId].orEmpty(),
            )
        }
    }

    private companion object {
        const val SELECT_MATCH = """
            select m.id, m.tournament_id, m.round, m.map_id, gm.name as map_name, m.played_at
            from tournament_match m
            join game_map gm on gm.id = m.map_id
        """

        // Ordered by id so a match's sides come back in the order they were recorded;
        // the ticker then floats the winner to the front.
        const val SELECT_PARTICIPANTS = """
            select mp.id, mp.match_id, mp.player_label,
                   mp.hero_id, h.name as hero_name, mp.health_remaining, mp.is_winner
            from match_participant mp
            join heroes h on h.id = mp.hero_id
            where mp.match_id in (:matchIds)
            order by mp.match_id, mp.id
        """

        const val SELECT_BANS = """
            select mb.match_id, mb.hero_id, h.name as hero_name
            from match_ban mb
            join heroes h on h.id = mb.hero_id
            where mb.match_id in (:matchIds)
            order by mb.match_id, h.name
        """
    }
}

/** A `tournament_match` row before its children are attached. */
private data class MatchHeader(
    val matchId: Long,
    val tournamentId: Long,
    val round: Int,
    val mapId: Long,
    val mapName: String,
    val playedAt: java.time.Instant,
)

private fun mapMatchRow(rs: ResultSet, @Suppress("UNUSED_PARAMETER") rowNum: Int) =
    MatchHeader(
        matchId = rs.getLong("id"),
        tournamentId = rs.getLong("tournament_id"),
        round = rs.getInt("round"),
        mapId = rs.getLong("map_id"),
        mapName = rs.getString("map_name"),
        playedAt = rs.getTimestamp("played_at").toInstant(),
    )
