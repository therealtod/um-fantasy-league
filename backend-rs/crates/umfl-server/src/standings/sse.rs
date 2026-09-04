//! One SSE connection per browser tab watching a tournament's standings, and a
//! bare "something changed" push after a match write **commits**.
//!
//! The payload is deliberately just the tournament id: the client already knows
//! how to pull fresh data from `/standings` and `/matches`, so the stream is a
//! "poll now" signal rather than a second copy of the board over the wire.
//!
//! This is the one place `AGENTS.md`'s "no background workers" has an
//! exception, and it is a narrow one. The keep-alive is not a scheduled task
//! at all: axum attaches it to each response stream, so there is no dedicated
//! keep-alive thread and no dispatch pool to park a slow client's write on --
//! a stalled client's send backs up in its own task and in its own broadcast
//! receiver. The subscriber caps below stay plain constants, per the `umfl.*`
//! invariant: how many tabs a Tokio runtime holds open is neither domain data
//! nor deployment topology.
//!
//! # The one behaviour to keep straight
//!
//! [`StandingsSseHub::notify`] is called **after the commit**, and only then --
//! unlike [`crate::r#match::MatchResultCache`], which is invalidated both inside
//! the writing transaction and again on *completion*. Telling browsers
//! "something changed" about a write that rolled back would be a lie, whereas a
//! rollback un-writes rows a cache may already hold and so invalidates just as
//! surely as a commit does. Two signals, two phases, deliberately -- see
//! `match::admin_service`, where all three calls sit next to each other.

use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::StreamExt;
use indexmap::IndexMap;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use umfl_domain::DomainError;

/// Held open through idle-timeout proxies and browsers.
pub const KEEP_ALIVE: Duration = Duration::from_secs(20);

pub const MAX_SUBSCRIBERS_PER_TOURNAMENT: usize = 200;
pub const MAX_TOTAL_SUBSCRIBERS: usize = 500;

/// A finite backstop, not the normal cleanup path -- the keep-alive and the
/// client's own capped-backoff reconnect already handle the common cases. This
/// bounds the worst case instead of leaving a leaked stream open forever.
pub const STREAM_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Rendered to the client verbatim; the frontend shows `ApiError` messages as
/// they arrive, so this string is wire contract.
const AT_CAPACITY: &str = "The standings stream is at capacity; please retry in a moment.";

/// Every message on this channel is the same bare signal, so a slow subscriber
/// falling behind loses nothing a single later "update" does not carry. The
/// buffer only has to absorb a burst of writes between two polls of one task.
const CHANNEL_CAPACITY: usize = 16;

/// The per-tournament subscriber registry.
///
/// Cloneable and shared through [`crate::state::AppState`] -- a per-request hub
/// would have nobody to notify, the same reason the rate limiter and the match
/// cache are shared.
#[derive(Clone, Default)]
pub struct StandingsSseHub {
    registry: Arc<Mutex<Registry>>,
}

#[derive(Default)]
struct Registry {
    /// `IndexMap` over `HashMap`. Nothing here is serialised, but iteration
    /// order being stable makes a capacity test reproducible.
    channels: IndexMap<i64, Channel>,
    total: usize,
}

struct Channel {
    sender: broadcast::Sender<i64>,
    /// Tracked here rather than read off `Sender::receiver_count`, because the
    /// cap has to be checked and the slot taken under one lock. A receiver
    /// count moves on its own as streams drop, which would make the check and
    /// the reservation two different numbers.
    subscribers: usize,
}

impl StandingsSseHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// A live stream for one tab, or a 503 when the caps are reached.
    ///
    /// The subscription is released when the returned stream is dropped,
    /// which covers every release path at once: axum drops the body when the
    /// client disconnects, when the send fails, and when the hour-long
    /// [`STREAM_TIMEOUT`] ends the stream. `Drop` runs once, so a
    /// double-decrement cannot happen here.
    /// Returned as an already-rendered [`Response`] rather than as an
    /// `Sse<S>`: `keep_alive` wraps the stream in a type axum does not export,
    /// so the alternative is a signature naming a private type or a hub that
    /// cannot attach its own heartbeat. The body owns the subscription either
    /// way, which is all the release path needs.
    pub fn subscribe(&self, tournament_id: i64) -> Result<Response, DomainError> {
        let (receiver, subscription) = self.register(tournament_id)?;

        let events = futures::stream::unfold(
            (receiver, subscription),
            move |(mut receiver, subscription)| async move {
                match receiver.recv().await {
                    // The item type is annotated once here: nothing downstream
                    // names it now that `subscribe` hands back a `Response`.
                    Ok(id) => Some((
                        Ok::<Event, Infallible>(update_event(id)),
                        (receiver, subscription),
                    )),
                    // Dropped signals are all the same signal: one "update" now
                    // says everything the missed ones would have. Coalescing is
                    // correct precisely because the payload carries no state.
                    Err(RecvError::Lagged(_)) => {
                        Some((Ok(update_event(tournament_id)), (receiver, subscription)))
                    }
                    Err(RecvError::Closed) => None,
                }
            },
        );

        // `take_until` rather than a per-message timeout: this caps the
        // *stream's* life, not the gap between events, and an idle stream is
        // the normal state of this endpoint.
        let events = events.take_until(tokio::time::sleep(STREAM_TIMEOUT));

        let sse = Sse::new(events.boxed()).keep_alive(
            KeepAlive::new()
                .interval(KEEP_ALIVE)
                // `:keep-alive` on the wire, as `SseEmitter.event().comment()`
                // wrote it -- axum's default comment is empty.
                .text("keep-alive"),
        );
        Ok(sse.into_response())
    }

    /// Pushes the signal to every tab watching this tournament.
    ///
    /// A tournament nobody is watching is a no-op, and so is a send with no
    /// receivers left -- both are ordinary, not errors.
    pub fn notify(&self, tournament_id: i64) {
        let Ok(registry) = self.registry.lock() else {
            return;
        };
        if let Some(channel) = registry.channels.get(&tournament_id) {
            let _ = channel.sender.send(tournament_id);
        }
    }

    /// Test seam: real callers have no reason to know how many tabs are
    /// watching.
    pub fn subscriber_count(&self, tournament_id: i64) -> usize {
        self.registry
            .lock()
            .map(|registry| {
                registry
                    .channels
                    .get(&tournament_id)
                    .map_or(0, |channel| channel.subscribers)
            })
            .unwrap_or(0)
    }

    /// Test seam: same rationale as [`Self::subscriber_count`].
    pub fn total_subscriber_count(&self) -> usize {
        self.registry.lock().map(|r| r.total).unwrap_or(0)
    }

    /// Checks both caps and takes the slot under **one** lock. Split across
    /// two locks, two concurrent subscribes could both see room.
    fn register(
        &self,
        tournament_id: i64,
    ) -> Result<(broadcast::Receiver<i64>, Subscription), DomainError> {
        let mut registry = self
            .registry
            .lock()
            .map_err(|_| DomainError::service_unavailable(AT_CAPACITY))?;

        let existing = registry
            .channels
            .get(&tournament_id)
            .map_or(0, |channel| channel.subscribers);
        if registry.total >= MAX_TOTAL_SUBSCRIBERS || existing >= MAX_SUBSCRIBERS_PER_TOURNAMENT {
            return Err(DomainError::service_unavailable(AT_CAPACITY));
        }

        let channel = registry
            .channels
            .entry(tournament_id)
            .or_insert_with(|| Channel {
                sender: broadcast::channel(CHANNEL_CAPACITY).0,
                subscribers: 0,
            });
        let receiver = channel.sender.subscribe();
        channel.subscribers += 1;
        registry.total += 1;

        Ok((
            receiver,
            Subscription {
                registry: Arc::clone(&self.registry),
                tournament_id,
            },
        ))
    }
}

fn update_event(tournament_id: i64) -> Event {
    Event::default()
        .event("update")
        .data(tournament_id.to_string())
}

/// The single release path.
///
/// It also prunes the tournament's entry once the last watcher leaves, so the
/// key set does not grow monotonically with every tournament ever watched --
/// the one unbounded structure in an otherwise carefully bounded type.
struct Subscription {
    registry: Arc<Mutex<Registry>>,
    tournament_id: i64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let Ok(mut registry) = self.registry.lock() else {
            return;
        };
        registry.total = registry.total.saturating_sub(1);
        let emptied = match registry.channels.get_mut(&self.tournament_id) {
            Some(channel) => {
                channel.subscribers = channel.subscribers.saturating_sub(1);
                channel.subscribers == 0
            }
            None => false,
        };
        if emptied {
            registry.channels.shift_remove(&self.tournament_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The subscription lives in the returned stream, so a test that drops it
    /// immediately would be measuring `Drop`, not the registry.
    fn subscribe(hub: &StandingsSseHub, tournament_id: i64) -> Response {
        hub.subscribe(tournament_id).expect("under the caps")
    }

    #[tokio::test]
    async fn subscribing_registers_exactly_one_watcher_for_that_tournament() {
        let hub = StandingsSseHub::new();
        let _stream = subscribe(&hub, 7);

        assert_eq!(hub.subscriber_count(7), 1);
    }

    #[tokio::test]
    async fn subscriptions_for_different_tournaments_are_kept_separate() {
        let hub = StandingsSseHub::new();
        let _a = subscribe(&hub, 7);
        let _b = subscribe(&hub, 7);
        let _c = subscribe(&hub, 99);

        assert_eq!(hub.subscriber_count(7), 2);
        assert_eq!(hub.subscriber_count(99), 1);
    }

    #[tokio::test]
    async fn an_update_for_a_tournament_with_no_subscribers_is_a_no_op() {
        let hub = StandingsSseHub::new();
        hub.notify(404);

        assert_eq!(hub.subscriber_count(404), 0);
    }

    #[tokio::test]
    async fn an_update_for_a_subscribed_tournament_does_not_throw() {
        let hub = StandingsSseHub::new();
        let _stream = subscribe(&hub, 7);

        hub.notify(7);

        assert_eq!(hub.subscriber_count(7), 1);
    }

    #[tokio::test]
    async fn subscribing_past_the_per_tournament_cap_is_refused_and_registers_nothing() {
        let hub = StandingsSseHub::new();
        let _held: Vec<_> = (0..MAX_SUBSCRIBERS_PER_TOURNAMENT)
            .map(|_| subscribe(&hub, 7))
            .collect();

        let refused = hub.subscribe(7).expect_err("the cap is reached");

        assert!(matches!(refused, DomainError::ServiceUnavailable(ref m) if m == AT_CAPACITY));
        assert_eq!(hub.subscriber_count(7), MAX_SUBSCRIBERS_PER_TOURNAMENT);
    }

    #[tokio::test]
    async fn the_per_tournament_cap_does_not_block_a_different_tournament() {
        let hub = StandingsSseHub::new();
        let _held: Vec<_> = (0..MAX_SUBSCRIBERS_PER_TOURNAMENT)
            .map(|_| subscribe(&hub, 7))
            .collect();
        hub.subscribe(7).expect_err("the cap is reached");

        let _other = subscribe(&hub, 99);

        assert_eq!(hub.subscriber_count(99), 1);
    }

    #[tokio::test]
    async fn subscribing_past_the_total_cap_is_refused_even_for_a_fresh_tournament() {
        let hub = StandingsSseHub::new();
        let mut held = Vec::new();
        let mut tournament_id = 1;
        while hub.total_subscriber_count() < MAX_TOTAL_SUBSCRIBERS {
            let remaining = MAX_TOTAL_SUBSCRIBERS - hub.total_subscriber_count();
            for _ in 0..remaining.min(MAX_SUBSCRIBERS_PER_TOURNAMENT) {
                held.push(subscribe(&hub, tournament_id));
            }
            tournament_id += 1;
        }

        hub.subscribe(tournament_id)
            .expect_err("the total cap is reached, well under this tournament's own");
    }

    /// The stream *is* the subscription, so dropping it is directly
    /// observable here.
    #[tokio::test]
    async fn dropping_a_stream_releases_its_slot_and_prunes_the_tournament() {
        let hub = StandingsSseHub::new();
        let stream = subscribe(&hub, 7);
        assert_eq!(hub.total_subscriber_count(), 1);

        drop(stream);

        assert_eq!(hub.subscriber_count(7), 0);
        assert_eq!(hub.total_subscriber_count(), 0);
        assert!(
            hub.registry.lock().unwrap().channels.is_empty(),
            "a tournament nobody watches must not sit in the map forever"
        );
    }
}
