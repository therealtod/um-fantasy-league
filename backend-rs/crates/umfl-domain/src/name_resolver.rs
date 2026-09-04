//! Matches a source site's hero and board names onto this league's own rows.
//!
//! Pure, like the policies -- the caller loads the catalogue and hands it over.
//!
//! **Exact match after normalisation, and nothing more.** There is deliberately
//! no fuzzy or nearest-match fallback: the cost of a wrong guess here is a match
//! silently recorded against the wrong hero, which then scores points for the
//! wrong manager and is invisible until someone reads the standings and doubts
//! them. An unmatched name is reported to the admin, who can see both the source
//! name and the catalogue. A near miss that resolves to the wrong row cannot be
//! seen at all.
//!
//! [`normalise`] covers the drift actually observed between the source site and
//! `V2__reference_data.sql` -- case, stray whitespace, the `.` in `Dr. Ellie
//! Sattler`, and `&` versus `and` in `Jekyll & Hyde`. It is not a fuzzy matcher
//! and should not become one; a genuinely differently-named hero is a catalogue
//! question, not a string-matching one.

use indexmap::IndexMap;

/// Normalised source alias -> normalised catalogue name.
///
/// The one deliberate exception to "exact match after normalisation", and it is
/// narrow on purpose: a curated, hand-verified list of short forms
/// tabletopleague.com is known to use in place of a catalogue name, as opposed
/// to [`normalise`]'s mechanical rules, which apply uniformly to every name. An
/// entry still resolves a source name straight to a hero id, so it carries the
/// same wrong-guess risk the module docstring warns about: add one only once the
/// source is confirmed to actually emit that string, never speculatively or by
/// similarity.
///
/// Both sides are written pre-normalised -- `dr sattler`, not `Dr. Sattler` --
/// so a lookup is a plain map hit rather than a second pass through
/// [`normalise`]. [`ALIASES_ARE_PRE_NORMALISED`] is the test that keeps them so.
const ALIASES: &[(&str, &str)] = &[("dr sattler", "dr ellie sattler")];

pub struct NameResolver {
    by_normalised_name: IndexMap<String, i64>,
}

impl NameResolver {
    /// Builds a resolver over `(name, id)` pairs.
    ///
    /// A collision after normalisation keeps the **first** entry: two catalogue
    /// rows that normalise together are a data problem, and quietly preferring
    /// either one is better than failing every import until it is fixed.
    pub fn new(entries: impl IntoIterator<Item = (String, i64)>) -> Self {
        let mut by_normalised_name = IndexMap::new();
        for (name, id) in entries {
            by_normalised_name.entry(normalise(&name)).or_insert(id);
        }
        Self { by_normalised_name }
    }

    pub fn resolve(&self, source_name: Option<&str>) -> Option<i64> {
        let normalised = normalise(source_name?);
        self.by_normalised_name
            .get(&normalised)
            .copied()
            .or_else(|| {
                let alias = ALIASES
                    .iter()
                    .find(|(from, _)| *from == normalised)
                    .map(|(_, to)| *to)?;
                self.by_normalised_name.get(alias).copied()
            })
    }
}

/// Case, padding, internal whitespace, the `.` in an abbreviated name, and `&`
/// against the word `and`. Nothing else -- see the module docstring.
pub fn normalise(raw: &str) -> String {
    let lowered = raw
        .trim()
        .to_lowercase()
        .replace('&', " and ")
        .replace('.', " ");
    // Collapses runs of whitespace without pulling in a regex engine for it.
    lowered.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver() -> NameResolver {
        NameResolver::new(
            [
                ("Alice", 1),
                ("Little Red Riding Hood", 2),
                ("Dr. Ellie Sattler", 3),
                ("Jekyll & Hyde", 4),
                ("T. Rex", 5),
            ]
            .into_iter()
            .map(|(name, id)| (name.to_string(), id)),
        )
    }

    #[test]
    fn resolves_an_exact_name() {
        assert_eq!(resolver().resolve(Some("Alice")), Some(1));
        assert_eq!(resolver().resolve(Some("Little Red Riding Hood")), Some(2));
    }

    #[test]
    fn ignores_case_and_surrounding_whitespace() {
        assert_eq!(resolver().resolve(Some("  alice ")), Some(1));
        assert_eq!(resolver().resolve(Some("LITTLE RED RIDING HOOD")), Some(2));
    }

    #[test]
    fn collapses_internal_whitespace() {
        assert_eq!(
            resolver().resolve(Some("Little  Red   Riding Hood")),
            Some(2)
        );
    }

    /// The source site writing "Dr Ellie Sattler" is the same hero as the
    /// catalogue's "Dr. Ellie Sattler".
    #[test]
    fn treats_a_dropped_period_as_the_same_name() {
        assert_eq!(resolver().resolve(Some("Dr Ellie Sattler")), Some(3));
        assert_eq!(resolver().resolve(Some("T Rex")), Some(5));
        assert_eq!(resolver().resolve(Some("T.Rex")), Some(5));
    }

    #[test]
    fn folds_ampersand_and_the_word_and_together() {
        assert_eq!(resolver().resolve(Some("Jekyll and Hyde")), Some(4));
        assert_eq!(resolver().resolve(Some("Jekyll&Hyde")), Some(4));
    }

    /// The source site's short form for the hero is a curated alias, not a
    /// fuzzy match.
    #[test]
    fn resolves_a_known_source_alias() {
        assert_eq!(resolver().resolve(Some("Dr. Sattler")), Some(3));
        assert_eq!(resolver().resolve(Some("  dr sattler  ")), Some(3));
    }

    #[test]
    fn returns_none_for_an_unknown_name_rather_than_guessing() {
        assert_eq!(resolver().resolve(Some("Alicia")), None);
        assert_eq!(resolver().resolve(Some("Red Riding Hood")), None);
        assert_eq!(resolver().resolve(Some("Little Red")), None);
    }

    #[test]
    fn returns_none_for_absent_or_blank() {
        assert_eq!(resolver().resolve(None), None);
        assert_eq!(resolver().resolve(Some("   ")), None);
    }

    /// Two catalogue rows that normalise together are a data problem, not an
    /// import problem -- keeping the first entry means every *other* name in
    /// the match still resolves instead of the whole import dying on one
    /// collision.
    #[test]
    fn keeps_the_first_entry_when_two_names_normalise_together() {
        let colliding = NameResolver::new([
            ("The Genie".to_string(), 10),
            ("the  genie".to_string(), 11),
        ]);

        assert_eq!(colliding.resolve(Some("The Genie")), Some(10));
    }

    /// `ALIASES` is written already-normalised so a lookup is one map hit
    /// rather than a second pass through `normalise`. That only stays correct
    /// if the literals really are what `normalise` produces.
    #[test]
    #[allow(non_snake_case)]
    fn ALIASES_ARE_PRE_NORMALISED() {
        for (from, to) in ALIASES {
            assert_eq!(&normalise(from), from);
            assert_eq!(&normalise(to), to);
        }
    }
}
