//! Whether the parsers are reading what the harnesses write.
//!
//! The rest of doctor asks whether cctop can *find* things. This asks whether
//! what it found adds up, because the failures that cost the most here have
//! all failed closed: Codex wrote its shell outcomes as plain text that the
//! parser did not read, so every failing command counted as a success, and
//! `cctop compare` priced most models at nothing because the price table was
//! never loaded on its path. Both ran for months, because "no errors" and
//! "$0.00" look like good news rather than like missing data.
//!
//! Neither leaves a mark in any single session — a session with no failures
//! and one with unread failures are the same row. They only show in the
//! aggregate, as numbers that are implausible across a whole corpus: thousands
//! of calls and not one of them failed, a model burning millions of tokens for
//! nothing. So each check here is a decision over totals, and each is tuned to
//! stay quiet unless the total is too large to be chance; a doctor that cries
//! wolf is one nobody reads.
//!
//! Everything found here is a `Warn`, never a `Fail`. These are inferences
//! from the data rather than facts about the installation, and the most common
//! of them — a model LiteLLM has not listed yet — is a day's lag upstream, not
//! something wrong with this machine. The exit code keeps meaning "is this
//! installation sound".

use super::{Check, Level, Section, ok, warn};
use crate::pricing::{Listing, Provider};
use crate::session::{ModelBreakdown, Session};
use crate::util::{compact_tokens, with_commas};
use std::collections::HashMap;

/// Calls a provider must have recorded before zero failures among them is
/// suspicious rather than lucky.
///
/// Agents fail a few percent of their calls in ordinary use — a test that
/// fails, a grep with no match, an edit whose anchor moved — and a real rate
/// below 1% is rare. At 1%, the chance of 500 calls in a row succeeding is
/// 0.99^500, about 0.7%; at a more typical 3% it is effectively nil. So a
/// zero at this size is the parser far more often than it is the user.
const OUTCOMES_MIN_CALLS: u64 = 500;

/// The failure share above which the parser is more likely reading successes
/// as failures than the agent is failing.
///
/// The mirror of the check above. Even a struggling session rarely fails a
/// third of its calls, and a whole provider's corpus averages far below that;
/// more than half, across [`OUTCOMES_MIN_CALLS`] or more, is a parser that has
/// its test the wrong way round.
const OUTCOMES_MAX_SHARE: f64 = 0.5;

/// Sessions with activity that a provider must have before zero tool calls, or
/// zero tokens, across all of them points at the parser.
///
/// A single session can be a question with no tool in it, and a handful can be
/// someone trying a harness out as a chat. Twenty sessions of a coding agent
/// that never once read a file is not how these tools get used.
const MIN_ACTIVE_SESSIONS: u64 = 20;

/// Tokens a model must carry before its missing price is worth a line.
///
/// Below this the missing dollars round to cents at any common rate (ten
/// thousand tokens is fifteen cents at $15 per million), and naming every
/// model someone tried once buries the one burning millions.
const UNPRICED_MIN_TOKENS: u64 = 10_000;

/// How many unpriced models are named before the rest are counted instead.
///
/// Someone routing through a gateway can have dozens; the largest few say
/// what is wrong, and the rest only make the section scroll.
const UNPRICED_SHOWN: usize = 8;

/// The order providers are reported in: the order the sources section above
/// lists them, so the two read against each other.
const ORDER: &[Provider] = &[
    Provider::Claude,
    Provider::Codex,
    Provider::Cursor,
    Provider::Devin,
    Provider::Gemini,
    Provider::OpenCode,
    Provider::Pi,
    Provider::Windsurf,
];

fn name(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "Claude Code",
        Provider::Codex => "Codex",
        Provider::Cursor => "Cursor",
        Provider::Devin => "Devin",
        Provider::Gemini => "Gemini CLI",
        Provider::OpenCode => "OpenCode",
        Provider::Pi => "Pi",
        Provider::Windsurf => "Windsurf",
    }
}

const REPORT: &str = "https://github.com/flolep2607/cctop/issues";

/// One provider's sessions, added up.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Tally {
    sessions: u64,
    calls: u64,
    errors: u64,
    tokens: u64,
    /// Sessions that recorded any tokens, which is what "active" means for a
    /// provider that records them at all.
    with_tokens: u64,
    with_calls: u64,
}

impl Tally {
    /// Sessions that did something, by whichever measure this provider keeps.
    ///
    /// Tokens where there are tokens, because a session opened and closed
    /// again without a word is neither a chat nor a parser fault. Where the
    /// harness records none, every session it wrote down has to stand in.
    fn active(&self, provider: Provider) -> u64 {
        match provider.records_token_usage() {
            true => self.with_tokens,
            false => self.sessions,
        }
    }
}

pub(super) fn tally(sessions: &[Session]) -> HashMap<Provider, Tally> {
    let mut out: HashMap<Provider, Tally> = HashMap::new();
    for s in sessions {
        let t = out.entry(s.provider).or_default();
        let tokens = s.input_tokens + s.output_tokens;
        t.sessions += 1;
        t.calls += s.tool_count;
        t.errors += s.tool_errors;
        t.tokens += tokens;
        t.with_tokens += u64::from(tokens > 0);
        t.with_calls += u64::from(s.tool_count > 0);
    }
    out
}

/// A provider that records outcomes, with many calls and not one failure.
pub(super) fn outcomes_unseen(provider: Provider, t: &Tally) -> Option<Check> {
    (provider.records_tool_outcomes() && t.calls >= OUTCOMES_MIN_CALLS && t.errors == 0).then(
        || {
            warn(
                name(provider),
                format!(
                    "{} tool calls and not one failed — the parser is probably not \
                     reading failures, so its error rates read 0%",
                    with_commas(t.calls)
                ),
                &format!(
                    "a real agent fails a few percent of its calls; if this harness \
                     updated recently, report it at {REPORT}"
                ),
            )
        },
    )
}

/// A provider whose failures outnumber its successes, which is a parser
/// reading the wrong way round rather than an agent that bad.
pub(super) fn outcomes_inverted(provider: Provider, t: &Tally) -> Option<Check> {
    let share = t.errors as f64 / t.calls.max(1) as f64;
    (provider.records_tool_outcomes()
        && t.calls >= OUTCOMES_MIN_CALLS
        && share > OUTCOMES_MAX_SHARE)
        .then(|| {
            warn(
                name(provider),
                format!(
                    "{} of {} tool calls counted as failed ({:.0}%) — more likely the \
                     parser is reading successes as failures",
                    with_commas(t.errors),
                    with_commas(t.calls),
                    share * 100.0
                ),
                &format!("report it at {REPORT}, with the harness's version"),
            )
        })
}

/// Many active sessions and not a single tool call among them.
pub(super) fn calls_unseen(provider: Provider, t: &Tally) -> Option<Check> {
    let active = t.active(provider);
    (active >= MIN_ACTIVE_SESSIONS && t.calls == 0).then(|| {
        warn(
            name(provider),
            format!(
                "{active} active session(s) and not one tool call recorded — the parser \
                 is probably not reading its calls"
            ),
            &format!(
                "tool counts, error rates and the Tools panel are empty for it; \
                 if the harness updated recently, report it at {REPORT}"
            ),
        )
    })
}

/// Many sessions that made calls and not a token between them, from a
/// harness that records tokens.
pub(super) fn tokens_unseen(provider: Provider, t: &Tally) -> Option<Check> {
    (provider.records_token_usage() && t.with_calls >= MIN_ACTIVE_SESSIONS && t.tokens == 0).then(
        || {
            warn(
                name(provider),
                format!(
                    "{} session(s) made tool calls and none recorded a token — the \
                     parser is probably not reading usage",
                    t.with_calls
                ),
                &format!(
                    "tokens and costs read zero for it, which looks free; \
                     report it at {REPORT}"
                ),
            )
        },
    )
}

/// The line for a provider with nothing wrong: what it adds up to, so a clean
/// run still says what it judged clean.
fn provider_ok(provider: Provider, t: &Tally) -> Check {
    let calls = match (provider.records_tool_outcomes(), t.calls) {
        (_, 0) => "no tool calls".to_string(),
        (true, n) => format!(
            "{} calls, {:.1}% failed",
            with_commas(n),
            t.errors as f64 * 100.0 / n as f64
        ),
        // Saying "0% failed" here is the exact claim this section exists to
        // stop cctop making; the harness writes no outcome to read.
        (false, n) => format!("{} calls, outcomes not recorded", with_commas(n)),
    };
    let tokens = match provider.records_token_usage() {
        true => format!(", {} tokens", compact_tokens(t.tokens)),
        false => String::new(),
    };
    ok(
        name(provider),
        format!("{} session(s), {calls}{tokens}", t.sessions),
    )
}

/// One model's tokens that came to `$0.00`, summed over its sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Zero {
    provider: Provider,
    model: String,
    sessions: u64,
    tokens: u64,
}

/// Every model that carried tokens and cost nothing, per provider.
///
/// Per model rather than per session, because a session that switched models
/// can be half priced: its total is not zero, and the half that is would hide
/// inside it. Only sessions whose provider records dollars at all, since a
/// Devin or Cursor `$0.00` is a blank, not a price.
pub(super) fn zero_cost_models<'a>(
    sessions: impl IntoIterator<Item = (&'a Session, &'a [ModelBreakdown])>,
) -> Vec<Zero> {
    let mut by: HashMap<(Provider, String), (u64, u64)> = HashMap::new();
    for (s, breakdown) in sessions {
        if !s.cost_available {
            continue;
        }
        for mb in breakdown {
            let tokens = mb.tokens.all_input() + mb.tokens.output;
            if tokens == 0 || mb.total != 0.0 {
                continue;
            }
            let e = by.entry((s.provider, mb.model.clone())).or_default();
            e.0 += 1;
            e.1 += tokens;
        }
    }
    let mut out: Vec<Zero> = by
        .into_iter()
        .filter(|(_, (_, tokens))| *tokens >= UNPRICED_MIN_TOKENS)
        .map(|((provider, model), (sessions, tokens))| Zero {
            provider,
            model,
            sessions,
            tokens,
        })
        .collect();
    // Largest first: the model costing the most missing dollars is the one
    // the reader came for, and the cap below must never be what hides it.
    out.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.model.cmp(&b.model)));
    out
}

/// Say what each zero is: free, unpriced, or priced and not applied.
///
/// `listing` is [`crate::pricing::listing`] in use and a fixed answer under
/// test. `table_age` says how old the LiteLLM table is, `None` when there is
/// none loaded — in which case "unlisted" means only "not built in", and the
/// pricing section above has already failed for the real reason.
pub(super) fn zero_checks(
    zeros: &[Zero],
    listing: impl Fn(Provider, &str) -> Listing,
    table_age: Option<String>,
) -> Vec<Check> {
    let mut checks = Vec::new();
    let mut unlisted = Vec::new();
    for z in zeros {
        let what = format!(
            "{} ({}): {} session(s), {} tokens at $0.00",
            display_model(&z.model),
            name(z.provider),
            z.sessions,
            compact_tokens(z.tokens)
        );
        match listing(z.provider, &z.model) {
            // Worth knowing and not worth fixing: the one zero that is true.
            Listing::Free => checks.push(ok("free model", format!("{what}, listed as free"))),
            Listing::Unlisted if named_free(&z.model) => {
                checks.push(ok("free model", format!("{what}, named as free")))
            }
            // The compare bug's shape: a rate exists and the figure did not
            // get it. Nothing upstream to wait for, so this is cctop's.
            Listing::Priced => checks.push(warn(
                "price not applied",
                format!("{what}, though the price table has a rate for it"),
                &format!(
                    "cctop --clear-cache re-prices every session; if it stays at $0.00, \
                     the parser is not pricing it — report it at {REPORT}"
                ),
            )),
            Listing::Unlisted => unlisted.push(what),
        }
    }

    let hidden = unlisted.len().saturating_sub(UNPRICED_SHOWN);
    let fix = match &table_age {
        Some(age) => format!(
            "no price table lists these, so their cost is missing, not free; LiteLLM's \
             table here is {age}, and a new model is priced once LiteLLM adds it"
        ),
        None => "no LiteLLM table is loaded (see Pricing above), so anything not \
                 built in prices at $0.00 until it downloads"
            .to_string(),
    };
    let shown = unlisted.len() - hidden;
    for (i, what) in unlisted.into_iter().take(shown).enumerate() {
        // The advice once, under the last of them: the cause is the same for
        // every line, and eight copies of it would bury the names.
        let last = i + 1 == shown;
        checks.push(Check {
            level: Level::Warn,
            label: "unpriced model".into(),
            detail: what,
            fix: last.then(|| match hidden {
                0 => fix.clone(),
                n => format!("and {n} more like these; {fix}"),
            }),
        });
    }
    checks
}

/// Whether a model's own name says it is free, as gateways spell their free
/// tiers: OpenRouter's `:free`, OpenCode Zen's `-free`.
///
/// Those tiers are exactly what LiteLLM does not list — there is no rate to
/// list — so without this every one of them would be reported as unpriced,
/// on every run, to someone who chose it because it costs nothing.
// ponytail: trusts the name. A paid model whose name ends in `-free` would be
// missed, but
// a vendor naming a paid model that way would be misleading its own buyers.
fn named_free(model: &str) -> bool {
    let lower = model.to_ascii_lowercase();
    lower.ends_with(":free") || lower.ends_with("-free")
}

/// A model name as the reader will search for it.
///
/// An empty name is its own finding — the parser saw usage and no model —
/// and printed bare it would look like a formatting slip.
fn display_model(model: &str) -> &str {
    match model {
        "" => "(no model recorded)",
        m => m,
    }
}

/// The Parsers section: every provider's totals judged, then every zero price.
pub(super) fn section(
    sessions: &[Session],
    data: &[std::sync::Arc<crate::session::SessionData>],
) -> Section {
    let tallies = tally(sessions);
    let mut checks = Vec::new();
    for &provider in ORDER {
        let Some(t) = tallies.get(&provider) else {
            continue;
        };
        let found: Vec<Check> = [
            outcomes_unseen(provider, t),
            outcomes_inverted(provider, t),
            calls_unseen(provider, t),
            tokens_unseen(provider, t),
        ]
        .into_iter()
        .flatten()
        .collect();
        match found.is_empty() {
            true => checks.push(provider_ok(provider, t)),
            false => checks.extend(found),
        }
    }

    let zeros = zero_cost_models(
        sessions
            .iter()
            .zip(data)
            .map(|(s, d)| (s, d.model_breakdown.as_slice())),
    );
    let table_age = (crate::pricing::pricing_epoch() != 0).then(|| {
        super::cache_age_secs(&crate::config::PRICING_CACHE_FILE)
            .map(|secs| format!("{} old", crate::util::long_duration((secs * 1000) as i64)))
            .unwrap_or_else(|| "of unknown age".into())
    });
    let priced = zero_checks(&zeros, crate::pricing::listing, table_age);
    // Only where something records dollars: a machine with nothing but Cursor
    // has no prices to vouch for, and saying they are fine would be a guess.
    if !priced.iter().any(|c| c.level != Level::Ok) && sessions.iter().any(|s| s.cost_available) {
        checks.push(ok(
            "pricing",
            "every model that used tokens has a rate, or is free",
        ));
    }
    checks.extend(priced);

    // Worst news first, as every section here promises; stable, so a
    // provider's own lines keep the order above.
    checks.sort_by_key(|c| std::cmp::Reverse(c.level));
    Section {
        title: "Parsers",
        checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Costs, Tokens};

    /// `n` sessions of `provider`, each carrying the given share of the
    /// totals — what a walk hands the section, minus everything it ignores.
    fn rows(provider: Provider, n: u64, calls: u64, errors: u64, tokens: u64) -> Vec<Session> {
        (0..n)
            .map(|i| {
                let mut s = Session::new(provider, format!("{}-{i}", provider.as_str()));
                s.tool_count = calls;
                s.tool_errors = errors;
                s.input_tokens = tokens;
                s
            })
            .collect()
    }

    fn total(provider: Provider, sessions: &[Session]) -> Tally {
        tally(sessions).remove(&provider).expect("a tally")
    }

    /// The Codex bug as it was: hundreds of calls, every one a success.
    #[test]
    fn many_calls_and_no_failure_is_called_out() {
        let t = total(Provider::Codex, &rows(Provider::Codex, 20, 40, 0, 1000));
        let check = outcomes_unseen(Provider::Codex, &t).expect("fires");
        assert_eq!(check.level, Level::Warn);
        assert!(check.detail.contains("800 tool calls"), "{}", check.detail);
    }

    /// Below the threshold, zero failures is plausibly a careful user, and
    /// one failure anywhere proves the parser can see them.
    #[test]
    fn no_failure_is_quiet_when_it_could_be_luck() {
        let few = total(Provider::Codex, &rows(Provider::Codex, 4, 100, 0, 1000));
        assert!(outcomes_unseen(Provider::Codex, &few).is_none());

        let mut many = rows(Provider::Codex, 20, 40, 0, 1000);
        many[3].tool_errors = 1;
        assert!(outcomes_unseen(Provider::Codex, &total(Provider::Codex, &many)).is_none());
    }

    /// Pi records calls but never an outcome, so zero failures is all it can
    /// ever say — and calling that broken would be a false alarm every run.
    #[test]
    fn a_harness_that_records_no_outcome_is_never_accused_of_hiding_one() {
        let t = total(Provider::Pi, &rows(Provider::Pi, 50, 100, 0, 1000));
        assert!(outcomes_unseen(Provider::Pi, &t).is_none());
        assert!(outcomes_inverted(Provider::Pi, &t).is_none());
        // And its summary line does not claim the 0% it cannot know.
        assert!(
            provider_ok(Provider::Pi, &t)
                .detail
                .contains("outcomes not recorded")
        );
    }

    #[test]
    fn mostly_failures_reads_as_a_parser_the_wrong_way_round() {
        let bad = total(Provider::Gemini, &rows(Provider::Gemini, 10, 100, 60, 1000));
        assert!(outcomes_inverted(Provider::Gemini, &bad).is_some());
        // A bad week is a high rate, not a majority.
        let rough = total(Provider::Gemini, &rows(Provider::Gemini, 10, 100, 30, 1000));
        assert!(outcomes_inverted(Provider::Gemini, &rough).is_none());
        // And a small sample can fail however it likes.
        let small = total(Provider::Gemini, &rows(Provider::Gemini, 2, 100, 90, 1000));
        assert!(outcomes_inverted(Provider::Gemini, &small).is_none());
    }

    #[test]
    fn active_sessions_with_no_call_at_all_point_at_the_parser() {
        let t = total(
            Provider::OpenCode,
            &rows(Provider::OpenCode, 25, 0, 0, 5000),
        );
        assert!(calls_unseen(Provider::OpenCode, &t).is_some());

        // A few chats are chats.
        let chats = total(Provider::OpenCode, &rows(Provider::OpenCode, 5, 0, 0, 5000));
        assert!(calls_unseen(Provider::OpenCode, &chats).is_none());

        // Sessions that never said a word are not activity, for a harness
        // that would have recorded the words.
        let empty = total(Provider::OpenCode, &rows(Provider::OpenCode, 25, 0, 0, 0));
        assert!(calls_unseen(Provider::OpenCode, &empty).is_none());

        // Windsurf records no tokens, so every session it lists has to count.
        let windsurf = total(Provider::Windsurf, &rows(Provider::Windsurf, 25, 0, 0, 0));
        assert!(calls_unseen(Provider::Windsurf, &windsurf).is_some());
    }

    #[test]
    fn calls_with_no_tokens_point_at_the_usage_parser_where_there_is_one() {
        let t = total(Provider::Claude, &rows(Provider::Claude, 25, 10, 1, 0));
        assert!(tokens_unseen(Provider::Claude, &t).is_some());
        // Cursor keeps its accounting server-side: no tokens is the norm.
        let cursor = total(Provider::Cursor, &rows(Provider::Cursor, 25, 10, 0, 0));
        assert!(tokens_unseen(Provider::Cursor, &cursor).is_none());
        let some = total(Provider::Claude, &rows(Provider::Claude, 25, 10, 1, 100));
        assert!(tokens_unseen(Provider::Claude, &some).is_none());
    }

    fn breakdown(model: &str, tokens: u64, total: f64) -> ModelBreakdown {
        ModelBreakdown {
            model: model.into(),
            tokens: Tokens {
                input: tokens,
                ..Default::default()
            },
            costs: Costs::default(),
            total,
        }
    }

    /// Per model, not per session: a session that is half priced is not a
    /// zero, and the half that is must still be found.
    #[test]
    fn zero_cost_models_are_found_inside_priced_sessions() {
        let a = Session::new(Provider::OpenCode, "a".into());
        let b = Session::new(Provider::OpenCode, "b".into());
        let mut devin = Session::new(Provider::Devin, "d".into());
        devin.cost_available = false;
        let a_models = vec![
            breakdown("paid", 50_000, 1.0),
            breakdown("mystery", 30_000, 0.0),
        ];
        let b_models = vec![
            breakdown("mystery", 20_000, 0.0),
            breakdown("crumb", 50, 0.0),
        ];
        let d_models = vec![breakdown("swe-1", 900_000, 0.0)];
        let zeros = zero_cost_models([
            (&a, a_models.as_slice()),
            (&b, b_models.as_slice()),
            (&devin, d_models.as_slice()),
        ]);
        // `paid` is priced, `crumb` is under the floor, and Devin records no
        // dollars at all, so its zero is a blank rather than a price.
        assert_eq!(
            zeros,
            vec![Zero {
                provider: Provider::OpenCode,
                model: "mystery".into(),
                sessions: 2,
                tokens: 50_000,
            }]
        );
    }

    fn zero(model: &str, tokens: u64) -> Zero {
        Zero {
            provider: Provider::OpenCode,
            model: model.into(),
            sessions: 1,
            tokens,
        }
    }

    /// The things a `$0.00` can be, kept apart: only a free model is fine,
    /// and a rate that exists but was not applied is cctop's own bug.
    #[test]
    fn a_zero_is_told_apart_as_free_unpriced_or_unapplied() {
        let zeros = [
            zero("listed-free", 3_000_000),
            zero("unknown", 2_000_000),
            zero("has-a-rate", 1_000_000),
            zero("zen-model-free", 500_000),
        ];
        let listing = |_: Provider, model: &str| match model {
            "listed-free" => Listing::Free,
            "has-a-rate" => Listing::Priced,
            _ => Listing::Unlisted,
        };
        let checks = zero_checks(&zeros, listing, Some("3h old".into()));
        let find = |label: &str| {
            checks
                .iter()
                .filter(|c| c.label == label)
                .collect::<Vec<_>>()
        };

        let free = find("free model");
        assert_eq!(free.len(), 2);
        assert!(free.iter().all(|c| c.level == Level::Ok && c.fix.is_none()));

        let unapplied = find("price not applied");
        assert_eq!(unapplied.len(), 1);
        assert!(unapplied[0].detail.contains("has-a-rate"));

        let unpriced = find("unpriced model");
        assert_eq!(unpriced.len(), 1);
        assert_eq!(unpriced[0].level, Level::Warn);
        assert!(unpriced[0].detail.contains("unknown"));
        assert!(unpriced[0].fix.as_deref().unwrap().contains("3h old"));
    }

    /// With no table at all, the pricing section has already failed, and the
    /// advice here has to point there rather than at LiteLLM's backlog.
    #[test]
    fn without_a_table_the_advice_names_the_table() {
        let checks = zero_checks(&[zero("x", 50_000)], |_, _| Listing::Unlisted, None);
        assert!(
            checks[0]
                .fix
                .as_deref()
                .unwrap()
                .contains("no LiteLLM table")
        );
    }

    /// The advice comes once, under the last name shown, and the overflow is
    /// counted rather than dropped.
    #[test]
    fn a_long_list_of_unpriced_models_is_capped_and_counted() {
        let zeros: Vec<Zero> = (0..UNPRICED_SHOWN + 3)
            .map(|i| zero(&format!("m{i}"), 20_000))
            .collect();
        let checks = zero_checks(&zeros, |_, _| Listing::Unlisted, Some("1h old".into()));
        assert_eq!(checks.len(), UNPRICED_SHOWN);
        let fixes: Vec<&str> = checks.iter().filter_map(|c| c.fix.as_deref()).collect();
        assert_eq!(fixes.len(), 1);
        assert!(fixes[0].starts_with("and 3 more"), "{}", fixes[0]);
    }

    #[test]
    fn usage_with_no_model_is_named_as_such() {
        let checks = zero_checks(
            &[zero("", 50_000)],
            |_, _| Listing::Unlisted,
            Some("1h old".into()),
        );
        assert!(checks[0].detail.starts_with("(no model recorded)"));
    }
}
