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
        mapId: Long,
        playedAt: Instant,
        participants: List<MatchParticipantInput>,
        bans: List<MatchBanInput>,
    ): MatchResult {
        tournamentService.requireTournament(tournamentId)
        validate(tournamentId, mapId, participants, bans)

        val saved = tournamentMatchRepository.save(
            TournamentMatch(
                tournamentId = tournamentId,
                round = round,
                mapId = mapId,
                playedAt = playedAt,
                participants = toParticipants(participants),
                bans = toBans(bans),
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
        mapId: Long,
        playedAt: Instant,
        participants: List<MatchParticipantInput>,
        bans: List<MatchBanInput>,
    ): MatchResult {
        val existing = requireMatch(tournamentId, matchId)
        validate(tournamentId, mapId, participants, bans)

        tournamentMatchRepository.save(
            existing.copy(
                round = round,
                mapId = mapId,
                playedAt = playedAt,
                participants = toParticipants(participants),
                bans = toBans(bans),
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
        mapId: Long,
        participants: List<MatchParticipantInput>,
        bans: List<MatchBanInput>,
    ) {
        val referencedHeroIds = participants.map { it.heroId } + bans.map { it.heroId }
        val violations = MatchResultPolicy.validate(
            mapId = mapId,
            validMapIds = mapPoolAdminRepository.poolMapIds(tournamentId),
            validHeroIds = heroRepository.findAllById(referencedHeroIds).mapNotNull { it.id }.toSet(),
            participants = participants,
            bans = bans,
        )
        if (violations.isNotEmpty()) throw MatchRuleException(violations)
    }

    private fun toParticipants(participants: List<MatchParticipantInput>): Set<MatchParticipant> =
        participants.map {
            MatchParticipant(
                playerLabel = it.playerLabel?.trim()?.ifEmpty { null },
                heroId = it.heroId,
                healthRemaining = it.healthRemaining,
                isWinner = it.isWinner,
            )
        }.toSet()

    private fun toBans(bans: List<MatchBanInput>): Set<MatchBan> =
        bans.map { MatchBan(heroId = it.heroId) }.toSet()
}
