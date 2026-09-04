//! IP-keyed token bucket in front of every `/api` route.
//!
//! Installed ahead of the authentication layer in every profile, which is the
//! point of `addFilterBefore(rateLimitFilter, BearerTokenAuthenticationFilter)`:
//! a flood should pay neither a JWT verification nor the dev manager lookup.
//!
//! Keyed on the address [`RateLimiter::client_ip`] resolves -- normally the
//! peer, because a forwarded-for header from a peer that could itself be the
//! flooder is worthless. `X-Forwarded-For` is read only when the peer falls
//! inside a trusted range, because otherwise a proxied deployment puts the
//! whole internet in one bucket.
//!
//! The tradeoff the direct-exposure design accepted still stands one hop out:
//! traffic arriving through the Cloudflare Worker shares a bucket per
//! Cloudflare edge IP rather than per visitor.
//!
//! The key space is "every IP that touches `/api/`", which is unbounded, so the
//! store must be bounded and self-evicting -- a plain map would let a port scan
//! grow it for the process' lifetime. `moka` is Caffeine's counterpart here and
//! is configured the same way: `time_to_idle` of two refill periods (a bucket
//! quiet that long would have fully refilled anyway, so nothing is lost by
//! starting over) and a hard cap of `max_tracked_ips`, least-recently-used
//! first.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use ipnet::IpNet;
use moka::future::Cache;
use tokio::sync::Mutex;

use crate::config::RateLimitConfig;
use crate::http::problem::ProblemDetail;
use crate::state::AppState;

const X_FORWARDED_FOR: &str = "X-Forwarded-For";

/// The key charged when the peer address is unavailable.
///
/// An axum request only has a peer address if the server was built with
/// `into_make_service_with_connect_info`, which `main` does and an in-process
/// test does not. Sharing one bucket is the safe direction to fail -- it
/// throttles rather than exempts.
const UNKNOWN_PEER: &str = "unknown";

/// The `Retry-After` a zero-refill-rate bucket reports instead of panicking.
/// Only reachable when `RateLimiter::new` is built with `capacity: 0`, which
/// `config::into_config` already refuses at startup -- see the comment at the
/// `try_from_secs_f64` call site in [`RateLimiter::try_consume`]. One day is
/// arbitrary but deliberately finite and small enough not to itself become a
/// header a client or proxy chokes on.
const UNREACHABLE_CAPACITY_RETRY_AFTER: Duration = Duration::from_secs(86_400);

#[derive(Debug)]
struct BucketState {
    tokens: f64,
    last_refill: Instant,
}

/// Bucket4j's `refillGreedy`: the bandwidth trickles back continuously at
/// `capacity / refill_period`, rather than arriving all at once at the end of
/// each period.
#[derive(Clone)]
pub struct RateLimiter {
    buckets: Cache<String, Arc<Mutex<BucketState>>>,
    capacity: f64,
    /// Tokens per second.
    refill_rate: f64,
    trusted_proxies: Arc<Vec<IpNet>>,
}

impl RateLimiter {
    pub fn new(config: &RateLimitConfig) -> Self {
        let trusted_proxies = config
            .trusted_proxies
            .iter()
            .filter_map(|cidr| match cidr.parse::<IpNet>() {
                Ok(net) => Some(net),
                Err(err) => {
                    tracing::warn!(cidr, error = %err, "Ignoring an unparseable trusted-proxy range");
                    None
                }
            })
            .collect();

        let refill_period = config.refill_period.max(Duration::from_nanos(1));
        Self {
            buckets: Cache::builder()
                .max_capacity(config.max_tracked_ips)
                .time_to_idle(refill_period * 2)
                .build(),
            capacity: config.capacity as f64,
            refill_rate: config.capacity as f64 / refill_period.as_secs_f64(),
            trusted_proxies: Arc::new(trusted_proxies),
        }
    }

    /// `Ok(())` if a token was available, `Err(wait)` with how long until one
    /// will be.
    async fn try_consume(&self, key: &str) -> Result<(), Duration> {
        let bucket = self
            .buckets
            .get_with(key.to_owned(), async {
                Arc::new(Mutex::new(BucketState {
                    tokens: self.capacity,
                    last_refill: Instant::now(),
                }))
            })
            .await;

        let mut state = bucket.lock().await;
        let now = Instant::now();
        let elapsed = now
            .saturating_duration_since(state.last_refill)
            .as_secs_f64();
        state.tokens = (state.tokens + elapsed * self.refill_rate).min(self.capacity);
        state.last_refill = now;

        if state.tokens >= 1.0 {
            state.tokens -= 1.0;
            return Ok(());
        }
        // `try_from_secs_f64` rather than `from_secs_f64`: a `RATE_LIMIT_API_CAPACITY=0`
        // config makes `self.refill_rate` `0.0`, so the division above is
        // `inf`, which the panicking constructor would take down every
        // throttled request with it. `config::into_config` rejects that
        // config at startup already (a zero-capacity throttle admits nothing
        // and is a misconfiguration, not a mode anyone wants), but this
        // constructor is also built directly by the unit tests below with a
        // hand-built `RateLimitConfig`, bypassing that check -- so the
        // fallback has to hold regardless of how a caller got here. The
        // fallback is unreachable through any config that passed validation;
        // its value only has to be finite and sane for a `Retry-After`
        // header, not exact.
        Err(
            Duration::try_from_secs_f64((1.0 - state.tokens) / self.refill_rate)
                .unwrap_or(UNREACHABLE_CAPACITY_RETRY_AFTER),
        )
    }

    /// The address to charge this request to.
    ///
    /// Reads the **last** `X-Forwarded-For` entry, not the first, and only from
    /// a trusted peer. A proxy *appends* the address it saw, so the trailing
    /// entry is the one our own proxy observed and the only one it vouches for;
    /// the earlier entries are client-supplied, and a flooder would happily
    /// rotate a fake prefix to mint a fresh bucket per request. An untrusted
    /// peer is the client, header or no header.
    fn client_ip(&self, req: &Request) -> String {
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map_or_else(
                || UNKNOWN_PEER.to_owned(),
                |ConnectInfo(addr)| canonical_ip(addr.ip()).to_string(),
            );

        let Ok(peer_ip) = peer.parse::<IpAddr>() else {
            return peer;
        };
        if !self
            .trusted_proxies
            .iter()
            .any(|net| net.contains(&peer_ip))
        {
            return peer;
        }

        let Some(forwarded) = req
            .headers()
            .get(X_FORWARDED_FOR)
            .and_then(|v| v.to_str().ok())
        else {
            return peer;
        };
        let last = forwarded
            .rsplit(',')
            .next()
            .unwrap_or_default()
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']');
        if last.is_empty() {
            peer
        } else {
            last.to_owned()
        }
    }
}

/// An IPv4-mapped IPv6 peer (`::ffff:127.0.0.1`, which a dual-stack listener
/// reports) is canonicalised down to its IPv4 form, so that the IPv4 trusted
/// ranges match it.
fn canonical_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        IpAddr::V4(_) => ip,
    }
}

/// The middleware. Non-`/api/` paths are never throttled, so the healthcheck
/// and the actuator stay unmetered.
pub async fn rate_limit(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if !req.uri().path().starts_with("/api/") {
        return next.run(req).await;
    }

    let key = state.rate_limiter.client_ip(&req);
    let Err(wait) = state.rate_limiter.try_consume(&key).await else {
        return next.run(req).await;
    };

    // `Duration.ofNanos(nanosToWaitForRefill).toSeconds() + 1` -- truncating
    // division, then one second of slack, so a sub-second wait still advertises
    // `Retry-After: 1` rather than 0.
    let retry_after = wait.as_secs() + 1;
    let problem = ProblemDetail::new(
        StatusCode::TOO_MANY_REQUESTS,
        "Too many requests",
        Some("Rate limit exceeded. Try again later.".to_owned()),
        "rate-limit-exceeded",
    );
    // Written straight to the response, so it carries no `instance`.
    let mut response = problem.into_response_without_instance();
    response.headers_mut().insert(
        header::RETRY_AFTER,
        retry_after
            .to_string()
            .parse()
            .expect("a decimal integer is a valid header value"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    fn limiter(capacity: u64, max_tracked_ips: u64) -> RateLimiter {
        RateLimiter::new(&RateLimitConfig {
            capacity,
            refill_period: Duration::from_secs(60),
            max_tracked_ips,
            trusted_proxies: vec![
                "127.0.0.1/32".into(),
                "::1/128".into(),
                "10.0.0.0/8".into(),
                "172.16.0.0/12".into(),
                "192.168.0.0/16".into(),
            ],
        })
    }

    fn request(peer: &str, forwarded_for: Option<&str>) -> Request {
        let mut builder = Request::builder().uri("/api/tournaments");
        if let Some(xff) = forwarded_for {
            builder = builder.header(X_FORWARDED_FOR, xff);
        }
        let mut req = builder.body(Body::empty()).unwrap();
        // An IPv6 literal needs brackets before it is a socket address.
        let addr: SocketAddr = if peer.contains(':') {
            format!("[{peer}]:41234")
        } else {
            format!("{peer}:41234")
        }
        .parse()
        .unwrap();
        req.extensions_mut().insert(ConnectInfo(addr));
        req
    }

    /// Reproduces, then closes, the panic a `RATE_LIMIT_API_CAPACITY=0`
    /// misconfiguration used to cause on the very first throttled request.
    /// `RateLimiter::new` is built directly here rather than through
    /// `Config::from_env` -- exactly what the fix has to keep safe, since
    /// `config::into_config` rejecting zero at parse time does not stop a
    /// test (or a future caller) from constructing a `RateLimitConfig` with
    /// `capacity: 0` by hand, as every test in this module already does.
    /// `block_on` rather than `#[tokio::test]` so the panic can be caught
    /// with a plain `catch_unwind` instead of laundering it through a
    /// `JoinError`.
    #[test]
    fn a_zero_capacity_bucket_does_not_panic_on_the_first_throttled_request() {
        let limiter = limiter(0, 100_000);
        // Starts empty (capacity 0), so this call is immediately the
        // over-capacity path that used to divide by a zero refill rate.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            futures::executor::block_on(limiter.try_consume("1.2.3.4"))
        }));
        let wait = outcome
            .expect("try_consume must not panic on a zero-capacity bucket")
            .expect_err("a zero-capacity bucket must never grant a token");
        assert_eq!(wait, UNREACHABLE_CAPACITY_RETRY_AFTER);
    }

    #[tokio::test]
    async fn requests_within_capacity_pass_through() {
        let limiter = limiter(3, 100_000);
        for _ in 0..3 {
            assert!(limiter.try_consume("1.2.3.4").await.is_ok());
        }
    }

    #[tokio::test]
    async fn the_request_past_capacity_is_refused_and_says_how_long_to_wait() {
        let limiter = limiter(3, 100_000);
        for _ in 0..3 {
            limiter.try_consume("1.2.3.4").await.unwrap();
        }
        let wait = limiter.try_consume("1.2.3.4").await.unwrap_err();
        // capacity 3 per 60s -> one token back every 20s.
        assert!(
            wait <= Duration::from_secs(20) && wait > Duration::from_secs(19),
            "{wait:?}"
        );
    }

    #[tokio::test]
    async fn different_ips_get_independent_buckets() {
        let limiter = limiter(1, 100_000);
        assert!(limiter.try_consume("1.1.1.1").await.is_ok());
        assert!(limiter.try_consume("2.2.2.2").await.is_ok());
    }

    /// The reason `client_ip` exists: with a TLS-terminating proxy on the same
    /// host every request arrives from the Docker bridge gateway, so keying on
    /// the peer alone would put the entire internet in one bucket.
    #[test]
    fn behind_a_trusted_proxy_each_forwarded_client_is_its_own_key() {
        let limiter = limiter(1, 100_000);
        assert_eq!(
            limiter.client_ip(&request("172.17.0.1", Some("1.1.1.1"))),
            "1.1.1.1"
        );
        assert_eq!(
            limiter.client_ip(&request("172.17.0.1", Some("2.2.2.2"))),
            "2.2.2.2"
        );
    }

    #[test]
    fn an_untrusted_peers_forwarded_for_is_ignored() {
        let limiter = limiter(1, 100_000);
        assert_eq!(
            limiter.client_ip(&request("9.9.9.9", Some("1.1.1.1"))),
            "9.9.9.9"
        );
    }

    /// A proxy appends the peer it saw, so the trailing entry is the vouched-for
    /// one. Reading the first would let a flooder mint a fresh bucket per
    /// request with a fake prefix.
    #[test]
    fn only_the_last_forwarded_entry_counts() {
        let limiter = limiter(1, 100_000);
        assert_eq!(
            limiter.client_ip(&request("127.0.0.1", Some("fake-a, 5.5.5.5"))),
            "5.5.5.5"
        );
        assert_eq!(
            limiter.client_ip(&request("127.0.0.1", Some("fake-b, 5.5.5.5"))),
            "5.5.5.5"
        );
    }

    #[test]
    fn a_trusted_peer_sending_no_forwarded_for_falls_back_to_its_own_address() {
        let limiter = limiter(1, 100_000);
        assert_eq!(limiter.client_ip(&request("127.0.0.1", None)), "127.0.0.1");
    }

    #[test]
    fn a_bracketed_ipv6_entry_loses_its_brackets() {
        let limiter = limiter(1, 100_000);
        assert_eq!(
            limiter.client_ip(&request("::1", Some("[2001:db8::1]"))),
            "2001:db8::1"
        );
    }

    /// A dual-stack listener reports an IPv4 client as `::ffff:a.b.c.d`, which
    /// must still match the IPv4 trusted ranges.
    #[test]
    fn an_ipv4_mapped_peer_is_treated_as_ipv4() {
        let limiter = limiter(1, 100_000);
        assert_eq!(
            limiter.client_ip(&request("::ffff:172.17.0.1", Some("1.1.1.1"))),
            "1.1.1.1"
        );
    }

    /// Regression for the unbounded map this cache replaced: every distinct IP
    /// used to allocate a bucket that lived for the process' lifetime.
    #[tokio::test]
    async fn the_bucket_store_evicts_once_it_exceeds_max_tracked_ips() {
        let limiter = limiter(1, 5);
        for i in 0..50 {
            let _ = limiter.try_consume(&format!("10.0.0.{i}")).await;
        }
        limiter.buckets.run_pending_tasks().await;
        assert!(
            limiter.buckets.entry_count() <= 5,
            "expected eviction to cap the store at 5, got {}",
            limiter.buckets.entry_count()
        );
    }
}
