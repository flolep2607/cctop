//! The hosts named in `~/.ssh/config`, for the launcher's Remote entry.
//!
//! Only the names, never the settings under them: whatever a block says about
//! users, ports, keys or jump hosts is OpenSSH's to apply, and `cctop sandbox`
//! hands ssh the alias exactly as written so it does. Reading anything more
//! here would be a second, worse implementation of ssh's own config logic.

use std::path::{Path, PathBuf};

/// One `Host` line that names something you can connect to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    /// The first concrete name on the line: what the launcher connects to.
    pub name: String,
    /// The line's other concrete names, shown beside it and matched by the
    /// filter.
    ///
    /// One entry per line rather than one per name, because the names on one
    /// line share one block of settings — `Host nz-b-procurementdb1 procdb` is
    /// a long name and its nickname for the same machine, and listing it twice
    /// would offer two rows that do the same thing. Whichever one the user
    /// remembers still finds it, since the filter reads all of them.
    pub aliases: Vec<String>,
}

impl Host {
    /// Whether `needle` (lowercased) is in any of this host's names.
    pub fn matches(&self, needle: &str) -> bool {
        std::iter::once(&self.name)
            .chain(&self.aliases)
            .any(|n| n.to_lowercase().contains(needle))
    }
}

/// How deep `Include` may nest, which is OpenSSH's own limit
/// (`READCONF_MAX_DEPTH`) — and what stops a file that includes itself.
const MAX_DEPTH: usize = 16;

/// The hosts in the user's ssh config, in the order the files name them.
///
/// Empty when there is no config, which is not an error: the Remote entry
/// still takes a typed `user@host`.
pub fn hosts() -> Vec<Host> {
    let Some(ssh_dir) = dirs::home_dir().map(|h| h.join(".ssh")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    read_into(&ssh_dir.join("config"), &ssh_dir, 0, &mut out);
    out
}

/// Parse `path` and everything it includes, appending what it names to `out`.
fn read_into(path: &Path, ssh_dir: &Path, depth: usize, out: &mut Vec<Host>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in parse(&text) {
        match line {
            Line::Host(host) => {
                // The same name in two files is one machine; the first file to
                // name it is the one ssh will mostly take its settings from.
                if !out.iter().any(|h| h.name == host.name) {
                    out.push(host);
                }
            }
            Line::Include(patterns) => {
                for pattern in patterns {
                    for file in expand_include(&pattern, ssh_dir) {
                        read_into(&file, ssh_dir, depth + 1, out);
                    }
                }
            }
        }
    }
}

/// What one line of a config can contribute.
#[derive(Debug, PartialEq, Eq)]
enum Line {
    Host(Host),
    Include(Vec<String>),
}

/// The `Host` and `Include` lines of one file, in order.
///
/// Keywords are case-insensitive and may be separated from their arguments by
/// `=` as well as by blanks, both as ssh_config(5) allows. A `Host` line of
/// nothing but patterns — `Host *`, `Host *.internal !bastion` — sets defaults
/// for other hosts and names none, so it contributes nothing.
fn parse(text: &str) -> Vec<Line> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        let (keyword, rest) = split_keyword(line);
        let words = words(rest);
        if keyword.eq_ignore_ascii_case("host") {
            let mut names = words.into_iter().filter(|w| is_concrete(w));
            if let Some(name) = names.next() {
                out.push(Line::Host(Host {
                    name,
                    aliases: names.collect(),
                }));
            }
        } else if keyword.eq_ignore_ascii_case("include") {
            out.push(Line::Include(words));
        }
    }
    out
}

/// A line without its comment: from a `#` that starts a word to the end.
///
/// ssh itself only promises whole-line comments, but people write trailing
/// ones, and a `#` glued to a word is not one.
fn strip_comment(line: &str) -> &str {
    let mut prev_blank = true;
    for (at, c) in line.char_indices() {
        if c == '#' && prev_blank {
            return &line[..at];
        }
        prev_blank = c.is_whitespace();
    }
    line
}

/// `Keyword args` or `Keyword=args`, as the keyword and the rest.
fn split_keyword(line: &str) -> (&str, &str) {
    let end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let rest = line[end..].trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
    (&line[..end], rest)
}

/// Blank-separated words, with double quotes grouping and then dropped.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in text.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut word));
                    any = false;
                }
            }
            c => {
                word.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(word);
    }
    out.retain(|w| !w.is_empty());
    out
}

/// A name that is one host, not a pattern over many or an exclusion.
fn is_concrete(word: &str) -> bool {
    !word.contains(['*', '?']) && !word.starts_with('!')
}

/// The files one `Include` argument names.
///
/// Relative to `~/.ssh`, as ssh reads a user config's includes, and with `~`
/// for the home. A wildcard is honoured in the file name, which is how every
/// `Include config.d/*` in the wild is written.
///
/// ponytail: a wildcard in a *directory* component (`Include */config`) is
/// taken literally and so matches nothing; nobody has been seen to write one,
/// and supporting it is a glob engine for a list of host names.
fn expand_include(pattern: &str, ssh_dir: &Path) -> Vec<PathBuf> {
    let path = match pattern.strip_prefix("~/") {
        Some(rest) => match dirs::home_dir() {
            Some(home) => home.join(rest),
            None => return Vec::new(),
        },
        None => ssh_dir.join(pattern),
    };
    let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return Vec::new();
    };
    if !name.contains(['*', '?']) {
        return vec![path];
    }
    let Some(dir) = path.parent() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| wildcard(&name, &e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    // ssh reads a glob's matches in sorted order, which is what makes
    // `config.d/10-work` come before `config.d/20-home`.
    files.sort();
    files
}

/// `*` and `?` over one file name. A leading dot is not matched by a
/// wildcard, as glob(3) would not match it.
fn wildcard(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], n) || (!n.is_empty() && go(p, &n[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &n[1..]),
            (Some(a), Some(b)) if a == b => go(&p[1..], &n[1..]),
            _ => false,
        }
    }
    if name.starts_with('.') && !pattern.starts_with('.') {
        return false;
    }
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    go(&p, &n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hosts_of(text: &str) -> Vec<Host> {
        parse(text)
            .into_iter()
            .filter_map(|line| match line {
                Line::Host(h) => Some(h),
                Line::Include(_) => None,
            })
            .collect()
    }

    fn host(name: &str, aliases: &[&str]) -> Host {
        Host {
            name: name.into(),
            aliases: aliases.iter().map(|a| a.to_string()).collect(),
        }
    }

    #[test]
    fn wildcards_and_negations_name_no_host() {
        let text = "\
Host *
  ServerAliveInterval 30
Host *.internal !bastion
  User me
Host devbox
  HostName 10.0.0.5
Host web-? db
";
        assert_eq!(hosts_of(text), vec![host("devbox", &[]), host("db", &[])]);
    }

    #[test]
    fn several_names_on_one_line_are_one_host_with_aliases() {
        let text = "Host nz-b-procurementdb1 procdb\n  User flo\n";
        assert_eq!(
            hosts_of(text),
            vec![host("nz-b-procurementdb1", &["procdb"])]
        );
        let h = &hosts_of(text)[0];
        assert!(h.matches("procdb"));
        assert!(h.matches("procurement"));
        assert!(!h.matches("web"));
    }

    #[test]
    fn comments_case_and_equals_are_read_as_ssh_reads_them() {
        let text = "\
# Host commented-out
host lower # a trailing comment
HOST=upper
  Host   indented\tother
Hostname not-a-host-line
Host \"quoted name\" with#hash
";
        assert_eq!(
            hosts_of(text),
            vec![
                host("lower", &[]),
                host("upper", &[]),
                host("indented", &["other"]),
                host("quoted name", &["with#hash"]),
            ]
        );
    }

    #[test]
    fn includes_are_followed_relative_to_dot_ssh_in_sorted_order() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ssh = dir.path();
        std::fs::create_dir(ssh.join("config.d")).expect("mkdir");
        std::fs::write(
            ssh.join("config"),
            "Include config.d/*\nHost main\nInclude config\n",
        )
        .expect("write");
        std::fs::write(ssh.join("config.d/20-b"), "Host second main\n").expect("write");
        std::fs::write(ssh.join("config.d/10-a"), "Host first\n").expect("write");
        std::fs::write(ssh.join("config.d/.hidden"), "Host hidden\n").expect("write");
        let mut out = Vec::new();
        // Includes itself, too: the depth limit is what ends that.
        read_into(&ssh.join("config"), ssh, 0, &mut out);
        let names: Vec<&str> = out.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(names, ["first", "second", "main"]);
    }

    #[test]
    fn a_wildcard_matches_one_file_name() {
        assert!(wildcard("*", "work"));
        assert!(wildcard("*.conf", "a.conf"));
        assert!(!wildcard("*.conf", "a.conf.bak"));
        assert!(wildcard("h?st", "host"));
        assert!(!wildcard("*", ".hidden"));
    }
}
