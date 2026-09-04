//! The Tabletop League scraper sidecar, and the URL rules that guard it.
//!
//! This is the application's one outbound HTTP call, and it exists because
//! tabletopleague.com is client-rendered Next.js: fetching a match page from
//! here would return the JS bundle and no data, so a real browser has to render
//! it. That browser is a separate process -- hence a sidecar rather than a
//! library.
//!
//! Failure modes are deliberately mapped to two different statuses, because the
//! admin's next action differs: a sidecar that is not reachable is a **503**
//! ("start it, then retry"), while a sidecar that answered with an error
//! scraped a real page and failed on it -- a **409** naming what went wrong,
//! usually selector drift after the source site changed its markup.
//!
//! [`ScraperClient`] is **the only trait in this crate**. It earns that
//! because it is a genuine test seam: `tests/it/match_import.rs`'s
//! `StubScraper` substitutes a stub rather than standing up a real browser.

use std::time::Duration;

use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use umfl_domain::DomainError;

use crate::error::ApiResult;

/// How long to wait for one scrape. Generous on purpose: the sidecar loads a
/// client-rendered page and waits for network idle, which its own `politeGoto`
/// caps at 60s, and it may sit behind that service's politeness delay if
/// another import is already running.
const SCRAPE_TIMEOUT: Duration = Duration::from_secs(90);

/// Short: either the sidecar is listening or it is not, and "not running"
/// should surface as a 503 the admin can act on rather than a stalled request.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Hosts an import URL may point at. The sidecar enforces this too -- this copy
/// fails a bad URL before it costs a round trip, and keeps the rule visible on
/// the side that takes it from a user.
///
/// A constant rather than configuration, unlike `scraper.base-url`: only the
/// address is bound to an environment variable in `application.yml`, and the
/// remaining `ScraperProperties` defaults were never overridden anywhere. Same
/// reasoning as `MatchResultCache`'s sizing -- see AGENTS.md's `umfl.*`
/// invariant.
const ALLOWED_HOSTS: &[&str] = &["www.tabletopleague.com"];

/// The seam. One method, because scraping one match page is the only thing the
/// sidecar does for us.
///
/// Returns a boxed future rather than using an `async fn`: `AppState` holds
/// this as `Arc<dyn ScraperClient>` so a test can swap in a stub, and an
/// `async fn` in a trait is not dyn-compatible.
pub trait ScraperClient: Send + Sync {
    fn scrape_match<'a>(&'a self, source_url: &'a str) -> BoxFuture<'a, ApiResult<ScrapedMatch>>;
}

/// The real one.
pub struct HttpScraperClient {
    http: reqwest::Client,
    base_url: String,
}

impl HttpScraperClient {
    pub fn new(base_url: String) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(SCRAPE_TIMEOUT)
            .build()
            // Only fails if the TLS backend cannot be initialised, which is a
            // start-up fault rather than a request-time one.
            .expect("build the scraper HTTP client");
        Self { http, base_url }
    }

    fn unreachable(&self, error: &reqwest::Error) -> DomainError {
        DomainError::service_unavailable(format!(
            "The match scraper at {} is not reachable. \
             Start it with `npm run serve` in tools/tabletopleague-scraper, \
             or bring up the `scraper` service. ({error})",
            self.base_url
        ))
    }
}

impl ScraperClient for HttpScraperClient {
    fn scrape_match<'a>(&'a self, source_url: &'a str) -> BoxFuture<'a, ApiResult<ScrapedMatch>> {
        Box::pin(async move {
            let response = self
                .http
                .post(format!("{}/scrape/match", self.base_url))
                .json(&serde_json::json!({ "url": source_url }))
                .send()
                .await
                // Connection refused, DNS failure, or the timeout elapsed --
                // `RestClient`'s `ResourceAccessException` by another name.
                .map_err(|e| self.unreachable(&e))?;

            if !response.status().is_success() {
                // The sidecar reports its own failures as {"error": "..."}.
                // Read it so the admin sees the real reason rather than a
                // status code.
                let body = response.text().await.ok();
                return Err(DomainError::conflict(format!(
                    "The scraper could not read that match page: {}",
                    scraper_error_message(body.as_deref())
                ))
                .into());
            }

            // A body that arrives but does not deserialise is the sidecar
            // answering something other than a scrape -- a 409, like any other
            // unusable response, never a 503: it *is* reachable.
            response.json::<ScrapedMatch>().await.map_err(|e| {
                DomainError::conflict(format!("The scraper returned an unusable response: {e}"))
                    .into()
            })
        })
    }
}

/// Pulls the sidecar's own error sentence out of its `{"error": "..."}` body.
///
/// Parsed as JSON rather than pattern-matched with a regex: `serde_json` is
/// already a dependency, so a full parse costs nothing extra even on an error
/// path.
fn scraper_error_message(body: Option<&str>) -> String {
    let Some(body) = body.map(str::trim).filter(|b| !b.is_empty()) else {
        return "no detail given".to_owned();
    };
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| body.chars().take(300).collect())
}

/// Rejects anything that is not a match detail page on an allowed host, before
/// it costs a round trip. `None` means the URL is usable.
///
/// The sidecar enforces the same rule -- this copy is the one that sees a URL
/// typed by a user, and every string it returns is rendered to that user, so
/// they are wire contract.
pub fn validate_source_url(source_url: &str) -> Option<String> {
    let trimmed = source_url.trim();
    let url = match reqwest::Url::parse(trimmed) {
        Ok(url) => url,
        Err(_) => {
            // `java.net.URI` is more permissive than `Url::parse`: a string
            // with no scheme parses there as a *relative* URI and falls
            // through to the scheme check below, while a string containing a
            // character URI forbids throws. Splitting the failure the same way
            // keeps both messages pointing at the actual problem.
            return Some(if trimmed.chars().any(is_uri_illegal) {
                "That is not a valid URL.".to_owned()
            } else {
                "The match URL must start with https://.".to_owned()
            });
        }
    };
    if url.scheme() != "https" {
        return Some("The match URL must start with https://.".to_owned());
    }
    let Some(host) = url.host_str() else {
        return Some("That URL has no host.".to_owned());
    };
    if !ALLOWED_HOSTS.contains(&host) {
        return Some(format!(
            "Only {} match pages can be imported.",
            ALLOWED_HOSTS.join(", ")
        ));
    }
    if !is_match_path(url.path()) {
        return Some(format!(
            "That is not a match page. Expected a link like \
             https://{host}/o/<org>/<competition>/matches/<id>."
        ));
    }
    None
}

/// `java.net.URI`'s reject set, near enough: everything outside RFC 2396's
/// allowed characters that a pasted URL realistically carries.
fn is_uri_illegal(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(c, '<' | '>' | '"' | '{' | '}' | '|' | '\\' | '^' | '`')
}

/// `/o/<org>/<competition>/matches/<id>`, with an optional trailing slash --
/// spelt out as a segment check rather than a regex, because this crate has
/// no regex engine and does not need one for a fixed six-segment shape.
fn is_match_path(path: &str) -> bool {
    let path = path.strip_suffix('/').unwrap_or(path);
    let segments: Vec<&str> = path.split('/').collect();
    segments.len() == 6
        && segments[0].is_empty()
        && segments[1] == "o"
        && !segments[2].is_empty()
        && !segments[3].is_empty()
        && segments[4] == "matches"
        && segments[5].len() >= 8
        && segments[5]
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '-')
}

// ---------------------------------------------------------------------------
// The sidecar's payload
// ---------------------------------------------------------------------------

/// The scraper sidecar's single-match payload, as produced by
/// `tools/tabletopleague-scraper/scrape-match.mjs`. Field names match that
/// script's JSON exactly -- see the "Output shape" section of its README.
///
/// Only the fields with a home in this domain are modelled. The source carries
/// several more (`seedLabel`, `upset`, `hasAdvantage`, `headToHeadRaw`,
/// `seriesWinner`, `score`, `status`, `title`, `competitionName`) that describe
/// that org's own presentation of the match and have nothing to bind to here;
/// serde ignores unknown fields by default, so they are simply not listed.
/// `roundName` and `seriesFormat` are the exceptions: neither maps to a column,
/// but both are shown to the admin as context while they pick a round number.
///
/// Everything is optional because the scraper's extractors return null for
/// anything a selector missed rather than throwing -- a partially-parsed match
/// should surface as named unresolved fields, not as a deserialization failure.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrapedMatch {
    #[serde(default)]
    pub match_id: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub round_name: Option<String>,
    #[serde(default)]
    pub series_format: Option<String>,
    #[serde(default)]
    pub played_at_raw: Option<String>,
    #[serde(default)]
    pub side_a: Option<ScrapedSide>,
    #[serde(default)]
    pub side_b: Option<ScrapedSide>,
    /// Heroes struck before sides were assigned -- `PRE_BAN`, belonging to
    /// neither side.
    #[serde(default)]
    pub pre_bans: Vec<String>,
    #[serde(default)]
    pub games: Vec<ScrapedGame>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrapedSide {
    #[serde(default)]
    pub player_label: Option<String>,
    /// Every hero this side drafted, in game order.
    #[serde(default)]
    pub picks: Vec<String>,
    #[serde(default)]
    pub bans: Vec<ScrapedBan>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrapedBan {
    #[serde(default)]
    pub hero_name: Option<String>,
    /// Already `PRE_BAN` / `OPPONENT_BAN` / `SELF_BAN` -- the scraper emits
    /// this repo's vocabulary.
    #[serde(default)]
    pub ban_type: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrapedGame {
    #[serde(default)]
    pub game_index: Option<i32>,
    #[serde(default)]
    pub map_name: Option<String>,
    #[serde(default)]
    pub side_a: Option<ScrapedGameSide>,
    #[serde(default)]
    pub side_b: Option<ScrapedGameSide>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrapedGameSide {
    #[serde(default)]
    pub hero_name: Option<String>,
    /// Negative for an overkill finish; the loser of a game is always 0 or
    /// less.
    #[serde(default)]
    pub health: Option<i32>,
    #[serde(default)]
    pub is_winner: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "https://www.tabletopleague.com/o/unmatched/season-3/matches/1a2b3c4d-5e6f";

    #[test]
    fn a_match_page_on_an_allowed_host_passes() {
        assert_eq!(validate_source_url(VALID), None);
        assert_eq!(validate_source_url(&format!("  {VALID}  ")), None);
        assert_eq!(
            validate_source_url(&format!("{VALID}/")),
            None,
            "trailing slash"
        );
    }

    #[test]
    fn a_non_https_url_is_named_as_such() {
        assert_eq!(
            validate_source_url("http://www.tabletopleague.com/o/a/b/matches/1a2b3c4d"),
            Some("The match URL must start with https://.".to_owned())
        );
    }

    #[test]
    fn a_url_with_no_scheme_is_treated_as_a_missing_https_rather_than_a_typo() {
        assert_eq!(
            validate_source_url("www.tabletopleague.com/o/a/b/matches/1a2b3c4d"),
            Some("The match URL must start with https://.".to_owned())
        );
    }

    #[test]
    fn a_string_that_is_not_a_url_at_all_says_so() {
        assert_eq!(
            validate_source_url("not a url"),
            Some("That is not a valid URL.".to_owned())
        );
    }

    #[test]
    fn another_host_is_refused_by_name() {
        assert_eq!(
            validate_source_url("https://example.com/o/a/b/matches/1a2b3c4d"),
            Some("Only www.tabletopleague.com match pages can be imported.".to_owned())
        );
    }

    #[test]
    fn a_page_on_the_right_host_that_is_not_a_match_is_refused_with_an_example() {
        let message = validate_source_url("https://www.tabletopleague.com/o/unmatched/season-3")
            .expect("not a match page");
        assert!(
            message.starts_with("That is not a match page."),
            "{message}"
        );
        assert!(
            message.contains("/o/<org>/<competition>/matches/<id>"),
            "{message}"
        );
    }

    /// The id has to look like one: a short or non-hex tail is the sort of
    /// half-copied link that would otherwise cost a 90-second scrape to reject.
    #[test]
    fn a_match_path_with_an_implausible_id_is_refused() {
        for path in [
            "https://www.tabletopleague.com/o/a/b/matches/123",
            "https://www.tabletopleague.com/o/a/b/matches/not-hex-at-all-zz",
            "https://www.tabletopleague.com/o//b/matches/1a2b3c4d",
            "https://www.tabletopleague.com/o/a/b/matches/1a2b3c4d/extra",
        ] {
            assert!(
                validate_source_url(path).is_some(),
                "{path} should not be accepted"
            );
        }
    }

    #[test]
    fn the_sidecars_own_error_sentence_is_what_the_admin_sees() {
        assert_eq!(
            scraper_error_message(Some(r#"{"error": "no such match"}"#)),
            "no such match"
        );
        assert_eq!(scraper_error_message(Some("")), "no detail given");
        assert_eq!(scraper_error_message(None), "no detail given");
        assert_eq!(
            scraper_error_message(Some("<html>502 Bad Gateway</html>")),
            "<html>502 Bad Gateway</html>",
            "a body that is not the sidecar's JSON is passed through, truncated"
        );
        let long = "x".repeat(500);
        assert_eq!(scraper_error_message(Some(&long)).len(), 300);
    }
}
