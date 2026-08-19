package com.umfl.matchimport

import org.junit.jupiter.api.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class NameResolverTest {

    private val resolver = NameResolver.of(
        listOf(
            "Alice" to 1L,
            "Little Red Riding Hood" to 2L,
            "Dr. Ellie Sattler" to 3L,
            "Jekyll & Hyde" to 4L,
            "T. Rex" to 5L,
        )
    )

    @Test
    fun `resolves an exact name`() {
        assertEquals(1L, resolver.resolve("Alice"))
        assertEquals(2L, resolver.resolve("Little Red Riding Hood"))
    }

    @Test
    fun `ignores case and surrounding whitespace`() {
        assertEquals(1L, resolver.resolve("  alice "))
        assertEquals(2L, resolver.resolve("LITTLE RED RIDING HOOD"))
    }

    @Test
    fun `collapses internal whitespace`() {
        assertEquals(2L, resolver.resolve("Little  Red   Riding Hood"))
    }

    /** The source site writing "Dr Ellie Sattler" is the same hero as the catalogue's "Dr. Ellie Sattler". */
    @Test
    fun `treats a dropped period as the same name`() {
        assertEquals(3L, resolver.resolve("Dr Ellie Sattler"))
        assertEquals(5L, resolver.resolve("T Rex"))
        assertEquals(5L, resolver.resolve("T.Rex"))
    }

    @Test
    fun `folds ampersand and the word and together`() {
        assertEquals(4L, resolver.resolve("Jekyll and Hyde"))
        assertEquals(4L, resolver.resolve("Jekyll&Hyde"))
    }

    @Test
    fun `returns null for an unknown name rather than guessing`() {
        assertNull(resolver.resolve("Alicia"))
        assertNull(resolver.resolve("Red Riding Hood"))
        assertNull(resolver.resolve("Little Red"))
    }

    @Test
    fun `returns null for null or blank`() {
        assertNull(resolver.resolve(null))
        assertNull(resolver.resolve("   "))
    }

    /**
     * Two catalogue rows that normalise together are a data problem, not an
     * import problem — keeping the first entry means every *other* name in the
     * match still resolves instead of the whole import dying on one collision.
     */
    @Test
    fun `keeps the first entry when two names normalise together`() {
        val colliding = NameResolver.of(listOf("The Genie" to 10L, "the  genie" to 11L))
        assertEquals(10L, colliding.resolve("The Genie"))
    }
}
