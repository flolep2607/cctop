//! Text as a person should read it: what the harness wrote into a user turn
//! rather than the person, the tags it wraps its own words in, and who a
//! message is really from.

use super::*;

/// Whether a user turn is really the harness talking.
///
/// Claude Code writes several of its own things into `user` entries — the
/// expansion of a slash command, a hook's output, the reminder blocks it
/// injects — and showing those as something a person typed is the difference
/// between a transcript someone recognises and one they do not.
///
/// The named prefixes stay because those blocks are also written on one line,
/// where the shape below cannot see them. The shape is what catches the rest:
/// the list of names was a list that had to be kept up to date and was not, and
/// `<task-notification>` reached a handoff brief quoted as the user's own words
/// because nothing had added it. A person opening a message with a bare tag and
/// nothing else on the line is the rarer mistake to make.
fn is_harness_text(text: &str) -> bool {
    let head = text.trim_start();
    head.starts_with("<command-name>")
        || head.starts_with("<command-message>")
        || head.starts_with("<local-command")
        || head.starts_with("<system-reminder>")
        || head.starts_with("<user-prompt-submit-hook>")
        || head.starts_with("Caveat:")
        || opens_with_bare_tag(head)
}

/// Whether the first line is `<name>` and nothing else — the shape every
/// injected block shares, and the shape prose does not.
fn opens_with_bare_tag(text: &str) -> bool {
    let Some(line) = text.lines().next().map(str::trim) else {
        return false;
    };
    let Some(name) = line.strip_prefix('<').and_then(|l| l.strip_suffix('>')) else {
        return false;
    };
    // No attributes and no closing tag on the same line: `<p>hi</p>` is
    // somebody pasting markup, and `<b>` alone on a line is not a sentence.
    !name.is_empty()
        && !name.starts_with('/')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The text inside `<tag>…</tag>`, the first time it appears.
pub(super) fn tagged<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let rest = &text[text.find(&open)? + open.len()..];
    Some(&rest[..rest.find(&format!("</{tag}>"))?])
}

/// A harness turn as something to read.
///
/// Claude Code records a slash command as the three tags it parsed it into —
/// `<command-name>`, `<command-message>`, `<command-args>` — and the output as
/// a fourth. Shown raw, a `/clear` fills four lines with markup and buries the
/// one token that matters. So it comes back out as the command someone typed,
/// with whatever it printed beneath it.
///
/// Anything else the harness writes keeps its text and loses its wrapper: a
/// reminder still reads as a reminder without the tag announcing it as one.
pub(super) fn tidy_harness_text(text: &str) -> String {
    if text.trim_start().starts_with("<task-notification>") {
        return tidy_task_notification(text);
    }
    let out = tagged(text, "local-command-stdout").unwrap_or("").trim();
    if let Some(name) = tagged(text, "command-name") {
        let args = tagged(text, "command-args").unwrap_or("").trim();
        let said = format!("{} {args}", name.trim());
        return match out.is_empty() {
            true => said.trim().to_string(),
            false => format!("{}\n\n{out}", said.trim()),
        };
    }
    if text.trim_start().starts_with("<local-command") {
        return out.to_string();
    }
    // Every other block is a wrapper around prose. Dropping the tag lines is
    // enough — the text between them was written to be read.
    let stripped: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !(line.trim_start().starts_with('<') && line.trim_end().ends_with('>')))
        .collect();
    let stripped = stripped.join("\n");
    let stripped = stripped.trim();
    match stripped.is_empty() {
        // A block written entirely on one line has no line to keep, so the
        // tags come off it directly rather than leaving the turn empty.
        true => strip_outer_tags(text.trim()).trim().to_string(),
        false => stripped.to_string(),
    }
}

/// `<tag>body</tag>` on a single line, reduced to `body`.
fn strip_outer_tags(text: &str) -> &str {
    let Some(open_end) = text.find('>') else {
        return text;
    };
    if !text.starts_with('<') || !text.ends_with('>') {
        return text;
    }
    let body = &text[open_end + 1..];
    match body.rfind("</") {
        Some(close) => &body[..close],
        None => body,
    }
}

/// A `<task-notification>` as news, not markup.
///
/// The notice that a background task or a monitor spoke arrives as a `user`
/// entry full of the fields a dispatcher needs — `task-id`, `tool-use-id`,
/// `output-file`, `usage`, `worktree` — around the ones a reader does:
/// `summary`, `event` when a monitor is reporting, and `result` when a
/// finished agent left a note. Every field sits on its own `<field>value
/// </field>` line, so the generic tidy strips them all, finds the body
/// empty, and strips only the outer tag — which is how the whole field list
/// reached the transcript as the turn's text. Read the fields that carry
/// the news; a notification holding none of them says nothing worth a turn.
fn tidy_task_notification(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for field in ["summary", "event", "result"] {
        let Some(body) = tagged(text, field).map(str::trim) else {
            continue;
        };
        if body.is_empty() || lines.iter().any(|l| l == body) {
            continue;
        }
        lines.push(unescape_entities(body));
    }
    // Anything written after the closing tag is the entry's real text — the
    // notice is only the part inside it.
    if let Some(end) = text.find("</task-notification>") {
        let tail = text[end + "</task-notification>".len()..].trim();
        if !tail.is_empty() {
            lines.push(tail.to_string());
        }
    }
    lines.join("\n")
}

/// The entities the transcript writer escapes inside these fields. `&amp;`
/// goes last, or `&amp;gt;` decodes twice.
fn unescape_entities(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// A Devin system event, readable.
///
/// The block arrives dressed for the model, not for a reader: an
/// `additional_metadata` opens with instructions about when to mention it,
/// then a `user_actions` carrying one `[diff_block]` per file the user
/// touched — dozens of lines of pseudo-diff where the event was "you edited
/// these files". The block and its footnote go; the file names and any other
/// action prose stay. Anything else — a subagent's completion report — loses
/// only its envelope, same as every other harness block.
pub(super) fn tidy_devin_event(text: &str) -> String {
    let Some(actions) = tagged(text, "user_actions") else {
        return tidy_harness_text(text);
    };
    let mut files = Vec::new();
    let mut prose = Vec::new();
    let mut in_diff = false;
    for line in actions.lines().map(str::trim) {
        match line {
            "[diff_block_start]" => in_diff = true,
            "[diff_block_end]" => in_diff = false,
            _ if in_diff || line.is_empty() => {}
            l if l.starts_with("Please note that") => {}
            l => match l.strip_prefix("The following changes were made by the USER to: ") {
                Some(file) => files.push(file.trim_end_matches('.').to_string()),
                None => prose.push(l.to_string()),
            },
        }
    }
    if !files.is_empty() {
        prose.insert(0, format!("the user edited {}", files.join(", ")));
    }
    match prose.is_empty() {
        true => tidy_harness_text(text),
        false => prose.join("\n"),
    }
}

/// The [`Turn::kind`] of a message another agent sent this one: a subagent's
/// hand-back, another session writing in, or a coordinator briefing a worker.
pub const AGENT_MESSAGE: &str = "agent-message";

/// The line Claude Code opens another session's message with.
const AGENT_PREAMBLE: &str = "Another Claude session sent a message";

/// Who wrote a message Claude Code filed where a person's prompt goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Author {
    /// The person at the keyboard.
    Person,
    /// Another agent: the name the transcript gives it and its id, if any.
    Agent {
        name: Option<String>,
        id: Option<String>,
    },
    /// The harness: a slash command, a hook, a task notification.
    Harness,
}

/// Who wrote a prompt-shaped message — the one decision both a `user` entry
/// and a `queued_command` attachment go through.
///
/// `origin.kind` is the record's own answer and is read first. Nothing else on
/// the record separates the cases: `renderedRole` is `system` on everything
/// queued, including what the person typed, and neither `isMeta` nor
/// `commandMode` is written on every kind. Only `human` is the person; a kind
/// this does not know is someone else, because showing a machine's words as
/// the person's is the mistake this exists to stop, and the opposite one only
/// dims a line. Transcripts older than `origin` fall back to the shape of the
/// text.
pub(super) fn author(origin: Option<&Origin>, command_mode: Option<&str>, text: &str) -> Author {
    let kind = origin.and_then(|o| o.kind.get());
    let field = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string)
    };
    let agent = || Author::Agent {
        name: field(origin.and_then(|o| o.name.get())),
        id: field(origin.and_then(|o| o.from.get())),
    };
    match kind {
        // A slash command the person typed is still recorded as the harness's
        // expansion of it.
        Some("human") if is_harness_text(text) => Author::Harness,
        Some("human") => Author::Person,
        Some("peer" | "coordinator") => agent(),
        Some(_) if is_agent_text(text) => agent(),
        Some(_) => Author::Harness,
        None if command_mode == Some("task-notification") => Author::Harness,
        None if is_agent_text(text) => Author::Agent {
            name: None,
            id: agent_message_sender(text),
        },
        None if is_harness_text(text) => Author::Harness,
        None => Author::Person,
    }
}

/// Who wrote a prompt-shaped message, without the sender's name: what the
/// context meter needs to file its characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Speaker {
    Person,
    Agent,
    Harness,
}

/// [`author`], for the parser: the same decision the conversation view makes,
/// so the context meter can never file a message under someone the reader
/// does not show as its writer.
pub(crate) fn speaker(origin: Option<&Origin>, command_mode: Option<&str>, text: &str) -> Speaker {
    match author(origin, command_mode, text) {
        Author::Person => Speaker::Person,
        Author::Agent { .. } => Speaker::Agent,
        Author::Harness => Speaker::Harness,
    }
}

/// Whether a record's `origin` says someone other than the person wrote it.
///
/// For the readers that only need to know whether to trust a record as typed —
/// the effort switch — rather than how to show it. A record with no `origin`
/// is not ruled out here; it predates the field.
pub(crate) fn from_someone_else(origin: Option<&Origin>) -> bool {
    origin
        .and_then(|o| o.kind.get())
        .is_some_and(|kind| kind != "human")
}

/// Whether text has the shape of another agent's message, for records that do
/// not say who wrote them.
fn is_agent_text(text: &str) -> bool {
    let head = text.trim_start();
    head.starts_with(AGENT_PREAMBLE) || head.starts_with("<agent-message")
}

/// The `from` attribute of an `<agent-message>` tag: the sender's id, for a
/// record whose `origin` does not carry it.
pub(super) fn agent_message_sender(text: &str) -> Option<String> {
    let open = &text[text.find("<agent-message")?..];
    let tag = &open[..open.find('>')?];
    let rest = &tag[tag.find("from=\"")? + "from=\"".len()..];
    let id = &rest[..rest.find('"')?];
    (!id.is_empty()).then(|| id.to_string())
}

/// Another agent's message as the report it carries.
///
/// The harness wraps it three times over for the model's benefit: a preamble
/// line, `<agent-message from="…">` tags, and a trailing paragraph explaining
/// what the other session is. Inside, a subagent's hand-back opens with a
/// frame ending `The report follows:` and indents the report beneath it. A
/// reader wants the report, so all of that comes off and the report is
/// de-indented; text that does not have a given layer keeps what it has.
pub(super) fn tidy_agent_message(text: &str) -> String {
    let body = match text.find("<agent-message") {
        Some(open) => {
            let rest = &text[open..];
            let rest = rest.find('>').map_or("", |end| &rest[end + 1..]);
            rest.find("</agent-message>")
                .map_or(rest, |close| &rest[..close])
        }
        None => match text.trim_start().strip_prefix(AGENT_PREAMBLE) {
            Some(rest) => rest.split_once('\n').map_or("", |(_, after)| after),
            None => text,
        },
    };
    const FRAME_END: &str = "The report follows:";
    let body = match body.trim_start().starts_with("[Subagent hand-back]") {
        true => body
            .find(FRAME_END)
            .map_or(body, |at| &body[at + FRAME_END.len()..]),
        false => body,
    };
    dedent(body).trim().to_string()
}

/// Text with the indentation every non-blank line shares taken off.
fn dedent(text: &str) -> String {
    // Whatever shares the first line with the frame is not indented with the
    // rest, so it is measured separately and only trimmed.
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    let indent = rest
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let mut out = first.trim().to_string();
    for line in rest.lines() {
        out.push('\n');
        // Only whitespace is cut: a blank line shorter than the indent is
        // cut to nothing rather than into its neighbour's text.
        let cut = line.len() - line.trim_start().len();
        out.push_str(&line[cut.min(indent)..]);
    }
    out
}

/// A queued message's `prompt`: a string ordinarily, a list of blocks when an
/// image came with it. Only the words are kept, as for a `user` entry.
pub(crate) fn prompt_text(prompt: &Shape<Block>) -> String {
    let mut text = String::new();
    match prompt {
        Shape::Text(s) => text.push_str(s),
        Shape::List(blocks) => {
            for block in blocks {
                match block {
                    Shape::Text(s) => push_text(&mut text, s),
                    Shape::Object(block) => {
                        if let Some(t) = block.text.get() {
                            push_text(&mut text, t);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The named prefixes were a list that had to be kept up to date, and was
    /// not: a `<task-notification>` reached a handoff brief quoted as the
    /// user's own words. Every injected block shares a shape.
    #[test]
    fn a_block_the_harness_injected_is_not_something_a_person_said() {
        assert!(is_harness_text(
            "<system-reminder>\nbe careful\n</system-reminder>"
        ));
        assert!(is_harness_text(
            "<task-notification>\n<task-id>abc</task-id>"
        ));
        assert!(is_harness_text("<user-prompt-submit-hook>\nhook said this"));
        assert!(is_harness_text("<command-name>/clear</command-name>"));
        // Claude Code also writes the tags the other way round, message first.
        assert!(is_harness_text(
            "<command-message>yolo</command-message>\n<command-name>/yolo</command-name>\n<command-args>status</command-args>"
        ));
        assert!(is_harness_text("Caveat: the messages below were generated"));
    }

    /// Matching on shape must not swallow a person who happens to write markup.
    #[test]
    fn a_person_who_pastes_markup_is_still_a_person() {
        assert!(!is_harness_text("<p>hello</p>"));
        assert!(!is_harness_text("why does <div> break here?"));
        assert!(!is_harness_text("<img src=\"x\">"));
        assert!(!is_harness_text("</closing>"));
        assert!(!is_harness_text("fix the parser"));
    }

    #[test]
    fn a_slash_command_expansion_is_attributed_to_the_harness() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<command-name>/clear</command-name>"}}"#,
        ]);
        assert_eq!(chat.turns[0].role, "system");
    }

    /// What the harness records for one `/loop 5m` is four tags. What it did
    /// is one line, and that is what the page has room for.
    #[test]
    fn a_slash_command_reads_as_the_command_that_was_typed() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<command-name>/loop</command-name>\n<command-message>loop</command-message>\n<command-args>5m</command-args>\n<local-command-stdout>started</local-command-stdout>"}}"#,
        ]);
        assert_eq!(chat.turns[0].text, "/loop 5m\n\nstarted");
    }

    /// `/clear` prints nothing, so its turn is the command alone rather than
    /// the command and an empty line where the output would have been.
    #[test]
    fn a_command_that_printed_nothing_is_just_the_command() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<command-name>/clear</command-name>\n<local-command-stdout></local-command-stdout>"}}"#,
        ]);
        assert_eq!(chat.turns[0].text, "/clear");
    }

    /// A reminder is prose in a wrapper. The prose survives; the wrapper does
    /// not, and neither does a turn that was nothing but wrapper.
    #[test]
    fn a_reminder_keeps_its_words_and_loses_its_tags() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<system-reminder>\nthe file changed on disk\n</system-reminder>"}}"#,
            r#"{"type":"user","timestamp":"t2","message":{"content":"<local-command-stdout></local-command-stdout>"}}"#,
        ]);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].text, "the file changed on disk");
    }

    /// The same block written on one line has no line the filter can keep, and
    /// dropping it would lose the only thing it said.
    #[test]
    fn a_one_line_reminder_survives_the_same_way() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<system-reminder>read the file first</system-reminder>"}}"#,
        ]);
        assert_eq!(chat.turns[0].text, "read the file first");
    }

    /// A task notification is fields for a dispatcher around one line of
    /// news. The generic tidy used to strip every field line, find nothing
    /// left, and keep the whole list as the turn's text.
    #[test]
    fn a_task_notification_reads_as_its_summary() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<task-notification>\n<task-id>bgbdzpbxb</task-id>\n<tool-use-id>toolu_01EK11bfRvKo9QaZMQWsMBGf</tool-use-id>\n<output-file>/tmp/tasks/bgbdzpbxb.output</output-file>\n<status>completed</status>\n<summary>Background command &quot;cargo build 2&gt;&amp;1 | tail -3&quot; completed (exit code 0)</summary>\n</task-notification>"}}"#,
        ]);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].role, "system");
        assert_eq!(
            chat.turns[0].text,
            "Background command \"cargo build 2>&1 | tail -3\" completed (exit code 0)"
        );
    }

    /// A monitor's report names its watch in `summary` and carries the news
    /// in `event`; both belong on the turn.
    #[test]
    fn a_monitor_event_keeps_the_event_it_reports() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<task-notification>\n<task-id>br7crykx8</task-id>\n<summary>Monitor event: &quot;rebuild chain&quot;</summary>\n<event>[Monitor expired after 30m with 11 events delivered.]</event>\n</task-notification>"}}"#,
        ]);
        assert_eq!(
            chat.turns[0].text,
            "Monitor event: \"rebuild chain\"\n[Monitor expired after 30m with 11 events delivered.]"
        );
    }

    /// A finished agent can leave a `result` note under its summary.
    #[test]
    fn a_task_notification_keeps_an_agents_parting_note() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<task-notification>\n<task-id>ad13e524</task-id>\n<status>completed</status>\n<summary>Agent &quot;bench linkage&quot; finished</summary>\n<result>the build query is running in the background</result>\n<usage><subagent_tokens>83901</subagent_tokens></usage>\n</task-notification>"}}"#,
        ]);
        assert_eq!(
            chat.turns[0].text,
            "Agent \"bench linkage\" finished\nthe build query is running in the background"
        );
    }

    /// The shapes Claude Code writes for a message that is not the person's,
    /// with dummy text. A peer `user` entry: a subagent handing back.
    const PEER_USER: &str = r#"{"type":"user","timestamp":"t1","isMeta":true,"promptSource":"system","turnOrigin":"peer","origin":{"kind":"peer","from":"a0000000000000001","name":"general-purpose","handback":true,"body":"x"},"message":{"role":"user","content":"Another Claude session sent a message:\n<agent-message from=\"a0000000000000001\">\n[Subagent hand-back] A subagent you launched has finished. The report follows:\n  dummy report\n    with an indented line\n</agent-message>\n\nThat \"other Claude session\" is a dummy trailer."}}"#;

    /// A subagent's report arriving as a `user` entry is not the person, and
    /// reads as the report: no preamble, no tags, no frame, no trailer.
    #[test]
    fn a_peer_message_is_an_agent_message_without_its_envelope() {
        let chat = sink_claude(&[PEER_USER]);
        assert_eq!(chat.turns.len(), 1);
        let turn = &chat.turns[0];
        assert_eq!(
            (turn.role.as_ref(), turn.kind.as_ref()),
            ("system", AGENT_MESSAGE)
        );
        assert_eq!(turn.text, "dummy report\n  with an indented line");
        assert_eq!(turn.from.as_deref(), Some("general-purpose"));
        assert_eq!(turn.agent.as_deref(), Some("a0000000000000001"));
    }

    /// A transcript older than `origin` is told by its shape, and the sender's
    /// id comes off the tag.
    #[test]
    fn an_agent_message_without_an_origin_is_told_by_its_shape() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"Another Claude session sent a message:\n<agent-message from=\"a0000000000000003\">\nplain dummy note\n</agent-message>"}}"#,
        ]);
        assert_eq!(chat.turns[0].kind, AGENT_MESSAGE);
        assert_eq!(chat.turns[0].text, "plain dummy note");
        assert_eq!(chat.turns[0].agent.as_deref(), Some("a0000000000000003"));
        assert_eq!(chat.turns[0].from, None);
    }

    /// What the person types while the agent works is recorded only as a
    /// queued attachment. It was dropped, so the conversation lost it.
    #[test]
    fn a_message_typed_mid_turn_is_the_persons() {
        let chat = sink_claude(&[
            r#"{"type":"attachment","timestamp":"t1","renderedRole":"system","attachment":{"type":"queued_command","commandMode":"prompt","humanTurn":true,"origin":{"kind":"human"},"prompt":"dummy typed text"}}"#,
        ]);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].role, "user");
        assert_eq!(chat.turns[0].kind, "message");
        assert_eq!(chat.turns[0].ts, "t1");
        assert_eq!(chat.turns[0].text, "dummy typed text");
    }

    /// An image typed with the message makes `prompt` a list of blocks; the
    /// words are kept as for a `user` entry.
    #[test]
    fn a_queued_message_with_blocks_keeps_its_words() {
        let chat = sink_claude(&[
            r#"{"type":"attachment","timestamp":"t1","attachment":{"type":"queued_command","origin":{"kind":"human"},"prompt":[{"type":"text","text":"look at this"},{"type":"image","source":{"type":"base64","data":"AAAA"}},{"type":"text","text":"and this"}]}}"#,
        ]);
        assert_eq!(chat.turns[0].role, "user");
        assert_eq!(chat.turns[0].text, "look at this\n\nand this");
    }

    /// A peer or a coordinator writing in mid-turn is another agent, however
    /// the attachment's other fields read.
    #[test]
    fn a_queued_peer_or_coordinator_message_is_an_agent_message() {
        let chat = sink_claude(&[
            r#"{"type":"attachment","timestamp":"t1","renderedRole":"system","attachment":{"type":"queued_command","commandMode":"prompt","isMeta":true,"origin":{"kind":"peer","from":"a0000000000000002","name":"Plan","handback":true,"body":"x"},"prompt":"<agent-message from=\"a0000000000000002\">\n[Subagent hand-back] frame. The report follows:\n  dummy plan\n</agent-message>"}}"#,
            r#"{"type":"attachment","timestamp":"t2","attachment":{"type":"queued_command","isMeta":true,"origin":{"kind":"coordinator"},"prompt":"dummy instruction from the orchestrator"}}"#,
        ]);
        let shown: Vec<(&str, &str, Option<&str>)> = chat
            .turns
            .iter()
            .map(|t| (t.kind.as_ref(), t.text.as_str(), t.from.as_deref()))
            .collect();
        assert_eq!(
            shown,
            [
                (AGENT_MESSAGE, "dummy plan", Some("Plan")),
                (
                    AGENT_MESSAGE,
                    "dummy instruction from the orchestrator",
                    None
                ),
            ]
        );
        assert!(chat.turns.iter().all(|t| t.role == "system"));
    }

    /// A queued task notification is the harness's, with an `origin` or
    /// without, and in the plain-text form as well as the tagged one.
    #[test]
    fn a_queued_task_notification_is_the_harness() {
        let chat = sink_claude(&[
            r#"{"type":"attachment","timestamp":"t1","attachment":{"type":"queued_command","commandMode":"task-notification","origin":{"kind":"task-notification","producer":"session-task"},"prompt":"<task-notification>\n<task-id>x</task-id>\n<summary>dummy finished</summary>\n</task-notification>"}}"#,
            r#"{"type":"attachment","timestamp":"t2","attachment":{"type":"queued_command","commandMode":"task-notification","prompt":"<task-notification>\n<task-id>y</task-id>\n<summary>dummy older</summary>\n</task-notification>"}}"#,
            r#"{"type":"attachment","timestamp":"t3","attachment":{"type":"queued_command","commandMode":"task-notification","prompt":"Background agent \"dummy\" was stopped by the user."}}"#,
            r#"{"type":"user","timestamp":"t4","promptSource":"system","turnOrigin":"task_notification","origin":{"kind":"task-notification"},"message":{"role":"user","content":"Background agent \"dummy\" was stopped by the user."}}"#,
        ]);
        let shown: Vec<(&str, &str)> = chat
            .turns
            .iter()
            .map(|t| (t.role.as_ref(), t.text.as_str()))
            .collect();
        assert_eq!(
            shown,
            [
                ("system", "dummy finished"),
                ("system", "dummy older"),
                (
                    "system",
                    "Background agent \"dummy\" was stopped by the user."
                ),
                (
                    "system",
                    "Background agent \"dummy\" was stopped by the user."
                ),
            ]
        );
        assert!(chat.turns.iter().all(|t| t.kind == "message"));
    }

    /// An `origin` this does not know is not the person: the safe mistake is
    /// dimming a line, not putting words in someone's mouth.
    #[test]
    fn an_unknown_origin_is_never_the_person() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","origin":{"kind":"something-new"},"message":{"content":"dummy machine text"}}"#,
        ]);
        assert_eq!(chat.turns[0].role, "system");
    }
}
