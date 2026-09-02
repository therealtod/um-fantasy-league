package com.umfl.hero

import org.springframework.jdbc.core.simple.JdbcClient
import org.springframework.stereotype.Repository
import java.sql.ResultSet

/**
 * Sort options for the Roster Builder grid.
 *
 * The ORDER BY fragments are held here as a whitelist rather than accepting a
 * client-supplied column name — sort keys cannot be parameterised, so an enum is
 * what keeps this query injection-safe. Keep new sorts inside this enum.
 */
enum class HeroSort(internal val orderBy: String) {
    COST("th.cost desc, h.name asc"),
    NAME("h.name asc"),
}

data class HeroFilter(
    val search: String? = null,
    val sort: HeroSort = HeroSort.COST,
)

/** A hero as the Roster Builder sees it: identity plus *this* tournament's price. */
data class HeroView(
    val id: Long,
    val name: String,
    val imageUrl: String?,
    val cost: Int,
)

/**
 * The hero pool of one tournament.
 *
 * Everything here is keyed by `tournamentId` rather than by a season, because
 * cost is per tournament: the same hero is a bargain at one event and a premium
 * pick at the next. A hero absent from `tournament_heroes` is simply not in that
 * tournament's pool, which is what makes `UNKNOWN_HERO` a real check rather
 * than an existence test.
 */
@Repository
class HeroQueryRepository(private val jdbcClient: JdbcClient) {

    fun findByTournament(tournamentId: Long, filter: HeroFilter = HeroFilter()): List<HeroView> {
        val sql = buildString {
            append(SELECT_HERO_VIEW)
            append(" where th.tournament_id = :tournamentId")
            if (!filter.search.isNullOrBlank()) append(" and h.name ilike :search escape '\\'")
            append(" order by ").append(filter.sort.orderBy)
        }

        var spec = jdbcClient.sql(sql).param("tournamentId", tournamentId)
        filter.search?.takeIf { it.isNotBlank() }?.let { spec = spec.param("search", "%${escapeLike(it.trim())}%") }

        return spec.query(::mapHeroView).list()
    }

    fun findByIds(tournamentId: Long, ids: Collection<Long>): List<HeroView> {
        if (ids.isEmpty()) return emptyList()
        return jdbcClient
            .sql("$SELECT_HERO_VIEW where th.tournament_id = :tournamentId and h.id in (:ids) order by h.name")
            .param("tournamentId", tournamentId)
            .param("ids", ids)
            .query(::mapHeroView)
            .list()
    }

    /**
     * Identity for ids already committed to a roster (an `entry_slots`), priced
     * by this tournament's *current* pool — cost 0 if the hero has since left
     * it. Unlike [findByIds], this never drops an id: a locked or in-progress
     * roster must keep reporting every slot it holds, the same way
     * `StandingsQuery` does, rather than silently shrinking the roster and its
     * spend.
     */
    fun findRosterHeroes(tournamentId: Long, ids: Collection<Long>): List<HeroView> {
        if (ids.isEmpty()) return emptyList()
        return jdbcClient
            .sql(
                """
                select h.id, h.name, h.image_url, coalesce(th.cost, 0) as cost
                from heroes h
                left join tournament_heroes th
                    on th.tournament_id = :tournamentId and th.hero_id = h.id
                where h.id in (:ids)
                """
            )
            .param("tournamentId", tournamentId)
            .param("ids", ids)
            .query(::mapHeroView)
            .list()
    }

    private companion object {
        const val SELECT_HERO_VIEW = """
            select h.id, h.name, h.image_url, th.cost
            from tournament_heroes th
            join heroes h on h.id = th.hero_id
        """
    }
}

/** Escapes ILIKE metacharacters so a search for `_` or `%` matches those characters literally. */
private fun escapeLike(raw: String) = raw.replace("\\", "\\\\").replace("%", "\\%").replace("_", "\\_")

private fun mapHeroView(rs: ResultSet, @Suppress("UNUSED_PARAMETER") rowNum: Int) =
    HeroView(
        id = rs.getLong("id"),
        name = rs.getString("name"),
        imageUrl = rs.getString("image_url"),
        cost = rs.getInt("cost"),
    )
