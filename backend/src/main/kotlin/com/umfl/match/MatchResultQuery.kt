package com.umfl.match

import org.springframework.jdbc.core.simple.JdbcClient
import org.springframework.stereotype.Repository
import java.sql.ResultSet

/**
 * Reads recorded matches back out as whole [MatchResult]s.
 *
 * Five flat queries — matches, participants, games, game-participants, bans —
 * grouped in Kotlin, rather than one join. A match has N participants, M
 * games each with their own participants, and K bans, so a single join would
 * fan out to a ragged cross product that then has to be de-duplicated anyway.
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

    /** Attaches participants, games (with their own participants) and bans to a page of match headers. */
    private fun assemble(headers: List<MatchHeader>): List<MatchResult> {
        if (headers.isEmpty()) return emptyList()
        val matchIds = headers.map { it.matchId }

        val participantsByMatch = jdbcClient
            .sql(SELECT_PARTICIPANTS)
            .param("matchIds", matchIds)
            .query { rs, _ ->
                rs.getLong("match_id") to MatchParticipantResult(
                    side = rs.getInt("side"),
                    playerLabel = rs.getString("player_label"),
                )
            }
            .list()
            .groupBy({ it.first }, { it.second })

        val gameHeadersByMatch = jdbcClient
            .sql(SELECT_GAMES)
            .param("matchIds", matchIds)
            .query { rs, _ ->
                rs.getLong("match_id") to GameHeader(
                    gameId = rs.getLong("id"),
                    gameNumber = rs.getInt("game_number"),
                    mapId = rs.getLong("map_id"),
                    mapName = rs.getString("map_name"),
                )
            }
            .list()
            .groupBy({ it.first }, { it.second })

        val gameParticipantsByGame = jdbcClient
            .sql(SELECT_GAME_PARTICIPANTS)
            .param("matchIds", matchIds)
            .query { rs, _ ->
                rs.getLong("game_id") to GameParticipantResult(
                    side = rs.getInt("side"),
                    heroId = rs.getLong("hero_id"),
                    heroName = rs.getString("hero_name"),
                    healthRemaining = rs.getInt("health_remaining"),
                    isWinner = rs.getBoolean("is_winner"),
                )
            }
            .list()
            .groupBy({ it.first }, { it.second })

        val bansByMatch = jdbcClient
            .sql(SELECT_BANS)
            .param("matchIds", matchIds)
            .query { rs, _ ->
                rs.getLong("match_id") to BanResult(
                    heroId = rs.getLong("hero_id"),
                    heroName = rs.getString("hero_name"),
                    banType = BanType.valueOf(rs.getString("ban_type")),
                )
            }
            .list()
            .groupBy({ it.first }, { it.second })

        return headers.map { header ->
            val games = gameHeadersByMatch[header.matchId].orEmpty()
                .sortedBy { it.gameNumber }
                .map { game ->
                    GameResult(
                        gameId = game.gameId,
                        gameNumber = game.gameNumber,
                        mapId = game.mapId,
                        mapName = game.mapName,
                        participants = gameParticipantsByGame[game.gameId].orEmpty(),
                    )
                }
            MatchResult(
                matchId = header.matchId,
                tournamentId = header.tournamentId,
                round = header.round,
                playedAt = header.playedAt,
                externalLink = header.externalLink,
                participants = participantsByMatch[header.matchId].orEmpty(),
                games = games,
                bans = bansByMatch[header.matchId].orEmpty(),
            )
        }
    }

    private companion object {
        const val SELECT_MATCH = """
            select m.id, m.tournament_id, m.round, m.played_at, m.external_link
            from tournament_match m
        """

        const val SELECT_PARTICIPANTS = """
            select mp.match_id, mp.side, mp.player_label
            from match_participant mp
            where mp.match_id in (:matchIds)
            order by mp.match_id, mp.side
        """

        const val SELECT_GAMES = """
            select mg.id, mg.match_id, mg.game_number, mg.map_id, gm.name as map_name
            from match_game mg
            join game_map gm on gm.id = mg.map_id
            where mg.match_id in (:matchIds)
            order by mg.match_id, mg.game_number
        """

        // Joined through match_game to recover match_id, which match_game_participant
        // does not itself carry -- see the mapping note on MatchGameParticipant.
        const val SELECT_GAME_PARTICIPANTS = """
            select mgp.game_id, mg.match_id, mgp.side,
                   mgp.hero_id, h.name as hero_name, mgp.health_remaining, mgp.is_winner
            from match_game_participant mgp
            join match_game mg on mg.id = mgp.game_id
            join heroes h on h.id = mgp.hero_id
            where mg.match_id in (:matchIds)
            order by mgp.game_id, mgp.side
        """

        const val SELECT_BANS = """
            select hb.match_id, hb.hero_id, h.name as hero_name, hb.ban_type
            from hero_ban hb
            join heroes h on h.id = hb.hero_id
            where hb.match_id in (:matchIds)
            order by hb.match_id, h.name
        """
    }
}

/** A `tournament_match` row before its children are attached. */
private data class MatchHeader(
    val matchId: Long,
    val tournamentId: Long,
    val round: Int,
    val playedAt: java.time.Instant,
    val externalLink: String?,
)

/** A `match_game` row before its own participants are attached. */
private data class GameHeader(
    val gameId: Long,
    val gameNumber: Int,
    val mapId: Long,
    val mapName: String,
)

private fun mapMatchRow(rs: ResultSet, @Suppress("UNUSED_PARAMETER") rowNum: Int) =
    MatchHeader(
        matchId = rs.getLong("id"),
        tournamentId = rs.getLong("tournament_id"),
        round = rs.getInt("round"),
        playedAt = rs.getTimestamp("played_at").toInstant(),
        externalLink = rs.getString("external_link"),
    )
