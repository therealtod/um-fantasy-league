package com.umfl.hero

import com.umfl.common.ConflictException
import com.umfl.common.NotFoundException
import com.umfl.tournament.TournamentService
import org.springframework.stereotype.Service
import org.springframework.transaction.annotation.Transactional

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

    private fun requireHero(heroId: Long): Hero =
        heroRepository.findById(heroId).orElseThrow { NotFoundException("No hero with id $heroId") }
}
