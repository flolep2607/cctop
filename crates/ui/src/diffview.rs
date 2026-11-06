//! A changed file's diff, drawn over an agent's tab the way GitHub draws a
//! split diff: the old file on the left and the new on the right, each line
//! beside its counterpart, removed and added lines washed red and green, the
//! words a modified line actually changed washed stronger, and the unchanged
//! stretches between hunks folded to a line that opens them.
//!
//! Opened by a click on a file in the sidebar's tree (see [`super::sidebar`]).
//! The file is read once, on the worker, by
//! [`diff_file`](cctop_core::diff_file): git's own hunks, and both versions of
//! the file to fill in what the hunks leave out. Everything here is drawn from
//! that, so scrolling and opening a fold never run git again.
//!
//! Narrower than [`SPLIT_MIN`] the same rows are drawn unified — removed lines
//! above added ones, as `git diff` prints them — with the same washes and word
//! marks, since two halves of sixty columns are two halves of nothing.
//!
//! Code is coloured by [`super::syntax`], from both versions of the file, so
//! a string or a comment that opened above a hunk is still one inside it. The
//! colours are the text's; the washes and word marks stay the ground.

use std::collections::HashSet;
use std::path::PathBuf;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use super::syntax::{self, Kind};
use super::{App, Mode, changes, theme};
use cctop_core::diff_file::FileDiff;
use cctop_core::util;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};

/// The overlay's width from which the halves sit side by side.
pub(super) const SPLIT_MIN: u16 = 120;
/// The width inside the overlay's border from which the file list runs down
/// its left: what a 160-column terminal leaves it.
const LIST_MIN: u16 = 150;
/// The file list's width.
const LIST_WIDTH: u16 = 32;
/// Lines of unchanged text a fold has to hold to be folded at all: two lines
/// folded behind a line saying so is no saving.
const FOLD_MIN: u64 = 4;
/// Tokens on either side of a line pair past which the words are not compared:
/// the comparison is quadratic, and a line that long is not read word by word.
const WORDS_MAX: usize = 400;

/// One hunk of git's diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Hunk {
    /// The first line of each side the hunk covers, and how many it covers.
    pub old: (u64, u64),
    pub new: (u64, u64),
    /// What git prints after the second `@@`: the enclosing function or
    /// heading, when it found one.
    pub heading: String,
    /// The hunk's lines: ' ', '-' or '+', and the text, tabs expanded.
    pub lines: Vec<(char, String)>,
}

impl Hunk {
    /// The first line of each side past this hunk.
    fn next(&self) -> (u64, u64) {
        (first(self.old) + self.old.1, first(self.new) + self.new.1)
    }

    /// The header as git prints it.
    fn header(&self) -> String {
        let range = |(at, len): (u64, u64)| match len {
            1 => format!("{at}"),
            _ => format!("{at},{len}"),
        };
        let heading = match self.heading.is_empty() {
            true => String::new(),
            false => format!(" {}", self.heading),
        };
        format!("@@ -{} +{} @@{heading}", range(self.old), range(self.new))
    }
}

/// The first line a side of a hunk covers. Git names the line *before* an
/// empty side (`-4,0` is "after line 4"), so an empty side's first line is
/// the one after.
fn first((at, len): (u64, u64)) -> u64 {
    match len {
        0 => at + 1,
        _ => at,
    }
}

/// A tab as the columns it stands for: ratatui counts it as none and the
/// terminal as up to eight, and four is what an editor shows.
pub(super) fn untab(text: &str) -> String {
    text.replace('\t', "    ")
}

/// Both versions of `path` coloured, with their tabs expanded first as the
/// view expands them, so a colour's char range lands on the char it meant.
/// Runs on the worker, once per file opened.
pub(super) fn colour(path: &str, view: &FileDiff) -> syntax::Pair {
    let side = |lines: &Option<Vec<String>>| {
        let lines: Vec<String> = lines.as_ref()?.iter().map(|l| untab(l)).collect();
        syntax::highlight(path, &lines)
    };
    syntax::Pair {
        old: side(&view.old),
        new: side(&view.new),
    }
}

/// git's diff, from the first hunk on, as hunks. A diff with no hunk header —
/// an untracked file, every line added — is one hunk of all of it.
pub(super) fn parse(diff: &[String]) -> Vec<Hunk> {
    let mut out: Vec<Hunk> = Vec::new();
    for line in diff {
        if let Some(rest) = line.strip_prefix("@@ ") {
            let (ranges, heading) = rest.split_once(" @@").unwrap_or((rest, ""));
            let mut old = (0, 0);
            let mut new = (0, 0);
            for part in ranges.split_whitespace() {
                let side = |s: &str| -> (u64, u64) {
                    let (at, len) = s.split_once(',').unwrap_or((s, "1"));
                    (at.parse().unwrap_or(0), len.parse().unwrap_or(0))
                };
                if let Some(o) = part.strip_prefix('-') {
                    old = side(o);
                } else if let Some(n) = part.strip_prefix('+') {
                    new = side(n);
                }
            }
            out.push(Hunk {
                old,
                new,
                heading: untab(heading.trim()),
                lines: Vec::new(),
            });
            continue;
        }
        // "\ No newline at end of file" says something about the line above,
        // not a line of the file.
        if line.starts_with('\\') {
            continue;
        }
        let (kind, text) = match line.chars().next() {
            Some(c @ ('+' | '-' | ' ')) => (c, &line[1..]),
            // An empty context line, whose leading space an editor trimmed.
            None => (' ', ""),
            Some(_) => continue,
        };
        if out.is_empty() {
            out.push(Hunk {
                old: (0, 0),
                new: (1, 0),
                heading: String::new(),
                lines: Vec::new(),
            });
        }
        let hunk = out.last_mut().expect("pushed above");
        hunk.lines.push((kind, untab(text)));
    }
    // A synthesized hunk counts its own lines.
    if let Some(h) = out.first_mut()
        && h.old == (0, 0)
        && h.new == (1, 0)
    {
        h.new.1 = h.lines.iter().filter(|(k, _)| *k == '+').count() as u64;
        h.old.1 = h.lines.iter().filter(|(k, _)| *k == '-').count() as u64;
        if h.new.1 == 0 {
            h.new.0 = 0;
        }
    }
    out
}

/// One side of a changed line: its number, its text, and the character ranges
/// of the words that changed, when it was compared with a counterpart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Cell {
    pub no: u64,
    pub text: String,
    pub marks: Vec<(usize, usize)>,
}

/// A row of the view, as the split view draws it; the unified view draws the
/// same rows with a pair's two sides one above the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Row {
    /// A hunk's header.
    Hunk(String),
    /// Unchanged lines folded away: which fold, where it starts on each side,
    /// and how many lines. `open` is whether the text behind it was read, so
    /// a click can open it.
    Fold {
        id: usize,
        old: u64,
        new: u64,
        count: u64,
        open: bool,
    },
    /// An unchanged line, on both sides.
    Same { old: u64, new: u64, text: String },
    /// A removed line beside the added one it became; either side can be
    /// missing, where one side of a change has more lines than the other.
    Pair {
        left: Option<Cell>,
        right: Option<Cell>,
    },
}

/// The rows of a file's diff, with the folds in `opened` (or every one, with
/// `all`) opened from the file's own text where it was read.
pub(super) fn rows(
    view: &FileDiff,
    hunks: &[Hunk],
    opened: &HashSet<usize>,
    all: bool,
) -> Vec<Row> {
    let text = view.new.as_deref().or(view.old.as_deref());
    let mut out = Vec::new();
    let mut next = (1u64, 1u64);
    let fold = |out: &mut Vec<Row>, id: usize, at: (u64, u64), count: u64| {
        if count == 0 {
            return;
        }
        // From the side the text came from, by the number the fold starts at
        // there.
        let from_new = view.new.is_some();
        let start = if from_new { at.1 } else { at.0 };
        let lines = text.and_then(|t| {
            let begin = usize::try_from(start.checked_sub(1)?).ok()?;
            t.get(begin..begin + count as usize)
        });
        if (count < FOLD_MIN || all || opened.contains(&id))
            && let Some(lines) = lines
        {
            for (i, line) in lines.iter().enumerate() {
                out.push(Row::Same {
                    old: at.0 + i as u64,
                    new: at.1 + i as u64,
                    text: untab(line),
                });
            }
            return;
        }
        out.push(Row::Fold {
            id,
            old: at.0,
            new: at.1,
            count,
            open: lines.is_some(),
        });
    };
    for (i, hunk) in hunks.iter().enumerate() {
        let starts = (first(hunk.old), first(hunk.new));
        let before = starts
            .1
            .saturating_sub(next.1)
            .max(starts.0.saturating_sub(next.0));
        fold(&mut out, i, next, before);
        out.push(Row::Hunk(hunk.header()));
        let (mut old, mut new) = starts;
        let mut removed: Vec<Cell> = Vec::new();
        let mut added: Vec<Cell> = Vec::new();
        let flush = |out: &mut Vec<Row>, removed: &mut Vec<Cell>, added: &mut Vec<Cell>| {
            let len = removed.len().max(added.len());
            let mut left = std::mem::take(removed).into_iter();
            let mut right = std::mem::take(added).into_iter();
            for _ in 0..len {
                let (mut l, mut r) = (left.next(), right.next());
                if let (Some(a), Some(b)) = (l.as_mut(), r.as_mut()) {
                    let (ma, mb) = words(&a.text, &b.text);
                    a.marks = ma;
                    b.marks = mb;
                }
                out.push(Row::Pair { left: l, right: r });
            }
        };
        for (kind, line) in &hunk.lines {
            match kind {
                '-' => {
                    removed.push(Cell {
                        no: old,
                        text: line.clone(),
                        marks: Vec::new(),
                    });
                    old += 1;
                }
                '+' => {
                    added.push(Cell {
                        no: new,
                        text: line.clone(),
                        marks: Vec::new(),
                    });
                    new += 1;
                }
                _ => {
                    flush(&mut out, &mut removed, &mut added);
                    out.push(Row::Same {
                        old,
                        new,
                        text: line.clone(),
                    });
                    old += 1;
                    new += 1;
                }
            }
        }
        flush(&mut out, &mut removed, &mut added);
        next = hunk.next();
    }
    // What follows the last hunk, as far as the file's length says.
    let after = match (&view.new, &view.old) {
        (Some(n), _) => (n.len() as u64 + 1).saturating_sub(next.1),
        (None, Some(o)) => (o.len() as u64 + 1).saturating_sub(next.0),
        _ => 0,
    };
    if !hunks.is_empty() {
        fold(&mut out, hunks.len(), next, after);
    }
    out
}

/// Character ranges of a line, start inclusive and end exclusive.
pub(super) type Marks = Vec<(usize, usize)>;

/// The words two versions of a line do not share, as character ranges into
/// each: the stronger wash a modified line's changed words get.
///
/// Nothing is marked where the lines share less than a quarter of their
/// text: that is a line rewritten, which the line's own wash says already, and
/// marking every word of it would only be noise.
pub(super) fn words(old: &str, new: &str) -> (Marks, Marks) {
    let a = tokens(old);
    let b = tokens(new);
    if a.is_empty() || b.is_empty() || a.len() > WORDS_MAX || b.len() > WORDS_MAX {
        return (Vec::new(), Vec::new());
    }
    // Longest common subsequence of the tokens, by the usual table.
    let (n, m) = (a.len(), b.len());
    let mut table = vec![0u16; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[at(i, j)] = match a[i].1 == b[j].1 {
                true => table[at(i + 1, j + 1)] + 1,
                false => table[at(i + 1, j)].max(table[at(i, j + 1)]),
            };
        }
    }
    let (mut keep_a, mut keep_b) = (vec![false; n], vec![false; m]);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i].1 == b[j].1 {
            keep_a[i] = true;
            keep_b[j] = true;
            i += 1;
            j += 1;
        } else if table[at(i + 1, j)] >= table[at(i, j + 1)] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let shared: usize = a
        .iter()
        .zip(&keep_a)
        .filter(|(_, k)| **k)
        .map(|(t, _)| t.1.chars().count())
        .sum();
    let longer = old.chars().count().max(new.chars().count());
    if shared * 4 < longer {
        return (Vec::new(), Vec::new());
    }
    let ranges = |toks: &[(usize, &str)], keep: &[bool]| {
        let mut out: Vec<(usize, usize)> = Vec::new();
        for ((start, tok), kept) in toks.iter().zip(keep) {
            if *kept || tok.trim().is_empty() && out.is_empty() {
                continue;
            }
            let end = start + tok.chars().count();
            match out.last_mut() {
                Some(last) if last.1 == *start => last.1 = end,
                _ => out.push((*start, end)),
            }
        }
        out
    };
    (ranges(&a, &keep_a), ranges(&b, &keep_b))
}

/// A line as words, runs of space and single other characters, each with the
/// character offset it starts at.
fn tokens(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start: Option<(usize, usize, u8)> = None; // char at, byte at, class
    let class = |c: char| match c {
        c if c.is_alphanumeric() || c == '_' => 1,
        c if c.is_whitespace() => 2,
        _ => 3,
    };
    for (count, (byte, c)) in line.char_indices().enumerate() {
        let k = class(c);
        match start {
            Some((_, _, sk)) if sk == k && k != 3 => {}
            Some((ci, bi, _)) => {
                out.push((ci, &line[bi..byte]));
                start = Some((count, byte, k));
            }
            None => start = Some((count, byte, k)),
        }
    }
    if let Some((ci, bi, _)) = start {
        out.push((ci, &line[bi..]));
    }
    out
}

/// The diff view's state, while it is open.
#[derive(Debug)]
pub(super) struct State {
    /// The tab it was opened over; leaving that tab closes it.
    pub tab: usize,
    pub root: PathBuf,
    pub base: String,
    /// Every changed file, in the sidebar tree's order: path, lines added and
    /// removed, and whether it is untracked.
    pub files: Vec<super::sidebar::File>,
    /// The file on screen, by its place in `files`.
    pub at: usize,
    /// The file's read and its hunks, once the worker answers.
    pub read: Option<Result<(FileDiff, Vec<Hunk>), String>>,
    /// Both versions coloured, from the same read.
    pub syntax: syntax::Pair,
    pub scroll: usize,
    pub hscroll: usize,
    /// Side by side, unified, or — `None` — whichever the width suits.
    pub split: Option<bool>,
    /// Folds opened by a click or Enter, and whether `e` opened them all.
    pub opened: HashSet<usize>,
    pub all_open: bool,
    /// What the last draw found: the body's height, how many lines the file
    /// drew to, and the folds on screen. The keys page, clamp and open by
    /// these; only the draw knows the width they depend on.
    pub page: std::cell::Cell<usize>,
    pub length: std::cell::Cell<usize>,
    pub visible: std::cell::RefCell<Vec<usize>>,
    /// Whether the width the view was last drawn at suits side by side: what
    /// `s` turns away from when nothing was chosen yet.
    pub wide: std::cell::Cell<bool>,
}

impl State {
    pub(super) fn file(&self) -> Option<&super::sidebar::File> {
        self.files.get(self.at)
    }
}

/// Where the open view landed on screen, for the mouse.
#[derive(Debug, Default, Clone)]
pub(super) struct Hits {
    pub area: Option<Rect>,
    /// The diff body, and the folds in it by row.
    pub body: Option<Rect>,
    pub folds: Vec<(u16, usize)>,
    /// The file list's rows, by row and place in the files.
    pub list: Vec<(u16, usize)>,
}

/// Every file of the sidebar's tree in the order it lists them, everything
/// open: folders first, then files, at each level.
pub(super) fn in_tree_order(files: &[super::sidebar::File]) -> Vec<super::sidebar::File> {
    fn walk(dir: &super::sidebar::Dir, out: &mut Vec<String>) {
        for sub in &dir.dirs {
            walk(sub, out);
        }
        for (name, _, _) in &dir.files {
            out.push(match dir.path.is_empty() {
                true => name.clone(),
                false => format!("{}/{name}", dir.path),
            });
        }
    }
    let mut order = Vec::new();
    walk(&super::sidebar::tree(files), &mut order);
    order
        .into_iter()
        .filter_map(|path| files.iter().find(|f| f.path == path).cloned())
        .collect()
}

/// The faint ink of the divider and the empty side of a change.
fn rule() -> Style {
    Style::default().fg(theme::colors().dimmer)
}

/// The washes, as styles: a changed line's, and its changed words'. Under
/// `NO_COLOR` the line has none — its `-` or `+` marker says it — and the
/// words are underlined.
fn line_style(added: bool) -> Style {
    let c = theme::colors();
    match theme::no_color() {
        true => Style::default(),
        false => Style::default().bg(match added {
            true => c.diff_add_bg,
            false => c.diff_del_bg,
        }),
    }
}

fn word_style(added: bool) -> Style {
    let c = theme::colors();
    match theme::no_color() {
        true => Style::default().add_modifier(Modifier::UNDERLINED),
        false => Style::default()
            .bg(match added {
                true => c.diff_add_word,
                false => c.diff_del_word,
            })
            .add_modifier(Modifier::BOLD),
    }
}

/// GitHub's five squares: how much of the change is added and how much
/// removed, at a glance.
fn blocks(added: u64, removed: u64) -> Vec<Span<'static>> {
    let total = added + removed;
    let green = match total {
        0 => 0,
        _ => ((added * 5 + total / 2) / total).min(5) as usize,
    };
    let red = match total {
        0 => 0,
        _ => 5 - green,
    };
    let mut out = vec![
        Span::styled("■".repeat(green), changes::plus()),
        Span::styled("■".repeat(red), changes::minus()),
    ];
    if total == 0 {
        out.push(Span::styled("■■■■■", theme::dim()));
    }
    out
}

/// `text` from column `skip`, in `width` columns: cut with `…` where it runs
/// on, and padded so a wash runs the whole width. The word marks are char
/// ranges into the whole text, shifted by the skip.
fn cut(
    text: &str,
    marks: &[(usize, usize)],
    syn: &[(usize, usize, Kind)],
    skip: usize,
    width: usize,
    base: Style,
    mark: Style,
) -> Vec<Span<'static>> {
    if width == 0 {
        return Vec::new();
    }
    let chars: Vec<char> = text.chars().skip(skip).collect();
    let more = chars.len() > width;
    let shown = match more {
        true => width - 1,
        false => chars.len(),
    };
    // A char's style: its syntax colour as the text, over the line's wash or
    // the word mark's stronger one.
    let style_at = |i: usize| {
        let at = i + skip;
        let ground = match marks.iter().any(|&(a, b)| (a..b).contains(&at)) {
            true => mark,
            false => base,
        };
        match syntax::color_at(syn, at) {
            Some(fg) => ground.fg(fg),
            None => ground,
        }
    };
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut run_style = base;
    for (i, c) in chars.iter().take(shown).enumerate() {
        let style = style_at(i);
        if style != run_style && !run.is_empty() {
            out.push(Span::styled(std::mem::take(&mut run), run_style));
        }
        run_style = style;
        run.push(*c);
    }
    if !run.is_empty() {
        out.push(Span::styled(run, run_style));
    }
    if more {
        out.push(Span::styled("…", base.patch(theme::dim())));
    }
    let used = shown + usize::from(more);
    if used < width {
        out.push(Span::styled(" ".repeat(width - used), base));
    }
    out
}

/// The line number, right-aligned in the gutter, dimmed.
fn gutter(no: Option<u64>, digits: usize, base: Style) -> Span<'static> {
    let text = match no {
        Some(n) => format!("{n:>digits$} "),
        None => " ".repeat(digits + 1),
    };
    Span::styled(text, base.patch(theme::dim()))
}

/// One side of a split row, `width` wide: gutter, marker, text.
fn half(
    cell: Option<&Cell>,
    added: bool,
    syn: &syntax::Pair,
    digits: usize,
    hscroll: usize,
    width: usize,
) -> Vec<Span<'static>> {
    let Some(cell) = cell else {
        // Nothing on this side: a blank the width of the half, so the side
        // that has lines stands out against it as on GitHub.
        return vec![Span::styled(" ".repeat(width), rule())];
    };
    let base = line_style(added);
    let marker = if added { "+ " } else { "- " };
    let mut out = vec![
        gutter(Some(cell.no), digits, base),
        Span::styled(
            marker,
            base.patch(if added {
                changes::plus()
            } else {
                changes::minus()
            }),
        ),
    ];
    let room = width.saturating_sub(digits + 3);
    out.extend(cut(
        &cell.text,
        &cell.marks,
        syn.line(added, cell.no),
        hscroll,
        room,
        base,
        word_style(added),
    ));
    out
}

/// An unchanged line's half.
fn same_half(
    no: u64,
    text: &str,
    colours: &[(usize, usize, Kind)],
    digits: usize,
    hscroll: usize,
    width: usize,
) -> Vec<Span<'static>> {
    let base = theme::value();
    let mut out = vec![gutter(Some(no), digits, Style::default()), Span::raw("  ")];
    out.extend(cut(
        text,
        &[],
        colours,
        hscroll,
        width.saturating_sub(digits + 3),
        base,
        base,
    ));
    out
}

/// An unchanged line's colours: the new version's, or the old one's where
/// only that was read.
fn same_colours(syn: &syntax::Pair, old: u64, new: u64) -> &[(usize, usize, Kind)] {
    match syn.line(true, new) {
        [] => syn.line(false, old),
        found => found,
    }
}

/// The view's lines for `rows`, `width` wide: side by side when `split`, else
/// unified. Each line comes with the fold it is, if it is one.
pub(super) fn lines(
    rows: &[Row],
    syn: &syntax::Pair,
    split: bool,
    hscroll: usize,
    width: usize,
) -> Vec<(Line<'static>, Option<usize>)> {
    let most = rows
        .iter()
        .map(|r| match r {
            Row::Same { old, new, .. } => (*old).max(*new),
            Row::Pair { left, right } => left
                .as_ref()
                .map_or(0, |c| c.no)
                .max(right.as_ref().map_or(0, |c| c.no)),
            Row::Fold {
                old, new, count, ..
            } => (*old).max(*new) + count,
            Row::Hunk(_) => 0,
        })
        .max()
        .unwrap_or(1);
    let digits = most.to_string().len().max(3);
    let mut out = Vec::new();
    let full = |text: String, style: Style| -> Line<'static> {
        let pad = width.saturating_sub(util::cells(&text));
        Line::from(vec![
            Span::styled(util::clip(&text, width), style),
            Span::styled(" ".repeat(pad), style),
        ])
    };
    let mut removed: Vec<Cell> = Vec::new();
    let mut added: Vec<Cell> = Vec::new();
    let unified = |out: &mut Vec<(Line<'static>, Option<usize>)>,
                   removed: &mut Vec<Cell>,
                   added: &mut Vec<Cell>| {
        let run = std::mem::take(removed)
            .into_iter()
            .map(|c| (c, false))
            .chain(std::mem::take(added).into_iter().map(|c| (c, true)));
        for (cell, is_added) in run {
            let base = line_style(is_added);
            let (old, new) = match is_added {
                true => (None, Some(cell.no)),
                false => (Some(cell.no), None),
            };
            let marker = if is_added { "+ " } else { "- " };
            let mut spans = vec![
                gutter(old, digits, base),
                gutter(new, digits, base),
                Span::styled(
                    marker,
                    base.patch(if is_added {
                        changes::plus()
                    } else {
                        changes::minus()
                    }),
                ),
            ];
            let room = width.saturating_sub(2 * (digits + 1) + 2);
            spans.extend(cut(
                &cell.text,
                &cell.marks,
                syn.line(is_added, cell.no),
                hscroll,
                room,
                base,
                word_style(is_added),
            ));
            out.push((Line::from(spans), None));
        }
    };
    for row in rows {
        if !matches!(row, Row::Pair { .. }) {
            unified(&mut out, &mut removed, &mut added);
        }
        match row {
            Row::Hunk(header) => out.push((
                full(
                    header.clone(),
                    Style::default()
                        .fg(theme::colors().accent)
                        .bg(theme::colors().header_bg),
                ),
                None,
            )),
            Row::Fold {
                id, count, open, ..
            } => {
                let hint = match open {
                    true => "  · Enter or click to show",
                    false => "",
                };
                out.push((
                    full(format!("  ⋯ {count} unchanged lines{hint}"), theme::dim()),
                    Some(*id),
                ));
            }
            Row::Same { old, new, text } => match split {
                // An unchanged line is the same text on both sides, coloured
                // from whichever version was read: the new one first.
                true => {
                    let left_w = width.saturating_sub(1) / 2;
                    let right_w = width.saturating_sub(1) - left_w;
                    let colours = same_colours(syn, *old, *new);
                    let mut spans = same_half(*old, text, colours, digits, hscroll, left_w);
                    spans.push(Span::styled("│", rule()));
                    spans.extend(same_half(*new, text, colours, digits, hscroll, right_w));
                    out.push((Line::from(spans), None));
                }
                false => {
                    let mut spans = vec![
                        gutter(Some(*old), digits, Style::default()),
                        gutter(Some(*new), digits, Style::default()),
                        Span::raw("  "),
                    ];
                    let room = width.saturating_sub(2 * (digits + 1) + 2);
                    spans.extend(cut(
                        text,
                        &[],
                        same_colours(syn, *old, *new),
                        hscroll,
                        room,
                        theme::value(),
                        theme::value(),
                    ));
                    out.push((Line::from(spans), None));
                }
            },
            Row::Pair { left, right } => match split {
                true => {
                    let left_w = width.saturating_sub(1) / 2;
                    let right_w = width.saturating_sub(1) - left_w;
                    let mut spans = half(left.as_ref(), false, syn, digits, hscroll, left_w);
                    spans.push(Span::styled("│", rule()));
                    spans.extend(half(right.as_ref(), true, syn, digits, hscroll, right_w));
                    out.push((Line::from(spans), None));
                }
                // Unified, a change's removed lines all go above its added
                // ones, as `git diff` prints them: the run is held until the
                // row after it, and drawn then.
                false => {
                    if let Some(cell) = left {
                        removed.push(cell.clone());
                    }
                    if let Some(cell) = right {
                        added.push(cell.clone());
                    }
                }
            },
        }
    }
    unified(&mut out, &mut removed, &mut added);
    out
}

/// Draw the view over `area`, the tab's panes and sidebar together.
pub(super) fn draw(frame: &mut Frame, area: Rect, app: &App) -> Hits {
    let Some(state) = app.diff.as_ref() else {
        return Hits::default();
    };
    let mut hits = Hits {
        area: Some(area),
        ..Hits::default()
    };
    frame.render_widget(Clear, area);
    let file = state.file();
    let (added, removed) = file.map_or((0, 0), |f| (f.added.unwrap_or(0), f.removed.unwrap_or(0)));
    state.wide.set(area.width >= SPLIT_MIN);
    let split = state.split.unwrap_or(area.width >= SPLIT_MIN);
    let mut title = vec![Span::styled(
        format!(" {} ", file.map_or("", |f| f.path.as_str())),
        theme::title().add_modifier(Modifier::BOLD),
    )];
    title.push(Span::styled(format!("+{added}"), changes::plus()));
    title.push(Span::styled(format!(" −{removed} "), changes::minus()));
    title.extend(blocks(added, removed));
    title.push(Span::raw(" "));
    let keys = format!(
        " {}/{} · n/p file · s {} · ←→ · e open all · Esc ",
        state.at + 1,
        state.files.len(),
        if split { "unified" } else { "side by side" },
    );
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::colors().border_hi))
        .title(Line::from(title))
        .title_bottom(Line::from(Span::styled(keys, theme::dim())).right_aligned())
        .style(theme::canvas());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (list, body) = match inner.width >= LIST_MIN {
        true => (
            Some(Rect {
                width: LIST_WIDTH,
                ..inner
            }),
            Rect {
                x: inner.x + LIST_WIDTH + 1,
                width: inner.width - LIST_WIDTH - 1,
                ..inner
            },
        ),
        false => (None, inner),
    };
    if let Some(list) = list {
        let mut lines = Vec::new();
        // The current file kept in view.
        let top = state.at.saturating_sub(list.height as usize / 2);
        for (i, f) in state
            .files
            .iter()
            .enumerate()
            .skip(top)
            .take(list.height as usize)
        {
            let here = i == state.at;
            let name = f.path.rsplit('/').next().unwrap_or(&f.path);
            let dir = f.path.strip_suffix(name).unwrap_or("");
            let room = (LIST_WIDTH as usize).saturating_sub(2);
            let text = util::clip(&format!("{name}  {dir}"), room);
            let style = match here {
                true => theme::selected().add_modifier(Modifier::BOLD),
                false => theme::value(),
            };
            let pad = room.saturating_sub(util::cells(&text));
            lines.push(Line::from(vec![
                Span::styled(if here { "▶ " } else { "  " }, style),
                Span::styled(text, style),
                Span::styled(" ".repeat(pad), style),
            ]));
            hits.list.push((list.y + (i - top) as u16, i));
        }
        frame.render_widget(Paragraph::new(lines), list);
        let divider = Rect {
            x: list.x + list.width,
            width: 1,
            ..list
        };
        let bar: Vec<Line> = (0..divider.height)
            .map(|_| Line::from(Span::styled("│", rule())))
            .collect();
        frame.render_widget(Paragraph::new(bar), divider);
    }
    hits.body = Some(body);

    let note = |frame: &mut Frame, text: &str| {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(format!("  {text}"), theme::dim()))),
            body,
        );
    };
    let read = match &state.read {
        None => {
            note(frame, "reading…");
            return hits;
        }
        Some(Err(why)) => {
            note(frame, why);
            return hits;
        }
        Some(Ok(read)) => read,
    };
    let (view, hunks) = read;
    if view.binary {
        note(frame, "Binary file — no lines to show.");
        return hits;
    }
    if view.too_big {
        note(
            frame,
            "This file's diff is too large to draw here — Alt+g opens it in your git tool.",
        );
        return hits;
    }
    if hunks.is_empty() {
        note(
            frame,
            "No changes in the text of this file (a mode or a rename only).",
        );
        return hits;
    }
    let rows = rows(view, hunks, &state.opened, state.all_open);
    let drawn = lines(
        &rows,
        &state.syntax,
        split,
        state.hscroll,
        body.width as usize,
    );
    let height = body.height as usize;
    let top = state.scroll.min(drawn.len().saturating_sub(height));
    state.page.set(height.max(1));
    state.length.set(drawn.len());
    let shown: Vec<_> = drawn.into_iter().skip(top).take(height).collect();
    *state.visible.borrow_mut() = shown.iter().filter_map(|(_, fold)| *fold).collect();
    for (i, (_, fold)) in shown.iter().enumerate() {
        if let Some(id) = fold {
            hits.folds.push((body.y + i as u16, *id));
        }
    }
    frame.render_widget(
        Paragraph::new(shown.into_iter().map(|(l, _)| l).collect::<Vec<_>>()),
        body,
    );
    hits
}

impl App {
    /// A click on a file in the sidebar's tree: its diff, over the tab.
    pub(super) fn open_diff(&mut self, path: &str) {
        let Some((root, base)) = self.sidebar.checkout.clone() else {
            return self
                .set_status("These files came from the transcript — there is no checkout to diff");
        };
        let Some(Ok(listed)) = self.sidebar.files.as_ref() else {
            return;
        };
        let files = in_tree_order(listed);
        let Some(at) = files.iter().position(|f| f.path == path) else {
            return;
        };
        self.diff = Some(State {
            tab: self.tab,
            root,
            base,
            files,
            at,
            read: None,
            syntax: syntax::Pair::default(),
            scroll: 0,
            hscroll: 0,
            split: None,
            opened: HashSet::new(),
            all_open: false,
            page: std::cell::Cell::new(20),
            length: std::cell::Cell::new(0),
            visible: std::cell::RefCell::new(Vec::new()),
            wide: std::cell::Cell::new(true),
        });
        self.mode = Mode::Diff;
        self.load_diff();
    }

    /// Ask the worker for the file on screen, and start it from its top.
    fn load_diff(&mut self) {
        let Some(state) = self.diff.as_mut() else {
            return;
        };
        let Some(file) = state.files.get(state.at) else {
            return;
        };
        let request = super::worker::Request::FileDiff {
            root: state.root.clone(),
            base: state.base.clone(),
            path: file.path.clone(),
            untracked: file.untracked,
        };
        state.read = None;
        state.syntax = syntax::Pair::default();
        state.scroll = 0;
        state.hscroll = 0;
        state.opened.clear();
        state.all_open = false;
        let _ = self.tx.send(request);
        self.needs_redraw = true;
    }

    /// The worker's read, kept only if that file is still the one on screen.
    pub(super) fn got_file_diff(
        &mut self,
        path: String,
        result: Result<cctop_core::diff_file::FileDiff, String>,
        colours: syntax::Pair,
    ) {
        let Some(state) = self.diff.as_mut() else {
            return;
        };
        if state.file().map(|f| f.path.as_str()) != Some(path.as_str()) {
            return;
        }
        state.syntax = colours;
        state.read = Some(result.map(|view| {
            let hunks = parse(&view.diff);
            (view, hunks)
        }));
        self.needs_redraw = true;
    }

    /// Put the view down.
    pub(super) fn close_diff(&mut self) {
        self.diff = None;
        if self.mode == Mode::Diff {
            self.mode = Mode::List;
        }
        self.needs_redraw = true;
    }

    /// The view belongs to the tab it was opened over: leaving the tab puts it
    /// down rather than drawing one tab's file over another's agent.
    pub(super) fn diff_follows_tab(&mut self) {
        if self.diff.as_ref().is_some_and(|d| d.tab != self.tab) {
            self.close_diff();
        }
    }

    /// Step to the next or previous changed file, in the tree's order.
    fn step_diff(&mut self, delta: isize) {
        let Some(state) = self.diff.as_mut() else {
            return;
        };
        let len = state.files.len() as isize;
        if len == 0 {
            return;
        }
        state.at = (state.at as isize + delta).rem_euclid(len) as usize;
        self.load_diff();
    }

    fn scroll_diff(&mut self, delta: isize) {
        let Some(state) = self.diff.as_mut() else {
            return;
        };
        let most = state.length.get().saturating_sub(state.page.get());
        state.scroll = (state.scroll as isize + delta).clamp(0, most as isize) as usize;
        self.needs_redraw = true;
    }

    /// Open the folds on screen — what Enter does, since there is no cursor to
    /// say which one.
    fn open_visible_folds(&mut self) {
        let Some(state) = self.diff.as_mut() else {
            return;
        };
        let visible = state.visible.borrow().clone();
        state.opened.extend(visible);
        self.needs_redraw = true;
    }

    /// Keys while the view is up. They are the view's, not the agent's: the
    /// agent behind it is not being typed at, and Esc always puts it down.
    pub(super) fn on_key_diff(&mut self, key: KeyEvent) {
        let page = self.diff.as_ref().map_or(20, |d| d.page.get()) as isize;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.close_diff(),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_diff(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_diff(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_diff(page - 1),
            KeyCode::PageUp => self.scroll_diff(-(page - 1)),
            KeyCode::Home | KeyCode::Char('g') => self.scroll_diff(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.scroll_diff(isize::MAX / 2),
            KeyCode::Right | KeyCode::Char('l') => {
                if let Some(state) = self.diff.as_mut() {
                    state.hscroll += 8;
                    self.needs_redraw = true;
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if let Some(state) = self.diff.as_mut() {
                    state.hscroll = state.hscroll.saturating_sub(8);
                    self.needs_redraw = true;
                }
            }
            KeyCode::Char('n' | ']') => self.step_diff(1),
            KeyCode::Char('p' | '[') => self.step_diff(-1),
            KeyCode::Char('s') => {
                if let Some(state) = self.diff.as_mut() {
                    let now = state.split.unwrap_or(state.wide.get());
                    state.split = Some(!now);
                    self.needs_redraw = true;
                }
            }
            KeyCode::Char('e') => {
                if let Some(state) = self.diff.as_mut() {
                    state.all_open = !state.all_open;
                    self.needs_redraw = true;
                }
            }
            KeyCode::Enter => self.open_visible_folds(),
            _ => {}
        }
    }

    /// The mouse while the view is up: the wheel scrolls it, a click on a fold
    /// opens it and one on the file list goes to that file, and a click
    /// outside puts it down. Returns whether the event was the view's.
    pub(super) fn on_mouse_diff(&mut self, ev: MouseEvent, layout: &super::render::Layout) -> bool {
        if self.mode != Mode::Diff {
            return false;
        }
        let at = ratatui::layout::Position::new(ev.column, ev.row);
        let hits = &layout.diff;
        match ev.kind {
            MouseEventKind::ScrollDown => self.scroll_diff(3),
            MouseEventKind::ScrollUp => self.scroll_diff(-3),
            MouseEventKind::Down(MouseButton::Left) => {
                if !hits.area.is_some_and(|a| a.contains(at)) {
                    self.close_diff();
                    // The click lands where it was aimed, too: the tab bar
                    // or the footer.
                    return false;
                }
                if let Some((_, i)) = hits.list.iter().find(|(row, _)| *row == ev.row)
                    && hits.body.is_some_and(|b| ev.column < b.x)
                {
                    if let Some(state) = self.diff.as_mut() {
                        state.at = *i;
                    }
                    self.load_diff();
                } else if let Some((_, id)) = hits.folds.iter().find(|(row, _)| *row == ev.row)
                    && hits.body.is_some_and(|b| b.contains(at))
                    && let Some(state) = self.diff.as_mut()
                {
                    state.opened.insert(*id);
                    self.needs_redraw = true;
                }
            }
            _ => {}
        }
        true
    }
}

/// A diff with each kind of row in it, for the tests and the snapshots: a
/// hunk under a function, a modified line whose words changed, an added and a
/// removed line, and unchanged stretches before, between and after.
#[cfg(test)]
pub(super) fn sample() -> FileDiff {
    let old: Vec<String> = (1..=40)
        .map(|i| match i {
            12 => "    let timeout = Duration::from_secs(30);".to_string(),
            13 => "    retry(request, 3)".to_string(),
            30 => "fn legacy() {}".to_string(),
            _ => format!("line {i}"),
        })
        .collect();
    let mut new = old.clone();
    new[11] = "    let timeout = Duration::from_secs(90);".into();
    new[12] = "    retry(request, attempts)".into();
    new.insert(13, "    log::debug!(\"sent\");".into());
    new.remove(30);
    let diff = [
        "@@ -9,7 +9,8 @@ fn send(request: Request) {",
        " line 9",
        " line 10",
        " line 11",
        "-    let timeout = Duration::from_secs(30);",
        "-    retry(request, 3)",
        "+    let timeout = Duration::from_secs(90);",
        "+    retry(request, attempts)",
        "+    log::debug!(\"sent\");",
        " line 14",
        " line 15",
        "@@ -27,7 +28,6 @@ impl Client {",
        " line 27",
        " line 28",
        " line 29",
        "-fn legacy() {}",
        " line 31",
        " line 32",
        " line 33",
    ]
    .map(String::from)
    .to_vec();
    FileDiff {
        diff,
        old: Some(old),
        new: Some(new),
        binary: false,
        too_big: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(rows: &[Row]) -> String {
        rows.iter()
            .map(|r| match r {
                Row::Hunk(_) => 'H',
                Row::Fold { .. } => 'F',
                Row::Same { .. } => '=',
                Row::Pair {
                    left: Some(_),
                    right: Some(_),
                } => 'M',
                Row::Pair { left: Some(_), .. } => '-',
                Row::Pair { right: Some(_), .. } => '+',
                Row::Pair { .. } => '?',
            })
            .collect()
    }

    /// Removed and added lines pair by order, a side with more lines than the
    /// other leaves the other blank, context sits on both sides, and the
    /// stretches between hunks fold.
    #[test]
    fn a_hunk_lines_up_side_by_side() {
        let view = sample();
        let hunks = parse(&view.diff);
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].heading, "fn send(request: Request) {");
        let rows = rows(&view, &hunks, &HashSet::new(), false);
        // Fold (1–8), hunk, 3 same, 2 modified, 1 added, 2 same, fold (16–27
        // new), hunk, 3 same, 1 removed, 3 same, fold to the end.
        assert_eq!(kinds(&rows), "FH===MM+==FH===-===F");
        let Row::Pair {
            left: Some(l),
            right: Some(r),
        } = &rows[5]
        else {
            panic!("{:?}", rows[5]);
        };
        assert_eq!((l.no, r.no), (12, 12));
        let Row::Pair {
            left: None,
            right: Some(r),
        } = &rows[7]
        else {
            panic!("{:?}", rows[7]);
        };
        assert_eq!(r.no, 14);
        let Row::Pair {
            left: Some(l),
            right: None,
        } = &rows[15]
        else {
            panic!("{:?}", rows[15]);
        };
        assert_eq!(l.no, 30);
        let Row::Fold {
            old,
            new,
            count,
            open,
            ..
        } = rows[0]
        else {
            panic!()
        };
        assert_eq!((old, new, count, open), (1, 1, 8, true));
        let Row::Fold { count, .. } = rows[10] else {
            panic!()
        };
        assert_eq!(count, 11);
        let Row::Fold { old, count, .. } = rows[19] else {
            panic!()
        };
        assert_eq!((old, count), (34, 7));
    }

    /// A fold opened shows its lines, numbered on both sides; `all` opens
    /// every one; a fold of a few lines is never folded.
    #[test]
    fn a_fold_opens_into_its_lines() {
        let view = sample();
        let hunks = parse(&view.diff);
        let opened: HashSet<usize> = [1].into();
        let rows = rows(&view, &hunks, &opened, false);
        let same: Vec<_> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Same { old, new, text } if text == "line 20" => Some((*old, *new)),
                _ => None,
            })
            .collect();
        // One line was inserted above it, so it is a line further down on the right.
        assert_eq!(same, [(20, 21)]);
        let all = rows_of(&view, true);
        assert!(!all.iter().any(|r| matches!(r, Row::Fold { .. })));
        // Forty lines on the right, three of them added: the rest unchanged.
        assert_eq!(
            all.iter().filter(|r| matches!(r, Row::Same { .. })).count(),
            37
        );
    }

    fn rows_of(view: &FileDiff, all: bool) -> Vec<Row> {
        rows(view, &parse(&view.diff), &HashSet::new(), all)
    }

    /// An untracked file is one hunk of added lines; a deleted one, of removed.
    #[test]
    fn a_new_file_is_all_added() {
        let view = FileDiff {
            diff: vec!["+a".into(), "+b".into()],
            new: Some(vec!["a".into(), "b".into()]),
            ..FileDiff::default()
        };
        let rows = rows_of(&view, false);
        assert_eq!(kinds(&rows), "H++");
        let deleted = FileDiff {
            diff: vec!["@@ -1,2 +0,0 @@".into(), "-a".into(), "-b".into()],
            old: Some(vec!["a".into(), "b".into()]),
            ..FileDiff::default()
        };
        assert_eq!(kinds(&rows_of(&deleted, false)), "H--");
    }

    /// The words a modified line changed are marked, and nothing else; a line
    /// rewritten whole marks nothing, its wash says so already.
    #[test]
    fn the_words_that_changed_are_marked() {
        let (a, b) = words("retry(request, 3)", "retry(request, attempts)");
        assert_eq!(a, [(15, 16)]);
        assert_eq!(b, [(15, 23)]);
        let (a, b) = words("let x = 1;", "completely different words here");
        assert!(a.is_empty() && b.is_empty());
        let (a, b) = words("same", "same");
        assert!(a.is_empty() && b.is_empty());
    }

    /// Narrow, a pair is drawn as two rows, removed above added, each with
    /// both gutters; wide, as one row with a divider between its halves.
    #[test]
    fn unified_below_the_split_width() {
        let view = sample();
        let rows = rows_of(&view, false);
        let split = lines(&rows, &syntax::Pair::default(), true, 0, 140);
        let unified = lines(&rows, &syntax::Pair::default(), false, 0, 90);
        let pairs = rows
            .iter()
            .filter(|r| matches!(r, Row::Pair { .. }))
            .count();
        let both = rows
            .iter()
            .filter(|r| {
                matches!(
                    r,
                    Row::Pair {
                        left: Some(_),
                        right: Some(_)
                    }
                )
            })
            .count();
        assert_eq!(split.len(), rows.len());
        assert_eq!(unified.len(), rows.len() + both);
        assert!(pairs > both);
        let text = |l: &Line| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        };
        assert!(split.iter().any(|(l, _)| text(l).contains('│')));
        assert!(!unified.iter().any(|(l, _)| text(l).contains('│')));
        // Every line fills the width, so a wash runs edge to edge.
        for (l, _) in split.iter().chain(&unified) {
            let w = util::cells(&text(l));
            assert!(w == 140 || w == 90, "{w}: {}", text(l));
        }
    }

    /// `n` and `p` follow the tree: folders first, then files, at each level.
    #[test]
    fn files_step_in_the_trees_order() {
        let file = |p: &str| super::super::sidebar::File {
            path: p.into(),
            ..Default::default()
        };
        let files = [
            file("z.rs"),
            file("src/b.rs"),
            file("a.md"),
            file("src/a/x.rs"),
        ];
        let order: Vec<_> = in_tree_order(&files).into_iter().map(|f| f.path).collect();
        assert_eq!(order, ["src/a/x.rs", "src/b.rs", "a.md", "z.rs"]);
    }

    /// A long line is cut with `…` and scrolls sideways; the marks move with it.
    #[test]
    fn a_long_line_is_cut_and_scrolls() {
        let base = Style::default();
        let mark = Style::default().add_modifier(Modifier::BOLD);
        let text = |spans: Vec<Span>| {
            spans
                .iter()
                .map(|s| s.content.to_string())
                .collect::<String>()
        };
        assert_eq!(
            text(cut("abcdefghij", &[], &[], 0, 6, base, base)),
            "abcde…"
        );
        assert_eq!(
            text(cut("abcdefghij", &[], &[], 4, 6, base, base)),
            "efghij"
        );
        assert_eq!(text(cut("abc", &[], &[], 0, 6, base, base)), "abc   ");
        let spans = cut("abcdef", &[(2, 4)], &[], 1, 6, base, mark);
        let marked: String = spans
            .iter()
            .filter(|s| s.style == mark)
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(marked, "cd");
    }
}
