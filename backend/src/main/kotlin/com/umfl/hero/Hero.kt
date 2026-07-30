package com.umfl.hero

import org.springframework.data.annotation.Id
import org.springframework.data.relational.core.mapping.Table

/**
 * A hero's identity — name and artwork, nothing else. Cost is tournament-scoped
 * and lives in `tournament_hero`, not here; see [HeroPoolAdminRepository].
 */
@Table("heroes")
data class Hero(
    @Id val id: Long? = null,
    val name: String,
    val imageUrl: String? = null,
)
