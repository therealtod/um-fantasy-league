package com.umfl.standings

/** Published after a match write commits; tells the SSE hub who to notify. */
data class StandingsUpdateEvent(val tournamentId: Long)
