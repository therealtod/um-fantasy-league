package com.umfl.match

import com.umfl.common.ConflictException
import com.umfl.common.MatchRuleException
import com.umfl.common.NotFoundException
import com.umfl.hero.HeroRepository
import com.umfl.map.MapPoolAdminRepository
import com.umfl.standings.StandingsUpdateEvent
import com.umfl.tournament.TournamentService
import org.springframework.context.ApplicationEventPublisher
import org.springframework.dao.DataIntegrityViolationException
import org.springframework.stereotype.Service
import org.springframework.transaction.annotation.Transactional
import java.time.Instant

/**
 * The admin write path for match results. Everything downstream — standings,
 * ticker, points — is derived at read time from what this saves, so recording,
 * correcting or deleting a match here is the entire surface area; no total is
 * ever recomputed and stored.
 *
 * Because this is the *only* writer of `tournament_matches` and its children, the
 * [StandingsUpdateEvent] published by all three methods is a complete account of
 * when that data changes. Two things listen to it: [com.umfl.standings.StandingsSseHub],
 * which tells watching tabs to refetch, and [MatchResultCache], which drops the
 * assembled match list it holds for the tournament. Keep publishing it from any
 * method added here that writes a match.
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
        externalLink: String,
        participants: List<MatchParticipantInput>,
        games: List<MatchGameInput>,
        bans: List<MatchBanInput>,
    ): MatchResult {
        tournamentService.requireTournament(tournamentId)
        validate(tournamentId, participants, games, bans)

        val link = externalLink.trim()
        requireLinkUnused(tournamentId, link, correctingMatchId = null)

        val saved = savingLinkConflictAsConflict(link) {
            tournamentMatchRepository.save(
                TournamentMatch(
                    tournamentId = tournamentId,
                    round = round,
                    playedAt = playedAt,
                    externalLink = link,
                    participants = toParticipants(participants),
                    games = toGames(tournamentId, games),
                    bans = toBans(bans),
                    picks = toPicks(participants),
                )
            )
        }
        eventPublisher.publishEvent(StandingsUpdateEvent(tournamentId))
        return requireNotNull(matchResultQuery.findById(requireNotNull(saved.id))) { "Just-saved match not found" }
    }

    @Transactional
    fun correct(
        tournamentId: Long,
        matchId: Long,
        round: Int,
        playedAt: Instant,
        externalLink: String,
        participants: List<MatchParticipantInput>,
        games: List<MatchGameInput>,
        bans: List<MatchBanInput>,
    ): MatchResult {
        val existing = requireMatch(tournamentId, matchId)
        validate(tournamentId, participants, games, bans)

        val link = externalLink.trim()
        requireLinkUnused(tournamentId, link, correctingMatchId = matchId)

        savingLinkConflictAsConflict(link) {
            tournamentMatchRepository.save(
                existing.copy(
                    round = round,
                    playedAt = playedAt,
                    externalLink = link,
                    participants = toParticipants(participants),
                    games = toGames(tournamentId, games),
                    bans = toBans(bans),
                    picks = toPicks(participants),
                )
            )
        }
        eventPublisher.publishEvent(StandingsUpdateEvent(tournamentId))
        return requireNotNull(matchResultQuery.findById(matchId)) { "Just-saved match not found" }
    }

    @Transactional
    fun delete(tournamentId: Long, matchId: Long) {
        val existing = requireMatch(tournamentId, matchId)
        tournamentMatchRepository.delete(existing)
        eventPublisher.publishEvent(StandingsUpdateEvent(tournamentId))
    }

    /**
     * The source link is what stops the same match being imported twice, so a
     * link already spent in this tournament is refused rather than warned about
     * — a duplicated match silently double-counts every appearance, win and ban
     * it carries, and nothing surfaces it until someone doubts the standings.
     *
     * [correctingMatchId] is the match being corrected, excluded from the
     * search: re-saving a match under the URL it already holds is the ordinary
     * correction path and updates the row in place. Only moving one match's
     * link onto another's is a conflict.
     */
    private fun requireLinkUnused(tournamentId: Long, link: String, correctingMatchId: Long?) {
        val collision = matchResultQuery.findIdByExternalLink(tournamentId, link)
        if (collision != null && collision != correctingMatchId) {
            throw duplicateLinkConflict(collision, link)
        }
    }

    /**
     * The check above and the write are two statements, so a match recorded in
     * between slips past the check and is stopped by
     * `uq_tournament_match_external_link` instead. That window is narrow but
     * real, so the violation is translated into a [ConflictException] naming the
     * link rather than left to `GlobalExceptionHandler`'s catch-all, which
     * renders the generic "should never fire" 409 and names nothing the admin
     * can act on. Mirrors [com.umfl.map.AdminMapService.removeFromPool].
     *
     * The offending match cannot be named here the way [requireLinkUnused]
     * names it: the failed insert has already aborted the transaction, so no
     * further query can run inside it. Only that one index is translated —
     * every other integrity violation is still a bug worth surfacing as itself,
     * not mislabelled a duplicate link.
     */
    private fun <T> savingLinkConflictAsConflict(link: String, save: () -> T): T =
        try {
            save()
        } catch (e: DataIntegrityViolationException) {
            if (e.mostSpecificCause.message?.contains(LINK_INDEX) != true) throw e
            throw ConflictException(
                "Another match in this tournament was just recorded against $link. " +
                    "Correct that match instead of recording a second one."
            )
        }

    private fun duplicateLinkConflict(matchId: Long, link: String) = ConflictException(
        "Match $matchId is already recorded against $link. Correct that match instead of recording a second one."
    )

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
        bans.map { HeroBan(heroId = it.heroId, banType = it.banType, side = it.side) }.toSet()

    /**
     * The draft rides in on the participants — a pick belongs to a side — but
     * persists as a child of the match, since `match_participants` is composite-
     * keyed and cannot own children of its own. The side is the participant's
     * list position, the same ordinal `match_participants.side` is written from.
     */
    private fun toPicks(participants: List<MatchParticipantInput>): Set<HeroPick> =
        participants.flatMapIndexed { side, participant ->
            participant.draftedHeroIds.distinct().map { HeroPick(side = side, heroId = it) }
        }.toSet()

    private companion object {
        /** Named in `V1__core_schema.sql`; Postgres quotes it in the violation message. */
        const val LINK_INDEX = "uq_tournament_match_external_link"
    }
}
