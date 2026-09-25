//! Which sessions the table shows, and the search that widens or narrows it.
//!
//! Five filters stack — live-only, idle, age, cost floor and the query — and
//! [`App::refilter`] is the single place they are applied, because a row that
//! two of them disagree about would otherwise flicker depending on which ran
//! last. The content search sits here too rather than with the worker: the scan
//! itself is the worker's, but deciding when a query is worth reading every
//! transcript for, and what to do with hits that arrive after the user has
//! typed more, is a property of the filter.

use super::*;

/// How long typing has to pause before the query is scanned for.
///
/// A scan reads every transcript on disk, so it waits for a word rather than
/// chasing each character of one. Short enough that finishing a word and
/// looking up finds the results already there.
const SCAN_DEBOUNCE: Duration = Duration::from_millis(300);

/// Shortest query worth reading every transcript for.
const MIN_SCAN_CHARS: usize = 3;

/// A query split into its `user:` terms and the free text around them.
///
/// `user:` is a filter rather than a word to search for because a name is
/// also a word: `ana` as free text finds Ana's sessions and every session
/// titled "analysis", and on a machine root is watching the first is usually
/// what was meant. The term is a substring of the name, like the rest of the
/// query, so the rows narrow as the name is typed rather than vanishing until
/// it is complete. Several terms are alternatives — `user:ana user:bo` is both
/// of them — since one row can only have one owner and requiring all would
/// match nothing.
///
/// Only the free text is scanned for in transcripts: no transcript says whose
/// it is, and a scan for `user:ana` would find every one that mentions the
/// literal string.
pub(super) struct Query<'a> {
    pub users: Vec<&'a str>,
    pub text: String,
}

impl<'a> Query<'a> {
    /// `query` must already be lowercase.
    pub fn parse(query: &'a str) -> Self {
        let mut users = Vec::new();
        let mut text: Vec<&str> = Vec::new();
        for word in query.split_whitespace() {
            match word.strip_prefix("user:") {
                Some(name) => users.push(name),
                None => text.push(word),
            }
        }
        // A query with no `user:` in it is left exactly as typed, spaces and
        // all, so this changes nothing for anyone not using it.
        let text = match users.is_empty() {
            true => query.to_string(),
            false => text.join(" "),
        };
        Query { users, text }
    }

    fn admits(&self, s: &Session) -> bool {
        self.users.is_empty() || {
            let name = crate::config::user_label(s.owner.as_deref());
            self.users.iter().any(|u| contains_ascii_ci(name, u))
        }
    }
}

/// `haystack.to_ascii_lowercase().contains(needle)` without the allocation.
///
/// Comparing bytes is safe on UTF-8 here: ASCII case folding never touches a
/// continuation byte, so a match can only start at a character boundary.
fn contains_ascii_ci(haystack: &str, lowercase_needle: &str) -> bool {
    let (h, n) = (haystack.as_bytes(), lowercase_needle.as_bytes());
    if n.is_empty() {
        return true;
    }
    h.len() >= n.len()
        && h.windows(n.len())
            .any(|w| w.iter().zip(n).all(|(a, b)| a.to_ascii_lowercase() == *b))
}

impl App {
    /// Apply filters and sorting, then rebuild the visible index list.
    ///
    /// Selection is tracked by session key rather than row number, so a refresh
    /// that reorders the table doesn't move the cursor off whatever the user was
    /// looking at.
    pub fn refilter(&mut self) {
        let anchor = self.selected_row().map(|r| self.row_key(r));
        let now = chrono::Utc::now();
        let now_ms = now.timestamp_millis();
        let query = self.search.to_ascii_lowercase();

        let mut visible: Vec<usize> = (0..self.sessions.len())
            .filter(|&i| {
                let s = &self.sessions[i];
                if self.live_only && !s.is_running() {
                    return false;
                }
                if self.idle_only && !self.is_idle(s, now_ms) {
                    return false;
                }
                if let Some(age) = self.age_filter {
                    let ts = if s.last_active.is_empty() {
                        &s.started_at
                    } else {
                        &s.last_active
                    };
                    let within = crate::util::parse_ts(ts)
                        .map(|d| now_ms - d.timestamp_millis() <= age.max_age_ms())
                        .unwrap_or(false);
                    if !within {
                        return false;
                    }
                }
                if !self.matches_query(s, &query) {
                    return false;
                }
                if self.cost_floor > 0.0 {
                    // Sessions with unknown cost are kept: the floor can't say
                    // they're below it. "Included" cost (None) counts as zero.
                    let cost = if s.cost_available {
                        s.total_cost.unwrap_or(0.0)
                    } else {
                        // Unknown — can't disqualify.
                        return true;
                    };
                    if cost < self.cost_floor {
                        return false;
                    }
                }
                true
            })
            .collect();

        let col = self.sort_col;
        let asc = self.sort_asc;
        visible.sort_by(|&a, &b| {
            let ord = columns::compare(col, &self.sessions[a], &self.sessions[b], &now);
            if asc { ord } else { ord.reverse() }
        });

        // Sessions are sorted first, then each expanded one has its children
        // spliced in beneath it: subagents belong to their parent's position in
        // the table, not to the ordering the sort column would give them.
        let children = |i: usize| {
            let session = &self.sessions[i];
            if self.expanded.contains(&session.key()) {
                session.subagents.len()
            } else {
                0
            }
        };
        self.matched = visible.len();
        if self.tree {
            let tree = super::tree::build(
                &self.sessions,
                &visible,
                &self.collapsed,
                (col, asc),
                children,
            );
            self.visible = tree.rows;
            self.groups = tree.groups;
            self.indent = tree.indent;
        } else {
            self.visible = visible
                .iter()
                .flat_map(|&i| {
                    std::iter::once(Row::Session(i)).chain(
                        (0..children(i)).map(move |index| Row::Subagent { parent: i, index }),
                    )
                })
                .collect();
            self.groups.clear();
            self.indent.clear();
        }
        self.selected = anchor
            .and_then(|key| self.visible.iter().position(|&r| self.row_key(r) == key))
            .unwrap_or(self.selected)
            .min(self.visible.len().saturating_sub(1));
        self.ensure_available_tab();
        self.needs_redraw = true;
    }

    /// Fold this refresh's figures into the overview history buffers.
    pub(super) fn push_history(&mut self) {
        // This is a rate, not a refresh delta, so its meaning is stable when
        // the user changes --delay or a filesystem scan takes longer.
        self.global_spend.push(self.stats.spend_per_min);
        self.global_cpu.push(self.stats.total_cpu as f64);

        for s in &self.sessions {
            let Some(p) = &s.process else { continue };
            let key = s.key();
            self.cpu_history
                .entry(key.clone())
                .or_default()
                .push(p.cpu as f64);
            self.mem_history
                .entry(key)
                .or_default()
                .push(p.memory as f64 / (1024.0 * 1024.0));
        }
    }

    /// Whether the session matches the active text search.
    ///
    /// `refilter` calls [`matches_query`] directly with a query it lowercases
    /// once; this is the same predicate for callers that only have one session
    /// in hand, so the live filter and the `n`/`N` jump cannot drift apart.
    pub(super) fn matches_search(&self, s: &Session) -> bool {
        self.matches_query(s, &self.search.to_ascii_lowercase())
    }

    /// Whether a session matches `query`, which must already be lowercase.
    ///
    /// Content search widens the filter rather than replacing it: a query that
    /// names a project still finds that project's sessions, and the transcripts
    /// add whatever else mentions it. Hits only count while they belong to the
    /// query being typed — until the scan for a longer query lands, its rows are
    /// the metadata matches alone, which is a filter narrowing as you type
    /// rather than showing results for a query you have moved on from.
    pub(super) fn matches_query(&self, s: &Session, query: &str) -> bool {
        let parsed = Query::parse(query);
        if !parsed.admits(s) {
            return false;
        }
        let query = parsed.text.as_str();
        if query.is_empty() {
            return true;
        }
        // Field by field rather than one joined string. This runs per session
        // per refresh, and lowercasing them all was the whole per-refresh
        // allocation; it also stops a query matching across the seam between
        // two unrelated fields.
        let fields: [&str; 8] = [
            s.display_label(),
            &s.model,
            &s.harness,
            s.provider.as_str(),
            &s.session_id,
            &s.label_source,
            // Empty for this user's own rows, which no query can match, so
            // searching a name finds that person's sessions and nothing else.
            s.owner.as_deref().unwrap_or_default(),
            // Likewise empty for every harness but Claude Code, so `work` finds
            // that login's sessions rather than everything that mentions work.
            s.profile.as_deref().unwrap_or_default(),
        ];
        if fields.iter().any(|f| contains_ascii_ci(f, query)) {
            return true;
        }
        // The branch is derived rather than stored, so it is the one field that
        // cannot be borrowed straight off the session.
        if columns::branch_of(s).is_some_and(|b| contains_ascii_ci(&b, query)) {
            return true;
        }
        self.search_content && self.scan_query == query && self.scan_hits.contains_key(&s.key())
    }

    /// The transcript text around the selected session's content match.
    pub fn selected_snippet(&self) -> Option<&str> {
        let s = self.selected_session()?;
        let query = self.search.to_ascii_lowercase();
        (self.search_content && self.scan_query == Query::parse(&query).text)
            .then(|| self.scan_hits.get(&s.key()))
            .flatten()
            .map(String::as_str)
    }

    /// Note that the query changed, so the scan can be rescheduled.
    ///
    /// Every edit lands here, including the ones that only shorten the query:
    /// hits for a longer query are not hits for a shorter one, and leaving them
    /// applied would leave rows on screen that no longer match anything.
    pub(super) fn search_edited(&mut self) {
        self.history_cursor = None;
        self.scan_typed_at = Some(Instant::now());
        self.refilter();
    }

    /// Turn transcript searching on or off.
    pub(super) fn toggle_content_search(&mut self) {
        self.search_content = !self.search_content;
        if !self.search_content {
            // Results for a search nobody is running any more; keeping them
            // would make the next toggle show stale rows for an instant.
            self.scan_hits.clear();
            self.scan_query.clear();
        }
        self.scan_typed_at = Some(Instant::now());
        self.refilter();
    }

    /// Send the current query off to be scanned, once the typing has settled.
    ///
    /// Called every loop iteration rather than on each keystroke: a scan reads
    /// every transcript on disk, and firing one per character would spend the
    /// whole budget on prefixes of the word being typed.
    pub(super) fn tick_scan(&mut self) {
        if !self.search_content || self.scanning {
            return;
        }
        let query = Query::parse(&self.search.to_ascii_lowercase()).text;
        if query == self.scan_query {
            self.scan_typed_at = None;
            return;
        }
        // A one- or two-character query matches nearly every transcript, so it
        // is the most expensive scan to run and the least useful to read.
        // Deleting back to that length drops the results with it, rather than
        // leaving a count on screen for a query no longer being asked.
        if query.chars().count() < MIN_SCAN_CHARS {
            if !self.scan_query.is_empty() {
                self.scan_query.clear();
                self.scan_hits.clear();
                self.refilter();
                self.needs_redraw = true;
            }
            return;
        }
        match self.scan_typed_at {
            Some(at) if at.elapsed() < SCAN_DEBOUNCE => return,
            _ => {}
        }
        self.scan_typed_at = None;
        let targets: Vec<crate::session::search::Target> = self
            .sessions
            .iter()
            .map(crate::session::search::Target::of)
            .collect();
        if self.tx.send(Request::Scan { query, targets }).is_ok() {
            self.scanning = true;
            self.needs_redraw = true;
        }
    }

    /// Fold in a finished scan.
    pub(super) fn scanned(&mut self, query: String, hits: HashMap<String, String>) {
        self.scanning = false;
        self.scan_query = query;
        self.scan_hits = hits;
        self.refilter();
        self.needs_redraw = true;
    }

    /// Record the query that was just run, so ↑ can bring it back.
    pub(super) fn remember_query(&mut self) {
        let query = self.search.trim().to_string();
        if query.is_empty() {
            return;
        }
        // Re-running a query moves it to the front rather than adding a second
        // copy, which is what makes a short history worth walking.
        self.search_history.retain(|q| q != &query);
        self.search_history.insert(0, query);
        self.search_history
            .truncate(crate::cache::MAX_SEARCH_HISTORY);
        self.save_prefs();
    }

    /// Walk the query history: `1` towards older entries, `-1` back towards
    /// what was being typed when the walk started.
    pub(super) fn history_step(&mut self, delta: isize) {
        if self.search_history.is_empty() {
            return;
        }
        let (at, typed) = match self.history_cursor.take() {
            Some((at, typed)) => (at as isize + delta, typed),
            // Nothing walked yet: ↓ has nowhere older to come back from.
            None if delta < 0 => return,
            None => (0, self.search.to_string()),
        };
        // Stepping back past the newest entry restores the partial query, which
        // is the one thing the history itself cannot hold.
        if at < 0 {
            self.search.set(typed);
        } else {
            let at = (at as usize).min(self.search_history.len() - 1);
            self.search.set(self.search_history[at].as_str());
            self.history_cursor = Some((at, typed));
        }
        self.scan_typed_at = Some(Instant::now());
        self.refilter();
        self.needs_redraw = true;
    }

    /// Peel off one filter layer, narrowest first, and say which one went.
    ///
    /// One press per layer rather than all at once: filters are combined
    /// deliberately, and clearing four of them on a stray Esc would lose work
    /// that took four deliberate keystrokes to set up. Every layer that paints
    /// a badge in the footer is reachable from here, so nothing can stay on
    /// with no way to turn it off.
    /// Whether any filter layer is on, and so whether `Esc` would do anything.
    ///
    /// The same layers [`clear_one_filter`](Self::clear_one_filter) peels, in
    /// one place: a footer that offered `Esc Clear filter` with nothing to
    /// clear would be teaching a key that does nothing.
    pub(super) fn has_filter(&self) -> bool {
        !self.search.is_empty()
            || self.idle_only
            || self.cost_floor > 0.0
            || self.live_only
            || self.age_filter.is_some()
            || self.tool_tab != 0
            || self.tool_live_only
    }

    pub(super) fn clear_one_filter(&mut self) {
        let cleared = if !self.search.is_empty() {
            self.search.clear();
            "Search cleared"
        } else if self.idle_only {
            self.leave_idle_view();
            "Idle view closed"
        } else if self.cost_floor > 0.0 {
            self.cost_floor = 0.0;
            "Cost floor cleared"
        } else if self.live_only {
            self.live_only = false;
            "Showing stopped sessions too"
        } else if self.age_filter.is_some() {
            self.age_filter = None;
            "Age filter cleared"
        } else if self.tool_tab != 0 || self.tool_live_only {
            // The Tool Activity sidebar filters a panel rather than the table,
            // so it comes last: it is the layer the user is least likely to
            // have forgotten about.
            self.tool_tab = 0;
            self.tool_live_only = false;
            self.tool_follow = true;
            "Tool Activity filter cleared"
        } else {
            return;
        };
        self.refilter();
        self.save_prefs();
        self.set_status(cleared);
    }

    /// Jump to the next/previous session matching the active search, wrapping
    /// around both ends. With no search active every visible session matches.
    pub(super) fn cycle_matches(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        // Positions rather than rows: a child row matches on its parent's text,
        // so several rows can share one session and `position` would keep
        // sending the cursor back to the first of them.
        let matches: Vec<usize> = self
            .visible
            .iter()
            .enumerate()
            .filter(|(_, row)| {
                row.session()
                    .is_some_and(|i| self.matches_search(&self.sessions[i]))
            })
            .map(|(at, _)| at)
            .collect();
        if matches.is_empty() {
            return;
        }
        let pos = matches.iter().position(|&at| at == self.selected);
        let n = matches.len() as isize;
        let next = ((pos.unwrap_or(0) as isize + delta).rem_euclid(n)) as usize;
        self.selected = matches[next];
        self.ensure_available_tab();
        self.needs_redraw = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{key, session, test_app};
    use ratatui::crossterm::event::KeyCode;

    /// `user:` narrows by whose a row is, without the name also matching every
    /// title that happens to contain it; the free text beside it still applies.
    #[test]
    fn a_user_term_filters_by_owner() {
        let mut app = test_app();
        let mut winshen = session("w", true, "/srv/app");
        winshen.owner = Some("winshen".into());
        let mut analysis = session("m", true, "/srv/app");
        analysis.title = Some("winshen's analysis".into());
        let mut bo = session("b", true, "/srv/other");
        bo.owner = Some("bo".into());
        app.sessions = vec![winshen, analysis, bo];
        let shown = |app: &App| -> Vec<String> {
            app.visible
                .iter()
                .filter_map(|r| r.session())
                .map(|i| app.sessions[i].session_id.clone())
                .collect()
        };

        app.search = "user:winshen".into();
        app.refilter();
        assert_eq!(shown(&app), ["w"]);

        // Part of a name narrows as it is typed; two terms are either user.
        app.search = "user:win".into();
        app.refilter();
        assert_eq!(shown(&app), ["w"]);
        app.search = "user:winshen user:bo".into();
        app.refilter();
        assert_eq!(shown(&app).len(), 2);

        // Free text beside it still has to match.
        app.search = "user:bo other".into();
        app.refilter();
        assert_eq!(shown(&app), ["b"]);
        app.search = "user:bo /srv/app".into();
        app.refilter();
        assert!(shown(&app).is_empty());

        // This user's own rows answer to this user's own name.
        app.search = format!("user:{}", crate::config::MY_USER.to_ascii_lowercase()).into();
        app.refilter();
        assert_eq!(shown(&app), ["m"]);

        // Only the free text goes to the transcript scan.
        assert_eq!(Query::parse("user:bo  needle").text, "needle");
        assert_eq!(Query::parse("plain  query").text, "plain  query");
    }

    #[test]
    fn live_filter_hides_stopped_sessions() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "x"), session("b", false, "y")];
        app.live_only = true;
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(
            app.sessions[app.visible[0].session().unwrap()].session_id,
            "a"
        );
    }

    #[test]
    fn live_filter_includes_transcript_inferred_cursor_session() {
        let mut app = test_app();
        let mut cursor = Session::new(Provider::Cursor, "cursor".into());
        cursor.started_at = chrono::Utc::now().to_rfc3339();
        cursor.last_active = cursor.started_at.clone();
        cursor.inferred_running = true;
        app.sessions = vec![cursor];
        app.live_only = true;
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert!(app.sessions[app.visible[0].session().unwrap()].is_running());
        assert!(
            app.sessions[app.visible[0].session().unwrap()]
                .process
                .is_none()
        );
    }

    #[test]
    fn search_matches_label_and_id_case_insensitively() {
        let mut app = test_app();
        app.sessions = vec![
            session("aaa", false, "/home/x/alpha"),
            session("bbb", false, "/home/x/beta"),
        ];
        app.search = "alpha".into();
        app.refilter();
        assert_eq!(app.visible.len(), 1);

        app.search = "BBB".into();
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(
            app.sessions[app.visible[0].session().unwrap()].session_id,
            "bbb"
        );
    }

    /// The table abbreviates the working directory to fit its column, so the
    /// filter has to match the full path — otherwise the directory someone
    /// types is one the row is not admitting to.
    #[test]
    fn search_matches_the_full_working_directory() {
        let mut app = test_app();
        let mut deep = session("aaa", false, "/home/x/work/api/services/billing");
        // What the table actually shows for that row.
        deep.abbrev_label = "…/billing".into();
        app.sessions = vec![deep, session("bbb", false, "/home/x/other")];

        app.search = "work/api".into();
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(
            app.sessions[app.visible[0].session().unwrap()].session_id,
            "aaa"
        );
    }

    /// Content hits widen the filter, and only for the query they were found
    /// for: a scan that lands after the user has typed another character must
    /// not put its rows back on screen.
    #[test]
    fn transcript_hits_widen_the_filter_for_their_own_query_only() {
        let mut app = test_app();
        app.sessions = vec![
            session("aaa", false, "/home/x/alpha"),
            session("bbb", false, "/home/x/beta"),
        ];
        let hit = |key: &str| HashMap::from([(key.to_string(), "…flywheel…".to_string())]);

        // No content search: a word only the transcript knows finds nothing.
        app.search = "flywheel".into();
        app.refilter();
        assert!(app.visible.is_empty());

        // With one, the session whose transcript matched joins the metadata
        // matches rather than replacing them.
        app.search_content = true;
        app.scanned("flywheel".into(), hit("claude:bbb"));
        assert_eq!(app.visible.len(), 1);
        assert_eq!(
            app.sessions[app.visible[0].session().unwrap()].session_id,
            "bbb"
        );

        app.search = "alpha".into();
        app.scanned("alpha".into(), hit("claude:bbb"));
        assert_eq!(
            app.visible.len(),
            2,
            "metadata and content matches, not one"
        );

        // Hits belonging to a query that has since been extended are ignored.
        app.search = "alphabet".into();
        app.refilter();
        assert!(app.visible.is_empty());
        assert!(app.selected_snippet().is_none());
    }

    /// Turning content search off has to take its results with it, or the rows
    /// it found stay on screen with nothing matching them.
    #[test]
    fn leaving_content_search_drops_its_rows() {
        let mut app = test_app();
        app.sessions = vec![session("aaa", false, "/home/x/alpha")];
        app.search = "flywheel".into();
        app.search_content = true;
        app.scanned(
            "flywheel".into(),
            HashMap::from([("claude:aaa".into(), "…flywheel…".into())]),
        );
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.selected_snippet(), Some("…flywheel…"));

        app.toggle_content_search();
        assert!(app.visible.is_empty());
        assert!(app.selected_snippet().is_none());
    }

    /// A scan is worth its cost only once there is a word to look for, and only
    /// for a query that isn't already answered.
    #[test]
    fn a_scan_waits_for_a_word_and_for_the_typing_to_settle() {
        let mut app = test_app();
        app.sessions = vec![session("aaa", false, "/home/x/alpha")];
        app.search_content = true;

        // Too short to be worth reading every transcript for.
        app.search = "fl".into();
        app.search_edited();
        app.scan_typed_at = None;
        app.tick_scan();
        assert!(!app.scanning);

        // Long enough, but the user is still typing.
        app.search = "flywheel".into();
        app.search_edited();
        app.tick_scan();
        assert!(!app.scanning, "fired before the debounce elapsed");

        // Settled.
        app.scan_typed_at = Some(Instant::now() - SCAN_DEBOUNCE);
        app.tick_scan();
        assert!(app.scanning);

        // And the answer to a query already scanned for is not scanned again.
        app.scanned("flywheel".into(), HashMap::new());
        app.tick_scan();
        assert!(!app.scanning);
    }

    /// ↑ walks back through past queries and ↓ returns, ending on whatever was
    /// half-typed when the walk began.
    #[test]
    fn the_query_history_walks_both_ways() {
        let mut app = test_app();
        app.search_history = vec!["newest".into(), "older".into()];

        app.search = "half-typ".into();
        app.history_step(1);
        assert_eq!(app.search, "newest");
        app.history_step(1);
        assert_eq!(app.search, "older");
        // The end of the history is a floor, not a wrap.
        app.history_step(1);
        assert_eq!(app.search, "older");

        app.history_step(-1);
        assert_eq!(app.search, "newest");
        app.history_step(-1);
        assert_eq!(app.search, "half-typ");
        // Nothing older to come back from any more.
        app.history_step(-1);
        assert_eq!(app.search, "half-typ");
    }

    /// Re-running a query moves it to the front rather than filling the history
    /// with copies of the search someone runs most.
    #[test]
    fn a_repeated_query_is_remembered_once() {
        let mut app = test_app();
        app.search = "alpha".into();
        app.remember_query();
        app.search = "beta".into();
        app.remember_query();
        app.search = "alpha".into();
        app.remember_query();
        assert_eq!(app.search_history, vec!["alpha", "beta"]);

        // An abandoned modal leaves nothing behind.
        app.search = "   ".into();
        app.remember_query();
        assert_eq!(app.search_history, vec!["alpha", "beta"]);
    }

    #[test]
    fn age_filter_excludes_old_sessions() {
        let mut app = test_app();
        let mut old = session("old", false, "/x");
        old.last_active = (chrono::Utc::now() - chrono::Duration::days(10)).to_rfc3339();
        app.sessions = vec![session("new", false, "/x"), old];
        app.age_filter = Some(AgeFilter::Day);
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(
            app.sessions[app.visible[0].session().unwrap()].session_id,
            "new"
        );
    }

    #[test]
    fn cost_floor_filters_by_total_cost() {
        let mut app = test_app();
        let mut cheap = session("cheap", false, "/x");
        cheap.total_cost = Some(0.50);
        let mut pricey = session("pricey", false, "/x");
        pricey.total_cost = Some(5.00);
        app.sessions = vec![cheap, pricey];
        app.cost_floor = 1.0;
        app.refilter();
        assert_eq!(app.visible.len(), 1);
        assert_eq!(
            app.sessions[app.visible[0].session().unwrap()].session_id,
            "pricey"
        );
    }

    #[test]
    fn cost_floor_zero_shows_everything() {
        let mut app = test_app();
        app.sessions = vec![session("a", false, "/x"), session("b", false, "/x")];
        app.cost_floor = 0.0;
        app.refilter();
        assert_eq!(app.visible.len(), 2);
    }

    #[test]
    fn cycle_matches_wraps_around_search_matches() {
        let mut app = test_app();
        let now = chrono::Utc::now().to_rfc3339();
        let mk = |id: &str, label: &str| {
            let mut s = session(id, false, label);
            s.last_active = now.clone();
            s.started_at = now.clone();
            s
        };
        app.sessions = vec![mk("a", "/x/match"), mk("b", "/y"), mk("c", "/z/match")];
        app.search = "match".into();
        app.refilter();
        assert_eq!(app.visible.len(), 2);
        app.selected = 0;
        app.cycle_matches(1);
        assert_eq!(app.selected_session().unwrap().session_id, "c");
        app.cycle_matches(1);
        assert_eq!(app.selected_session().unwrap().session_id, "a");
        app.cycle_matches(-1);
        assert_eq!(app.selected_session().unwrap().session_id, "c");
    }

    /// Every filter that paints a badge must be reachable from Esc, one press
    /// at a time.
    #[test]
    fn esc_clears_one_filter_layer_per_press() {
        let mut app = test_app();
        app.sessions = vec![session("a", true, "/x")];
        app.search = "x".into();
        app.cost_floor = 1.0;
        app.live_only = true;
        app.age_filter = Some(AgeFilter::Day);
        app.tool_tab = 2;
        app.idle_only = true;
        app.refilter();

        for expected in 1..=6 {
            app.on_key(key(KeyCode::Esc));
            let left = [
                !app.search.is_empty(),
                app.idle_only,
                app.cost_floor > 0.0,
                app.live_only,
                app.age_filter.is_some(),
                app.tool_tab != 0,
            ]
            .iter()
            .filter(|on| **on)
            .count();
            assert_eq!(
                left,
                6 - expected,
                "press {expected} cleared the wrong count"
            );
        }
        // A seventh press is harmless.
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.mode, Mode::List);
    }

    /// The live filter and the n/N jump must agree, because they are now the
    /// same predicate.
    #[test]
    fn refilter_and_matches_search_agree() {
        let mut app = test_app();
        app.sessions = vec![
            session("aaa", false, "/home/x/Alpha"),
            session("bbb", false, "/home/x/beta"),
        ];
        for query in ["alpha", "ALPHA", "x/", "", "nomatch"] {
            app.search = query.into();
            app.refilter();
            let by_predicate: Vec<usize> = (0..app.sessions.len())
                .filter(|&i| app.matches_search(&app.sessions[i]))
                .collect();
            assert_eq!(app.visible.len(), by_predicate.len(), "query {query:?}");
        }
    }

    #[test]
    fn case_insensitive_contains_matches_std() {
        for (h, n) in [
            ("Alpha/Beta", "beta"),
            ("Alpha", "alpha"),
            ("Alpha", ""),
            ("a", "aa"),
            ("héllo-World", "world"),
            ("nope", "zz"),
        ] {
            assert_eq!(
                contains_ascii_ci(h, n),
                h.to_ascii_lowercase().contains(n),
                "{h:?} / {n:?}"
            );
        }
    }
}
