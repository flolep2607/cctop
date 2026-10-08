//! Provider status pages: what the vendor says is broken right now.
//!
//! This exists to answer one question the table cannot: a session has gone red,
//! and is that this machine's problem or the provider's? The red dot says a
//! request failed; it cannot say that Anthropic has an open incident against
//! Claude Code, which is the difference between debugging a proxy for twenty
//! minutes and going for coffee.
//!
//! Both vendors run Atlassian Statuspage (OpenAI behind incident.io's
//! compatibility layer), so `/api/v2/summary.json` serves the current indicator,
//! per-component health, and every unresolved incident with its updates — a
//! couple of kilobytes of JSON, no feed parser, and no new dependency. The
//! `history.rss` and `feed.rss` feeds carry the same incidents but only after
//! the fact, and cost thirty times the bytes to say so; they are the wrong
//! source for "is it broken *now*".
//!
//! Parsing and matching are pure and pinned by the fixtures beside this file;
//! [`fetch`] is the only part that touches the network, and the dashboard calls
//! it from a thread of its own — the loop in [`poll`], which the dashboard and
//! a standalone `cctop serve` both run, so the vendor is asked on one schedule
//! whichever of them is watching.

use crate::pricing::Provider;
use crate::session::{ActivityState, Session};
use crate::util;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

pub mod poll;
pub use poll::spawn_poller;

/// Short, because the answer is only useful while the user is still looking at
/// the failure — and because the likeliest reason for a slow page is that this
/// machine's network is the thing that is broken.
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// A status page cctop knows how to read.
///
/// Deliberately only the two vendors whose models cctop can attribute a session
/// to. Google, Cursor and the rest publish pages too, but a session's failing
/// request cannot be pinned to one of them from the transcript, and a status
/// line that might be about your outage is worse than none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Page {
    Anthropic,
    OpenAi,
}

impl Page {
    pub const ALL: [Page; 2] = [Page::Anthropic, Page::OpenAi];

    pub fn label(self) -> &'static str {
        match self {
            Page::Anthropic => "Anthropic",
            Page::OpenAi => "OpenAI",
        }
    }

    /// The page a human should open to read more.
    pub fn site(self) -> &'static str {
        match self {
            Page::Anthropic => "https://status.claude.com",
            Page::OpenAi => "https://status.openai.com",
        }
    }

    fn url(self) -> &'static str {
        match self {
            Page::Anthropic => "https://status.claude.com/api/v2/summary.json",
            Page::OpenAi => "https://status.openai.com/api/v2/summary.json",
        }
    }
}

/// Statuspage's severity ladder, ordered so `max` picks the worst.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    #[default]
    Operational,
    /// Planned work. Worth showing, but it is not an outage.
    Maintenance,
    Minor,
    Major,
    Critical,
}

impl Level {
    fn parse(value: Option<&str>) -> Level {
        match value.unwrap_or("none") {
            "maintenance" => Level::Maintenance,
            "minor" => Level::Minor,
            "major" => Level::Major,
            "critical" => Level::Critical,
            // "none", and anything a vendor invents later: assume healthy rather
            // than crying wolf about a word we do not recognise.
            _ => Level::Operational,
        }
    }

    /// Whether the page is claiming anything at all is wrong.
    pub fn is_degraded(self) -> bool {
        self != Level::Operational
    }

    /// Whether this is an outage rather than planned work — the distinction that
    /// decides how loudly the footer says it.
    pub fn is_outage(self) -> bool {
        self >= Level::Minor
    }
}

/// One unresolved incident.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Incident {
    pub name: String,
    /// Where the vendor is in handling it: investigating, identified, monitoring.
    pub stage: String,
    pub level: Level,
    /// Unix seconds.
    pub started_at: Option<i64>,
    /// Body of the most recent update, which is where the "why" actually lives.
    pub update: Option<String>,
    pub components: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Report {
    pub level: Level,
    /// The vendor's own one-line summary, e.g. "Partial System Degradation".
    pub description: String,
    pub incidents: Vec<Incident>,
    /// Components not reporting operational, whether or not an incident names
    /// them. OpenAI's page routinely leaves an incident's component list empty
    /// while flagging the components themselves, so this is the only place the
    /// affected surface shows up.
    pub degraded: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum PageStatus {
    /// Not fetched yet.
    #[default]
    Pending,
    Ok(Report),
    /// The page could not be read. Says nothing about the provider, which is
    /// why nothing downstream treats it as either healthy or broken.
    Unavailable(String),
}

impl PageStatus {
    pub fn report(&self) -> Option<&Report> {
        match self {
            PageStatus::Ok(r) => Some(r),
            _ => None,
        }
    }
}

/// `{"state": "pending" | "ok" | "unavailable", …}`, with the report's fields
/// beside the tag or the reason under `reason`.
///
/// Written by hand because the enum's shape is not the wire's: `Unavailable`
/// carries a bare string, which serde's internal tagging cannot flatten, and a
/// page reading `state` first is simpler than one probing for which key exists.
impl Serialize for PageStatus {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(tag = "state", rename_all = "lowercase")]
        enum Wire<'a> {
            Pending,
            Ok(&'a Report),
            Unavailable { reason: &'a str },
        }
        match self {
            PageStatus::Pending => Wire::Pending,
            PageStatus::Ok(r) => Wire::Ok(r),
            PageStatus::Unavailable(why) => Wire::Unavailable { reason: why },
        }
        .serialize(serializer)
    }
}

/// What every page last said.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    pub anthropic: PageStatus,
    pub openai: PageStatus,
}

impl Status {
    pub fn get(&self, page: Page) -> &PageStatus {
        match page {
            Page::Anthropic => &self.anthropic,
            Page::OpenAi => &self.openai,
        }
    }

    pub fn set(&mut self, page: Page, status: PageStatus) {
        match page {
            Page::Anthropic => self.anthropic = status,
            Page::OpenAi => self.openai = status,
        }
    }
}

/// Which status page covers a session, when one does.
///
/// The model is asked first and the harness second, because the harness is the
/// weaker signal: Cursor and OpenCode route to whichever vendor the user picked,
/// so `cursor` alone says nothing about who to blame, while `claude-opus-5`
/// does. Sessions whose vendor cannot be established return `None` and are left
/// out of the matching entirely.
pub fn page_for(provider: Provider, model: &str) -> Option<Page> {
    let m = model.to_ascii_lowercase();
    if m.contains("claude") || m.contains("anthropic") {
        return Some(Page::Anthropic);
    }
    if m.contains("gpt") || m.contains("codex") || m.starts_with("o1") || m.starts_with("o3") {
        return Some(Page::OpenAi);
    }
    match provider {
        Provider::Claude => Some(Page::Anthropic),
        Provider::Codex => Some(Page::OpenAi),
        Provider::Cursor
        | Provider::Devin
        | Provider::Gemini
        | Provider::OpenCode
        | Provider::Pi
        | Provider::Windsurf => None,
    }
}

/// Running sessions currently in an API error state, counted per status page.
///
/// Only running sessions: a transcript that ended on a failed request is
/// history, and history cannot be corroborated by a page that only reports
/// what is open now. Sessions whose vendor is unknown are skipped rather than
/// guessed at — see [`page_for`].
pub fn erroring_by_page<'a>(sessions: impl IntoIterator<Item = &'a Session>) -> Vec<(Page, usize)> {
    let mut counts: Vec<(Page, usize)> = Vec::new();
    for s in sessions {
        if !s.is_running() || s.activity_state != ActivityState::ApiError {
            continue;
        }
        let Some(page) = page_for(s.provider, &s.model) else {
            continue;
        };
        match counts.iter_mut().find(|(p, _)| *p == page) {
            Some((_, n)) => *n += 1,
            None => counts.push((page, 1)),
        }
    }
    counts
}

/// A page's state set against how many of the user's sessions fail against it.
#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    pub page: Page,
    /// Running sessions attributed to this page that are in an API error state.
    pub erroring: usize,
    pub level: Level,
    /// The worst open incident's name when there is one, else the page's own
    /// description. Timing and update bodies stay in [`Report`], which is what
    /// the panel reads; a footer line has room for neither.
    pub headline: String,
}

/// The fields, plus the two verdicts a reader would otherwise recompute — and
/// could recompute differently.
impl Serialize for Alert {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            page: Page,
            erroring: usize,
            level: Level,
            headline: &'a str,
            confirmed: bool,
            probably_local: bool,
        }
        Wire {
            page: self.page,
            erroring: self.erroring,
            level: self.level,
            headline: &self.headline,
            confirmed: self.confirmed(),
            probably_local: self.probably_local(),
        }
        .serialize(serializer)
    }
}

impl Alert {
    /// The vendor is corroborating what the user is seeing. This is the whole
    /// point of the feature, so it gets a name.
    pub fn confirmed(&self) -> bool {
        self.erroring > 0 && self.level.is_outage()
    }

    /// Sessions fail while the page reports nothing wrong: the cause is most
    /// likely on this machine — network, proxy, credentials, a model name.
    pub fn probably_local(&self) -> bool {
        self.erroring > 0 && !self.level.is_degraded()
    }
}

/// Pair each page's state with how many of the user's sessions are failing
/// against it, keeping only the pages worth saying something about, most
/// pressing first.
///
/// A page that has not answered — pending or unreachable — yields nothing, even
/// when sessions are failing against it: cctop not reaching a status page says
/// nothing about the provider, and on a dead network it would be one more
/// alarming line about a cause the user can already see.
pub fn alerts(status: &Status, erroring: &[(Page, usize)]) -> Vec<Alert> {
    let mut out = Vec::new();
    for page in Page::ALL {
        let Some(report) = status.get(page).report() else {
            continue;
        };
        let count = erroring
            .iter()
            .find(|(p, _)| *p == page)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        if !report.level.is_degraded() && report.incidents.is_empty() && count == 0 {
            continue;
        }
        // The worst open incident is the headline; its name is more specific
        // than "Partial System Degradation" and is what the user wants to read.
        let worst = report.incidents.iter().max_by_key(|i| i.level);
        out.push(Alert {
            page,
            erroring: count,
            level: report.level.max(worst.map(|i| i.level).unwrap_or_default()),
            headline: match worst {
                Some(i) => i.name.clone(),
                None if report.description.is_empty() => "no incident reported".into(),
                None => report.description.clone(),
            },
        });
    }
    // Corroborated trouble first, then the "probably local" hint, which explains
    // something on screen too, then by severity: with one footer line to spend,
    // the alert that explains a failure the user is looking at outranks a minor
    // incident on a provider they are not using.
    out.sort_by_key(|a| {
        (
            !a.confirmed(),
            !a.probably_local(),
            std::cmp::Reverse(a.level),
            std::cmp::Reverse(a.erroring),
        )
    });
    out
}

// ---------------------------------------------------------------------------
// Wording
// ---------------------------------------------------------------------------
//
// Here rather than in the dashboard because two screens say it: the TUI's
// footer and `!` panel, and the web dashboard's line and dialog. Worded once,
// they cannot drift into telling the same user two different things.

/// How loudly a line should be drawn. A tone rather than a colour, so each
/// screen keeps its own palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    /// The vendor reports an outage.
    Outage,
    /// Maintenance, or the "probably this machine" hint: a warning, not a
    /// verdict.
    Warning,
    /// Nothing worth raising a voice over.
    Quiet,
}

impl Tone {
    fn of(level: Level) -> Tone {
        match level.is_outage() {
            true => Tone::Outage,
            false => Tone::Warning,
        }
    }
}

/// The one-line summary, when there is anything to say.
///
/// Three cases. The vendor reports an incident — name it, and say how many of
/// yours fail against it when any do, because then it is the answer. Sessions
/// fail while the page that covers them says all is well — say it is probably
/// this machine, which is worth knowing early. Silent otherwise, including when
/// no page could be reached: a line that permanently reads "operational", or
/// "unknown" on every offline afternoon, is a line nobody reads.
///
/// The level is the alert's, which is what the TUI colours by; a probably-local
/// alert's level is [`Level::Operational`], which reads as amber, not red.
pub fn summary_line(alerts: &[Alert]) -> Option<(String, Level)> {
    let a = alerts.first()?;
    let what = match a.level {
        Level::Maintenance => "maintenance",
        _ => "incident",
    };
    let yours = match a.erroring {
        0 => String::new(),
        n => format!(" — {n} of yours failing"),
    };
    let text = match a.probably_local() {
        true => format!(
            "⚠ {} failing, {} reports all clear: probably this machine",
            a.erroring,
            a.page.label()
        ),
        false => format!(
            "⚠ {} {what}: {}{yours}",
            a.page.label(),
            util::truncate(&a.headline, 40)
        ),
    };
    Some((text, a.level))
}

/// The answer to "is it me?", which every detailed view leads with: on a bad
/// day it is the only sentence that gets read.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Verdict {
    pub text: String,
    /// A second line, for the case that needs telling where to look.
    pub detail: Option<String>,
    pub tone: Tone,
}

pub fn verdict(alerts: &[Alert], erroring: &[(Page, usize)]) -> Verdict {
    let total: usize = erroring.iter().map(|(_, n)| n).sum();
    let are = |n: usize| if n == 1 { "is" } else { "are" };
    let plain = |text: String| Verdict {
        text,
        detail: None,
        tone: Tone::Quiet,
    };
    match alerts.first() {
        Some(a) if a.confirmed() => Verdict {
            text: format!(
                "{} of your sessions {} failing, and {} reports an incident",
                a.erroring,
                are(a.erroring),
                a.page.label()
            ),
            detail: None,
            tone: Tone::of(a.level),
        },
        Some(a) if a.probably_local() => Verdict {
            text: format!(
                "{} of your sessions {} failing, but {} reports all clear",
                a.erroring,
                are(a.erroring),
                a.page.label()
            ),
            detail: Some(
                "so suspect this machine: network, proxy, credentials, a model name".into(),
            ),
            tone: Tone::Warning,
        },
        _ if total == 0 => plain("None of your running sessions is reporting API errors".into()),
        // Failing, but no page that covers them has answered.
        _ => plain(format!(
            "{total} of your sessions {} failing; their status page has not answered",
            are(total)
        )),
    }
}

/// Everything a reader needs to draw both the line and the panel, from the
/// pages' last answers and the sessions on screen.
#[derive(Debug, Clone, Serialize)]
pub struct Document {
    /// Every page, answered or not, in [`Page::ALL`] order.
    pub pages: Vec<PageDocument>,
    /// Most pressing first; see [`alerts`].
    pub alerts: Vec<Alert>,
    /// The one-line summary, absent when there is nothing to say.
    pub line: Option<Line>,
    pub verdict: Verdict,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageDocument {
    pub page: Page,
    pub label: &'static str,
    /// The page a human should open to read more.
    pub site: &'static str,
    /// Running sessions failing against this page.
    pub erroring: usize,
    #[serde(flatten)]
    pub status: PageStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub text: String,
    pub tone: Tone,
}

/// Build the [`Document`] for these sessions.
pub fn document<'a>(status: &Status, sessions: impl IntoIterator<Item = &'a Session>) -> Document {
    let erroring = erroring_by_page(sessions);
    let alerts = alerts(status, &erroring);
    let mine = |page: Page| {
        erroring
            .iter()
            .find(|(p, _)| *p == page)
            .map_or(0, |(_, n)| *n)
    };
    Document {
        pages: Page::ALL
            .into_iter()
            .map(|page| PageDocument {
                page,
                label: page.label(),
                site: page.site(),
                erroring: mine(page),
                status: status.get(page).clone(),
            })
            .collect(),
        line: summary_line(&alerts).map(|(text, level)| Line {
            text,
            tone: Tone::of(level),
        }),
        verdict: verdict(&alerts, &erroring),
        alerts,
    }
}

// ---------------------------------------------------------------------------
// Fetching
// ---------------------------------------------------------------------------

/// Read one page over the network. Blocks for up to [`HTTP_TIMEOUT`], so it
/// belongs on a background thread.
pub fn fetch(page: Page) -> PageStatus {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        // Read the code rather than letting a 503 collapse into a transport
        // error: a status page that is itself down is worth naming in the panel.
        .http_status_as_error(false)
        .build()
        .into();

    let mut resp = match agent.get(page.url()).call() {
        Ok(r) => r,
        Err(e) => return PageStatus::Unavailable(short_error(&e)),
    };
    let code = resp.status().as_u16();
    if !(200..300).contains(&code) {
        return PageStatus::Unavailable(format!("HTTP {code}"));
    }
    match resp.body_mut().read_to_string() {
        Ok(text) => parse(&text),
        Err(e) => PageStatus::Unavailable(short_error(&e)),
    }
}

fn short_error(e: &impl std::fmt::Display) -> String {
    let text = e.to_string();
    let one_line = text.lines().next().unwrap_or(&text).trim();
    util::truncate(one_line, 40)
}

/// Read a Statuspage v2 summary. Pure, so every vendor quirk is pinned by a
/// fixture rather than by whatever the page happens to say today.
pub fn parse(text: &str) -> PageStatus {
    let Ok(root) = serde_json::from_str::<Value>(text) else {
        return PageStatus::Unavailable("bad response".into());
    };
    // `status` is the one field the whole page hangs off; without it this is not
    // a Statuspage summary and guessing "operational" would be a lie.
    let Some(status) = root.get("status") else {
        return PageStatus::Unavailable("unexpected response".into());
    };

    let level = Level::parse(status.get("indicator").and_then(Value::as_str));
    let description = status
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let incidents = root
        .get("incidents")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(parse_incident).collect())
        .unwrap_or_default();

    PageStatus::Ok(Report {
        level,
        description,
        incidents,
        degraded: degraded_components(root.get("components")),
    })
}

fn parse_incident(v: &Value) -> Incident {
    let str_at = |key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    // `started_at` is when it began; `created_at` is when someone opened the
    // record. The former is the honest answer to "since when", and Statuspage
    // omits it often enough to need the fallback.
    let started_at = ["started_at", "created_at"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str))
        .and_then(|ts| util::parse_ts(ts).map(|dt| dt.timestamp()));

    Incident {
        name: str_at("name"),
        stage: str_at("status"),
        level: Level::parse(v.get("impact").and_then(Value::as_str)),
        started_at,
        update: latest_update(v),
        components: v
            .get("components")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.get("name").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// The newest update body. Statuspage lists updates newest-first and incident.io
/// follows suit, but the ordering is not documented, so the timestamps decide.
fn latest_update(incident: &Value) -> Option<String> {
    let updates = incident.get("incident_updates")?.as_array()?;
    let newest = updates.iter().max_by_key(|u| {
        u.get("created_at")
            .and_then(Value::as_str)
            .and_then(util::parse_ts)
            .map(|dt| dt.timestamp())
            .unwrap_or(i64::MIN)
    })?;
    let body = newest.get("body").and_then(Value::as_str)?.trim();
    match body.is_empty() {
        true => None,
        // Bodies are written for a web page and arrive with newlines and runs of
        // spaces; every consumer here lays them out itself.
        false => Some(body.split_whitespace().collect::<Vec<_>>().join(" ")),
    }
}

/// Names of components that are not operational.
///
/// Group rows are skipped — they aggregate their children, so keeping them
/// reports the same outage twice under two names. Duplicates are dropped because
/// OpenAI's page really does list two distinct components both called "Login".
fn degraded_components(components: Option<&Value>) -> Vec<String> {
    let Some(list) = components.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for c in list {
        if c.get("group").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let status = c.get("status").and_then(Value::as_str).unwrap_or("");
        if status.is_empty() || status == "operational" {
            continue;
        }
        if let Some(name) = c.get("name").and_then(Value::as_str)
            && !out.iter().any(|n| n == name)
        {
            out.push(name.to_string());
        }
    }
    out
}

/// The fixtures, for the tests of the crates above that draw them, and for
/// the server's debug build to stand a page in for the vendor's.
#[cfg(any(test, feature = "test-support", feature = "debug"))]
pub mod fixtures {
    /// Anthropic's page, healthy.
    pub const OPERATIONAL: &str = include_str!("fixtures/operational.json");
    /// OpenAI's page mid-incident: the incident names no components, so the
    /// affected surface is only knowable from the component list.
    pub const DEGRADED: &str = include_str!("fixtures/degraded.json");
    /// Anthropic's page in a major incident with a second, minor one open.
    pub const MAJOR: &str = include_str!("fixtures/major.json");
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    fn report(text: &str) -> Report {
        match parse(text) {
            PageStatus::Ok(r) => r,
            other => panic!("expected a report, got {other:?}"),
        }
    }

    #[test]
    fn an_operational_page_reports_nothing_open() {
        let r = report(OPERATIONAL);
        assert_eq!(r.level, Level::Operational);
        assert!(!r.level.is_degraded());
        assert_eq!(r.description, "All Systems Operational");
        assert!(r.incidents.is_empty());
        assert!(r.degraded.is_empty());
    }

    #[test]
    fn a_degraded_page_yields_its_incident_and_components() {
        let r = report(DEGRADED);
        assert_eq!(r.level, Level::Minor);
        assert!(r.level.is_outage());
        assert_eq!(r.incidents.len(), 1);
        let i = &r.incidents[0];
        assert_eq!(i.name, "Increased error rates");
        assert_eq!(i.stage, "monitoring");
        // `started_at` wins over `created_at`, and the newest update wins over
        // the first one listed.
        assert_eq!(
            i.started_at,
            util::parse_ts("2026-10-08T13:55:00Z").map(|d| d.timestamp())
        );
        assert_eq!(
            i.update.as_deref(),
            Some("We have applied the mitigation and are monitoring.")
        );
        // The group row is dropped and the duplicate name appears once.
        assert_eq!(r.degraded, vec!["Responses", "Login"]);
    }

    #[test]
    fn a_major_incident_keeps_its_components_and_newest_update() {
        let r = report(MAJOR);
        assert_eq!(r.level, Level::Major);
        assert_eq!(r.incidents.len(), 2);
        let i = &r.incidents[0];
        assert_eq!(i.level, Level::Major);
        // No `started_at`: the record's creation time stands in.
        assert_eq!(
            i.started_at,
            util::parse_ts("2026-10-08T09:41:00Z").map(|d| d.timestamp())
        );
        // Listed first here, but chosen for being newest.
        assert_eq!(
            i.update.as_deref(),
            Some("A bad deploy is being rolled back.")
        );
        assert_eq!(
            i.components,
            vec!["Claude API (api.anthropic.com)", "Claude Code"]
        );
        // An incident with no updates has no body rather than an empty one.
        assert_eq!(r.incidents[1].update, None);
    }

    #[test]
    fn garbage_is_unavailable_rather_than_healthy() {
        assert!(matches!(parse("not json"), PageStatus::Unavailable(_)));
        assert!(matches!(parse("{}"), PageStatus::Unavailable(_)));
        // An unknown indicator is treated as healthy, but a *missing* status
        // block is not a Statuspage summary at all.
        assert_eq!(
            report(r#"{"status":{"indicator":"wat"}}"#).level,
            Level::Operational
        );
    }

    #[test]
    fn the_model_beats_the_harness_when_attributing_a_session() {
        // Cursor alone says nothing; Cursor running Opus says Anthropic.
        assert_eq!(page_for(Provider::Cursor, ""), None);
        assert_eq!(
            page_for(Provider::Cursor, "claude-opus-5"),
            Some(Page::Anthropic)
        );
        assert_eq!(page_for(Provider::OpenCode, "gpt-5.1"), Some(Page::OpenAi));
        // The harness is the fallback, not the first answer.
        assert_eq!(page_for(Provider::Claude, ""), Some(Page::Anthropic));
        assert_eq!(page_for(Provider::Codex, ""), Some(Page::OpenAi));
        assert_eq!(page_for(Provider::Gemini, "gemini-3-pro"), None);
    }

    #[test]
    fn only_running_failing_attributable_sessions_count() {
        let make = |provider, model: &str, running: bool, state| {
            let mut s = Session::new(provider, format!("{model}-{running}"));
            s.model = model.into();
            s.inferred_running = running;
            s.activity_state = state;
            s
        };
        let sessions = [
            make(
                Provider::Claude,
                "claude-opus-5",
                true,
                ActivityState::ApiError,
            ),
            make(
                Provider::Cursor,
                "claude-sonnet-5",
                true,
                ActivityState::ApiError,
            ),
            make(Provider::Codex, "gpt-5.1", true, ActivityState::ApiError),
            // Ended on an error: history, which no page can corroborate.
            make(
                Provider::Claude,
                "claude-opus-5",
                false,
                ActivityState::ApiError,
            ),
            // Running and healthy.
            make(
                Provider::Claude,
                "claude-opus-5",
                true,
                ActivityState::default(),
            ),
            // Failing, but nothing says whose fault it would be.
            make(
                Provider::Gemini,
                "gemini-3-pro",
                true,
                ActivityState::ApiError,
            ),
        ];
        assert_eq!(
            erroring_by_page(&sessions),
            vec![(Page::Anthropic, 2), (Page::OpenAi, 1)]
        );
    }

    fn status(anthropic: &str, openai: &str) -> Status {
        Status {
            anthropic: parse(anthropic),
            openai: parse(openai),
        }
    }

    #[test]
    fn an_incident_surfaces_whether_or_not_it_touches_your_sessions() {
        let s = status(OPERATIONAL, DEGRADED);
        let quiet = alerts(&s, &[]);
        assert_eq!(quiet.len(), 1, "the clean page stays quiet: {quiet:?}");
        assert_eq!(quiet[0].page, Page::OpenAi);
        assert_eq!(quiet[0].headline, "Increased error rates");
        assert!(!quiet[0].confirmed());
        assert!(!quiet[0].probably_local());

        let mine = alerts(&s, &[(Page::OpenAi, 2)]);
        assert!(mine[0].confirmed());
        assert_eq!(mine[0].erroring, 2);
    }

    #[test]
    fn the_worst_incident_is_the_headline() {
        let a = &alerts(&status(MAJOR, OPERATIONAL), &[])[0];
        assert_eq!(a.headline, "Elevated errors on Claude Opus");
        assert_eq!(a.level, Level::Major);
    }

    #[test]
    fn failing_against_a_clean_page_is_probably_local() {
        let s = status(OPERATIONAL, OPERATIONAL);
        assert!(
            alerts(&s, &[]).is_empty(),
            "nothing to say when all is well"
        );

        let a = alerts(&s, &[(Page::Anthropic, 3)]);
        assert_eq!(a.len(), 1);
        assert!(a[0].probably_local());
        assert!(!a[0].confirmed());
        assert_eq!(a[0].erroring, 3);
    }

    #[test]
    fn corroborated_trouble_ranks_first_and_a_local_hint_next() {
        // Anthropic sessions fail against a clean page while OpenAI has an
        // incident that touches nothing of yours: the hint explains what is on
        // screen, the incident does not.
        let s = status(OPERATIONAL, DEGRADED);
        let a = alerts(&s, &[(Page::Anthropic, 2)]);
        assert_eq!(a[0].page, Page::Anthropic);
        assert!(a[0].probably_local());
        assert_eq!(a[1].page, Page::OpenAi);

        // Once OpenAI is failing yours too, the confirmed outage leads.
        let a = alerts(&s, &[(Page::Anthropic, 2), (Page::OpenAi, 1)]);
        assert_eq!(a[0].page, Page::OpenAi);
        assert!(a[0].confirmed());
    }

    #[test]
    fn an_unreachable_page_says_nothing() {
        // Offline: nothing answered, so no line at all — not "all clear", and
        // not an error either, even with sessions visibly failing.
        let s = Status {
            anthropic: PageStatus::Unavailable("connection refused".into()),
            openai: PageStatus::Pending,
        };
        assert!(alerts(&s, &[(Page::Anthropic, 3), (Page::OpenAi, 1)]).is_empty());
    }

    #[test]
    fn a_page_serialises_with_its_state_first() {
        let v = |status: PageStatus| serde_json::to_value(status).unwrap();
        assert_eq!(
            v(PageStatus::Pending),
            serde_json::json!({"state": "pending"})
        );
        assert_eq!(
            v(PageStatus::Unavailable("HTTP 503".into())),
            serde_json::json!({"state": "unavailable", "reason": "HTTP 503"})
        );
        let major = v(parse(MAJOR));
        assert_eq!(major["state"], "ok");
        assert_eq!(major["level"], "major");
        let first = &major["incidents"][0];
        assert_eq!(first["name"], "Elevated errors on Claude Opus");
        assert_eq!(first["stage"], "identified");
        assert_eq!(first["level"], "major");
        assert!(first["started_at"].is_i64(), "{first}");
        assert_eq!(first["update"], "A bad deploy is being rolled back.");
        assert_eq!(first["components"][1], "Claude Code");
        assert_eq!(
            v(parse(DEGRADED))["degraded"],
            serde_json::json!(["Responses", "Login"])
        );
    }

    #[test]
    fn an_alert_serialises_its_verdicts() {
        let a = &alerts(&status(OPERATIONAL, OPERATIONAL), &[(Page::Anthropic, 3)])[0];
        assert_eq!(
            serde_json::to_value(a).unwrap(),
            serde_json::json!({
                "page": "anthropic",
                "erroring": 3,
                "level": "operational",
                "headline": "All Systems Operational",
                "confirmed": false,
                "probably_local": true,
            })
        );
    }

    #[test]
    fn the_summary_line_says_who_is_to_blame() {
        let line = |s: &Status, erroring: &[(Page, usize)]| {
            summary_line(&alerts(s, erroring)).map(|(text, _)| text)
        };
        assert_eq!(line(&status(OPERATIONAL, OPERATIONAL), &[]), None);
        assert_eq!(
            line(&status(MAJOR, OPERATIONAL), &[]).as_deref(),
            Some("⚠ Anthropic incident: Elevated errors on Claude Opus")
        );
        assert_eq!(
            line(&status(OPERATIONAL, DEGRADED), &[(Page::OpenAi, 2)]).as_deref(),
            Some("⚠ OpenAI incident: Increased error rates — 2 of yours failing")
        );
        assert_eq!(
            line(&status(OPERATIONAL, OPERATIONAL), &[(Page::Anthropic, 3)]).as_deref(),
            Some("⚠ 3 failing, Anthropic reports all clear: probably this machine")
        );
    }

    fn failing(model: &str) -> Session {
        let mut s = Session::new(Provider::Claude, model.into());
        s.model = model.into();
        s.inferred_running = true;
        s.activity_state = ActivityState::ApiError;
        s
    }

    #[test]
    fn the_document_carries_every_page_and_the_line() {
        let sessions = [failing("claude-opus-5")];
        let doc = document(&status(MAJOR, OPERATIONAL), &sessions);
        assert_eq!(doc.pages.len(), 2);
        assert_eq!(doc.pages[0].erroring, 1);
        assert_eq!(doc.pages[1].site, "https://status.openai.com");
        assert!(doc.alerts[0].confirmed());
        assert_eq!(doc.line.as_ref().unwrap().tone, Tone::Outage);
        assert_eq!(
            doc.verdict.text,
            "1 of your sessions is failing, and Anthropic reports an incident"
        );
        let json = serde_json::to_value(&doc).unwrap();
        assert_eq!(json["pages"][0]["state"], "ok");
        assert_eq!(json["pages"][0]["label"], "Anthropic");
        assert_eq!(json["line"]["tone"], "outage");

        // Nothing fetched: every page pending, nothing to say.
        let doc = document(&Status::default(), &sessions);
        assert!(doc.alerts.is_empty() && doc.line.is_none());
        let json = serde_json::to_value(&doc).unwrap();
        assert_eq!(json["pages"][1]["state"], "pending");
        assert_eq!(json["line"], serde_json::Value::Null);
        assert_eq!(doc.verdict.tone, Tone::Quiet);
    }

    #[test]
    fn an_incident_clears_when_the_page_does() {
        let mut s = status(MAJOR, OPERATIONAL);
        assert_eq!(alerts(&s, &[]).len(), 1);
        s.set(Page::Anthropic, parse(OPERATIONAL));
        assert!(alerts(&s, &[]).is_empty());
    }
}
