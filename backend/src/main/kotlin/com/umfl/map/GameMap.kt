package com.umfl.map

import org.springframework.data.annotation.Id
import org.springframework.data.relational.core.mapping.Table

/** A board a match can be played on. Legality per tournament lives in `tournament_maps`. */
@Table("game_maps")
data class GameMap(
    @Id val id: Long? = null,
    val name: String,
)
