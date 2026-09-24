//! The tree view: sessions grouped by repository, then by checkout.
//!
//! htop's `F5` for agents. A flat table answers "what is running"; with a dozen
//! agents across three repositories and their worktrees, the question is as
//! often "what is running *in this repository*", and the flat table makes that
//! a matter of reading every PROJECT cell.
//!
//! Drawn inside the existing table rather than with `tui-tree-widget`. That
//! crate renders a tree of one-line labels; the rows here are the table's own,
//! with a dozen aligned columns, per-cell colour, click-to-sort headers, query
//! highlighting and a cursor that everything from the bottom panels to the row
//! menu already follows through [`App::visible`]. Adopting the widget would mean
//! either giving all that up or rebuilding it inside a widget that owns its own
//! selection state — two cursors to keep in agreement. The tree only needs a
//! few more rows in `visible` and a prefix of `├─` glyphs on the label, so that
//! is all it adds.
//!
//! Only what is known is nested. A repository is known from its git common
//! directory, which is what makes every worktree of one repository land under
//! one heading; a checkout is the directory holding the `.git`. Subagents nest
//! under their session exactly as they do in the flat table. No harness records
//! that one *session* started another — a Claude resume or a Codex fork keeps
//! only the id it came from, not a live parent — so sessions are never nested
//! under each other.

use super::*;
use crate::session::ActivityState;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};

/// One heading in the tree: a repository, or one checkout of it.
///
/// The aggregates are over the sessions *shown* beneath it, after filtering, so
/// the heading always adds up to the rows it folds — a total that included
/// sessions the filter had hidden would be a number with nothing on screen to
/// check it against.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Group {
    /// Stable across refreshes, and what the fold state is remembered under.
    pub key: String,
    pub label: String,
    /// 0 for a repository, 1 for a checkout inside one.
    pub depth: usize,
    pub collapsed: bool,
    pub sessions: usize,
    pub running: usize,
    /// Running and waiting on you, whether for a reply or an answer.
    pub waiting: usize,
    /// Blocked on a question, the loudest of the waiting states.
    pub asking: usize,
    /// Sum of the priced sessions; `None` when not one of them has a price.
    pub cost: Option<f64>,
    /// The most recent activity among the members.
    pub last_active: String,
    /// The branch a checkout has out, for its heading's BRANCH cell. A
    /// repository has several, so it has none.
    pub branch: Option<String>,
}

impl Group {
    fn add(&mut self, s: &Session) {
        self.sessions += 1;
        if s.is_running() {
            self.running += 1;
            match s.activity_state {
                ActivityState::Asking => {
                    self.waiting += 1;
                    self.asking += 1;
                }
                ActivityState::WaitingForInput => self.waiting += 1,
                _ => {}
            }
        }
        if s.cost_available
            && !s.cost_is_free
            && let Some(c) = s.total_cost
        {
            self.cost = Some(self.cost.unwrap_or(0.0) + c);
        }
        let newer = match (
            crate::util::parse_ts(&s.last_active),
            crate::util::parse_ts(&self.last_active),
        ) {
            (Some(a), Some(b)) => a > b,
            (Some(_), None) => true,
            _ => false,
        };
        if newer {
            self.last_active = s.last_active.clone();
        }
    }
}

/// The table's rows in tree order, the headings they reference, and the glyphs
/// leading each row's label.
pub(super) struct Tree {
    pub groups: Vec<Group>,
    pub rows: Vec<Row>,
    pub indent: Vec<String>,
}

/// Where a session's directory sits: a repository and a checkout of it.
#[derive(Debug, Clone, PartialEq)]
struct Place {
    repo: String,
    repo_label: String,
    /// `None` outside a repository, where there is no second level to show.
    checkout: Option<(String, String)>,
}

/// How long a directory's place is trusted; the same bargain as the branch
/// cache in [`columns`], since a checkout moves about as often as a branch.
const PLACE_TTL: Duration = Duration::from_secs(60);

type Located = Option<(PathBuf, PathBuf)>;

static PLACES: LazyLock<Mutex<HashMap<String, (Located, Instant)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The repository's common git directory and the checkout root, for a
/// directory inside one. Cached, since the tree is rebuilt on every refresh.
fn locate_cached(dir: &str) -> Located {
    let mut cache = PLACES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((found, at)) = cache.get(dir)
        && at.elapsed() < PLACE_TTL
    {
        return found.clone();
    }
    let found = locate(Path::new(dir));
    cache.insert(dir.to_string(), (found.clone(), Instant::now()));
    found
}

/// Resolve `dir` to `(common git dir, checkout root)`.
///
/// The common directory is what every worktree of a repository shares, which
/// is why it and not the checkout root is the repository's identity: two
/// worktrees have two roots and one common directory. A linked worktree's `.git`
/// is a file naming its private git dir, and that dir's `commondir` names the
/// shared one. A submodule's git dir has no `commondir` — it is a repository of
/// its own, and is grouped as one.
///
/// Relative paths are refused: they would resolve against cctop's own working
/// directory, and group a session under whatever repository cctop was started
/// in.
fn locate(dir: &Path) -> Located {
    if !dir.is_absolute() {
        return None;
    }
    let root = dir.ancestors().find(|d| d.join(".git").exists())?;
    let git = root.join(".git");
    if git.is_dir() {
        return Some((git, root.to_path_buf()));
    }
    let pointer = std::fs::read_to_string(&git).ok()?;
    let target = PathBuf::from(pointer.trim().strip_prefix("gitdir:")?.trim());
    let target = if target.is_absolute() {
        target
    } else {
        root.join(target)
    };
    let common = match std::fs::read_to_string(target.join("commondir")) {
        Ok(rel) => {
            let joined = target.join(rel.trim());
            PathBuf::from(crate::collide::normalise(&joined.to_string_lossy(), "/"))
        }
        Err(_) => target,
    };
    Some((common, root.to_path_buf()))
}

/// The directory a repository is known by: the one holding its `.git`, or the
/// common directory itself for a bare repository, which has no other.
fn repo_dir(common: &Path) -> PathBuf {
    match common.file_name() {
        Some(name) if name == ".git" => common.parent().unwrap_or(common).to_path_buf(),
        _ => common.to_path_buf(),
    }
}

fn place(s: &Session) -> Place {
    let dir = s.label_source.as_str();
    // A remote row's directory is a path on another machine, so looking it up
    // here would read whatever lives at the same path locally. It is grouped by
    // host and directory, which is all this side can say about it.
    if let Some(r) = &s.remote {
        return Place {
            repo: format!("host:{}:{dir}", r.host),
            repo_label: format!("{}:{dir}", r.host),
            checkout: None,
        };
    }
    match locate_cached(dir) {
        Some((common, root)) => {
            let home = repo_dir(&common);
            let checkout_label = if root == home {
                // The main checkout is the one the repository is named after,
                // so its heading says which of the checkouts that is.
                format!("{} (main)", file_name(&root))
            } else {
                match root.strip_prefix(&home) {
                    Ok(inside) => inside.to_string_lossy().into_owned(),
                    Err(_) => crate::util::tildify(&root.to_string_lossy()),
                }
            };
            Place {
                repo: format!("repo:{}", common.display()),
                repo_label: crate::util::tildify(&home.to_string_lossy()),
                checkout: Some((format!("wt:{}", root.display()), checkout_label)),
            }
        }
        // Outside a repository the directory is its own group, the same
        // fallback the conflict check makes: one shared "no repository" bucket
        // would put every such session under one heading that means nothing.
        None if dir.is_empty() => Place {
            repo: "dir:".into(),
            repo_label: "(no directory)".into(),
            checkout: None,
        },
        None => Place {
            repo: format!("dir:{dir}"),
            repo_label: crate::util::tildify(dir),
            checkout: None,
        },
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}

/// A checkout's `(key, label)`, or `None` outside a repository, with the
/// sessions in it.
type Checkout = (Option<(String, String)>, Vec<usize>);

/// Lay `ordered` out as a tree.
///
/// `ordered` is the filtered sessions already sorted, and the tree keeps that
/// order at every level: sessions within a checkout, checkouts within a
/// repository and repositories themselves are each placed by their first
/// member. So the sort still decides what is on top — the repository holding
/// the most expensive session leads a cost sort — without ever splitting a
/// group to honour it.
///
/// `children` is how many subagent rows follow a session, which is the flat
/// table's business (whether it is expanded) and is asked rather than decided
/// here.
pub(super) fn build(
    sessions: &[Session],
    ordered: &[usize],
    collapsed: &HashSet<String>,
    children: impl Fn(usize) -> usize,
) -> Tree {
    // (repo place, [(checkout, members)]), in first-seen order. Linear lookup:
    // a table holds tens of repositories, not thousands.
    let mut repos: Vec<(Place, Vec<Checkout>)> = Vec::new();
    for &i in ordered {
        let p = place(&sessions[i]);
        let at = match repos.iter().position(|(r, _)| r.repo == p.repo) {
            Some(at) => at,
            None => {
                repos.push((p.clone(), Vec::new()));
                repos.len() - 1
            }
        };
        let checkouts = &mut repos[at].1;
        match checkouts.iter_mut().find(|(c, _)| *c == p.checkout) {
            Some((_, members)) => members.push(i),
            None => checkouts.push((p.checkout.clone(), vec![i])),
        }
    }

    let mut tree = Tree {
        groups: Vec::new(),
        rows: Vec::new(),
        indent: Vec::new(),
    };
    let heading = |tree: &mut Tree, key: String, label: String, depth, members: &[usize]| {
        let mut g = Group {
            collapsed: collapsed.contains(&key),
            key,
            label,
            depth,
            ..Group::default()
        };
        for &i in members {
            g.add(&sessions[i]);
        }
        let folded = g.collapsed;
        tree.groups.push(g);
        tree.rows.push(Row::Group(tree.groups.len() - 1));
        folded
    };
    let leaves = |tree: &mut Tree, members: &[usize], cont: &str| {
        for (k, &i) in members.iter().enumerate() {
            let last = k + 1 == members.len();
            tree.rows.push(Row::Session(i));
            tree.indent
                .push(format!("{cont}{}", if last { "└─ " } else { "├─ " }));
            // A subagent's own cell already draws its `├─`; it only needs the
            // rails of everything above it.
            let rail = format!("{cont}{}", if last { "   " } else { "│  " });
            for index in 0..children(i) {
                tree.rows.push(Row::Subagent { parent: i, index });
                tree.indent.push(rail.clone());
            }
        }
    };

    for (repo, checkouts) in repos {
        let all: Vec<usize> = checkouts.iter().flat_map(|(_, m)| m.clone()).collect();
        tree.indent.push(String::new());
        if heading(&mut tree, repo.repo, repo.repo_label, 0, &all) {
            continue;
        }
        // A repository seen through one checkout has nothing for a second level
        // to distinguish, and a heading per level would only push the sessions
        // further right.
        if checkouts.len() == 1 {
            leaves(&mut tree, &all, "");
            continue;
        }
        let n = checkouts.len();
        for (c, (checkout, members)) in checkouts.into_iter().enumerate() {
            let last = c + 1 == n;
            let (key, label) = checkout.unwrap_or_default();
            tree.indent
                .push(if last { "└─ " } else { "├─ " }.to_string());
            let folded = heading(&mut tree, key, label, 1, &members);
            if let Some(g) = tree.groups.last_mut() {
                g.branch = columns::branch_of(&sessions[members[0]]);
            }
            if !folded {
                leaves(&mut tree, &members, if last { "   " } else { "│  " });
            }
        }
    }
    tree
}

impl App {
    /// The heading under the cursor, when it is on one.
    pub fn selected_group(&self) -> Option<&Group> {
        match self.selected_row()? {
            Row::Group(g) => self.groups.get(g),
            _ => None,
        }
    }

    pub fn on_group(&self) -> bool {
        self.selected_group().is_some()
    }

    /// Switch between the flat table and the tree.
    ///
    /// The cursor keeps its session across the switch, the way it keeps it
    /// across a resort: `refilter` anchors on the row's key.
    pub(super) fn toggle_tree(&mut self) {
        self.tree = !self.tree;
        self.refilter();
        self.save_prefs();
        self.set_status(if self.tree {
            "Tree view: grouped by repository and checkout"
        } else {
            "Flat view"
        });
    }

    /// Fold or unfold the heading under the cursor. The cursor stays on the
    /// heading either way, since it is the one row certain to still be there.
    pub(super) fn toggle_group(&mut self) {
        let Some(key) = self.selected_group().map(|g| g.key.clone()) else {
            return;
        };
        if !self.collapsed.remove(&key) {
            self.collapsed.insert(key);
        }
        self.refilter();
        self.save_prefs();
    }

    /// Unfold whatever is hiding a session, so a jump to it has a row to land
    /// on. A bell from a folded repository is still a bell.
    pub(super) fn reveal(&mut self, key: &str) {
        if !self.tree {
            return;
        }
        let Some(s) = self.sessions.iter().find(|s| s.key() == key) else {
            return;
        };
        let p = place(s);
        let mut changed = self.collapsed.remove(&p.repo);
        if let Some((checkout, _)) = p.checkout {
            changed |= self.collapsed.remove(&checkout);
        }
        if changed {
            self.refilter();
            self.save_prefs();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{session, test_app};

    /// A scratch tree of directories, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let root = std::env::temp_dir().join(format!(
                "cctop-tree-{}-{name}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).expect("scratch directory");
            Scratch(root)
        }

        /// A repository with its main checkout at `rel`.
        fn repo(&self, rel: &str) -> String {
            let dir = self.0.join(rel);
            std::fs::create_dir_all(dir.join(".git")).expect("repo");
            std::fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n").expect("HEAD");
            dir.to_string_lossy().into_owned()
        }

        /// A linked worktree of the repository at `repo`, laid out as `git
        /// worktree add` does it: a `.git` file, and a private git dir whose
        /// `commondir` points back at the shared one.
        fn worktree(&self, repo: &str, rel: &str, name: &str) -> String {
            let gitdir = Path::new(repo).join(".git/worktrees").join(name);
            std::fs::create_dir_all(&gitdir).expect("gitdir");
            std::fs::write(gitdir.join("commondir"), "../..\n").expect("commondir");
            std::fs::write(gitdir.join("HEAD"), format!("ref: refs/heads/{name}\n")).expect("HEAD");
            let dir = self.0.join(rel);
            std::fs::create_dir_all(&dir).expect("worktree");
            std::fs::write(dir.join(".git"), format!("gitdir: {}\n", gitdir.display()))
                .expect(".git file");
            dir.to_string_lossy().into_owned()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn groups_of(app: &App) -> Vec<(usize, String)> {
        app.visible
            .iter()
            .filter_map(|r| match r {
                Row::Group(g) => Some((app.groups[*g].depth, app.groups[*g].label.clone())),
                _ => None,
            })
            .collect()
    }

    fn position(app: &App, id: &str) -> usize {
        app.visible
            .iter()
            .position(|r| {
                matches!(r, Row::Session(_))
                    && r.session().map(|i| app.sessions[i].session_id.as_str()) == Some(id)
            })
            .expect("session row")
    }

    /// The point of grouping by the common directory rather than the checkout
    /// root: every worktree of one repository lands under one heading, each
    /// worktree its own checkout beneath it.
    #[test]
    fn worktrees_of_one_repository_share_a_heading() {
        let fx = Scratch::new("worktrees");
        let repo = fx.repo("cctop");
        let wt = fx.worktree(&repo, "cctop/.claude/worktrees/agent-1", "agent-1");
        let deep = format!("{repo}/src");
        std::fs::create_dir_all(&deep).unwrap();
        let other = fx.repo("elsewhere");

        let mut app = test_app();
        app.tree = true;
        app.sessions = vec![
            session("main", true, &repo),
            session("sub", true, &deep),
            session("wt", true, &wt),
            session("other", false, &other),
        ];
        app.refilter();

        let groups = groups_of(&app);
        let repos: Vec<_> = groups.iter().filter(|g| g.0 == 0).collect();
        assert_eq!(repos.len(), 2, "two repositories: {groups:?}");
        let checkouts: Vec<&str> = groups
            .iter()
            .filter(|g| g.0 == 1)
            .map(|g| g.1.as_str())
            .collect();
        assert_eq!(checkouts.len(), 2, "cctop has two checkouts: {groups:?}");
        assert!(checkouts.contains(&"cctop (main)"));
        assert!(checkouts.contains(&".claude/worktrees/agent-1"));

        // A subdirectory is the same checkout as its root.
        let cctop = app
            .groups
            .iter()
            .find(|g| g.depth == 0 && g.label.ends_with("cctop"))
            .expect("cctop heading");
        assert_eq!(cctop.sessions, 3);
        let main = app
            .groups
            .iter()
            .find(|g| g.label == "cctop (main)")
            .unwrap();
        assert_eq!(main.sessions, 2);
        assert_eq!(main.branch.as_deref(), Some("main"));

        // A repository seen through one checkout gets no second level.
        let at = app
            .visible
            .iter()
            .position(|r| matches!(r, Row::Group(g) if app.groups[*g].label.ends_with("elsewhere")))
            .unwrap();
        assert_eq!(app.visible[at + 1], Row::Session(3));
        assert_eq!(app.indent[at + 1], "└─ ");
    }

    /// The heading adds up exactly the rows beneath it.
    #[test]
    fn a_heading_totals_its_sessions() {
        let fx = Scratch::new("totals");
        let repo = fx.repo("r");
        let mut app = test_app();
        app.tree = true;
        let mut a = session("a", true, &repo);
        a.cost_available = true;
        a.total_cost = Some(1.5);
        a.activity_state = ActivityState::WaitingForInput;
        let mut b = session("b", true, &repo);
        b.cost_available = true;
        b.total_cost = Some(2.0);
        b.activity_state = ActivityState::Asking;
        let mut c = session("c", false, &repo);
        // Waiting on nobody: a stopped session's last state is history.
        c.activity_state = ActivityState::WaitingForInput;
        app.sessions = vec![a, b, c];
        app.refilter();

        let g = &app.groups[0];
        assert_eq!(g.sessions, 3);
        assert_eq!(g.running, 2);
        assert_eq!(g.waiting, 2);
        assert_eq!(g.asking, 1);
        assert_eq!(g.cost, Some(3.5));
        assert_eq!(app.matched, 3);
    }

    /// Folding hides the members and keeps the cursor on the heading; unfolding
    /// brings them back. The fold outlives a refresh, since it is keyed.
    #[test]
    fn folding_a_heading_hides_its_sessions() {
        let fx = Scratch::new("fold");
        let repo = fx.repo("r");
        let mut app = test_app();
        app.tree = true;
        app.sessions = vec![session("a", true, &repo), session("b", true, &repo)];
        app.refilter();
        assert_eq!(app.visible.len(), 3);
        app.selected = 0;
        assert!(app.on_group());
        assert!(app.selected_session().is_none(), "a heading is no session");

        app.toggle_group();
        assert_eq!(app.visible, vec![Row::Group(0)]);
        assert!(app.groups[0].collapsed);
        assert_eq!(app.groups[0].sessions, 2, "the folded total still counts");
        app.refilter();
        assert_eq!(app.visible.len(), 1, "the fold survives a refresh");

        app.toggle_group();
        assert_eq!(app.visible.len(), 3);
        assert_eq!(app.selected, 0);
    }

    /// Sorting reorders inside a group and orders groups by their best member,
    /// but never splits one.
    #[test]
    fn the_sort_applies_within_each_group() {
        let fx = Scratch::new("sort");
        let (x, y) = (fx.repo("x"), fx.repo("y"));
        let mut app = test_app();
        app.tree = true;
        let priced = |id: &str, dir: &str, cost: f64| {
            let mut s = session(id, true, dir);
            s.cost_available = true;
            s.total_cost = Some(cost);
            s
        };
        app.sessions = vec![
            priced("x-cheap", &x, 1.0),
            priced("y-mid", &y, 5.0),
            priced("x-dear", &x, 9.0),
        ];
        app.sort_col = ColumnId::Cost;
        app.sort_asc = false;
        app.refilter();

        // x holds the dearest session, so it leads, with both of its own
        // sessions beneath it before y begins.
        assert!(position(&app, "x-dear") < position(&app, "x-cheap"));
        assert!(position(&app, "x-cheap") < position(&app, "y-mid"));
        assert_eq!(app.indent[position(&app, "x-dear")], "├─ ");
        assert_eq!(app.indent[position(&app, "x-cheap")], "└─ ");
    }

    /// A filtered tree shows the matches with the headings they sit under, and
    /// nothing else — a heading with no match beneath it is gone.
    #[test]
    fn filtering_keeps_a_match_and_its_ancestors() {
        let fx = Scratch::new("filter");
        let (x, y) = (fx.repo("x"), fx.repo("y"));
        let mut app = test_app();
        app.tree = true;
        let mut hit = session("hit", true, &x);
        hit.title = Some("kingfisher".into());
        app.sessions = vec![hit, session("miss", true, &x), session("far", true, &y)];
        app.search = "kingfisher".into();
        app.refilter();

        assert_eq!(app.visible.len(), 2, "{:?}", app.visible);
        assert!(matches!(app.visible[0], Row::Group(_)));
        assert_eq!(app.visible[1], Row::Session(0));
        assert_eq!(app.groups[0].sessions, 1);
        assert_eq!(app.matched, 1);
    }

    /// Subagents nest one level further in, under their session's own rail.
    #[test]
    fn subagents_hang_under_their_session_in_the_tree() {
        let fx = Scratch::new("subagents");
        let repo = fx.repo("r");
        let mut app = test_app();
        app.tree = true;
        let mut parent = session("p", true, &repo);
        parent.subagents = vec![crate::session::Subagent {
            agent_id: "agent-1".into(),
            agent_type: "general-purpose".into(),
            description: "look".into(),
            model: "claude-opus-5".into(),
            started_at: None,
            last_active: None,
            duration_ms: 0,
            status: crate::session::SubagentStatus::Running,
            cost: 0.0,
            tool_count: 0,
            tool_use_id: None,
            context: None,
            ghost: false,
        }];
        // Older, so the newest-first sort puts it below `p` whatever the clock
        // did between the two fixtures.
        let mut older = session("q", true, &repo);
        older.last_active = (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339();
        app.sessions = vec![parent, older];
        app.expanded.insert(app.sessions[0].key());
        app.refilter();

        let at = position(&app, "p");
        assert_eq!(
            app.visible[at + 1],
            Row::Subagent {
                parent: 0,
                index: 0
            }
        );
        // `p` is not the last session, so its children carry the rail down.
        assert_eq!(app.indent[at], "├─ ");
        assert_eq!(app.indent[at + 1], "│  ");
    }

    /// Switching views keeps the cursor on the session it was on, and the
    /// flat table has no headings or glyphs left over.
    #[test]
    fn toggling_the_tree_keeps_the_cursor_on_its_session() {
        let fx = Scratch::new("toggle");
        let (x, y) = (fx.repo("x"), fx.repo("y"));
        let mut app = test_app();
        app.sessions = vec![session("a", true, &x), session("b", true, &y)];
        app.refilter();
        app.selected = position(&app, "b");

        app.toggle_tree();
        assert!(app.tree);
        assert_eq!(app.selected_session().unwrap().session_id, "b");
        assert_eq!(app.visible.len(), 4);

        app.toggle_tree();
        assert!(!app.tree);
        assert_eq!(app.selected_session().unwrap().session_id, "b");
        assert_eq!(app.visible.len(), 2);
        assert!(app.groups.is_empty() && app.indent.is_empty());
    }

    /// The keys as pressed: `T` switches the view, and on a heading Enter and
    /// Space fold it rather than opening a menu or marking — while ←/→ still
    /// move the bottom panels, heading or not.
    #[test]
    fn the_keys_fold_a_heading_and_leave_the_panels_alone() {
        use crate::ui::tests::key;
        use ratatui::crossterm::event::KeyCode;
        let fx = Scratch::new("keys");
        let repo = fx.repo("r");
        let mut app = test_app();
        app.sessions = vec![session("a", true, &repo)];
        app.refilter();

        app.on_key(key(KeyCode::Char('T')));
        assert!(app.tree);
        app.selected = 0;
        assert!(app.on_group());

        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.mode, Mode::List, "no row menu for a heading");
        assert_eq!(app.visible.len(), 1, "Enter folded it");
        app.on_key(key(KeyCode::Char(' ')));
        assert_eq!(app.visible.len(), 2, "Space unfolded it");
        assert!(app.marked.is_empty(), "and marked nothing");

        let tab = app.bottom_tab;
        app.on_key(key(KeyCode::Right));
        assert_ne!(app.bottom_tab, tab, "→ still moves the panels");
        assert_eq!(app.visible.len(), 2);
    }

    /// A session in a folded group is unfolded to, not silently unreachable.
    #[test]
    fn revealing_a_session_unfolds_its_group() {
        let fx = Scratch::new("reveal");
        let repo = fx.repo("r");
        let mut app = test_app();
        app.tree = true;
        app.sessions = vec![session("a", true, &repo)];
        app.refilter();
        app.selected = 0;
        app.toggle_group();
        assert_eq!(app.visible.len(), 1);

        let key = app.sessions[0].key();
        app.reveal(&key);
        assert_eq!(app.visible.len(), 2);
    }

    /// A relative directory must not be looked up against cctop's own working
    /// directory, which is itself a checkout when the tests run.
    #[test]
    fn a_relative_directory_is_its_own_group() {
        assert_eq!(locate(Path::new("src")), None);
        let s = session("a", false, "src");
        assert_eq!(place(&s).repo, "dir:src");
    }
}
