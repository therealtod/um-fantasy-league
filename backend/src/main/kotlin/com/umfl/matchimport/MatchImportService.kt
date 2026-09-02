package com.umfl.matchimport

import com.umfl.common.ConflictException
import com.umfl.hero.HeroRepository
import com.umfl.map.GameMapRepository
import com.umfl.map.MapPoolAdminRepository
import com.umfl.match.BanType
import com.umfl.match.MatchResultQuery
import com.umfl.tournament.TournamentService
import org.springframework.stereotype.Service

/**
 * Turns a Tabletop League match URL into a reviewable draft of a match result.
 *
 * This service **writes nothing**. It scrapes, resolves names to ids, and hands
 * back a [MatchImportPreview]; the admin reviews it in the match wizard and
 * saves through the existing admin endpoint. That keeps `MatchResultPolicy`,
 * `AdminMatchService` and the standings event on exactly one path, so an
 * imported match is validated identically to a hand-typed one and this class
 * never needs to know the result rules.
 *
 * Nothing here throws on an unresolvable name. A missing hero or an
 * out-of-pool board comes back in [MatchImportPreview.unresolved] with the
 * source's own spelling, because the admin is the one who can tell whether it's
 * a catalogue gap, a pool they forgot to populate, or the source site renaming
 * something. Failing the whole import would only make one odd name cost them the
 * other twenty-odd it got right.
 */
@Service
class MatchImportService(
    private val tournamentService: TournamentService,
    private val scraperClient: ScraperClient,
    private val heroRepository: HeroRepository,
    private val gameMapRepository: GameMapRepository,
    private val mapPoolAdminRepository: MapPoolAdminRepository,
    private val matchResultQuery: MatchResultQuery,
) {

    // Deliberately not @Transactional: scrapeMatch below is an outbound HTTP call
    // that can hold a socket open for up to ScraperProperties.timeout (90s), and a
    // transaction would pin a Hikari connection for the same span. Ten concurrent
    // imports — or one hung scraper plus ordinary traffic — would starve the pool
    // for everyone else. The scrape happens first and the resolution reads after
    // are independent lookups with no need of one snapshot, so nothing here wants
    // a transaction at all.
    fun preview(tournamentId: Long, sourceUrl: String): MatchImportPreview {
        tournamentService.requireTournament(tournamentId)

        val trimmedUrl = sourceUrl.trim()
        scraperClient.validateSourceUrl(trimmedUrl)?.let { throw ConflictException(it) }

        val scraped = scraperClient.scrapeMatch(trimmedUrl)
        val sideA = scraped.sideA
        val sideB = scraped.sideB
        if (sideA == null || sideB == null) {
            throw ConflictException(
                "The scraper could not read both sides of that match. " +
                    "It may not be a completed match, or the source site's markup may have changed.",
            )
        }

        val heroes = NameResolver.of(heroRepository.findAll().mapNotNull { h -> h.id?.let { h.name to it } })
        val maps = NameResolver.of(gameMapRepository.findAll().mapNotNull { m -> m.id?.let { m.name to it } })
        val poolMapIds = mapPoolAdminRepository.poolMapIds(tournamentId)

        // Collected as the resolution runs, then de-duplicated: one hero missing
        // from the catalogue can appear as a pick, a game participant and a ban,
        // and the admin needs to be told about it once.
        val unresolved = LinkedHashSet<UnresolvedName>()

        fun resolveHero(name: String?): Long? {
            if (name.isNullOrBlank()) return null
            return heroes.resolve(name) ?: run {
                unresolved += UnresolvedName(
                    kind = UnresolvedKind.HERO,
                    sourceName = name,
                    reason = UnresolvedReason.UNKNOWN_HERO,
                    mapId = null,
                    message = "No hero named \"$name\" exists. Add it under Heroes, then import again.",
                )
                null
            }
        }

        fun resolveMap(name: String?): Long? {
            if (name.isNullOrBlank()) return null
            val mapId = maps.resolve(name)
            if (mapId == null) {
                unresolved += UnresolvedName(
                    kind = UnresolvedKind.MAP,
                    sourceName = name,
                    reason = UnresolvedReason.UNKNOWN_MAP,
                    mapId = null,
                    message = "No board named \"$name\" exists. Add it under Maps, then import again.",
                )
                return null
            }
            if (mapId !in poolMapIds) {
                unresolved += UnresolvedName(
                    kind = UnresolvedKind.MAP,
                    sourceName = name,
                    reason = UnresolvedReason.MAP_NOT_IN_POOL,
                    mapId = mapId,
                    message = "\"$name\" is not in this tournament's board pool, " +
                        "so a game cannot be recorded on it.",
                )
                return null
            }
            return mapId
        }

        val participants = listOf(sideA, sideB).map { side ->
            ImportedParticipant(
                playerLabel = side.playerLabel?.trim()?.ifEmpty { null },
                draftedHeroIds = side.picks.mapNotNull(::resolveHero).distinct(),
            )
        }

        val games = scraped.games.sortedBy { it.gameIndex ?: 0 }.mapIndexed { index, game ->
            ImportedGame(
                // The source's own index is trusted when present, but games must
                // be a dense 1..N run (GAME_NUMBERS_NOT_SEQUENTIAL) — so position
                // in the sorted list is the fallback, not a null that can't be saved.
                gameNumber = game.gameIndex ?: (index + 1),
                mapId = resolveMap(game.mapName),
                mapName = game.mapName,
                participants = listOf(game.sideA, game.sideB).map { gameSide ->
                    ImportedGameParticipant(
                        heroId = resolveHero(gameSide?.heroName),
                        heroName = gameSide?.heroName,
                        healthRemaining = gameSide?.health ?: 0,
                        isWinner = gameSide?.isWinner ?: false,
                    )
                },
            )
        }

        // Both sides' typed bans and the shared pre-ban pool flatten into one
        // list, as `hero_bans` stores them — but the side survives now. The
        // source already groups a typed ban under the side that owned the hero,
        // and `hero_bans.side` finally has somewhere to put it; a pre-ban
        // precedes side assignment and so carries none.
        //
        // The table is still keyed (match_id, hero_id), so the same hero cannot
        // be banned twice in one series even if both sides struck it —
        // de-duplicating here matches DUPLICATE_BAN's view and avoids handing
        // the admin a draft that cannot be saved. That the survivor keeps the
        // first side rather than merging the two is inherent to that key, not a
        // choice made here.
        val bans = buildList {
            for ((side, scrapedSide) in listOf(sideA, sideB).withIndex()) {
                for (ban in scrapedSide.bans) {
                    val banType = parseBanType(ban.banType)
                    add(
                        ImportedBan(
                            heroId = resolveHero(ban.heroName),
                            heroName = ban.heroName,
                            banType = banType,
                            // A source filing a pre-ban under a side is mistaken
                            // about what a pre-ban is; dropping the side keeps
                            // BAN_SIDE_INVALID from firing on the import.
                            side = side.takeUnless { banType == BanType.PRE_BAN },
                        ),
                    )
                }
            }
            for (heroName in scraped.preBans) {
                add(
                    ImportedBan(
                        heroId = resolveHero(heroName),
                        heroName = heroName,
                        banType = BanType.PRE_BAN,
                        side = null,
                    ),
                )
            }
        }.distinctBy { it.heroId ?: it.heroName }

        return MatchImportPreview(
            sourceUrl = trimmedUrl,
            roundName = scraped.roundName,
            seriesFormat = scraped.seriesFormat,
            playedAt = ScrapedTimestamps.parse(scraped.playedAtRaw),
            playedAtRaw = scraped.playedAtRaw,
            alreadyImportedMatchId = matchResultQuery.findIdByExternalLink(tournamentId, trimmedUrl),
            participants = participants,
            games = games,
            bans = bans,
            unresolved = unresolved.toList(),
        )
    }

    /**
     * The scraper already emits this repo's own vocabulary, so this is a lookup
     * rather than a translation. An unrecognised value falls back to `PRE_BAN`:
     * the hero was definitely banned (it came out of the draft card), and
     * `PRE_BAN` is the type that scores neither ban metric, so an unknown label
     * cannot silently pay out points it shouldn't.
     */
    private fun parseBanType(raw: String?): BanType =
        BanType.entries.firstOrNull { it.name.equals(raw?.trim(), ignoreCase = true) } ?: BanType.PRE_BAN
}
