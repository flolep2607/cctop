//! Syntax colours for the diff view and for the fenced code in a rendered
//! reply: which stretches of a file's lines are a keyword, a string, a
//! comment, a number, a function's or a type's name.
//!
//! The grammars are syntect's bundled ones, and only its parser is used: a
//! line is parsed into scopes (`keyword.control.rust`, `string.quoted.double`)
//! and each scope is sorted into one of a handful of [`Kind`]s, which the
//! palette colours. So the colours are cctop's own, in the light and the dark
//! theme alike, rather than a Sublime theme's that knows nothing of the washes
//! drawn under them.
//!
//! Done once per file, on the worker, from both versions of it — never per
//! frame, and never on a hunk's lines alone, since a line out of context is a
//! line whose string or comment may have started above it.
//!
//! A fenced block in a reply ([`highlight_fence`]) is found by its info
//! string rather than a path, and is parsed on the UI thread, since that is
//! where the reader lays its turns out. The reader keeps a laid-out turn until
//! the width or a fold changes, and the blocks parsed are kept here as well, by
//! their text — so dragging the window wider re-wraps code without parsing it
//! again for every column it passes through.
//!
//! ponytail: sixteen colours and `NO_COLOR` get none. Sixteen shades already
//! carry the washes and the word marks, and a keyword in the same green as an
//! added line's ground is a keyword nobody can see.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock};

use ratatui::style::Color;
use syntect::parsing::{ParseState, Scope, ScopeStack, ScopeStackOp, SyntaxReference, SyntaxSet};

use super::theme::{self, Depth, Variant};

/// A file past this many bytes is drawn without colours: parsing it would
/// hold the worker for longer than the diff took to read.
const MAX_BYTES: usize = 1 << 20;
/// A line past this many bytes is left uncoloured, and the parse goes on from
/// the next. Minified files are one such line, and a backtracking grammar can
/// spend seconds on one.
const MAX_LINE: usize = 2_000;

/// What a stretch of code is, as far as colouring it goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Keyword,
    String,
    Comment,
    Number,
    Constant,
    Function,
    Type,
}

/// A line's coloured stretches, as char ranges into the line.
pub(super) type Spans = Vec<(usize, usize, Kind)>;

/// Each line's stretches, by line (`[0]` is line 1).
pub(super) type Lines = Vec<Spans>;

/// Both versions of a file, coloured: what the diff view draws from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Pair {
    pub old: Option<Lines>,
    pub new: Option<Lines>,
}

impl Pair {
    /// The stretches of line `no` (1-based) of the new version, or of the
    /// old one; none where that side was not coloured.
    pub(super) fn line(&self, new: bool, no: u64) -> &[(usize, usize, Kind)] {
        let side = match new {
            true => &self.new,
            false => &self.old,
        };
        side.as_ref()
            .and_then(|lines| lines.get((no as usize).checked_sub(1)?))
            .map_or(&[], Vec::as_slice)
    }
}

fn set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// The grammar for `path`: by its extension, then its file name (a
/// `Makefile`, a `Dockerfile`), then its first line (a shebang).
fn syntax_for<'a>(
    set: &'a SyntaxSet,
    path: &str,
    first: Option<&str>,
) -> Option<&'a SyntaxReference> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map(|(_, e)| e);
    ext.and_then(|e| set.find_syntax_by_extension(e))
        .or_else(|| set.find_syntax_by_extension(name))
        .or_else(|| first.and_then(|line| set.find_syntax_by_first_line(line)))
        .filter(|s| s.name != "Plain Text")
}

/// `lines` of the file at `path`, coloured. `None` for a file there is no
/// grammar for, or one too big to parse.
pub(super) fn highlight(path: &str, lines: &[String]) -> Option<Lines> {
    if lines.iter().map(|l| l.len() + 1).sum::<usize>() > MAX_BYTES {
        return None;
    }
    let set = set();
    let syntax = syntax_for(set, path, lines.first().map(String::as_str))?;
    Some(parse(set, syntax, lines))
}

/// `lines` parsed by `syntax`, line by line in one state.
fn parse(set: &SyntaxSet, syntax: &SyntaxReference, lines: &[String]) -> Lines {
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        if line.len() > MAX_LINE {
            out.push(Vec::new());
            continue;
        }
        // The `newlines` grammars match on the line's end, so it is given one.
        let text = format!("{line}\n");
        let Ok(ops) = state.parse_line(&text, set) else {
            // A grammar that failed partway colours nothing past it.
            out.resize(lines.len(), Vec::new());
            return out;
        };
        out.push(spans(line, &ops, &mut stack));
    }
    out
}

/// Whether the theme draws syntax at all — what [`color`] answers, asked
/// before any parsing, so a sixteen-colour or colourless terminal never loads
/// the grammars for a reply.
pub(super) fn colours_code() -> bool {
    let palette = theme::colors();
    palette.depth == Depth::Indexed && palette.variant != Variant::Mono
}

/// What a fence's language is called among syntect's grammars, for the names
/// people write that are neither a grammar's name nor its extension. `None`
/// for a fence that says it is not code.
///
/// TypeScript and JSX go to JavaScript, the nearest grammar bundled: its
/// keywords, strings and comments are theirs, and only a type annotation goes
/// uncoloured.
fn fence_alias(lang: &str) -> Option<&str> {
    Some(match lang {
        "" | "text" | "txt" | "plain" | "plaintext" | "output" => return None,
        "shell" | "bash" | "zsh" | "console" | "shell-session" => "sh",
        "python" | "python3" | "py3" => "py",
        "javascript" | "node" | "mjs" | "cjs" | "jsx" | "ts" | "typescript" | "tsx" => "js",
        "golang" => "go",
        "c++" | "cxx" | "hpp" => "cpp",
        "yml" => "yaml",
        "objc" | "objective-c" => "m",
        "patch" => "diff",
        _ => lang,
    })
}

/// The grammar a fenced block's info string names: its first word (`rust` of
/// `rust,ignore`, `py` of `{.py}`), as an alias, an extension or a grammar's
/// name, whatever its case.
fn syntax_for_fence<'a>(set: &'a SyntaxSet, info: &str) -> Option<&'a SyntaxReference> {
    let word = info
        .split(|c: char| c.is_whitespace() || c == ',')
        .next()?
        .trim_start_matches('{')
        .trim_start_matches('.')
        .trim_end_matches('}')
        .to_ascii_lowercase();
    set.find_syntax_by_token(fence_alias(&word)?)
        .filter(|s| s.name != "Plain Text")
}

/// How many fenced blocks' colours are kept. Past it the lot is dropped and
/// built again as the reader asks for them; one conversation rarely shows as
/// many.
const FENCES_KEPT: usize = 512;

/// A fenced block's `lines`, coloured by the language its info string names.
/// `None` where the theme draws no syntax, the language is none cctop knows,
/// or the block is too big to parse — and the block is drawn in one colour.
///
/// Kept by the block's info string and text, so a turn laid out again at a
/// new width does not parse its code again. What is kept is kinds, not
/// colours, so a theme switched meanwhile still applies.
pub(super) fn highlight_fence(info: &str, lines: &[String]) -> Option<Arc<Lines>> {
    if !colours_code() || lines.iter().map(|l| l.len() + 1).sum::<usize>() > MAX_BYTES {
        return None;
    }
    static KEPT: OnceLock<Mutex<HashMap<u64, Option<Arc<Lines>>>>> = OnceLock::new();
    let key = {
        let mut h = DefaultHasher::new();
        (info, lines).hash(&mut h);
        h.finish()
    };
    let kept = KEPT.get_or_init(Mutex::default);
    if let Some(hit) = kept.lock().ok().and_then(|k| k.get(&key).cloned()) {
        return hit;
    }
    let set = set();
    let out = syntax_for_fence(set, info).map(|syntax| Arc::new(parse(set, syntax, lines)));
    if let Ok(mut kept) = kept.lock() {
        if kept.len() >= FENCES_KEPT {
            kept.clear();
        }
        kept.insert(key, out.clone());
    }
    out
}

/// One line's ops, as char ranges of the kinds the scope stack names.
fn spans(line: &str, ops: &[(usize, ScopeStackOp)], stack: &mut ScopeStack) -> Spans {
    // Byte offsets from the parser, char offsets for the view, which cuts and
    // marks by char.
    let char_at = |byte: usize| line[..byte.min(line.len())].chars().count();
    let mut out: Spans = Vec::new();
    let mut from = 0;
    let push = |out: &mut Spans, a: usize, b: usize, stack: &ScopeStack| {
        if a < b
            && let Some(kind) = kind_of(stack.as_slice())
        {
            match out.last_mut() {
                Some(last) if last.1 == a && last.2 == kind => last.1 = b,
                _ => out.push((a, b, kind)),
            }
        }
    };
    for (byte, op) in ops {
        let at = char_at(*byte);
        push(&mut out, from, at, stack);
        from = at;
        let _ = stack.apply(op);
    }
    push(&mut out, from, line.chars().count(), stack);
    out
}

/// The kind of the innermost scope that has one: a string inside a function
/// call is a string.
fn kind_of(scopes: &[Scope]) -> Option<Kind> {
    scopes.iter().rev().find_map(|scope| {
        let name = scope.build_string();
        let starts = |p: &str| name.starts_with(p);
        Some(match () {
            _ if starts("comment") => Kind::Comment,
            _ if starts("string") => Kind::String,
            _ if starts("constant.numeric") => Kind::Number,
            _ if starts("constant.language") || starts("constant.character") => Kind::Constant,
            _ if starts("keyword.operator") || starts("punctuation") => return None,
            _ if starts("keyword") || starts("storage") => Kind::Keyword,
            _ if starts("entity.name.function") || starts("support.function") => Kind::Function,
            _ if starts("entity.name.type")
                || starts("entity.name.class")
                || starts("entity.name.struct")
                || starts("entity.name.enum")
                || starts("support.type")
                || starts("support.class") =>
            {
                Kind::Type
            }
            _ => return None,
        })
    })
}

/// The colour a kind is drawn in, or `None` where the theme draws none.
///
/// Picked against each theme's own ground and its diff washes: GitHub's
/// hues, stepped so each still reads on the red and green washes of a
/// changed line.
pub(super) fn color(kind: Kind) -> Option<Color> {
    let palette = theme::colors();
    if palette.depth != Depth::Indexed {
        return None;
    }
    let index = match (palette.variant, kind) {
        (Variant::Mono, _) => return None,
        (Variant::Dark, Kind::Keyword) => 211,
        (Variant::Dark, Kind::String) => 153,
        (Variant::Dark, Kind::Comment) => 245,
        (Variant::Dark, Kind::Number | Kind::Constant) => 117,
        (Variant::Dark, Kind::Function) => 183,
        (Variant::Dark, Kind::Type) => 215,
        (Variant::Light, Kind::Keyword) => 161,
        (Variant::Light, Kind::String) => 25,
        (Variant::Light, Kind::Comment) => 243,
        (Variant::Light, Kind::Number | Kind::Constant) => 31,
        (Variant::Light, Kind::Function) => 91,
        (Variant::Light, Kind::Type) => 130,
    };
    Some(Color::Indexed(index))
}

/// The colour of the char at `i` of a line, from its stretches.
pub(super) fn color_at(spans: &[(usize, usize, Kind)], i: usize) -> Option<Color> {
    spans
        .iter()
        .find(|&&(a, b, _)| (a..b).contains(&i))
        .and_then(|&(_, _, kind)| color(kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_string).collect()
    }

    /// A keyword, a string, a comment and a number each come out as what they
    /// are, by char, in a file known by its extension.
    #[test]
    fn rust_is_coloured_by_what_each_word_is() {
        let file = lines("// note\nfn main() {\n    let s = \"hé\"; let n = 42;\n}");
        let out = highlight("src/main.rs", &file).expect("rust has a grammar");
        assert_eq!(out.len(), 4);
        assert!(
            out[0]
                .iter()
                .any(|&(a, b, k)| k == Kind::Comment && a == 0 && b == 7)
        );
        let kinds = |i: usize| out[i].iter().map(|&(_, _, k)| k).collect::<Vec<_>>();
        assert!(kinds(1).contains(&Kind::Keyword), "{:?}", out[1]);
        assert!(kinds(1).contains(&Kind::Function), "{:?}", out[1]);
        // By char, past a two-byte é: the string is `"hé"`, cols 12..16.
        assert!(out[2].contains(&(12, 16, Kind::String)), "{:?}", out[2]);
        assert!(kinds(2).contains(&Kind::Number), "{:?}", out[2]);
    }

    /// A comment opened on one line still colours the next: the file is
    /// parsed whole, not line by line.
    #[test]
    fn a_comment_carries_across_lines() {
        let out = highlight("a.c", &lines("/* one\n   two */ int x;")).unwrap();
        assert_eq!(out[1].first().map(|s| s.2), Some(Kind::Comment));
    }

    /// Found by name, then by shebang; and nothing for a file there is no
    /// grammar for, or one too big to parse.
    #[test]
    fn a_grammar_is_found_by_name_or_first_line_or_not_at_all() {
        assert!(highlight("Makefile", &lines("all:\n\techo hi")).is_some());
        assert!(highlight("bin/run", &lines("#!/bin/sh\necho hi")).is_some());
        assert!(highlight("notes.unknownext", &lines("plain words")).is_none());
        let huge = vec!["x".repeat(1000); 2_000];
        assert!(highlight("big.rs", &huge).is_none());
    }

    /// A fenced block is parsed once: asked again for the same text, as a
    /// relayout at a new width does, it is the same colours, not a new parse.
    /// A theme that draws no syntax parses nothing.
    #[test]
    fn a_fence_is_parsed_once_and_not_at_all_without_colours() {
        theme::set_theme(Some("dark"));
        let code = lines("fn kept_once() {}");
        let first = highlight_fence("rust", &code).expect("rust has a grammar");
        let again = highlight_fence("rust", &code).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
        assert!(highlight_fence("notalanguage", &code).is_none());
        theme::set_theme(Some("mono"));
        assert!(highlight_fence("rust", &code).is_none());
        theme::set_theme(Some("dark"));
    }

    /// The palette's sixteen-colour and colourless themes draw no syntax.
    #[test]
    fn only_full_colour_themes_colour_code() {
        theme::set_theme(Some("dark"));
        assert!(color(Kind::Keyword).is_some());
        theme::set_theme(Some("mono"));
        assert!(color(Kind::Keyword).is_none());
        theme::set_theme(Some("dark"));
    }
}
