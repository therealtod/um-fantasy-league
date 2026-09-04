//! A tournament's whole match list, held in memory between writes -- a
//! read-through front for [`super::query`] exposing the same two methods the
//! standings path used to call on it directly.
//!
//! The pressure this relieves is a burst, not a trickle. `GET /standings` is
//! public and unauthenticated, and the SSE hub tells up to a few hundred tabs
//! per tournament to refetch the instant an admin records a match -- each of
//! which then pulls *both* the board and the ticker head. Uncached, one admin
//! keystroke is hundreds of simultaneous requests, every one of them running
//! [`super::query::find_by_tournament`]'s six unbounded queries against a
//! ten-connection pool.
//!
//! **Only the fold's input is cached, never the board.** Matches have exactly
//! one writer, [`super::admin_service`], and it announces all three of its
//! writes -- so this key has an invalidation signal that is complete by
//! construction. Coefficients, hero costs, credit grants and rosters have no
//! such signal (AGENTS.md documents retuning a scoring weight as a bare
//! `UPDATE`), so they stay read live on every request. Only a *match* can ever
//! be a write behind here.
//!
//! # Why a version stamp and not a plain evict
//!
//! Eviction alone cannot survive an invalidation that lands mid-load:
//!
//! 1. a reader misses and starts loading;
//! 2. a write commits, and invalidation drops an entry that is not there yet;
//! 3. the reader finishes and stores a list that is already wrong -- and that
//!    nothing will ever invalidate again, because the invalidation it needed
//!    has already been and gone.
//!
//! moka's `try_get_with` is atomic per key, which is exactly what collapses the
//! burst onto one database read; but that atomicity is also why the loader
//! cannot decline to store -- declining would mean "absent", and every blocked
//! caller would then load for itself and the collapse would be gone. So the
//! entry is always stored, carrying the version its load *started* under, and
//! the reader checks it afterwards. Reading the version before the load rather
//! than after is what makes the check err safely: a spurious mismatch costs one
//! extra load, while stamping afterwards would swallow the very invalidation
//! this is here to catch.
//!
//! # Why two invalidations, in two different phases
//!
//! [`MatchResultCache::invalidate`] runs once *inside* the writing transaction
//! and once after that transaction ends, called explicitly from each of
//! [`super::admin_service`]'s write methods. Neither is sufficient alone:
//!
//! - without the second, a reader that loads between the write and the commit
//!   reads pre-write rows, stamps them with the already-bumped version, and
//!   that stale entry then matches forever;
//! - without the first, a reader inside the writing transaction is served the
//!   list as it stood before the write.
//!
//! The second fires on **completion**, not on commit, and the difference is
//! load-bearing rather than cosmetic: a rollback un-writes rows this cache may
//! already have loaded inside that transaction, so it invalidates just as
//! surely as a commit does. The standings SSE hub listens to the same moment
//! and is correctly commit-only, because telling browsers "something changed"
//! about a write that rolled back would be a lie. Two invalidations, two
//! phases, deliberately -- do not unify them.
//!
//! # Bounds
//!
//! Bounded by size and idleness, and deliberately **not** by write age. Unlike
//! [`crate::ratelimit`], whose key space is every IP on the internet, this one
//! is the `tournaments` table -- rows only an admin can create -- so the bounds
//! are memory hygiene rather than a defence against a hostile key space. There
//! is no `expire_after_write`: a TTL cannot add a guarantee the stamp does not
//! already give, and its only effect would be on a *missing* hook, where it
//! converts "this rename never appears" into "this rename appears eventually,
//! depending on when you look" -- strictly harder to notice and reproduce. If a
//! hook is ever missing, the fix is the hook.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dashmap::DashMap;
use moka::future::Cache;
use moka::ops::compute::Op;
use sqlx::PgPool;
use umfl_domain::match_result::MatchResult;

/// Entries, not bytes -- a tournament's match list is the largest thing this
/// application assembles.
const MAX_CACHED_TOURNAMENTS: u64 = 64;

/// Idle eviction, not a TTL: a live tournament is re-read constantly and never
/// ages out.
const IDLE_EVICTION: Duration = Duration::from_secs(30 * 60);

/// See [`MatchResultCache::find_by_tournament`] -- a bound on retries, not a
/// correctness knob.
const MAX_LOAD_ATTEMPTS: usize = 3;

/// A loaded list together with the version its load began under.
///
/// Shared behind an [`Arc`] rather than cloned, both because the lists are read
/// concurrently by every standings request and because `Arc::ptr_eq` is what
/// makes the conditional removal below mean "drop the entry *I* just saw"
/// -- identity, which is far cheaper than structurally comparing a few
/// hundred assembled matches.
struct Stamped {
    version: u64,
    matches: Arc<Vec<MatchResult>>,
}

/// The cache. Cloned into [`crate::state::AppState`], so every clone shares one
/// store -- moka's `Cache` and `DashMap` are both already `Arc`-backed inside.
#[derive(Clone)]
pub struct MatchResultCache {
    cache: Cache<i64, Arc<Stamped>>,
    /// One counter per tournament, bumped by every invalidation and never
    /// removed -- a version that vanished mid-load would take the invalidation
    /// with it. Unbounded is right here for the same reason the bounds above
    /// are hygiene: the key space is a table only an admin writes to, and an
    /// entry is one `AtomicU64`.
    versions: Arc<DashMap<i64, Arc<AtomicU64>>>,
}

impl Default for MatchResultCache {
    fn default() -> Self {
        Self::new()
    }
}

impl MatchResultCache {
    pub fn new() -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(MAX_CACHED_TOURNAMENTS)
                .time_to_idle(IDLE_EVICTION)
                .build(),
            versions: Arc::new(DashMap::new()),
        }
    }

    /// Every recorded match in the tournament, oldest first -- the same
    /// contract as [`super::query::find_by_tournament`] with no round filter.
    ///
    /// The retry is bounded rather than a loop without end: a relentless write
    /// stream should degrade to an uncached read rather than spin.
    pub async fn find_by_tournament(
        &self,
        pool: &PgPool,
        tournament_id: i64,
    ) -> sqlx::Result<Arc<Vec<MatchResult>>> {
        self.get_or_load(tournament_id, || load(pool, tournament_id))
            .await
    }

    /// The version-stamped read-through itself, with the loader passed in.
    ///
    /// This is the seam that lets the unit tests below assert that a burst
    /// collapses onto one load, that a load racing an invalidation is
    /// discarded, and that an unceasing invalidator degrades to a
    /// read-through. None of those are claims about SQL, and none of them
    /// should need Postgres to check.
    ///
    /// A **parameter**, not a trait: this crate allows exactly one trait and
    /// it is `ScraperClient`. Passing a rule's input in is the same move
    /// `umfl-domain` makes everywhere else.
    pub async fn get_or_load<F, Fut>(
        &self,
        tournament_id: i64,
        load: F,
    ) -> sqlx::Result<Arc<Vec<MatchResult>>>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = sqlx::Result<Vec<MatchResult>>>,
    {
        let version = self.version_of(tournament_id);
        for _ in 0..MAX_LOAD_ATTEMPTS {
            // Read before the load, never after -- see the module doc.
            // Registering the key here, ahead of any load for it, is also what
            // lets `invalidate_all`'s sweep catch a load already running.
            let version_before_load = version.load(Ordering::SeqCst);
            let entry = self
                .cache
                .try_get_with(tournament_id, async {
                    load().await.map(|matches| {
                        Arc::new(Stamped {
                            version: version_before_load,
                            matches: Arc::new(matches),
                        })
                    })
                })
                .await
                .map_err(unwrap_load_error)?;

            if entry.version == version.load(Ordering::SeqCst) {
                return Ok(Arc::clone(&entry.matches));
            }
            // Loaded across an invalidation. Value-conditional, so an entry
            // another task has already replaced this one with is never the
            // casualty; every caller blocked on this key sees the same stale
            // value and tries the same removal, and only the first succeeds.
            self.cache
                .entry(tournament_id)
                .and_compute_with(|current| {
                    let stale = Arc::clone(&entry);
                    async move {
                        match current {
                            Some(e) if Arc::ptr_eq(e.value(), &stale) => Op::Remove,
                            _ => Op::Nop,
                        }
                    }
                })
                .await;
        }
        tracing::debug!(
            tournament_id,
            "Tournament was invalidated under every cache load attempt; reading through."
        );
        load().await.map(Arc::new)
    }

    /// The newest `limit` matches after `since_match_id` -- the ticker's page,
    /// and the same contract as [`super::query::find_by_tournament_since`],
    /// served off the cached list instead of a seventh query.
    ///
    /// The derivation is exact rather than approximate, and rests on one fact:
    /// `(played_at, id)` is a total order, since `id` is the primary key and
    /// `played_at` is `not null`. So the descending order the ticker's SQL asks
    /// for is the precise reverse of the ascending order the cached list is
    /// already in, ties included. This reverses **the database's own ordering**
    /// rather than re-sorting -- do not "simplify" it into a sort by key, which
    /// would reintroduce a comparator that could disagree with Postgres about a
    /// timestamp. The iterator is lazy so `take` stops early instead of
    /// filtering a whole tournament to hand back forty rows.
    pub async fn find_by_tournament_since(
        &self,
        pool: &PgPool,
        tournament_id: i64,
        since_match_id: i64,
        limit: usize,
    ) -> sqlx::Result<Vec<MatchResult>> {
        let matches = self.find_by_tournament(pool, tournament_id).await?;
        Ok(slice_since(&matches, since_match_id, limit))
    }

    /// Bumps `tournament_id`'s version, so the next reader reloads.
    ///
    /// Deliberately does **not** evict. It does not need to -- a bumped version
    /// already means no reader will accept the entry, and the first one to look
    /// removes it on its way past. And evicting here would be actively harmful:
    /// dropping the key has to take that key's lock, so calling it while a
    /// reader is mid-load on that key blocks until that reader's query returns.
    /// Since invalidation runs on the admin's own request, that would put an
    /// admin's write behind a stranger's database round trip. An entry nobody
    /// reads again is reclaimed by [`IDLE_EVICTION`] instead.
    pub fn invalidate(&self, tournament_id: i64) {
        self.version_of(tournament_id)
            .fetch_add(1, Ordering::SeqCst);
    }

    /// Bumps every tournament -- for a change that is scoped to none of them,
    /// which is what a hero or board **rename** is.
    ///
    /// Deliberately global rather than targeted. The ids *are* on the cached
    /// objects, so a precise invalidation is possible, but it would mean
    /// walking every cached match of every tournament to save reloading a
    /// handful of lists on an operation that happens when an admin fixes a
    /// catalogue typo.
    ///
    /// The sweep is not atomic and does not need to be: a load already running
    /// registered its key in `versions` before starting, so it is bumped too
    /// and its entry is rejected on publication.
    pub fn invalidate_all(&self) {
        for version in self.versions.iter() {
            version.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Test seam: real callers have no reason to know whether a list was
    /// cached.
    ///
    /// Counts entries *present*, not entries valid -- [`Self::invalidate`]
    /// leaves a superseded entry in place for the next reader to clear out.
    pub async fn cached_tournament_count(&self) -> u64 {
        self.cache.run_pending_tasks().await;
        self.cache.entry_count()
    }

    fn version_of(&self, tournament_id: i64) -> Arc<AtomicU64> {
        Arc::clone(
            self.versions
                .entry(tournament_id)
                .or_insert_with(|| Arc::new(AtomicU64::new(0)))
                .value(),
        )
    }
}

/// The ticker's page, derived from a list the database already ordered
/// ascending -- pure, so `match_cache.rs`'s SQL oracle has something to compare
/// against without a cache in the way.
///
/// See [`MatchResultCache::find_by_tournament_since`] for why reversing is
/// exact and re-sorting would not be.
pub fn slice_since(matches: &[MatchResult], since_match_id: i64, limit: usize) -> Vec<MatchResult> {
    matches
        .iter()
        .rev()
        .filter(|m| m.match_id > since_match_id)
        .take(limit)
        .cloned()
        .collect()
}

/// The six assembly queries, under **one** snapshot.
///
/// Run on a pooled connection in autocommit, each would take its own READ
/// COMMITTED snapshot and a write committing between two of them could tear
/// the assembled list -- a header whose games query then returned nothing,
/// for instance. Running them inside one transaction here closes that gap.
///
/// Opening a transaction here is only safe because the caller no longer holds
/// one. `standings::service` reads this cache **before** it opens its own
/// snapshot precisely so that a miss is not asking the pool for a second
/// connection while sitting on the first; do that and ten concurrent requests
/// deadlock a ten-connection pool. The ordering there and the transaction here
/// are one design, so read that module's `# Why REPEATABLE READ` note before
/// changing either.
async fn load(pool: &PgPool, tournament_id: i64) -> sqlx::Result<Vec<MatchResult>> {
    let mut tx = crate::state::read_snapshot(pool).await?;
    let matches = super::query::find_by_tournament(&mut tx, tournament_id, None).await?;
    tx.commit().await?;
    tracing::debug!(
        tournament_id,
        count = matches.len(),
        "Loaded matches into the match cache."
    );
    Ok(matches)
}

/// moka hands a failed load back to every waiter as a shared error, so the one
/// that actually ran is the only owner and the rest cannot have it moved out.
///
/// Re-classifying rather than unwrapping the `Arc`: what reaches the client is
/// the status, and every query behind this is a `SELECT`, so
/// `ApiError::from_sqlx`'s constraint branch is unreachable and a row-decode
/// failure is the same 500 either way. `PoolTimedOut` is preserved as itself so
/// the log still says the pool was exhausted rather than blaming the schema.
fn unwrap_load_error(err: Arc<sqlx::Error>) -> sqlx::Error {
    tracing::error!(error = %err, "Loading a tournament's matches failed");
    sqlx::Error::Protocol(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// No Postgres -- every claim below is about the caching mechanics, not
    /// about SQL. What genuinely needs a database is in
    /// `tests/it/match_cache.rs`.
    fn a_match(match_id: i64) -> MatchResult {
        MatchResult {
            match_id,
            tournament_id: 1,
            round: 1,
            played_at: chrono::Utc
                .timestamp_opt(1_700_000_000 + match_id, 0)
                .unwrap(),
            external_link: format!("urn:umfl:match:{match_id}"),
            participants: Vec::new(),
            games: Vec::new(),
            bans: Vec::new(),
        }
    }

    /// A loader that counts its calls and answers with whatever `matches` says.
    #[derive(Clone)]
    struct CountingLoader {
        loads: Arc<AtomicU64>,
    }

    impl CountingLoader {
        fn new() -> Self {
            Self {
                loads: Arc::new(AtomicU64::new(0)),
            }
        }

        fn count(&self) -> u64 {
            self.loads.load(Ordering::SeqCst)
        }
    }

    #[tokio::test]
    async fn a_second_read_is_served_from_the_cache() {
        let cache = MatchResultCache::new();
        let loader = CountingLoader::new();

        for _ in 0..2 {
            let matches = cache
                .get_or_load(1, || {
                    let loader = loader.clone();
                    async move {
                        loader.loads.fetch_add(1, Ordering::SeqCst);
                        Ok(vec![a_match(1)])
                    }
                })
                .await
                .unwrap();
            assert_eq!(matches.len(), 1);
        }

        assert_eq!(loader.count(), 1);
        assert_eq!(cache.cached_tournament_count().await, 1);
    }

    #[tokio::test]
    async fn each_tournament_is_cached_separately() {
        let cache = MatchResultCache::new();
        let loader = CountingLoader::new();

        for tournament_id in [1, 2, 1, 2] {
            cache
                .get_or_load(tournament_id, || {
                    let loader = loader.clone();
                    async move {
                        loader.loads.fetch_add(1, Ordering::SeqCst);
                        Ok(vec![a_match(tournament_id)])
                    }
                })
                .await
                .unwrap();
        }

        assert_eq!(loader.count(), 2, "one load each, not one shared");
        assert_eq!(cache.cached_tournament_count().await, 2);
    }

    #[tokio::test]
    async fn invalidate_forces_the_next_read_to_query_again() {
        let cache = MatchResultCache::new();
        let loader = CountingLoader::new();
        let read = async || {
            cache
                .get_or_load(1, || {
                    let loader = loader.clone();
                    async move {
                        loader.loads.fetch_add(1, Ordering::SeqCst);
                        Ok(vec![a_match(1)])
                    }
                })
                .await
                .unwrap()
        };

        read().await;
        cache.invalidate(1);
        read().await;

        assert_eq!(loader.count(), 2);
    }

    /// A hero or board rename is scoped to no tournament, so it drops
    /// everything rather than hunting the ids.
    #[tokio::test]
    async fn invalidate_all_drops_every_tournament() {
        let cache = MatchResultCache::new();
        let loader = CountingLoader::new();
        let read = async |tournament_id: i64| {
            cache
                .get_or_load(tournament_id, || {
                    let loader = loader.clone();
                    async move {
                        loader.loads.fetch_add(1, Ordering::SeqCst);
                        Ok(vec![a_match(tournament_id)])
                    }
                })
                .await
                .unwrap()
        };

        read(1).await;
        read(2).await;
        assert_eq!(loader.count(), 2);

        cache.invalidate_all();
        read(1).await;
        read(2).await;

        assert_eq!(loader.count(), 4);
    }

    /// A match write invalidates only its own tournament.
    #[tokio::test]
    async fn invalidate_is_scoped_to_one_tournament() {
        let cache = MatchResultCache::new();
        let loader = CountingLoader::new();
        let read = async |tournament_id: i64| {
            cache
                .get_or_load(tournament_id, || {
                    let loader = loader.clone();
                    async move {
                        loader.loads.fetch_add(1, Ordering::SeqCst);
                        Ok(vec![a_match(tournament_id)])
                    }
                })
                .await
                .unwrap()
        };

        read(1).await;
        read(2).await;
        cache.invalidate(1);
        read(1).await;
        read(2).await;

        assert_eq!(loader.count(), 3, "tournament 2 should still be cached");
    }

    /// The burst the cache exists for: one match write wakes hundreds of tabs,
    /// each of which asks for the board and the ticker at once. They must
    /// collapse onto a single load rather than each running their own.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_readers_of_a_cold_cache_load_once() {
        let cache = MatchResultCache::new();
        let loader = CountingLoader::new();

        let readers: Vec<_> = (0..32)
            .map(|_| {
                let cache = cache.clone();
                let loader = loader.clone();
                tokio::spawn(async move {
                    cache
                        .get_or_load(1, || {
                            let loader = loader.clone();
                            async move {
                                loader.loads.fetch_add(1, Ordering::SeqCst);
                                // Hold the load open long enough that every
                                // task is genuinely contending, rather than
                                // arriving after the first one finished.
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                Ok(vec![a_match(1)])
                            }
                        })
                        .await
                        .unwrap()
                })
            })
            .collect();

        for reader in readers {
            assert_eq!(reader.await.unwrap().len(), 1);
        }
        assert_eq!(
            loader.count(),
            1,
            "the burst should have collapsed onto one load"
        );
    }

    /// The race the version stamp exists for, with the threads taken out: the
    /// invalidation happens *inside* the load, which is what a write landing
    /// mid-query amounts to, and makes the assertion exact rather than merely
    /// very likely.
    ///
    /// Calling [`MatchResultCache::invalidate`] from within the loader is safe
    /// only because it bumps a counter and never touches the entry map -- the
    /// same property that keeps an admin's write off the back of a reader's
    /// query in production.
    #[tokio::test]
    async fn a_load_invalidated_while_it_runs_is_discarded_and_retried() {
        let cache = MatchResultCache::new();
        let loader = CountingLoader::new();
        let read = async || {
            cache
                .get_or_load(1, || {
                    let loader = loader.clone();
                    let cache = cache.clone();
                    async move {
                        // The first load alone is overtaken by a write; later
                        // ones are clean.
                        let n = loader.loads.fetch_add(1, Ordering::SeqCst) + 1;
                        if n == 1 {
                            cache.invalidate(1);
                        }
                        Ok(vec![a_match(n as i64)])
                    }
                })
                .await
                .unwrap()
        };

        let matches = read().await;
        assert_eq!(matches[0].match_id, 2, "the overtaken load was published");
        assert_eq!(loader.count(), 2);

        // And the discarded entry is not still sitting there for the next
        // reader.
        let again = read().await;
        assert_eq!(again[0].match_id, 2);
        assert_eq!(loader.count(), 2, "the retry's result should now be cached");
    }

    /// A cache that can never settle must degrade to the uncached behaviour it
    /// replaced, rather than spinning. It takes concurrent admin writes to
    /// reach, which is to say it does not happen -- but the bound is what makes
    /// that a fact rather than a hope.
    #[tokio::test]
    async fn an_unceasing_invalidator_degrades_to_a_read_through() {
        let cache = MatchResultCache::new();
        let loader = CountingLoader::new();

        let matches = cache
            .get_or_load(1, || {
                let loader = loader.clone();
                let cache = cache.clone();
                async move {
                    loader.loads.fetch_add(1, Ordering::SeqCst);
                    cache.invalidate(1);
                    Ok(vec![a_match(1)])
                }
            })
            .await
            .unwrap();

        assert_eq!(matches[0].match_id, 1);
        assert_eq!(
            loader.count(),
            MAX_LOAD_ATTEMPTS as u64 + 1,
            "should have given up after the retry budget and read through once"
        );
    }

    /// The ticker slice reverses, filters and truncates -- and reverses **the
    /// database's own ordering**, which is why the fixture here is already in
    /// ascending order.
    #[test]
    fn the_ticker_slice_reverses_filters_and_truncates() {
        let matches: Vec<MatchResult> = (1..=5).map(a_match).collect();

        let ids = |since, limit| -> Vec<i64> {
            slice_since(&matches, since, limit)
                .iter()
                .map(|m| m.match_id)
                .collect()
        };

        assert_eq!(ids(0, 200), [5, 4, 3, 2, 1]);
        assert_eq!(ids(0, 2), [5, 4]);
        assert_eq!(ids(3, 200), [5, 4]);
        assert_eq!(ids(5, 200), Vec::<i64>::new());
        assert_eq!(ids(99, 200), Vec::<i64>::new());
    }
}
