package com.umfl.hero

import com.umfl.common.ConflictException
import com.umfl.common.NotFoundException
import com.umfl.tournament.TournamentService
import org.springframework.stereotype.Service
import org.springframework.transaction.annotation.Transactional

data class HeroPoolEntryInput(val heroId: Long, val cost: Int)

@Service
class AdminHeroService(
    private val heroRepository: HeroRepository,
    private val heroPoolAdminRepository: HeroPoolAdminRepository,
    private val heroQueryRepository: HeroQueryRepository,
    private val tournamentService: TournamentService,
) {

    @Transactional
    fun create(name: String, imageUrl: String?): Hero {
        if (heroRepository.findByName(name) != null) {
            throw ConflictException("A hero named '$name' already exists.")
        }
        return heroRepository.save(Hero(name = name, imageUrl = imageUrl))
    }

    @Transactional
    fun update(heroId: Long, name: String, imageUrl: String?): Hero {
        val existing = requireHero(heroId)
        val collision = heroRepository.findByName(name)
        if (collision != null && collision.id != heroId) {
            throw ConflictException("A hero named '$name' already exists.")
        }
        return heroRepository.save(existing.copy(name = name, imageUrl = imageUrl))
    }

    /** Adds [heroId] to [tournamentId]'s pool at [cost], or re-prices it if already present. */
    @Transactional
    fun setPoolCost(tournamentId: Long, heroId: Long, cost: Int): HeroView {
        tournamentService.requireTournament(tournamentId)
        requireHero(heroId)
        heroPoolAdminRepository.upsertCost(tournamentId, heroId, cost)
        return heroQueryRepository.findByIds(tournamentId, listOf(heroId)).firstOrNull()
            ?: error("Just-priced hero not found")
    }

    /**
     * Adds/re-prices many heroes into [tournamentId]'s pool in one round trip —
     * the batch counterpart to [setPoolCost] for admin UIs that stage several
     * picks before submitting once. A heroId repeated within [entries] behaves
     * like calling [setPoolCost] for it twice: last cost wins.
     */
    @Transactional
    fun addBatchToPool(tournamentId: Long, entries: List<HeroPoolEntryInput>): List<HeroView> {
        tournamentService.requireTournament(tournamentId)
        requireHeroes(entries.map { it.heroId })
        val deduped = entries.associateBy { it.heroId }.values.toList()
        heroPoolAdminRepository.upsertCosts(tournamentId, deduped)
        return heroQueryRepository.findByIds(tournamentId, deduped.map { it.heroId })
    }

    /** A tournament's hero pool, priced — see `AdminHeroController.listPool` for why this is its own endpoint. */
    fun pool(tournamentId: Long): List<HeroView> {
        tournamentService.requireTournament(tournamentId)
        return heroQueryRepository.findByTournament(tournamentId)
    }

    /** Removes [heroId] from [tournamentId]'s pool. See [HeroPoolAdminRepository.removeFromPool]'s doc for what this does to rosters that already hold it. */
    @Transactional
    fun removeFromPool(tournamentId: Long, heroId: Long) {
        tournamentService.requireTournament(tournamentId)
        requireHero(heroId)
        if (!heroPoolAdminRepository.removeFromPool(tournamentId, heroId)) {
            throw NotFoundException("Hero $heroId is not in tournament $tournamentId's pool")
        }
    }

    private fun requireHero(heroId: Long): Hero =
        heroRepository.findById(heroId).orElseThrow { NotFoundException("No hero with id $heroId") }

    private fun requireHeroes(heroIds: List<Long>) {
        val distinct = heroIds.toSet()
        val found = heroRepository.findAllById(distinct).mapNotNull { it.id }.toSet()
        val missing = distinct - found
        if (missing.isNotEmpty()) {
            throw NotFoundException("No hero(es) with id(s) ${missing.sorted().joinToString()}")
        }
    }
}
