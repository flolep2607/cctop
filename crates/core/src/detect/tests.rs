//! The engine's semantics, ported from herdr's `src/detect/manifest/tests.rs`
//! (Apache-2.0, commit `ee5bfb5041bb3e4a49d591c72308aae3c81ecdde`) with its
//! override and remote-update cases left out — cctop has neither layer — and
//! the bundled manifests checked against screens: Claude Code's and Codex's
//! captured off real sessions for cctop's own footer tests, the rest written
//! to the shapes herdr's manifests describe.

use super::*;

/// A manifest of `rules` alone, leaked so its readings can borrow rule ids
/// for `'static` the way a bundled one's do.
fn manifest(rules: &str) -> &'static Manifest {
    let parsed = parse(&format!("id = \"test\"\n\n{rules}")).expect("manifest parses");
    Box::leak(Box::new(parsed))
}

fn read(manifest: &'static Manifest, screen: &str) -> Detection {
    manifest.detect(Input {
        screen,
        ..Input::default()
    })
}

fn region_of(screen: &str, spec: &str) -> String {
    region(
        Input {
            screen,
            ..Input::default()
        },
        parse_region(spec).expect("region parses"),
    )
    .to_string()
}

/// What `harness`'s bundled manifest makes of `screen` as rows.
fn bundled_state(harness: &str, screen: &str) -> State {
    let rows: Vec<String> = screen.lines().map(String::from).collect();
    let text = screen_text(&rows);
    detect(
        harness,
        Input {
            screen: &text,
            ..Input::default()
        },
    )
    .expect("a bundled manifest")
    .state
}

/// The one deliberate break from herdr: a known agent whose screen matches
/// nothing is not called idle, because cctop has a clock to ask instead.
#[test]
fn no_match_is_unknown_rather_than_idle() {
    let m = manifest(
        r#"
[[rules]]
id = "test"
state = "working"
contains = ["active-marker"]
"#,
    );
    let reading = read(m, "unmatched-marker");
    assert_eq!(reading.state, State::Unknown);
    assert_eq!(reading.rule, None);
}

#[test]
fn rule_semantics_apply_gates_priority_and_line_regex() {
    let m = manifest(
        r#"
[[rules]]
id = "low_contains"
state = "idle"
priority = 1
contains = ["match"]

[[rules]]
id = "high_nested_gates"
state = "working"
priority = 10
contains = ["match"]
all = [
  { any = [{ regex = ["w[io]n"] }, { contains = ["fallback"] }] },
]
not = [
  { contains = ["blocked"] },
]

[[rules]]
id = "line_regex"
state = "blocked"
priority = 20
line_regex = ["^exact line$"]
"#,
    );
    let high = read(m, "match win");
    assert_eq!(high.state, State::Working);
    assert_eq!(high.rule, Some("high_nested_gates"));

    let not_gate = read(m, "match win blocked");
    assert_eq!(not_gate.state, State::Idle);
    assert_eq!(not_gate.rule, Some("low_contains"));

    let line = read(m, "before\nexact line\nafter");
    assert_eq!(line.state, State::Blocked);
    assert_eq!(line.rule, Some("line_regex"));

    // `contains` ignores case; a regex does not unless it says so.
    assert_eq!(read(m, "MATCH WIN").rule, Some("low_contains"));
}

/// Equal priorities go to the rule written first, as herdr's do.
#[test]
fn a_tie_goes_to_the_first_rule_in_the_file() {
    let m = manifest(
        r#"
[[rules]]
id = "first"
state = "working"
priority = 5
contains = ["x"]

[[rules]]
id = "second"
state = "blocked"
priority = 5
contains = ["x"]
"#,
    );
    assert_eq!(read(m, "x").rule, Some("first"));
}

// A synthetic terminal protocol: live controls may precede a named divider
// and an optional panel, but a newer prompt/output makes them historical.
#[test]
fn end_anchored_footer_gates_allow_only_auxiliary_tail() {
    let m = manifest(
        r#"
[[rules]]
id = "live_controls"
state = "blocked"
region = "whole_recent"
regex = ['(?i)(?:^|\n)controls:\s+(?:confirm-marker|select-marker\s+navigate-marker)\s+cancel-marker[ \t]*\n---[^\n]*(?:\npanel: \d+(?:\nrow: [^\n]+)+(?:\n… \+\d+ pending)?)?(?:\n[ \t]*)*\z']
"#,
    );
    let footer = "controls: select-marker navigate-marker cancel-marker";
    for tail in [
        "--- name",
        "--- name\npanel: 5\nrow: active\nrow: pending\nrow: pending\nrow: pending\nrow: done\n",
        "--- name\npanel: 5\nrow: active\nrow: done\n… +3 pending",
    ] {
        let live = format!("old output\n{footer}\n{tail}");
        assert_eq!(read(m, &live).state, State::Blocked);
        for later in [
            "new output",
            "prompt:",
            "--- next\nprompt:",
            "prompt: continue…",
            "working: processing…",
            "…ordinary output",
            "finished…",
            "… +2 arbitrary words",
        ] {
            assert_eq!(
                read(m, &format!("{live}\n{later}")).state,
                State::Unknown,
                "{later}"
            );
        }
    }
    for tail in [
        "--- name\n… +2 pending",
        "--- name\nrow: active",
        "--- name\npanel: 5",
        "--- name\npanel: 5\nrow: active\n  working: processing…",
        "--- name\npanel: 5\nrow: active\n  prompt: continue…",
    ] {
        assert_eq!(read(m, &format!("{footer}\n{tail}")).state, State::Unknown);
    }
    for controls in [
        "controls: confirm-marker cancel-marker",
        "controls: select-marker\nnavigate-marker cancel-marker",
        "controls:\nselect-marker\nnavigate-marker\ncancel-marker",
    ] {
        assert_eq!(
            read(m, &format!("{controls}\n--- name")).state,
            State::Blocked
        );
    }
}

#[test]
fn paired_alternatives_require_complete_gates_at_bottom_buffer() {
    let m = manifest(
        r#"
[[rules]]
id = "paired"
state = "blocked"
region = "bottom_non_empty_lines(3)"
visible_blocker = true
any = [
  { all = [
    { contains = ["alpha heading"] },
    { regex = ['alpha\s+control\s*\z'] },
  ] },
  { all = [
    { contains = ["beta heading"] },
    { regex = ['beta\s+control\s*\z'] },
  ] },
]
"#,
    );
    for text in [
        "alpha heading\nalpha control",
        "beta heading\nbeta\ncontrol",
    ] {
        let reading = read(m, text);
        assert_eq!(reading.state, State::Blocked);
        assert!(reading.visible);
    }
    for text in [
        "alpha heading\nbeta control",
        "beta heading\nalpha control",
        "beta control",
        "beta heading",
        "beta heading\nbeta control\nnewer content",
        "beta heading\nfiller one\nfiller two\nbeta control",
    ] {
        let reading = read(m, text);
        assert!(reading.rule.is_none(), "{text}");
        assert!(!reading.visible, "{text}");
    }
}

#[test]
fn osc_regions_use_separate_inputs_and_share_rule_priority() {
    let m = manifest(
        r#"
[[rules]]
id = "screen"
state = "idle"
priority = 10
region = "whole_recent"
visible_idle = true
contains = ["screen-marker"]

[[rules]]
id = "title"
state = "working"
priority = 20
region = "osc_title"
visible_working = true
regex = ['^title-marker$']

[[rules]]
id = "progress"
state = "blocked"
priority = 30
region = "osc_progress"
visible_blocker = true
regex = ['^progress-marker$']
"#,
    );
    for (screen, title, progress, state, rule) in [
        ("screen-marker", "", "", State::Idle, "screen"),
        ("screen-marker", "title-marker", "", State::Working, "title"),
        (
            "screen-marker",
            "title-marker",
            "progress-marker",
            State::Blocked,
            "progress",
        ),
        (
            "screen-marker title-marker progress-marker",
            "",
            "",
            State::Idle,
            "screen",
        ),
    ] {
        let reading = m.detect(Input {
            screen,
            title,
            progress,
        });
        assert_eq!(reading.state, state);
        assert_eq!(reading.rule, Some(rule));
        assert!(reading.visible);
    }
    let swapped = m.detect(Input {
        screen: "",
        title: "progress-marker",
        progress: "title-marker",
    });
    assert!(swapped.rule.is_none());
}

#[test]
fn skip_rule_suppresses_state_update_without_visible_state_evidence() {
    let m = manifest(
        r#"
[[rules]]
id = "activity"
state = "working"
priority = 10
visible_working = true
contains = ["activity-marker"]

[[rules]]
id = "overlay"
state = "unknown"
priority = 20
skip_state_update = true
contains = ["overlay-marker"]
"#,
    );
    let reading = read(m, "activity-marker overlay-marker");
    assert_eq!(reading.state, State::Unknown);
    assert!(reading.skip);
    assert_eq!(reading.rule, Some("overlay"));
    assert!(!reading.visible);
}

#[test]
fn screen_regions_extract_structure_without_classifying_agent_state() {
    for (screen, spec, expected) in [
        ("old\n\nnew\n", "bottom_lines(2)", "\nnew\n"),
        (
            "before\n› input\nafter\n",
            "after_last_prompt_marker",
            "after\n",
        ),
        (
            "before\n› input\nafter\n",
            "before_current_prompt_marker",
            "before\n",
        ),
        (
            "before\n› input\nafter\n",
            "whole_recent_without_current_prompt_marker",
            "",
        ),
        (
            "no marker\n",
            "whole_recent_without_current_prompt_marker",
            "no marker\n",
        ),
        (
            "• old\n■ latest\n› input\n",
            "current_prompt_block_marker",
            "■ latest",
        ),
        (
            "• old\n■ latest\n› input\n",
            "after_current_prompt_block_marker",
            "■ latest\n› input\n",
        ),
        ("› old\n• new\n", "current_prompt_block_marker", ""),
        (
            "above\n\n───\nbody\n───\nfooter\n",
            "above_prompt_box",
            "above\n\n",
        ),
        (
            "above\n\n───\nbody\n───\nfooter\n",
            "last_non_empty_above_prompt_box",
            "above",
        ),
        (
            "above\n───\nbody\n───\nfooter\n",
            "prompt_box_body",
            "body\n",
        ),
        (
            "above\n───\nbody\n───\nfooter\n",
            "after_last_horizontal_rule",
            "footer\n",
        ),
        (
            "marker\nold\n\nmiddle\nmarker\nnew\n",
            "bottom_non_empty_lines(2)",
            "marker\nnew\n",
        ),
        (
            "\nmarker\nold\n\nmiddle\nmarker\nnew\n",
            "top_non_empty_lines(2)",
            "\nmarker\nold\n",
        ),
    ] {
        assert_eq!(region_of(screen, spec), expected, "region={spec}");
    }
}

#[test]
fn top_non_empty_lines_requires_a_canonical_positive_bounded_count() {
    let name = "top_non_empty_lines";
    assert!(parse_region(&format!("{name}(1)")).is_some());
    assert!(parse_region(&format!("{name}({})", u16::MAX)).is_some());
    for count in ["0", "01", "+1", "65536", "999999999999999999999999"] {
        assert!(
            parse_region(&format!("{name}({count})")).is_none(),
            "{name} accepted invalid count {count}"
        );
    }
}

#[test]
fn manifest_validation_rejects_unknown_fields_invalid_regions_and_regexes() {
    let parse_rules = |rules: &str| parse(&format!("id = \"test\"\n\n{rules}"));
    // A typo is refused rather than read as a rule with no matcher.
    assert!(
        parse_rules(
            r#"
[[rules]]
id = "typo"
state = "working"
contain = ["Working"]
"#
        )
        .is_err()
    );
    assert!(parse("id = \"test\"\n").is_err(), "no rules");
    assert!(
        parse_rules(
            r#"
[[rules]]
id = "region"
state = "working"
region = "bottom_rows(3)"
contains = ["x"]
"#
        )
        .is_err()
    );
    assert!(
        parse_rules(
            r#"
[[rules]]
id = "regex"
state = "working"
regex = ["("]
"#
        )
        .is_err()
    );
    // Only negations can never match anything on their own.
    assert!(
        parse_rules(
            r#"
[[rules]]
id = "negative"
state = "working"
not = [{ contains = ["x"] }]
"#
        )
        .is_err()
    );
    // A skip rule says nothing about state.
    assert!(
        parse_rules(
            r#"
[[rules]]
id = "skip"
state = "working"
skip_state_update = true
contains = ["x"]
"#
        )
        .is_err()
    );
}

/// Every manifest compiled in parses, and answers to exactly the names its
/// own `id` and `aliases` give it — the lookup table repeats them so finding
/// one compiles nothing, and this keeps the copy honest.
#[test]
fn every_bundled_manifest_parses_and_answers_to_its_own_names() {
    for bundled in &BUNDLED {
        let parsed = parse(bundled.source);
        assert!(parsed.is_ok(), "{}: {:?}", bundled.names[0], parsed.err());
        let doc: toml_edit::DocumentMut = bundled.source.parse().expect("toml");
        let mut names = vec![doc["id"].as_str().expect("id").to_string()];
        if let Some(aliases) = doc.get("aliases").and_then(Item::as_array) {
            names.extend(aliases.iter().filter_map(|a| a.as_str()).map(String::from));
        }
        assert_eq!(bundled.names, names.as_slice());
        assert!(known(bundled.names[0]));
    }
    assert_eq!(agents().count(), 22);
    assert!(!known("aider"));
    assert!(detect("aider", Input::default()).is_none());
    assert!(known("Claude"), "names are matched without case");
}

#[test]
fn screen_text_is_the_shape_the_rules_are_written_for() {
    let rows: Vec<String> = ["one   ", "", "two", "  ", ""]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(screen_text(&rows), "one\n\ntwo\n");
    assert_eq!(screen_text(&[]), "");
}

/// Claude Code 2.1.283's own screens, the ones cctop's footer table was
/// captured from: at its prompt, mid-turn, on a permission prompt and on an
/// AskUserQuestion.
#[test]
fn claude_code_screens() {
    let idle = "\
────────────────────────────
❯ Try \"create a util logging.py that...\"
────────────────────────────
  ⚠ Transcript saving is off
  ⏸ manual mode on · ? for shortcuts
";
    assert_eq!(bundled_state("claude", idle), State::Idle);
    let busy = "\
✢ Quantumizing… (2s · thinking)
────────────────────────────
❯
────────────────────────────
  ⚠ Transcript saving is off
  ⏸ manual mode on · esc to interrupt";
    assert_eq!(bundled_state("claude", busy), State::Working);
    let permission = "\
 Bash command
   rm -rf build
 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, and always allow access to this folder
   3. No

 Esc to cancel · Tab to amend";
    assert_eq!(bundled_state("claude", permission), State::Blocked);
    let question = "\
 ☐ Deploy

How should the deploy step happen?

❯ 1. You deploy, I run SQL (Recommended)
  2. I restart the worker
  3. Type something.
──────────────────────────────────────
  4. Chat about this

Enter to select · ↑/↓ to navigate · Esc to cancel";
    assert_eq!(bundled_state("claude", question), State::Blocked);
    // A finished turn's line has no ellipsis, so it is no spinner, and the
    // empty prompt box under it is the agent waiting.
    let finished = "\
✻ Baked for 5s
────────────
❯
────────────
  ⏵⏵ accept edits on (shift+tab to cycle)";
    assert_eq!(bundled_state("claude", finished), State::Idle);
}

/// Claude Code's title is its spinner, when the title is readable.
#[test]
fn claude_code_title() {
    let read = |title: &str| {
        detect(
            "claude",
            Input {
                title,
                ..Input::default()
            },
        )
        .map(|d| d.state)
    };
    assert_eq!(read("⠂ Fixing the parser"), Some(State::Working));
    assert_eq!(read("◐ Fixing the parser"), Some(State::Working));
    assert_eq!(read("✳ Claude Code"), Some(State::Idle));
    assert_eq!(read(""), Some(State::Unknown));
}

/// Codex 0.157.1's own screens. Codex keeps `? for shortcuts` on its bottom
/// line throughout and draws a frame mid-turn with no working line, so the
/// manifest calls neither idle — only its title does that.
#[test]
fn codex_screens() {
    let busy = "\
› Run the shell command: touch hello.txt
• I’ll create hello.txt in the current workspace.
◦ Working (5s • esc to interrupt)

› Ask Codex to do anything

  GPT-5.6-Terra high · ~/play · No changes
  ← for agents · ? for shortcuts";
    assert_eq!(bundled_state("codex", busy), State::Working);
    let between = "\
› Run the shell command: touch hello.txt
• I’ll create hello.txt in the current workspace.

› Ask Codex to do anything

  GPT-5.6-Terra high · ~/play · No changes
  ← for agents · ? for shortcuts";
    assert_eq!(bundled_state("codex", between), State::Unknown);
    let approval = "\
• Running touch ../outside.txt
  Would you like to run the following command?
  $ touch ../outside.txt
› 1. Yes, proceed (y)
  2. Yes, and don't ask again for commands that start with `touch ../outside.txt` (p)
  3. No, and tell Codex what to do differently (esc)
  Press enter to confirm or esc to cancel";
    assert_eq!(bundled_state("codex", approval), State::Blocked);
}

/// One screen per state for the other agents, in the words their manifests
/// look for — so the harnesses whose words overlap stay apart: `esc to
/// cancel` is Gemini CLI at work and Claude Code asking.
#[test]
fn every_other_agent_by_its_own_words() {
    use State::{Blocked, Working};
    for (harness, screen, want) in [
        ("gemini", "⠏ Reading files (esc to cancel, 3s)", Working),
        (
            "gemini",
            "│ Allow execution of: 'touch x'?\n│ ● 1. Yes",
            Blocked,
        ),
        ("opencode", "■■■■⬝⬝⬝⬝  esc interrupt", Working),
        ("opencode", "△ Permission required\n  bash touch x", Blocked),
        ("droid", "⠋ Thinking…  (esc to stop)", Working),
        (
            "droid",
            "> Yes, allow\n  No, cancel\nEnter to select · ↑↓ to navigate · Esc to cancel",
            Blocked,
        ),
        ("pi", "── ⠋ Working ──────", Working),
        ("pi", "Working...", Working),
    ] {
        assert_eq!(
            bundled_state(harness, screen),
            want,
            "{harness}: {screen:?}"
        );
    }
    assert_eq!(bundled_state("pi", "> hello"), State::Unknown);
}
