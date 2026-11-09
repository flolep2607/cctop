//! `cctop recall`: the command-line face of [`cctop_core::recall`], which the
//! MCP tools and the dashboard's recall view share.

pub use cctop_core::recall::{DEFAULT_LIMIT, caller, read, recall, render};

pub const HELP: &str = "\
cctop recall — what past sessions said about something, from any agent

USAGE:
  cctop recall QUERY... [--limit N] [--json]
  cctop recall --read SESSION PASSAGE [--around N]

Searches every transcript on this machine — Claude Code, Codex, OpenCode and the
rest — and returns the passages that best answer QUERY: by its words, and by its
meaning when the search model is fetched (`cctop --fetch-search-model`). Agents
get the same thing through `cctop --mcp`; see `cctop --install-mcp`.

OPTIONS:
  --limit N        Passages to return (default 8).
  --read S P       Print passage P of session S (an id or a unique prefix) with
                   its neighbours, to read a hit in context.
  --around N       Neighbours either side for --read (default 1).
  --json           Machine-readable.
  -h, --help       This.

Read-only, apart from the topical index cctop already keeps up to date.
";

/// `cctop recall`.
pub fn run(argv: &[String]) -> i32 {
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        print!("{HELP}");
        return 0;
    }
    let mut limit = DEFAULT_LIMIT;
    let mut json = false;
    let mut read: Option<(String, usize)> = None;
    let mut around = 1;
    let mut words: Vec<String> = Vec::new();
    let mut args = argv.iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--json" => json = true,
            "--limit" => match args.next().and_then(|n| n.parse().ok()) {
                Some(n) if n > 0 => limit = n,
                _ => return usage("--limit needs a number"),
            },
            "--around" => match args.next().and_then(|n| n.parse().ok()) {
                Some(n) => around = n,
                None => return usage("--around needs a number"),
            },
            "--read" => match (args.next(), args.next().and_then(|n| n.parse().ok())) {
                (Some(s), Some(n)) => read = Some((s.clone(), n)),
                _ => return usage("--read needs a session and a passage number"),
            },
            flag if flag.starts_with("--") => return usage(&format!("unknown option {flag}")),
            word => words.push(word.to_string()),
        }
    }

    let mut loader = cctop_core::loader::Loader::new();
    let sessions = loader.load(cctop_core::pricing::Plan::Retail);
    loader.store().save();

    if let Some((session, n)) = read {
        return match self::read(&sessions, &session, n, around) {
            Ok(text) => {
                print!("{text}");
                0
            }
            Err(e) => {
                eprintln!("cctop recall: {e}");
                2
            }
        };
    }
    let query = words.join(" ");
    if query.trim().is_empty() {
        return usage("give something to recall, e.g. `cctop recall why the cache is not shared`");
    }
    let answer = recall(&sessions, &query, limit, None);
    match json {
        true => println!(
            "{}",
            serde_json::to_string_pretty(&answer).unwrap_or_else(|_| "{}".into())
        ),
        false => print!("{}", render(&answer)),
    }
    0
}

fn usage(why: &str) -> i32 {
    eprintln!("cctop recall: {why}; see --help");
    2
}
