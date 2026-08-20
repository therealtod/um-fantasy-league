package com.umfl.matchimport

/**
 * Matches a source site's hero and board names onto this league's own rows.
 *
 * Pure, with no Spring or persistence dependency, like [com.umfl.tournament.RosterPolicy]
 * and [com.umfl.match.MatchResultPolicy] — the caller loads the catalogue and
 * hands it over.
 *
 * **Exact match after normalisation, and nothing more.** There is deliberately
 * no fuzzy or nearest-match fallback: the cost of a wrong guess here is a match
 * silently recorded against the wrong hero, which then scores points for the
 * wrong manager and is invisible until someone reads the standings and doubts
 * them. An unmatched name is reported to the admin, who can see both the source
 * name and the catalogue. A near miss that resolves to the wrong row cannot be
 * seen at all.
 *
 * Normalisation covers the drift actually observed between the source site and
 * `V2__reference_data.sql` — case, stray whitespace, the `.` in `Dr. Ellie
 * Sattler`, and `&` versus `and` in `Jekyll & Hyde`. It is not a fuzzy matcher
 * and should not become one; a genuinely differently-named hero is a catalogue
 * question, not a string-matching one.
 *
 * [ALIASES] is the one deliberate exception, and it is narrow on purpose: a
 * curated, hand-verified list of short/alternate forms tabletopleague.com is
 * known to use in place of a catalogue name — e.g. "Dr. Sattler" for "Dr.
 * Ellie Sattler" — as opposed to [normalise]'s mechanical rules, which apply
 * uniformly to every name. An entry still resolves a source name straight to
 * a hero id, so it carries the same wrong-guess risk the class's docstring
 * warns about; add one only once the source is confirmed to actually emit
 * that string, never speculatively or by similarity.
 */
class NameResolver private constructor(private val byNormalisedName: Map<String, Long>) {

    fun resolve(sourceName: String?): Long? {
        val normalised = sourceName?.let { normalise(it) } ?: return null
        return byNormalisedName[normalised] ?: ALIASES[normalised]?.let { byNormalisedName[it] }
    }

    companion object {
        /**
         * Builds a resolver over `name to id` pairs. A collision after
         * normalisation keeps the first entry: two catalogue rows that
         * normalise together are a data problem, and quietly preferring
         * either one is better than failing every import until it's fixed.
         */
        fun of(entries: Iterable<Pair<String, Long>>): NameResolver {
            val index = LinkedHashMap<String, Long>()
            for ((name, id) in entries) {
                index.putIfAbsent(normalise(name), id)
            }
            return NameResolver(index)
        }

        fun normalise(raw: String): String =
            raw.trim()
                .lowercase()
                .replace("&", " and ")
                .replace(".", " ")
                .replace(WHITESPACE, " ")
                .trim()

        private val WHITESPACE = Regex("""\s+""")

        /**
         * Normalised source alias → normalised catalogue name. See the class
         * docstring for what belongs here. Both sides are pre-normalised so a
         * lookup is a plain map hit rather than a second pass through
         * [normalise].
         */
        private val ALIASES: Map<String, String> = mapOf(
            normalise("Dr. Sattler") to normalise("Dr. Ellie Sattler"),
        )
    }
}
