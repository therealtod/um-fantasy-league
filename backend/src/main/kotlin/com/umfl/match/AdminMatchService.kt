package com.umfl.match

import com.umfl.common.MatchRuleException
import com.umfl.common.NotFoundException
import com.umfl.hero.HeroRepository
import com.umfl.map.MapPoolAdminRepository
import com.umfl.standings.StandingsUpdateEvent
import com.umfl.tournament.TournamentService
import org.springframework.context.ApplicationEventPublisher
import org.springframework.stereotype.Service
import org.springframework.transaction.annotation.Transactional
import java.time.Instant

/**
 * The admin write path for match results. Everything downstream — standings,
 * ticker, points — is derived at read time from what this saves, so recording,
 * correcting or deleting a match here is the entire surface area; nothing
 * else needs to be recomputed or invalidated.
 */
@Service
class AdminMatchService(
    private val tournamentService: TournamentService,
    private val tournamentMatchRepository: TournamentMatchRepository,
    private val mapPoolAdminRepository: MapPoolAdminRepository,
    private val heroRepository: HeroRepository,
    private val matchResultQuery: MatchResultQuery,
    private val eventPublisher: ApplicationEventPublisher,
) {

    @Transactional
    fun record(
        tournamentId: Long,
        round: Int,
        playedAt: Instant,
        externalLink: String?,
        participants: List<MatchParticipantInput>,
        games: List<MatchGameInput>,
        bans: List<MatchBanInput>,
    ): MatchResult {
        tournamentService.requireTournament(tournamentId)
        validate(tournamentId, participants, games, bans)

        val saved = tournamentMatchRepository.save(
            TournamentMatch(
                tournamentId = tournamentId,
                round = round,
                playedAt = playedAt,
                externalLink = blankToNull(externalLink),
                participants = toParticipants(participants),
                games = toGames(tournamentId, games),
                bans = toBans(bans),
                picks = toPicks(participants),
            )
        )
        eventPublisher.publishEvent(StandingsUpdateEvent(tournamentId))
        return requireNotNull(matchResultQuery.findById(requireNotNull(saved.id))) { "Just-saved match not found" }
    }

    @Transactional
    fun correct(
        tournamentId: Long,
        matchId: Long,
        round: Int,
        playedAt: Instant,
        externalLink: String?,
        participants: List<MatchParticipantInput>,
        games: List<MatchGameInput>,
        bans: List<MatchBanInput>,
    ): MatchResult {
        val existing = requireMatch(tournamentId, matchId)
        validate(tournamentId, participants, games, bans)

        tournamentMatchRepository.save(
            existing.copy(
                round = round,
                playedAt = playedAt,
                externalLink = blankToNull(externalLink),
                participants = toParticipants(participants),
                games = toGames(tournamentId, games),
                bans = toBans(bans),
                picks = toPicks(participants),
            )
        )
        eventPublisher.publishEvent(StandingsUpdateEvent(tournamentId))
        return requireNotNull(matchResultQuery.findById(matchId)) { "Just-saved match not found" }
    }

    @Transactional
    fun delete(tournamentId: Long, matchId: Long) {
        val existing = requireMatch(tournamentId, matchId)
        tournamentMatchRepository.delete(existing)
        eventPublisher.publishEvent(StandingsUpdateEvent(tournamentId))
    }

    private fun requireMatch(tournamentId: Long, matchId: Long): TournamentMatch =
        tournamentMatchRepository.findById(matchId)
            .filter { it.tournamentId == tournamentId }
            .orElseThrow { NotFoundException("No match $matchId in tournament $tournamentId") }

    private fun validate(
        tournamentId: Long,
        participants: List<MatchParticipantInput>,
        games: List<MatchGameInput>,
        bans: List<MatchBanInput>,
    ) {
        val referencedHeroIds = games.flatMap { it.participants.map { p -> p.heroId } } +
            participants.flatMap { it.draftedHeroIds } +
            bans.map { it.heroId }
        val violations = MatchResultPolicy.validate(
            validMapIds = mapPoolAdminRepository.poolMapIds(tournamentId),
            validHeroIds = heroRepository.findAllById(referencedHeroIds).mapNotNull { it.id }.toSet(),
            participants = participants,
            games = games,
            bans = bans,
        )
        if (violations.isNotEmpty()) throw MatchRuleException(violations)
    }

    /**
     * "Absent" for optional free text is null, never `""`. An admin form posts
     * an untouched text input as an empty string, and Jackson's `non_null`
     * inclusion only drops nulls — so without this a blank box comes back out
     * of the API as `""` and a consumer testing for null sees a link, or a
     * player, that was never entered.
     */
    private fun blankToNull(text: String?): String? = text?.trim()?.ifEmpty { null }

    private fun toParticipants(participants: List<MatchParticipantInput>): List<MatchParticipant> =
        participants.map { MatchParticipant(playerLabel = blankToNull(it.playerLabel)) }

    private fun toGames(tournamentId: Long, games: List<MatchGameInput>): Set<MatchGame> =
        games.map { game ->
            MatchGame(
                tournamentId = tournamentId,
                gameNumber = game.gameNumber,
                mapId = game.mapId,
                participants = game.participants.map {
                    MatchGameParticipant(
                        heroId = it.heroId,
                        healthRemaining = it.healthRemaining,
                        isWinner = it.isWinner,
                    )
                },
            )
        }.toSet()

    private fun toBans(bans: List<MatchBanInput>): Set<HeroBan> =
        bans.map { HeroBan(heroId = it.heroId, banType = it.banType) }.toSet()

    /**
     * The draft rides in on the participants — a pick belongs to a side — but
     * persists as a child of the match, since `match_participant` is composite-
     * keyed and cannot own children of its own. The side is the participant's
     * list position, the same ordinal `match_participant.side` is written from.
     */
    private fun toPicks(participants: List<MatchParticipantInput>): Set<HeroPick> =
        participants.flatMapIndexed { side, participant ->
            participant.draftedHeroIds.distinct().map { HeroPick(side = side, heroId = it) }
        }.toSet()
}
