//! The per-harness readers: [`Sink`] takes a transcript's records one at a
//! time, in whichever harness's shape they were written, and keeps the turns
//! the page will show.

use super::*;

/// Turns as they are read, with the oldest dropped once there are too many.
///
/// A tool result arrives in a later entry than the call it belongs to, so the
/// call has to stay reachable by id. Keeping the window as a deque and the index
/// in *sequence* numbers rather than positions is what makes that survive the
/// dropping: an id whose turn has already fallen off the front resolves to
/// nothing and its result is discarded, instead of landing on whichever turn
/// happens to sit at that position now.
#[derive(Default)]
pub(super) struct Sink {
    pub(super) turns: VecDeque<Turn>,
    /// Sequence number of the turn at the front of `turns`.
    pub(super) first: usize,
    /// Sequence number the next turn will get.
    pub(super) next: usize,
    /// When set, the window stops at this sequence number: turns at or past it
    /// still count (`next` keeps moving, so a later window's `earlier` is
    /// unchanged) but are not kept. Their records are still read — a tool
    /// result can arrive entries after the boundary and still needs to land on
    /// a call inside the window.
    pub(super) before: Option<usize>,
    /// `tool_use` id -> (turn sequence, index within that turn's tools).
    pub(super) index: HashMap<String, (usize, usize)>,
    /// Codex repeats an entry when a turn is retried; the second copy of a
    /// `call_id` is the same call, not another one.
    pub(super) seen_calls: std::collections::HashSet<String>,
    /// The assistant turn still being added to, if there is one.
    ///
    /// Both harnesses write one reply as several records — the text, then each
    /// call — so a turn per record makes one answer into four boxes, three of
    /// them holding nothing but a tool name. Merging every consecutive record
    /// instead collapses a whole session into two boxes with sixty calls each.
    /// A tool result is the seam: it means the model has been asked again, and
    /// what it says next is a new turn. This is the fallback rule, used where a
    /// harness gives nothing better.
    pub(super) run: Option<usize>,
    /// The API request the open turn belongs to, where the transcript says.
    ///
    /// Claude stamps every record of one response with the same `requestId`,
    /// which is the exact answer the rule above approximates: an entry carrying
    /// a request id already seen is part of that reply, however many thinking
    /// blocks and parallel tool calls it was written as.
    pub(super) run_request: Option<(String, usize)>,
    pub(super) limits: Limits,
    /// Read sidechain records rather than skip them: set when the file being
    /// read is a subagent's own, where every record is one.
    pub(super) sidechains: bool,
}

impl Sink {
    fn push(&mut self, turn: Turn) -> usize {
        // Anything pushed directly ends the run: a user turn, a compaction, a
        // block of reasoning. Only `open_assistant` reopens one.
        self.run = None;
        let seq = self.next;
        self.next += 1;
        // Past the `before` boundary the turn is numbered but not kept: its
        // sequence has to exist so `first` still counts it, but the window a
        // paged-back request is answering ends before it.
        if self.before.is_none_or(|before| seq < before) {
            let mut turn = turn;
            turn.seq = seq;
            self.turns.push_back(turn);
            while self.turns.len() > self.limits.turns {
                self.turns.pop_front();
                self.first += 1;
            }
        }
        seq
    }

    /// The assistant turn more of one reply belongs to, opening a new one when
    /// this record starts a different reply.
    ///
    /// `request` is the harness's own name for the response this record came
    /// from, where it has one. With it the grouping is exact; without it, the
    /// run rule in [`Sink::run`] stands in.
    fn open_assistant(&mut self, ts: &str, request: Option<&str>) -> usize {
        let roomy = |sink: &mut Sink, seq: usize| {
            let cap = sink.limits.tools;
            sink.turn_mut(seq)
                .is_some_and(|turn| turn.tools.len() < cap)
        };
        if let Some(id) = request {
            if let Some((open, seq)) = self.run_request.clone()
                && open == id
                && roomy(self, seq)
            {
                self.run = Some(seq);
                return seq;
            }
        } else if let Some(seq) = self.run
            && roomy(self, seq)
        {
            return seq;
        }
        let seq = self.push(Turn::new("assistant", "message", ts));
        self.run = Some(seq);
        if let Some(id) = request {
            self.run_request = Some((id.to_string(), seq));
        }
        seq
    }

    fn turn_mut(&mut self, seq: usize) -> Option<&mut Turn> {
        let position = seq.checked_sub(self.first)?;
        self.turns.get_mut(position)
    }

    fn add_tool(&mut self, seq: usize, id: Option<&str>, tool: ToolUse) {
        let cap = self.limits.tools;
        let Some(turn) = self.turn_mut(seq) else {
            return;
        };
        if turn.tools.len() >= cap {
            return;
        }
        let at = turn.tools.len();
        turn.tools.push(tool);
        if let Some(id) = id {
            self.index.insert(id.to_string(), (seq, at));
        }
    }

    /// Attach a result to the call it came back from, if that call is still in
    /// the window.
    fn resolve(&mut self, id: &str, result: Option<String>, failed: bool, delta: Option<Delta>) {
        // Whatever the model says after this is a new reply, whether or not the
        // call it answers is still in the window.
        self.run = None;
        let Some((seq, at)) = self.index.remove(id) else {
            return;
        };
        let Some(tool) = self.turn_mut(seq).and_then(|t| t.tools.get_mut(at)) else {
            return;
        };
        // A result is recorded even when it is empty, because the presence of
        // one is what distinguishes a finished call from a running one.
        tool.result = Some(result.unwrap_or_default());
        tool.failed = failed;
        if let Some(delta) = delta {
            tool.added = delta.added;
            tool.removed = delta.removed;
            tool.diff = delta.hunks.into_iter().take(MAX_DIFF_LINES).collect();
        }
    }

    pub(super) fn finish(self) -> Conversation {
        Conversation {
            supported: true,
            earlier: self.first,
            turns: self
                .turns
                .into_iter()
                .filter(|turn| !turn.is_empty())
                .collect(),
            note: None,
            stamp: None,
            unchanged: false,
        }
    }

    // --- Claude Code ---

    pub(super) fn claude(&mut self, item: &Line) {
        // A subagent's turns are a different conversation that happens to share
        // a file. See the module docs.
        if !self.sidechains && item.sidechain() {
            return;
        }
        let ts = item.timestamp.or_empty();
        match item.kind.get() {
            Some("user") => self.claude_user(item, ts),
            Some("assistant") => self.claude_assistant(item, ts),
            Some("attachment") => self.claude_queued(item, ts),
            _ => {}
        }
    }

    /// Something that arrived while the agent was busy.
    ///
    /// Claude Code writes a message that lands mid-turn as a `queued_command`
    /// attachment rather than a `user` entry — whoever wrote it. A person
    /// typing while the agent works is recorded only this way, so ignoring
    /// attachments lost every word said mid-turn; and a subagent's report or
    /// a task notification arrives the same way, so the record says nothing
    /// about who spoke until its `origin` is read.
    fn claude_queued(&mut self, item: &Line, ts: &str) {
        let Some(queued) = item.attachment().filter(|a| a.kind.is("queued_command")) else {
            return;
        };
        let text = prompt_text(&queued.prompt);
        if text.trim().is_empty() {
            return;
        }
        let by = author(queued.origin.get(), queued.command_mode.get(), &text);
        self.claude_said(by, false, &text, ts);
    }

    fn claude_user(&mut self, item: &Line, ts: &str) {
        // The patch an edit applied is recorded on the entry carrying its
        // result, not on the call, so it is read once here and handed to
        // whichever `tool_result` block claims it.
        let mut delta = claude_delta(item);
        let mut text = String::new();
        let mut had_result = false;

        match item.content() {
            Shape::Text(s) => text.push_str(s),
            Shape::List(blocks) => {
                for block in blocks {
                    let block = match block {
                        Shape::Text(s) => {
                            push_text(&mut text, s);
                            continue;
                        }
                        Shape::Object(block) => block,
                        _ => continue,
                    };
                    match block.kind.get() {
                        Some("tool_result") => {
                            had_result = true;
                            let Some(id) = block.tool_use_id.get() else {
                                continue;
                            };
                            let failed = block.is_error.value() == Some(true);
                            let body = flatten_raw(block.content);
                            self.resolve(id, Some(body), failed, delta.take());
                        }
                        _ => {
                            if let Some(t) = block.text.get() {
                                push_text(&mut text, t);
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        if text.trim().is_empty() {
            return;
        }
        // A tool result and a typed message can share one entry when someone
        // types while a tool is running. The result has already been attached;
        // what is left is a person talking — unless the entry is the harness's
        // own (`isMeta`), as a hook's output is.
        let compaction = item.compact_summary();
        let by = match had_result && item.meta() {
            true => Author::Harness,
            false => author(item.origin.get(), None, &text),
        };
        self.claude_said(by, compaction, &text, ts);
    }

    /// A message filed where a prompt goes, as the turn its author makes it.
    fn claude_said(&mut self, by: Author, compaction: bool, text: &str, ts: &str) {
        let text_in = text;
        let mut sender = (None, None);
        let (role, kind, text) = match (compaction, by) {
            (true, _) => ("system", "compaction", text.to_string()),
            (false, Author::Person) => ("user", "message", text.to_string()),
            // The harness writes for a parser, not for a reader. What it says
            // is worth keeping; the tags around it are not, and a turn that is
            // only tags says nothing at all.
            (false, Author::Harness) => ("system", "message", tidy_harness_text(text)),
            (false, Author::Agent { name, id }) => {
                sender = (name, id.or_else(|| agent_message_sender(text)));
                ("system", AGENT_MESSAGE, tidy_agent_message(text))
            }
        };
        if text.trim().is_empty() {
            return;
        }
        if kind == "message" && role == "system" {
            // A task notification about a subagent names it by its task id; kept
            // so the join can tie the notice to the agent, and dropped there if
            // it names no agent.
            sender.1 = tagged(text_in, "task-id").map(|id| id.trim().to_string());
        }
        let mut turn = Turn::new(role, kind, ts);
        (turn.from, turn.agent) = sender;
        turn.set_text(&text, self.limits.text);
        self.push(turn);
    }

    fn claude_assistant(&mut self, item: &Line, ts: &str) {
        let request = item.request_id.get();
        let Some(blocks) = item.content().list() else {
            return;
        };

        let mut text = String::new();
        let mut thinking = String::new();
        let mut calls: Vec<(Option<String>, ToolUse)> = Vec::new();
        for block in blocks.iter().filter_map(Shape::object) {
            match block.kind.get() {
                Some("text") => {
                    if let Some(t) = block.text.get() {
                        push_text(&mut text, t);
                    }
                }
                Some("thinking") => {
                    if let Some(t) = block.thinking.get() {
                        push_text(&mut thinking, t);
                    }
                }
                Some("tool_use") => {
                    let name = block.name.get().unwrap_or("tool");
                    let (short, full) = extract::tool_detail(name, &block.input.value());
                    calls.push((
                        block.id.get().map(str::to_string),
                        ToolUse {
                            name: util::pretty_mcp_name(name),
                            detail: short,
                            full,
                            id: block.id.or_empty().to_string(),
                            ..ToolUse::default()
                        },
                    ));
                }
                _ => {}
            }
        }

        // Thinking is its own turn rather than a prefix of the reply: it is
        // shown differently, and folding it into the text would mean either
        // hiding what the agent said or leading with all of its reasoning.
        //
        // Pushing it does not end the reply it belongs to — `run_request` is
        // what reopens that — which matters because a harness with extended
        // thinking writes a thinking block into most records, and closing the
        // reply on each one puts every tool call in a box of its own.
        if !thinking.trim().is_empty() {
            let mut turn = Turn::new("assistant", "reasoning", ts);
            turn.set_text(&thinking, self.limits.text);
            self.push(turn);
        }

        if text.trim().is_empty() && calls.is_empty() {
            return;
        }
        // One reply, however many entries it took. Claude writes a turn's text
        // and each of its tool calls as separate records, so a turn shown per
        // record is one answer split across four boxes with three of them
        // holding nothing but a tool name. Everything between two user turns is
        // one thing the agent said, which is how its own interface reads it.
        let seq = self.open_assistant(ts, request);
        let cap = self.limits.text;
        if let Some(turn) = self.turn_mut(seq) {
            turn.append_text(&text, cap);
        }
        for (id, tool) in calls {
            self.add_tool(seq, id.as_deref(), tool);
        }
    }

    // --- Codex ---

    pub(super) fn codex(&mut self, item: &codex_line::Line) {
        let ts = item.timestamp.or_empty();
        let Some(payload) = item.payload.get() else {
            return;
        };
        // A rollout writes the same shapes either at the top level or wrapped
        // in a `response_item`, exactly as the extraction path finds them.
        match item.effective_kind() {
            "message" => self.codex_message(payload, ts),
            "reasoning" => {
                let text = codex_summary(payload);
                if !text.trim().is_empty() {
                    let mut turn = Turn::new("assistant", "reasoning", ts);
                    turn.set_text(&text, self.limits.text);
                    self.push(turn);
                }
            }
            "function_call" | "custom_tool_call" => self.codex_call(payload, ts),
            "function_call_output" | "custom_tool_call_output" => {
                let Some(id) = payload.call_id.get() else {
                    return;
                };
                let output = payload.output.to_value();
                let failed = codex_output_failed(output.as_ref());
                self.resolve(id, Some(flatten_content(output.as_ref())), failed, None);
            }
            _ => {}
        }
    }

    fn codex_message(&mut self, payload: &codex_line::Payload, ts: &str) {
        let role = match payload.role.get() {
            Some("user") => "user",
            Some("assistant") => "assistant",
            // `system` and `developer` are both the harness talking: the
            // instructions, the environment block, the wrapper around a slash
            // command.
            _ => "system",
        };
        let text = codex_text(payload);
        if text.trim().is_empty() {
            return;
        }
        let mut turn = Turn::new(role, "message", ts);
        turn.set_text(&text, self.limits.text);
        let seq = self.push(turn);
        // The calls this reply makes are written as their own entries after it,
        // so the reply stays open for them until a result comes back.
        if role == "assistant" {
            self.run = Some(seq);
        }
    }

    fn codex_call(&mut self, payload: &codex_line::Payload, ts: &str) {
        if let Some(id) = payload.call_id.get()
            && !self.seen_calls.insert(id.to_string())
        {
            return;
        }
        let name = payload.name.get().unwrap_or("tool");
        // `arguments` is a JSON-encoded string on a `function_call` and `input`
        // on a `custom_tool_call`, and `apply_patch` sends a raw patch through
        // either — so the argument is parsed if it parses and shown verbatim if
        // it does not, which is what the extraction path does with the same
        // entries.
        let (raw, args) = payload.call_args();
        let raw = raw.as_ref().and_then(Str::get);

        let mut tool = ToolUse {
            name: util::pretty_mcp_name(name),
            ..ToolUse::default()
        };
        if let Some(patch) = raw.filter(|_| name == "apply_patch" || args.is_null()) {
            match name {
                "apply_patch" => {
                    let (summary, delta) = extract::parse_apply_patch(patch);
                    tool.detail = summary;
                    tool.full = Some(patch.to_string());
                    tool.added = delta.added;
                    tool.removed = delta.removed;
                    tool.diff = delta.hunks.into_iter().take(MAX_DIFF_LINES).collect();
                }
                _ => {
                    tool.detail = extract::flatten_public(patch, 300);
                    tool.full = Some(patch.to_string());
                }
            }
        } else {
            let (short, full) = extract::tool_detail(name, &args);
            tool.detail = short;
            tool.full = full;
        }

        let seq = self.open_assistant(ts, None);
        self.add_tool(seq, payload.call_id.get(), tool);
    }

    // --- OpenCode ---

    /// One OpenCode message: what it said, and what it did while saying it.
    ///
    /// A message is already a whole reply. Its reasoning, its words and every
    /// call it made are parts of this one row, so there is no run to carry
    /// across entries as there is for the harnesses that write a reply in
    /// pieces, and no result waiting on a later one — which is also why two
    /// messages in a row are two replies rather than one spread over two.
    pub(super) fn opencode(&mut self, message: &Value, ts: &str, parts: &[Value]) {
        let mut text = String::new();
        let mut thinking = String::new();
        let mut calls: Vec<ToolUse> = Vec::new();
        let mut compacted = false;

        for part in parts {
            match part.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if opencode_is_played_back(part) {
                        continue;
                    }
                    if let Some(t) = part.get("text").and_then(Value::as_str) {
                        push_text(&mut text, t);
                    }
                }
                Some("reasoning") => {
                    if let Some(t) = part.get("text").and_then(Value::as_str) {
                        push_text(&mut thinking, t);
                    }
                }
                Some("tool") => calls.push(opencode_call(part)),
                Some("compaction") => compacted = true,
                // `step-start` and `step-finish` bracket a model call and carry
                // its cost; `patch` is the file state a call left behind, which
                // the call itself already reports; `file` is an attachment.
                _ => {}
            }
        }

        if compacted {
            // The messages before a compaction are deleted rather than scrolled
            // past, so the transcript jumps with nothing to mark it. This is the
            // only thing in it that says the conversation went on.
            let mut turn = Turn::new("system", "compaction", ts);
            turn.set_text(
                "the context was compacted here — every turn before it \
                           is no longer in this session's transcript",
                self.limits.text,
            );
            self.push(turn);
        }
        if !thinking.trim().is_empty() {
            let mut turn = Turn::new("assistant", "reasoning", ts);
            turn.set_text(&thinking, self.limits.text);
            self.push(turn);
        }
        if text.trim().is_empty() && calls.is_empty() {
            return;
        }
        let role = match message.get("role").and_then(Value::as_str) {
            Some("assistant") => "assistant",
            Some("user") => "user",
            _ => "system",
        };
        let mut turn = Turn::new(role, "message", ts);
        turn.set_text(&text, self.limits.text);
        let seq = self.push(turn);
        // No call id: the result is on the same part as the call, so nothing
        // later can resolve it and an index entry would only be a leak.
        for call in calls {
            self.add_tool(seq, None, call);
        }
    }

    // --- Devin ---

    /// One ATIF step: the source says who is speaking, and an agent step is a
    /// whole model response — reasoning, reply text, tool calls, and the
    /// results they produced, which Devin records on the same step's
    /// `observation` rather than as the later entry Claude and Codex use.
    pub(super) fn devin(&mut self, step: &Value, statuses: &HashMap<String, String>) {
        let ts = step.get("timestamp").and_then(Value::as_str).unwrap_or("");
        match step.get("source").and_then(Value::as_str) {
            Some("user") => {
                let Some(text) = step.get("message").and_then(Value::as_str) else {
                    return;
                };
                if text.trim().is_empty() {
                    return;
                }
                let mut turn = Turn::new("user", "message", ts);
                turn.set_text(text, self.limits.text);
                self.push(turn);
            }
            Some("agent") => self.devin_agent(step, ts, statuses),
            Some("system") => {
                // Most `system` steps are the prompt being assembled — the
                // `sysprompt` and `rules` telemetry sources say so directly, and
                // the rest are context blocks re-injected at each turn:
                // `<available_skills>`, `<system_info>`, loose chunks of the
                // prompt. What is worth a turn is an *event* — a tagged block
                // reporting that something happened, like a backgrounded
                // subagent finishing or the user editing a file mid-run.
                match step
                    .pointer("/extra/telemetry/source")
                    .and_then(Value::as_str)
                {
                    Some("sysprompt") | Some("rules") => return,
                    _ => {}
                }
                let Some(text) = step.get("message").and_then(Value::as_str) else {
                    return;
                };
                match text
                    .trim_start()
                    .strip_prefix('<')
                    .and_then(|r| r.split(['>', ' ', '\n', '\t', '/']).next())
                {
                    Some(tag)
                        if !tag.is_empty()
                            && tag
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                            && !matches!(tag, "available_skills" | "system_info") => {}
                    _ => return,
                }
                let mut turn = Turn::new("system", "message", ts);
                turn.set_text(&tidy_devin_event(text), self.limits.text);
                self.push(turn);
            }
            _ => {}
        }
    }

    fn devin_agent(&mut self, step: &Value, ts: &str, statuses: &HashMap<String, String>) {
        if let Some(thinking) = step.get("reasoning_content").and_then(Value::as_str)
            && !thinking.trim().is_empty()
        {
            let mut turn = Turn::new("assistant", "reasoning", ts);
            turn.set_text(thinking, self.limits.text);
            self.push(turn);
        }

        let text = step.get("message").and_then(Value::as_str).unwrap_or("");
        let empty: Vec<Value> = Vec::new();
        let calls = step
            .get("tool_calls")
            .and_then(Value::as_array)
            .unwrap_or(&empty);
        if text.trim().is_empty() && calls.is_empty() {
            return;
        }
        // `step_id` names the response: every step gets a fresh id, so one
        // step is one turn and nothing is ever folded into the reply before.
        // It is a number in ATIF, not a string.
        let request = step.get("step_id").map(|id| match id {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });
        let seq = self.open_assistant(ts, request.as_deref());
        let cap = self.limits.text;
        if let Some(turn) = self.turn_mut(seq) {
            turn.append_text(text, cap);
        }
        for call in calls {
            let name = call
                .get("function_name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let args = call.get("arguments").cloned().unwrap_or(Value::Null);
            let (short, full) = extract::tool_detail(name, &args);
            let mut tool = ToolUse {
                name: util::pretty_mcp_name(name),
                detail: short,
                full,
                ..ToolUse::default()
            };
            if matches!(name, "edit" | "write")
                && let Some(delta) = extract::edit_delta(&args)
            {
                tool.added = delta.added;
                tool.removed = delta.removed;
                tool.diff = delta.hunks.into_iter().take(MAX_DIFF_LINES).collect();
            }
            let id = call.get("tool_call_id").and_then(Value::as_str);
            self.add_tool(seq, id, tool);
        }
        for result in step
            .get("observation")
            .and_then(|o| o.get("results"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = result.get("source_call_id").and_then(Value::as_str) else {
                continue;
            };
            let body = flatten_content(result.get("content"));
            let failed = statuses.get(id).is_some_and(|s| s != "completed");
            self.resolve(id, Some(body), failed, None);
        }
    }

    // --- Cursor ---

    /// One Cursor entry: the role says who is speaking, and the blocks beneath
    /// it are what they said and what they asked the agent to do.
    ///
    /// A Cursor transcript dates nothing and pairs nothing, so a turn here has
    /// no time and a call has nothing to resolve — which is why each call is
    /// given an empty result rather than none. A missing one would say the call
    /// is still running, and for a harness that keeps no outcomes at all that
    /// would be a claim the transcript cannot support.
    pub(super) fn cursor(&mut self, item: &Value) {
        let Some(role) = item.get("role").and_then(Value::as_str) else {
            return;
        };
        let Some(blocks) = item
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        else {
            return;
        };
        let mut text = String::new();
        let mut calls = Vec::new();
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(t) = block.get("text").and_then(Value::as_str) {
                        push_text(&mut text, t);
                    }
                }
                Some("tool_use") => {
                    let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                    let args = block.get("input").cloned().unwrap_or(Value::Null);
                    calls.push(call_tool(name, &args, Outcome::NotRecorded));
                }
                _ => {}
            }
        }
        // Cursor wraps what a person typed in a `<user_query>` block, and hangs
        // attachments off the same message as their own tag.
        let tidy = tidy_harness_text(&text);
        if tidy.trim().is_empty() && calls.is_empty() {
            return;
        }
        let mut turn = Turn::new(
            match role {
                "assistant" => "assistant",
                "user" => "user",
                _ => "system",
            },
            "message",
            "",
        );
        turn.set_text(&tidy, self.limits.text);
        let seq = self.push(turn);
        for call in calls {
            self.add_tool(seq, None, call);
        }
    }

    // --- Gemini ---

    /// One Gemini record: the header and the `$set` patches that revise it are
    /// not conversation, so a record without one of the three types a person
    /// reads as nothing.
    pub(super) fn gemini(&mut self, item: &Value) {
        let ts = item
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        match item.get("type").and_then(Value::as_str) {
            Some("user") => {
                let mut turn = Turn::new("user", "message", &ts);
                turn.set_text(&gemini_text(item.get("content")), self.limits.text);
                self.push(turn);
            }
            Some("info") => {
                // "Switched to Plan Mode." and the like: the harness speaking
                // for itself, which is what a `system` turn is.
                let text = item.get("content").and_then(Value::as_str).unwrap_or("");
                if text.trim().is_empty() {
                    return;
                }
                let mut turn = Turn::new("system", "message", &ts);
                turn.set_text(&tidy_harness_text(text), self.limits.text);
                self.push(turn);
            }
            Some("gemini") => self.gemini_reply(item, &ts),
            _ => {}
        }
    }

    /// One Gemini reply: what it thought, what it said, and every call it made
    /// in between.
    ///
    /// A `gemini` record is a whole response, and it carries each call's
    /// outcome on the call itself — the same one-record-one-reply shape
    /// OpenCode uses, and for the same reason there is no run to carry.
    fn gemini_reply(&mut self, item: &Value, ts: &str) {
        let mut thinking = String::new();
        for thought in item
            .get("thoughts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let body = thought
                .get("description")
                .and_then(Value::as_str)
                .or_else(|| thought.get("subject").and_then(Value::as_str))
                .unwrap_or("");
            push_text(&mut thinking, body);
        }
        if !thinking.trim().is_empty() {
            let mut turn = Turn::new("assistant", "reasoning", ts);
            turn.set_text(&thinking, self.limits.text);
            self.push(turn);
        }

        let text = item.get("content").and_then(Value::as_str).unwrap_or("");
        let empty: Vec<Value> = Vec::new();
        let calls = item
            .get("toolCalls")
            .and_then(Value::as_array)
            .unwrap_or(&empty);
        if text.trim().is_empty() && calls.is_empty() {
            return;
        }
        let mut turn = Turn::new("assistant", "message", ts);
        turn.set_text(&tidy_harness_text(text), self.limits.text);
        let seq = self.push(turn);
        for call in calls {
            let name = call.get("name").and_then(Value::as_str).unwrap_or("tool");
            let args = call.get("args").cloned().unwrap_or(Value::Null);
            // A Gemini call records what came back on the call itself, so a
            // missing result is a call still running rather than one whose
            // outcome the harness threw away.
            let outcome = match call.get("result") {
                Some(result) => Outcome::Recorded(
                    gemini_result(result),
                    call.get("status").and_then(Value::as_str) == Some("error"),
                ),
                None => Outcome::Running,
            };
            self.add_tool(seq, None, call_tool(name, &args, outcome));
        }
    }

    // --- Pi ---

    /// One Pi entry, of which only a `message` is conversation: the rest of the
    /// file is the harness narrating itself — labels, model changes, branch
    /// summaries, compactions.
    pub(super) fn pi(&mut self, item: &Value) {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            return;
        }
        let Some(message) = item.get("message") else {
            return;
        };
        let ts = pi::message_ts(item, message);
        match message.get("role").and_then(Value::as_str) {
            Some("toolResult") => {
                let Some(id) = message.get("toolCallId").and_then(Value::as_str) else {
                    return;
                };
                let body = flatten_content(message.get("content"));
                let failed = message.get("isError").and_then(Value::as_bool) == Some(true);
                self.resolve(id, Some(body), failed, None);
            }
            Some(role @ ("user" | "assistant")) => {
                let mut text = String::new();
                let mut thinking = String::new();
                let mut calls = Vec::new();
                match message.get("content") {
                    // A person's turn is the string; an agent's is blocks.
                    Some(Value::String(s)) => push_text(&mut text, s),
                    Some(Value::Array(blocks)) => {
                        for block in blocks {
                            match block.get("type").and_then(Value::as_str) {
                                Some("text") => {
                                    if let Some(t) = block.get("text").and_then(Value::as_str) {
                                        push_text(&mut text, t);
                                    }
                                }
                                Some("thinking") => {
                                    if let Some(t) = block.get("thinking").and_then(Value::as_str) {
                                        push_text(&mut thinking, t);
                                    }
                                }
                                Some("toolCall") => {
                                    let name =
                                        block.get("name").and_then(Value::as_str).unwrap_or("tool");
                                    let args =
                                        block.get("arguments").cloned().unwrap_or(Value::Null);
                                    // The result is a later entry keyed by
                                    // `toolCallId`, so the call starts in flight
                                    // and `resolve` is what fills it in.
                                    let call = call_tool(name, &args, Outcome::Running);
                                    calls.push((id_of(block), call));
                                }
                                // An image is what a person pasted; the page
                                // shows the words, not the bytes.
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                if !thinking.trim().is_empty() {
                    let mut turn = Turn::new("assistant", "reasoning", &ts);
                    turn.set_text(&thinking, self.limits.text);
                    self.push(turn);
                }
                if text.trim().is_empty() && calls.is_empty() {
                    return;
                }
                let mut turn = Turn::new(
                    if role == "user" { "user" } else { "assistant" },
                    "message",
                    &ts,
                );
                turn.set_text(&tidy_harness_text(&text), self.limits.text);
                let seq = self.push(turn);
                for (id, call) in calls {
                    self.add_tool(seq, id.as_deref(), call);
                }
            }
            _ => {}
        }
    }

    // --- Vibe ---

    /// One Vibe message, in either of its layouts' shapes.
    ///
    /// The unified store keeps `content` as typed parts — `text`, `reasoning`
    /// (whose own `content` is text parts), `tool_call` with its `arguments`
    /// already parsed under `value` — and says how a call ended in the tool
    /// message's `outcome`. The Python CLI wrote OpenAI's chat shape: string
    /// content, `reasoning_content` beside it, `tool_calls` whose arguments are
    /// a JSON document in a string, and no outcome at all. Neither stamps a
    /// time on a message.
    pub(super) fn vibe(&mut self, message: &Value) {
        let id_of = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
        match message.get("role").and_then(Value::as_str) {
            Some("tool") => {
                let Some(id) = id_of(message, "tool_call_id") else {
                    return;
                };
                let body = flatten_content(message.get("content"));
                let failed = message.get("outcome").and_then(Value::as_str) == Some("failure");
                self.resolve(&id, Some(body), failed, None);
            }
            Some(role @ ("user" | "assistant")) => {
                if message.get("injected").and_then(Value::as_bool) == Some(true) {
                    return;
                }
                let mut text = String::new();
                let mut thinking = String::new();
                let mut calls = Vec::new();
                if let Some(t) = message.get("reasoning_content").and_then(Value::as_str) {
                    push_text(&mut thinking, t);
                }
                match message.get("content") {
                    Some(Value::String(s)) => push_text(&mut text, s),
                    Some(Value::Array(parts)) => {
                        for part in parts {
                            match part.get("type").and_then(Value::as_str) {
                                Some("text") => {
                                    if let Some(t) = part.get("text").and_then(Value::as_str) {
                                        push_text(&mut text, t);
                                    }
                                }
                                Some("reasoning") => {
                                    push_text(&mut thinking, &flatten_content(part.get("content")))
                                }
                                Some("tool_call") => {
                                    let name =
                                        part.get("name").and_then(Value::as_str).unwrap_or("tool");
                                    let args = part
                                        .pointer("/arguments/value")
                                        .cloned()
                                        .unwrap_or(Value::Null);
                                    calls.push((
                                        id_of(part, "id"),
                                        call_tool(name, &args, Outcome::Running),
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
                for call in message
                    .get("tool_calls")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let name = call
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool");
                    let args = call
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .and_then(|a| serde_json::from_str(a).ok())
                        .unwrap_or(Value::Null);
                    calls.push((id_of(call, "id"), call_tool(name, &args, Outcome::Running)));
                }
                if !thinking.trim().is_empty() {
                    let mut turn = Turn::new("assistant", "reasoning", "");
                    turn.set_text(&thinking, self.limits.text);
                    self.push(turn);
                }
                if text.trim().is_empty() && calls.is_empty() {
                    return;
                }
                let role = if role == "user" { "user" } else { "assistant" };
                let mut turn = Turn::new(role, "message", "");
                turn.set_text(&tidy_harness_text(&text), self.limits.text);
                let seq = self.push(turn);
                for (id, call) in calls {
                    self.add_tool(seq, id.as_deref(), call);
                }
            }
            _ => {}
        }
    }
}

/// The id a Pi tool call is answered by, if it gave one.
///
/// Pi pairs a result to its call by id, so an idless call is one no later entry
/// can resolve and indexing it would be a key nothing ever looks up.
fn id_of(block: &Value) -> Option<String> {
    block.get("id").and_then(Value::as_str).map(str::to_string)
}

/// What a transcript says about how a call ended.
enum Outcome {
    /// The outcome is here, and this is whether it was a failure.
    Recorded(String, bool),
    /// The call is in flight. This harness records results, and there is not
    /// one yet — which is a different statement from there being no kind.
    Running,
    /// This harness records no outcomes at all, so a call in the transcript
    /// has finished and whatever came back was never kept.
    NotRecorded,
}

/// One call as a transcript records it.
///
/// [`Outcome::NotRecorded`] is the case worth the type: a missing result on a
/// harness that keeps some is a call still running, and the page draws those
/// two differently. Reading one as the other is the difference between a live
/// conversation and a transcript claiming every call in it is in flight.
fn call_tool(name: &str, args: &Value, outcome: Outcome) -> ToolUse {
    let (detail, full) = extract::tool_detail(name, args);
    let (result, failed) = match outcome {
        Outcome::Recorded(result, failed) => (Some(result), failed),
        Outcome::Running => (None, false),
        Outcome::NotRecorded => (Some(String::new()), false),
    };
    let delta = edits_file(name)
        .then(|| extract::edit_delta(args))
        .flatten();
    ToolUse {
        name: util::pretty_mcp_name(name),
        detail,
        full,
        result,
        failed,
        added: delta.as_ref().map_or(0, |d| d.added),
        removed: delta.as_ref().map_or(0, |d| d.removed),
        diff: delta
            .map(|d| d.hunks.into_iter().take(MAX_DIFF_LINES).collect())
            .unwrap_or_default(),
        ..ToolUse::default()
    }
}

/// Whether a tool is one of the harnesses' own file editors, which are the
/// calls whose arguments are the patch.
///
/// The names differ by capitalisation and by synonym between harnesses, and
/// none of them is the same word, so this is the list rather than a rule about
/// the arguments — a call's `content` key can hold anything, and treating that
/// as a diff would invent one.
fn edits_file(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "edit" | "write" | "replace" | "multiedit" | "str_replace" | "str_replace_editor"
    )
}

/// Whether a text part is something a person said rather than something
/// OpenCode or a plugin put in their mouth.
///
/// Two flags mark it, and both land on `user` messages: `ignored` is a plugin's
/// own progress output (a compression notice with a bar of block characters
/// under it), and `synthetic` is a replay of a tool call the reader has already
/// been shown. Neither is in the conversation, and a transcript showing either
/// is one nobody recognises.
fn opencode_is_played_back(part: &Value) -> bool {
    part.get("ignored").and_then(Value::as_bool) == Some(true)
        || part.get("synthetic").and_then(Value::as_bool) == Some(true)
}

/// One OpenCode tool call, with whatever came back from it.
///
/// The call and its outcome are one record, so there is nothing to wait for: a
/// `state` with no `output` yet is a call still running, and that absence is
/// what the page already draws for a live call from any other harness.
fn opencode_call(part: &Value) -> ToolUse {
    let state = part.get("state");
    let name = part.get("tool").and_then(Value::as_str).unwrap_or("tool");
    let input = state
        .and_then(|s| s.get("input"))
        .cloned()
        .unwrap_or(Value::Null);
    let (mut detail, full) = extract::tool_detail(name, &input);
    // OpenCode writes the one line it shows for the call, which is the only
    // thing that can describe a tool cctop has no rule for — a plugin's, or an
    // MCP server's, where `tool_detail` has nothing to go on.
    if detail.is_empty()
        && let Some(title) = state.and_then(|s| s.get("title")).and_then(Value::as_str)
    {
        detail = title.to_string();
    }
    let output = state.and_then(|s| s.get("output"));
    let why = state
        .and_then(|s| s.get("error"))
        .and_then(Value::as_str)
        .filter(|e| !e.trim().is_empty());
    let mut tool = ToolUse {
        name: util::pretty_mcp_name(name),
        detail,
        full,
        result: output
            .map(|out| flatten_content(Some(out)))
            .or_else(|| why.map(str::to_string)),
        failed: state.and_then(|s| s.get("status")).and_then(Value::as_str) == Some("error"),
        ..ToolUse::default()
    };
    if let Some(delta) = state.and_then(|s| opencode::tool_delta(name, s)) {
        tool.added = delta.added;
        tool.removed = delta.removed;
        tool.diff = delta.hunks.into_iter().take(MAX_DIFF_LINES).collect();
    }
    tool
}

/// The diff a Claude edit reported, from the entry carrying its result.
fn claude_delta(item: &Line) -> Option<Delta> {
    let lines = item.tool_use_result.get()?.patch_lines()?;
    let mut delta = Delta::default();
    for line in lines {
        if line.starts_with('+') {
            delta.added += 1;
        } else if line.starts_with('-') {
            delta.removed += 1;
        }
        if delta.hunks.len() < MAX_DIFF_LINES {
            delta.hunks.push(line.to_string());
        }
    }
    (delta.added > 0 || delta.removed > 0).then_some(delta)
}

/// A Claude tool result's content, as [`flatten_content`] reads it, without
/// building the base64 of an image result to throw it away.
///
/// A list — text blocks, image blocks — is the case worth reading typed; a
/// string or anything else is built and handed over whole, since it is all
/// kept anyway.
fn flatten_raw(content: Raw) -> String {
    #[derive(Default, serde::Deserialize)]
    #[serde(default)]
    struct Part<'a> {
        #[serde(rename = "type", borrow)]
        kind: Str<'a>,
        #[serde(borrow)]
        text: Str<'a>,
    }
    let Some(raw) = content.0 else {
        return flatten_content(None);
    };
    let Some(parts) = raw
        .get()
        .starts_with('[')
        .then(|| serde_json::from_str::<Shape<Part>>(raw.get()).ok())
        .flatten()
    else {
        return flatten_content(Some(&content.value()));
    };
    let mut out = String::new();
    for part in parts.list().unwrap_or_default() {
        match part {
            Shape::Text(s) => push_text(&mut out, s),
            Shape::Object(part) => {
                if let Some(t) = part.text.get() {
                    push_text(&mut out, t);
                } else if part.kind.is("image") {
                    // See `flatten_content`.
                    push_text(&mut out, "[image]");
                }
            }
            _ => {}
        }
    }
    crate::util::clip(out.trim(), MAX_RESULT_CHARS)
}

/// Text out of a Codex message payload's content blocks.
fn codex_text(payload: &codex_line::Payload) -> String {
    let mut out = String::new();
    match &payload.content {
        Shape::Text(s) => out.push_str(s),
        Shape::List(blocks) => {
            for block in blocks {
                match block {
                    Shape::Text(s) => push_text(&mut out, s),
                    Shape::Object(block) => {
                        if let Some(t) = block.text.get() {
                            push_text(&mut out, t);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    out
}

/// The reasoning summary Codex records, which is a list of its own blocks.
fn codex_summary(payload: &codex_line::Payload) -> String {
    let mut out = String::new();
    for block in payload
        .summary
        .list()
        .unwrap_or_default()
        .iter()
        .filter_map(Shape::object)
    {
        if let Some(t) = block.text.get() {
            push_text(&mut out, t);
        }
    }
    out
}

/// Whether a Codex tool output says the call failed.
///
/// The field is not always there and not always a bool: a shell call reports an
/// exit status inside its output text instead, so both are checked and neither
/// is required.
fn codex_output_failed(output: Option<&Value>) -> bool {
    let Some(output) = output else {
        return false;
    };
    if output.get("success").and_then(Value::as_bool) == Some(false) {
        return true;
    }
    let text = match output {
        Value::String(s) => s.clone(),
        other => other
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    };
    let head: String = text.chars().take(400).collect();
    head.contains("exit code 1")
        || head.contains("Error:")
        || head.contains("command not found")
        || head.contains("No such file or directory")
}

/// A tool result's content, whatever shape it arrived in, cut to size.
fn flatten_content(content: Option<&Value>) -> String {
    let mut out = String::new();
    match content {
        Some(Value::String(s)) => out.push_str(s),
        Some(Value::Array(blocks)) => {
            for block in blocks {
                match block {
                    Value::String(s) => push_text(&mut out, s),
                    _ => {
                        if let Some(t) = block.get("text").and_then(Value::as_str) {
                            push_text(&mut out, t);
                        } else if block.get("type").and_then(Value::as_str) == Some("image") {
                            // The bytes are megabytes of base64 and the page has
                            // nothing to do with them, but a result that was an
                            // image should not read as an empty one.
                            push_text(&mut out, "[image]");
                        }
                    }
                }
            }
        }
        Some(Value::Object(map)) => {
            // Codex's `output` is an object with the text under one of a few
            // keys depending on the tool.
            for key in ["content", "output", "stdout", "text"] {
                if let Some(t) = map.get(key).and_then(Value::as_str) {
                    push_text(&mut out, t);
                }
            }
            if out.is_empty() {
                out = content.map(|c| c.to_string()).unwrap_or_default();
            }
        }
        Some(other) => out = other.to_string(),
        None => {}
    }
    crate::util::clip(out.trim(), MAX_RESULT_CHARS)
}

/// Append with a blank line between blocks, so two text blocks do not run into
/// one word.
pub(super) fn push_text(out: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(text);
}

/// What a Gemini record said, whichever of its two shapes holds it.
///
/// A person's turn is a list of text blocks; an agent's is the string itself.
/// Both appear on the same field, so the string case is the one that has to be
/// told apart rather than assumed.
fn gemini_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(blocks) => {
            let mut out = String::new();
            for block in blocks.as_array().into_iter().flatten() {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    push_text(&mut out, t);
                }
            }
            out
        }
        None => String::new(),
    }
}

/// What a Gemini call returned, out of the envelope it arrives in.
///
/// The outcome sits under `functionResponse.response`, whose shape is the
/// called tool's own, so the first response that carries any text is the one
/// shown and the whole envelope is the fallback — an empty box on a call that
/// demonstrably did something is worse than an ugly one.
fn gemini_result(result: &Value) -> String {
    for response in result
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part.get("functionResponse"))
    {
        let body = response
            .get("response")
            .map(|inner| flatten_content(Some(inner)))
            .unwrap_or_default();
        if !body.trim().is_empty() {
            return body;
        }
    }
    flatten_content(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sink_codex(lines: &[&str]) -> Conversation {
        let mut sink = Sink::default();
        for line in lines {
            sink.codex(&codex_line::Line::parse(line.as_bytes()).unwrap());
        }
        sink.finish()
    }

    fn sink_cursor(lines: &[&str]) -> Conversation {
        let mut sink = Sink::default();
        for line in lines {
            sink.cursor(&serde_json::from_str(line).unwrap());
        }
        sink.finish()
    }

    fn sink_gemini(lines: &[&str]) -> Conversation {
        let mut sink = Sink::default();
        for line in lines {
            sink.gemini(&serde_json::from_str(line).unwrap());
        }
        sink.finish()
    }

    fn sink_pi(lines: &[&str]) -> Conversation {
        let mut sink = Sink::default();
        for line in lines {
            sink.pi(&serde_json::from_str(line).unwrap());
        }
        sink.finish()
    }

    /// A database shaped the way OpenCode writes one, holding the rows of
    /// `ses_f1d129d6dffeGj3ft9REKBbzIY` verbatim.
    ///
    /// The envelope of each message and the JSON of each part are exactly what
    /// the real database held, so a reader that gets this right is right about
    /// the format rather than about a shape invented beside it.
    fn opencode_db(
        messages: &[(&str, i64, &str, &str, &[&str])],
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opencode.db");
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE message (id text PRIMARY KEY, session_id text NOT NULL, \
             time_created integer NOT NULL, time_updated integer NOT NULL, data text NOT NULL); \
             CREATE TABLE part (id text PRIMARY KEY, message_id text NOT NULL, \
             session_id text NOT NULL, time_created integer NOT NULL, \
             time_updated integer NOT NULL, data text NOT NULL);",
        )
        .unwrap();
        for (index, (id, created, data, session, parts)) in messages.iter().enumerate() {
            db.execute(
                "INSERT INTO message (id, session_id, time_created, time_updated, data) \
                 VALUES (?1, ?4, ?2, ?2, ?3)",
                rusqlite::params![id, created, data, session],
            )
            .unwrap();
            for (part, data) in parts.iter().enumerate() {
                db.execute(
                    "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) \
                     VALUES (?1, ?2, ?4, ?3, ?3, ?5)",
                    rusqlite::params![
                        format!("prt_{index}_{part:02}"),
                        id,
                        created,
                        session,
                        *data
                    ],
                )
                .unwrap();
            }
        }
        (dir, path)
    }

    /// What a person asked and what the agent did about it, read out of two
    /// tables rather than a line of JSON: the text is a part of a message, and
    /// so is the call the reply made.
    #[test]
    fn an_opencode_exchange_reads_the_same_way() {
        let (_dir, path) = opencode_db(&[
            (
                "msg_user",
                1_790_513_603_273,
                r#"{"role":"user","time":{"created":1790513603273},"agent":"build","model":{"providerID":"opencode-go","modelID":"deepseek-v4.1-flash"},"summary":{"diffs":[]}}"#,
                "ses_1",
                &[r#"{"type":"text","text":"fix that"}"#],
            ),
            (
                "msg_assistant",
                1_790_513_603_841,
                r#"{"parentID":"msg_user","role":"assistant","mode":"build","agent":"build","cost":0.005038026,"tokens":{"total":35130,"input":33267,"output":43,"reasoning":28,"cache":{"write":0,"read":1792}},"modelID":"deepseek-v4.1-flash","providerID":"opencode-go","time":{"created":1790513603841,"completed":1790513623910},"finish":"tool-calls"}"#,
                "ses_1",
                &[
                    r#"{"type":"step-start","snapshot":"b5c54e70bee180a4dfda72a57242f7cf0cf790c0"}"#,
                    r#"{"type":"reasoning","text":"the note reads wrong","time":{"start":1790513603841,"end":1790513604000}}"#,
                    r#"{"type":"tool","tool":"grep","callID":"call_00_2oelhco6qfl7o8wyhsmg9e2s","state":{"status":"completed","input":{"pattern":"cannot read a OpenCode conversation"},"output":"No files found","metadata":{"matches":0,"truncated":false},"title":"cannot read a OpenCode conversation","time":{"start":1790513604100,"end":1790513604110}}}"#,
                    r#"{"type":"step-finish","reason":"tool-calls","snapshot":"b5c54e70bee180a4dfda72a57242f7cf0cf790c0","cost":0.005038026}"#,
                ],
            ),
        ]);
        let mut session = Session::new(Provider::OpenCode, "ses_1".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        assert!(chat.supported, "{:?}", chat.note);
        // The thinking part and the two bookkeeping parts are not turns, but
        // the thinking is a turn of its own: it reads differently on purpose.
        assert_eq!(chat.turns.len(), 3);
        assert_eq!(
            (chat.turns[0].role.as_ref(), chat.turns[0].kind.as_ref()),
            ("user", "message")
        );
        assert_eq!(chat.turns[0].text, "fix that");
        assert_eq!(
            (chat.turns[1].role.as_ref(), chat.turns[1].kind.as_ref()),
            ("assistant", "reasoning")
        );
        assert_eq!(chat.turns[1].text, "the note reads wrong");
        // A message is already a whole reply, so the turn it becomes is its own
        // rather than an extension of the reasoning above it.
        assert_eq!(chat.turns[2].seq, 2);
        assert_eq!(chat.turns[2].role, "assistant");
        let call = &chat.turns[2].tools[0];
        assert_eq!(call.name, "grep");
        assert_eq!(call.detail, "cannot read a OpenCode conversation");
        assert_eq!(call.result.as_deref(), Some("No files found"));
        assert!(!call.failed);
        // The millisecond the envelope records is the turn's time, in the form
        // the page hands to its clock.
        assert_eq!(chat.turns[0].ts, util::ms_to_rfc3339(1_790_513_603_273));
    }

    /// A call that has not come back yet, and one that came back wrong, are
    /// both things the live view exists to show: the first has no result to
    /// show, the second has a message to show instead of output.
    #[test]
    fn an_opencode_call_says_whether_it_is_still_running() {
        let (_dir, path) = opencode_db(&[(
            "msg_assistant",
            1_790_513_603_841,
            r#"{"role":"assistant","time":{"created":1790513603841}}"#,
            "ses_1",
            &[
                r#"{"type":"tool","tool":"bash","state":{"status":"running","input":{"command":"cargo test"},"title":"cargo test"}}"#,
                r#"{"type":"tool","tool":"read","state":{"status":"error","input":{"filePath":"/tmp/backend_test.log"},"error":"Cannot read binary file: /tmp/backend_test.log","title":"/tmp/backend_test.log"}}"#,
            ],
        )]);
        let mut session = Session::new(Provider::OpenCode, "ses_1".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        let tools = &chat.turns[0].tools;
        assert_eq!(tools.len(), 2);
        assert!(tools[0].result.is_none(), "a running call has no result");
        assert!(!tools[0].failed);
        assert_eq!(
            tools[1].result.as_deref(),
            Some("Cannot read binary file: /tmp/backend_test.log")
        );
        assert!(tools[1].failed);
    }

    /// The lines an edit added and removed come out of the input OpenCode
    /// recorded, and the table already counts its `+` and `-` from there, so a
    /// call in the conversation view and its row in the table say one number.
    #[test]
    fn an_opencode_edit_carries_the_same_lines_the_table_counted() {
        let (_dir, path) = opencode_db(&[(
            "msg_assistant",
            1_790_513_603_841,
            r#"{"role":"assistant","time":{"created":1790513603841}}"#,
            "ses_1",
            &[
                r#"{"type":"tool","tool":"edit","state":{"status":"completed","input":{"filePath":"/a/b.rs","oldString":"let a = 1;\nlet b = 2;\n","newString":"let a = 1;\nlet b = 3;\nlet c = 4;\n"},"output":"ok","title":"/a/b.rs"}}"#,
            ],
        )]);
        let mut session = Session::new(Provider::OpenCode, "ses_1".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        let call = &chat.turns[0].tools[0];
        assert_eq!(call.name, "edit");
        assert_eq!((call.added, call.removed), (3, 2));
        assert!(call.diff.iter().any(|l| l.contains("let b = 3;")));
        assert!(call.diff.iter().any(|l| l.contains("let b = 2;")));
    }

    /// A plugin writing its progress and a tool call replayed into the prompt
    /// both land on a user message. Shown, they are a person being shown their
    /// own tool output a second time, as though they had asked for it.
    #[test]
    fn text_the_harness_played_back_is_not_something_a_person_said() {
        let (_dir, path) = opencode_db(&[(
            "msg_user",
            1_790_513_603_273,
            r#"{"role":"user","time":{"created":1790513603273},"summary":{"diffs":[]}}"#,
            "ses_1",
            &[
                r#"{"type":"text","text":"the real question"}"#,
                r#"{"type":"text","text":"▣ DCP | -31.1K removed, +4.7K summary","ignored":true}"#,
                r#"{"type":"text","text":"Called the Read tool with the following input: {\"filePath\":\"/a/b.rs\"}","synthetic":true}"#,
            ],
        )]);
        let mut session = Session::new(Provider::OpenCode, "ses_1".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].text, "the real question");
    }

    /// OpenCode drops the messages before a compaction and records the seam. A
    /// transcript that opens mid-conversation says so, rather than implying the
    /// session began where the scroll does.
    #[test]
    fn an_opencode_compaction_is_a_seam_not_a_message() {
        let (_dir, path) = opencode_db(&[(
            "msg_compaction",
            1_790_513_603_273,
            r#"{"role":"user","model":{"providerID":"opencode-go"},"agent":"explore","time":{"created":1790513603273},"summary":{"diffs":[]}}"#,
            "ses_1",
            &[r#"{"type":"compaction","auto":true,"overflow":false,"tail_start_id":"msg_tail"}"#],
        )]);
        let mut session = Session::new(Provider::OpenCode, "ses_1".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(
            (chat.turns[0].role.as_ref(), chat.turns[0].kind.as_ref()),
            ("system", "compaction")
        );
        assert!(chat.turns[0].text.contains("compacted"));
    }

    /// Four hundred and ninety of the seven hundred sessions here are a
    /// subagent, whose messages are their own. A parent that read its child's
    /// would show work twice, on two rows of the table at once.
    #[test]
    fn an_opencode_subagent_does_not_leak_into_its_parent() {
        let (_dir, path) = opencode_db(&[
            (
                "msg_parent",
                1_790_513_603_273,
                r#"{"role":"user","time":{"created":1790513603273}}"#,
                "ses_parent",
                &[r#"{"type":"text","text":"the parent's question"}"#],
            ),
            (
                "msg_child",
                1_790_513_603_274,
                r#"{"role":"user","time":{"created":1790513603274}}"#,
                "ses_child",
                &[r#"{"type":"text","text":"the subagent's brief"}"#],
            ),
        ]);
        let mut session = Session::new(Provider::OpenCode, "ses_parent".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].text, "the parent's question");
    }

    /// A Cursor exchange reads the way every other does: the role says who is
    /// speaking, the blocks are the words and the calls, and what the harness
    /// does not keep is left as a gap rather than filled in.
    ///
    /// These are the shapes Cursor actually writes, taken from transcripts in
    /// `~/.cursor/projects`: no `type` on a message entry at all, `turn_ended`
    /// entries beside them, and user text wrapped in `<user_query>`.
    #[test]
    fn a_cursor_exchange_reads_the_same_way() {
        let chat = sink_cursor(&[
            r#"{"role":"user","message":{"content":[{"type":"text","text":"<user_query>\nextract doesn't extract the lorebook\n</user_query>"}]}}"#,
            r#"{"role":"assistant","message":{"content":[{"type":"text","text":"I'll trace how `extract` works."},{"type":"tool_use","name":"Grep","input":{"pattern":"lorebook","path":"/home/flo/JAR/jar_cli"}}]}}"#,
            r#"{"type":"turn_ended","status":"success"}"#,
            r#"{"type":"turn_ended","status":"error","error":"User aborted request"}"#,
        ]);
        assert_eq!(chat.turns.len(), 2);
        assert_eq!(chat.turns[0].role, "user");
        // The wrapper is Cursor's, not the person's.
        assert_eq!(chat.turns[0].text, "extract doesn't extract the lorebook");
        assert_eq!(chat.turns[1].role, "assistant");
        assert_eq!(chat.turns[1].text, "I'll trace how `extract` works.");
        assert_eq!(chat.turns[1].tools.len(), 1);
        assert_eq!(chat.turns[1].tools[0].name, "Grep");
        assert!(chat.turns[1].tools[0].detail.contains("lorebook"));
        // Cursor dates nothing, so there is no time to show.
        assert_eq!(chat.turns[1].ts, "");
    }

    /// A Cursor call carries no outcome, and saying so must not read as a call
    /// still running: the page shows a missing result as a live one.
    #[test]
    fn a_cursor_call_is_not_forever_running() {
        let chat = sink_cursor(&[
            r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"path":"src/main.rs"}}]}}"#,
        ]);
        let tool = &chat.turns[0].tools[0];
        assert_eq!(tool.result.as_deref(), Some(""));
        assert!(!tool.failed);
    }

    /// A Gemini reply is one record holding its thoughts, its words and every
    /// call it made, with each result on the call itself.
    #[test]
    fn a_gemini_reply_reads_the_same_way() {
        let chat = sink_gemini(&[
            r#"{"sessionId":"79709c93","startTime":"2026-05-14T17:34:01.387Z","kind":"main"}"#,
            r#"{"$set":{"summary":"Improving script latency"}}"#,
            r#"{"type":"user","timestamp":"2026-05-14T17:34:06.028Z","content":[{"text":"go"}]}"#,
            r#"{"type":"gemini","timestamp":"2026-05-14T17:34:13.475Z","content":"on it","thoughts":[{"subject":"Where it blocks","description":"the parser re-reads the file per record"}],"toolCalls":[{"id":"read_file_1","name":"read_file","args":{"file_path":"README.md"},"status":"success","result":[{"functionResponse":{"id":"read_file_1","name":"read_file","response":{"output":"the readme"}}}]}]}"#,
            r#"{"type":"info","timestamp":"2026-05-14T17:34:20.000Z","content":"Switched to Plan Mode."}"#,
        ]);
        // The header and the `$set` patch are not conversation.
        assert_eq!(chat.turns.len(), 4);
        assert_eq!(chat.turns[0].role, "user");
        assert_eq!(chat.turns[0].text, "go");
        assert_eq!(chat.turns[0].ts, "2026-05-14T17:34:06.028Z");
        assert_eq!(chat.turns[1].kind, "reasoning");
        assert_eq!(
            chat.turns[1].text,
            "the parser re-reads the file per record"
        );
        assert_eq!(chat.turns[2].text, "on it");
        let tool = &chat.turns[2].tools[0];
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.result.as_deref(), Some("the readme"));
        assert!(!tool.failed);
        // The harness speaking for itself.
        assert_eq!(chat.turns[3].role, "system");
        assert_eq!(chat.turns[3].text, "Switched to Plan Mode.");
    }

    /// A Gemini call that failed says so, and one with no result yet is a call
    /// in flight — this harness records outcomes, so the two are tellable apart.
    #[test]
    fn a_gemini_call_says_whether_it_still_is_running() {
        let chat = sink_gemini(&[
            r#"{"type":"gemini","timestamp":"t","content":"","toolCalls":[{"id":"a","name":"replace","args":{"file_path":"src/main.rs"},"status":"error","result":[{"functionResponse":{"response":{"output":"no such file"}}}]}]}"#,
            r#"{"type":"gemini","timestamp":"t2","content":"","toolCalls":[{"id":"b","name":"read_file","args":{}}]}"#,
        ]);
        assert!(chat.turns[0].tools[0].failed);
        assert_eq!(
            chat.turns[0].tools[0].result.as_deref(),
            Some("no such file")
        );
        // No result recorded: Gemini keeps them, so this one is still running.
        assert!(chat.turns[1].tools[0].result.is_none());
    }

    /// A Pi exchange: a person's turn is a bare string, an agent's is blocks,
    /// and a result is an entry of its own keyed to the call it answers.
    #[test]
    fn a_pi_exchange_reads_the_same_way() {
        let chat = sink_pi(&[
            r#"{"type":"session","version":3,"id":"pi-1","timestamp":"2024-12-03T14:00:00.000Z","cwd":"/work"}"#,
            r#"{"type":"label","label":"a name for this session"}"#,
            r#"{"type":"message","id":"a1","parentId":null,"timestamp":"2024-12-03T14:00:01.000Z","message":{"role":"user","content":"Hello"}}"#,
            r#"{"type":"message","id":"b2","parentId":"a1","timestamp":"2024-12-03T14:00:02.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"they said hello"},{"type":"text","text":"Hi!"},{"type":"toolCall","id":"call_123","name":"bash","arguments":{"command":"ls"}}]}}"#,
            r#"{"type":"message","id":"c3","parentId":"b2","timestamp":"2024-12-03T14:00:03.000Z","message":{"role":"toolResult","toolCallId":"call_123","toolName":"bash","content":[{"type":"text","text":"README.md"}],"isError":false}}"#,
        ]);
        // The session header and the label are the harness, not the conversation.
        assert_eq!(chat.turns.len(), 3);
        assert_eq!(chat.turns[0].role, "user");
        assert_eq!(chat.turns[0].text, "Hello");
        assert_eq!(chat.turns[1].kind, "reasoning");
        assert_eq!(chat.turns[1].text, "they said hello");
        assert_eq!(chat.turns[2].text, "Hi!");
        let tool = &chat.turns[2].tools[0];
        assert_eq!(tool.name, "bash");
        // The result arrived as a later entry and found its call.
        assert_eq!(tool.result.as_deref(), Some("README.md"));
        assert!(!tool.failed);
    }

    /// A Pi result that failed is marked as one, and a call with no result
    /// entry behind it is a call in flight.
    #[test]
    fn a_pi_call_says_whether_it_still_is_running() {
        let chat = sink_pi(&[
            r#"{"type":"message","timestamp":"t","message":{"role":"assistant","content":[{"type":"toolCall","id":"c1","name":"bash","arguments":{}}]}}"#,
            r#"{"type":"message","timestamp":"t2","message":{"role":"toolResult","toolCallId":"c1","toolName":"bash","content":[{"type":"text","text":"boom"}],"isError":true}}"#,
        ]);
        assert!(chat.turns[0].tools[0].failed);
        assert_eq!(chat.turns[0].tools[0].result.as_deref(), Some("boom"));
    }

    fn sink_vibe(lines: &[&str]) -> Conversation {
        let mut sink = Sink::default();
        for line in lines {
            sink.vibe(&serde_json::from_str(line).unwrap());
        }
        sink.finish()
    }

    /// The unified store's parts: reasoning is a turn of its own, a call's
    /// arguments are already parsed, and the tool message's `outcome` says it
    /// failed.
    #[test]
    fn a_vibe_checkpoint_exchange_reads_the_same_way() {
        let chat = sink_vibe(&[
            r#"{"role":"user","content":[{"type":"text","text":"why 429?"}]}"#,
            r#"{"role":"assistant","content":[{"type":"reasoning","content":[{"type":"text","text":"grep it"}]},{"type":"tool_call","id":"k1","name":"bash","arguments":{"raw":"{}","value":{"command":"grep -r 429"}}}]}"#,
            r#"{"role":"tool","name":"bash","tool_call_id":"k1","outcome":"failure","content":[{"type":"text","text":"exit 2"}]}"#,
            r#"{"role":"assistant","content":[{"type":"text","text":"Rate limits."}]}"#,
        ]);
        assert_eq!(chat.turns.len(), 4);
        assert_eq!(chat.turns[0].text, "why 429?");
        assert_eq!(chat.turns[1].kind, "reasoning");
        let tool = &chat.turns[2].tools[0];
        assert_eq!(tool.name, "bash");
        assert!(tool.failed);
        assert_eq!(tool.result.as_deref(), Some("exit 2"));
        assert_eq!(chat.turns[3].text, "Rate limits.");
    }

    /// The Python CLI's OpenAI shape: arguments as a JSON string, reasoning
    /// beside the content, and an injected message the person never typed.
    #[test]
    fn a_vibe_python_exchange_reads_the_same_way() {
        let chat = sink_vibe(&[
            r#"{"role":"user","content":"Hello","injected":false}"#,
            r#"{"role":"user","content":"<system reminder>","injected":true}"#,
            r#"{"role":"assistant","reasoning_content":"say hi","tool_calls":[{"id":"U6","function":{"name":"read_file","arguments":"{\"path\": \"/x\"}"}}]}"#,
            r#"{"role":"tool","content":"path: /x","tool_call_id":"U6"}"#,
        ]);
        assert_eq!(chat.turns.len(), 3);
        assert_eq!(chat.turns[0].text, "Hello");
        assert_eq!(chat.turns[1].kind, "reasoning");
        let tool = &chat.turns[2].tools[0];
        assert_eq!(tool.name, "read_file");
        assert_eq!(tool.result.as_deref(), Some("path: /x"));
        assert!(!tool.failed);
    }

    /// An editor's arguments are the patch, whichever harness spelled the tool
    /// its own way — this is what the report's per-file diffs read.
    #[test]
    fn an_edit_carries_the_patch_whatever_the_tool_is_called() {
        let chat = sink_cursor(&[
            r#"{"role":"assistant","message":{"content":[{"type":"tool_use","name":"Edit","input":{"path":"a.rs","oldString":"let x = 1;","newString":"let x = 2;"}}]}}"#,
        ]);
        let tool = &chat.turns[0].tools[0];
        assert_eq!((tool.added, tool.removed), (1, 1));
        assert_eq!(tool.diff, vec!["-let x = 1;", "+let x = 2;"]);
    }

    #[test]
    fn a_claude_exchange_becomes_a_user_turn_and_an_assistant_turn() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"role":"user","content":"fix the parser"}}"#,
            r#"{"type":"assistant","timestamp":"t2","message":{"role":"assistant","content":[{"type":"text","text":"on it"}]}}"#,
        ]);
        assert!(chat.supported);
        assert_eq!(chat.turns.len(), 2);
        assert_eq!(chat.turns[0].role, "user");
        assert_eq!(chat.turns[0].text, "fix the parser");
        assert_eq!(chat.turns[1].role, "assistant");
        assert_eq!(chat.turns[1].text, "on it");
    }

    /// The call and its result are two entries a long way apart in the file, and
    /// the whole point of the id index is that they come back as one thing.
    #[test]
    fn a_tool_result_lands_on_the_call_it_answers() {
        let chat = sink_claude(&[
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a/b.rs"}}]}}"#,
            r#"{"type":"assistant","timestamp":"t2","message":{"content":[{"type":"text","text":"meanwhile"}]}}"#,
            r#"{"type":"user","timestamp":"t3","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"fn main() {}"}]}}"#,
        ]);
        let tool = &chat.turns[0].tools[0];
        assert_eq!(tool.name, "Read");
        assert_eq!(tool.detail, "/a/b.rs");
        assert_eq!(tool.result.as_deref(), Some("fn main() {}"));
        assert!(!tool.failed);
        // The result entry carried no text of its own, so it is not a turn —
        // and the two assistant entries are one reply, since nothing came back
        // in between.
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].text, "meanwhile");
    }

    /// Claude writes the text of a reply and each of its tool calls as separate
    /// records. Rendered one box per record, a single answer becomes four, three
    /// of them empty but for a tool name.
    #[test]
    fn a_run_of_assistant_entries_is_one_reply() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t0","message":{"content":"go"}}"#,
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"text","text":"first"}]}}"#,
            r#"{"type":"assistant","timestamp":"t2","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a"}}]}}"#,
            r#"{"type":"assistant","timestamp":"t3","message":{"content":[{"type":"text","text":"second"}]}}"#,
            r#"{"type":"user","timestamp":"t4","message":{"content":"again"}}"#,
            r#"{"type":"assistant","timestamp":"t5","message":{"content":[{"type":"text","text":"a new reply"}]}}"#,
        ]);
        let roles: Vec<&str> = chat.turns.iter().map(|t| t.role.as_ref()).collect();
        assert_eq!(roles, vec!["user", "assistant", "user", "assistant"]);
        assert_eq!(chat.turns[1].text, "first\n\nsecond");
        assert_eq!(chat.turns[1].tools.len(), 1);
        // A user turn between them ends the run, so the next reply is its own.
        assert_eq!(chat.turns[3].text, "a new reply");
    }

    #[test]
    fn a_failed_result_is_marked_as_one() {
        let chat = sink_claude(&[
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"cargo test"}}]}}"#,
            r#"{"type":"user","timestamp":"t2","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":true,"content":"no such command"}]}}"#,
        ]);
        assert!(chat.turns[0].tools[0].failed);
    }

    /// A call with no result yet is what a live session looks like, and the page
    /// shows it as running — so the absence has to survive to the JSON.
    #[test]
    fn a_call_still_running_has_no_result() {
        let chat = sink_claude(&[
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"sleep 60"}}]}}"#,
        ]);
        assert!(chat.turns[0].tools[0].result.is_none());
    }

    #[test]
    fn an_edits_patch_is_carried_with_its_tool_call() {
        let chat = sink_claude(&[
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Edit","input":{"file_path":"/a/b.rs"}}]}}"#,
            r#"{"type":"user","timestamp":"t2","toolUseResult":{"structuredPatch":[{"lines":["-old","+new","+also"]}]},"message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"ok"}]}}"#,
        ]);
        let tool = &chat.turns[0].tools[0];
        assert_eq!((tool.added, tool.removed), (2, 1));
        assert_eq!(tool.diff, vec!["-old", "+new", "+also"]);
    }

    /// A subagent writes into the same file, and its turns are another
    /// conversation. Including them interleaves two agents into one thread.
    #[test]
    fn sidechain_turns_are_left_out() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"the real ask"}}"#,
            r#"{"type":"user","timestamp":"t2","isSidechain":true,"message":{"content":"a subagent's brief"}}"#,
        ]);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].text, "the real ask");
    }

    #[test]
    fn a_compaction_summary_is_a_seam_not_a_message() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","isCompactSummary":true,"message":{"content":"everything so far"}}"#,
        ]);
        assert_eq!(chat.turns[0].kind, "compaction");
        assert_eq!(chat.turns[0].role, "system");
    }

    /// A queued message on a sidechain is a subagent's, like every sidechain
    /// record, and other attachments are not messages at all.
    #[test]
    fn sidechain_and_other_attachments_are_skipped() {
        let chat = sink_claude(&[
            r#"{"type":"attachment","timestamp":"t1","isSidechain":true,"attachment":{"type":"queued_command","origin":{"kind":"human"},"prompt":"a subagent's queue"}}"#,
            r#"{"type":"attachment","timestamp":"t2","attachment":{"type":"file","content":"dummy file"}}"#,
        ]);
        assert!(chat.turns.is_empty(), "{:?}", chat.turns);
    }

    /// A notification carrying none of the fields a reader needs is not a
    /// turn at all — the field list itself was the bug, not the content.
    #[test]
    fn a_notification_with_no_news_is_no_turn() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","message":{"content":"<task-notification>\n<task-id>x</task-id>\n<output-file>/tmp/x.output</output-file>\n<status>completed</status>\n</task-notification>"}}"#,
            r#"{"type":"user","timestamp":"t2","message":{"content":"a real message"}}"#,
        ]);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].text, "a real message");
    }

    /// Everything after the cap is dropped from the *front*, because a
    /// conversation is read from where it got to — and the count of what was
    /// dropped is what stops the page implying the session started there.
    #[test]
    fn only_the_newest_turns_survive_and_the_rest_are_counted() {
        let lines: Vec<String> = (0..MAX_TURNS + 10)
            .map(|i| {
                format!(
                    r#"{{"type":"user","timestamp":"t","message":{{"content":"message {i}"}}}}"#
                )
            })
            .collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let chat = sink_claude(&refs);
        assert_eq!(chat.turns.len(), MAX_TURNS);
        assert_eq!(chat.earlier, 10);
        assert_eq!(chat.turns[0].text, "message 10");
    }

    /// A result whose call has already fallen off the front must not land on
    /// whichever turn now occupies that slot. This is the bug the sequence
    /// numbering exists to prevent.
    #[test]
    fn a_result_for_a_dropped_call_is_discarded() {
        let mut lines = vec![
            r#"{"type":"assistant","timestamp":"t0","message":{"content":[{"type":"tool_use","id":"toolu_old","name":"Read","input":{"file_path":"/gone.rs"}}]}}"#.to_string(),
        ];
        for i in 0..MAX_TURNS + 5 {
            lines.push(format!(
                r#"{{"type":"user","timestamp":"t","message":{{"content":"filler {i}"}}}}"#
            ));
        }
        lines.push(
            r#"{"type":"user","timestamp":"tz","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_old","content":"late"}]}}"#
                .to_string(),
        );
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let chat = sink_claude(&refs);
        assert!(
            chat.turns
                .iter()
                .all(|t| t.tools.iter().all(|tool| tool.result.is_none())),
            "a result was attached to a turn that did not make the call"
        );
    }

    #[test]
    fn text_past_the_cap_is_cut_and_says_so() {
        let long = "x".repeat(MAX_TEXT_CHARS + 100);
        let chat = sink_claude(&[&format!(
            r#"{{"type":"user","timestamp":"t","message":{{"content":"{long}"}}}}"#
        )]);
        assert!(chat.turns[0].clipped);
        assert_eq!(chat.turns[0].text.chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn thinking_is_its_own_turn_ahead_of_the_reply() {
        let chat = sink_claude(&[
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"thinking","thinking":"weighing it up"},{"type":"text","text":"here goes"}]}}"#,
        ]);
        assert_eq!(chat.turns.len(), 2);
        assert_eq!(chat.turns[0].kind, "reasoning");
        assert_eq!(chat.turns[1].text, "here goes");
    }

    /// Fields added, fields of the wrong type, fields gone: the conversation
    /// keeps every turn the right fields still describe.
    #[test]
    fn a_changed_claude_schema_loses_fields_not_turns() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t1","isSidechain":"yes","origin":7,"brandNew":[1],"message":{"content":"look at this"}}"#,
            r#"{"type":"assistant","timestamp":"t2","requestId":{"x":1},"message":{"content":[{"type":"text","text":"looking","extra":true},{"type":"tool_use","id":"toolu_1","name":3,"input":"not an object"},7]}}"#,
            r#"{"type":"user","timestamp":"t3","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":"yes","content":[{"type":"image","source":{"data":"AAAA"}},{"text":"done"}]}]}}"#,
            r#"{"type":"assistant","timestamp":"t4","message":"gone"}"#,
            r#"{"type":"assistant"}"#,
        ]);
        assert_eq!(chat.turns.len(), 2);
        assert_eq!(chat.turns[0].text, "look at this");
        assert_eq!(chat.turns[1].text, "looking");
        let tool = &chat.turns[1].tools[0];
        // A name that is not a string is the placeholder a missing one gets.
        assert_eq!(tool.name, "tool");
        assert_eq!(tool.result.as_deref(), Some("[image]\n\ndone"));
        assert!(!tool.failed);
    }

    #[test]
    fn a_changed_codex_schema_loses_fields_not_turns() {
        let chat = sink_codex(&[
            r#"{"type":"response_item","timestamp":"t1","new":{},"payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"ship it","annotations":[]},5]}}"#,
            r#"{"type":"response_item","timestamp":"t2","payload":{"type":"message","role":7,"content":[{"text":"system words"}]}}"#,
            r#"{"type":"response_item","timestamp":"t3","payload":{"type":"function_call","name":"shell","call_id":"c1","arguments":{"command":["ls"]}}}"#,
            r#"{"type":"response_item","timestamp":"t4","payload":{"type":"function_call_output","call_id":"c1","output":{"output":"a\nb","metadata":{"exit_code":0}}}}"#,
            r#"{"type":"response_item","timestamp":"t5","payload":"gone"}"#,
            r#"{"type":"event_msg"}"#,
        ]);
        let roles: Vec<&str> = chat.turns.iter().map(|t| t.role.as_ref()).collect();
        assert_eq!(roles, ["user", "system", "assistant"]);
        assert_eq!(chat.turns[0].text, "ship it");
        let tool = &chat.turns[2].tools[0];
        assert_eq!(tool.result.as_deref(), Some("a\nb"));
    }

    #[test]
    fn a_codex_exchange_reads_the_same_way() {
        let chat = sink_codex(&[
            r#"{"type":"response_item","timestamp":"t1","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"ship it"}]}}"#,
            r#"{"type":"response_item","timestamp":"t2","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"shipping"}]}}"#,
        ]);
        assert_eq!(chat.turns.len(), 2);
        assert_eq!(chat.turns[0].role, "user");
        assert_eq!(chat.turns[1].text, "shipping");
    }

    /// Codex writes a call as its own entry with nothing linking it to the
    /// message that issued it, so it has to attach to the turn in progress.
    #[test]
    fn a_codex_call_attaches_to_the_assistant_turn_in_progress() {
        let chat = sink_codex(&[
            r#"{"type":"response_item","timestamp":"t1","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"looking"}]}}"#,
            r#"{"type":"response_item","timestamp":"t2","payload":{"type":"function_call","call_id":"c1","name":"shell","arguments":"{\"command\":\"ls\"}"}}"#,
            r#"{"type":"response_item","timestamp":"t3","payload":{"type":"function_call_output","call_id":"c1","output":"a.rs\nb.rs"}}"#,
        ]);
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].tools.len(), 1);
        assert_eq!(chat.turns[0].tools[0].result.as_deref(), Some("a.rs\nb.rs"));
    }

    #[test]
    fn a_repeated_codex_call_id_is_one_call() {
        let entry = r#"{"type":"response_item","timestamp":"t","payload":{"type":"function_call","call_id":"c1","name":"shell","arguments":"{\"command\":\"ls\"}"}}"#;
        let chat = sink_codex(&[entry, entry]);
        assert_eq!(chat.turns[0].tools.len(), 1);
    }

    #[test]
    fn a_codex_apply_patch_carries_its_diff() {
        let chat = sink_codex(&[
            r#"{"type":"response_item","timestamp":"t","payload":{"type":"custom_tool_call","call_id":"c1","name":"apply_patch","input":"*** Begin Patch\n*** Update File: src/a.rs\n-old\n+new\n*** End Patch"}}"#,
        ]);
        let tool = &chat.turns[0].tools[0];
        assert!(tool.added >= 1 && tool.removed >= 1, "{tool:?}");
        assert!(!tool.diff.is_empty());
    }

    /// The other half of the merge: everything between two user turns is *not*
    /// one reply, because a tool result means the model was asked again. Without
    /// this bound an afternoon's session comes back as two boxes holding sixty
    /// tool calls each — which is what the first cut of this did.
    /// A reply with extended thinking writes a thinking block into every record
    /// of itself, and its parallel tool calls arrive as separate records too.
    /// The request id is what says they are all one answer.
    #[test]
    fn records_sharing_a_request_id_are_one_reply() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t0","message":{"content":"go"}}"#,
            r#"{"type":"assistant","timestamp":"t1","requestId":"req_1","message":{"content":[{"type":"thinking","thinking":"weighing"},{"type":"text","text":"two at once"}]}}"#,
            r#"{"type":"assistant","timestamp":"t2","requestId":"req_1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a"}}]}}"#,
            r#"{"type":"assistant","timestamp":"t3","requestId":"req_1","message":{"content":[{"type":"tool_use","id":"toolu_2","name":"Read","input":{"file_path":"/b"}}]}}"#,
        ]);
        let kinds: Vec<&str> = chat.turns.iter().map(|t| t.kind.as_ref()).collect();
        assert_eq!(kinds, vec!["message", "reasoning", "message"]);
        assert_eq!(chat.turns[2].text, "two at once");
        assert_eq!(chat.turns[2].tools.len(), 2, "{:?}", chat.turns[2]);
    }

    #[test]
    fn a_tool_result_ends_the_reply_it_came_back_to() {
        let chat = sink_claude(&[
            r#"{"type":"user","timestamp":"t0","message":{"content":"go"}}"#,
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"text","text":"looking"},{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a"}}]}}"#,
            r#"{"type":"user","timestamp":"t2","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"contents"}]}}"#,
            r#"{"type":"assistant","timestamp":"t3","message":{"content":[{"type":"text","text":"now I know"}]}}"#,
        ]);
        let roles: Vec<&str> = chat.turns.iter().map(|t| t.role.as_ref()).collect();
        assert_eq!(roles, vec!["user", "assistant", "assistant"]);
        assert_eq!(chat.turns[1].text, "looking");
        assert_eq!(chat.turns[1].tools.len(), 1);
        assert_eq!(chat.turns[2].text, "now I know");
        assert!(chat.turns[2].tools.is_empty());
    }

    fn sink_devin(steps: &[&str], statuses: &HashMap<String, String>) -> Conversation {
        let mut sink = Sink::default();
        for step in steps {
            sink.devin(&serde_json::from_str(step).unwrap(), statuses);
        }
        sink.finish()
    }

    /// One ATIF step is one model response: the message is the reply text,
    /// `reasoning_content` is the thinking that preceded it.
    #[test]
    fn a_devin_exchange_reads_the_same_way() {
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"user","timestamp":"t1","message":"fix the parser"}"#,
                r#"{"step_id":2,"source":"agent","timestamp":"t2","reasoning_content":"weighing it up","message":"on it"}"#,
            ],
            &HashMap::new(),
        );
        assert!(chat.supported);
        let roles: Vec<(&str, &str)> = chat
            .turns
            .iter()
            .map(|t| (t.role.as_ref(), t.kind.as_ref()))
            .collect();
        assert_eq!(
            roles,
            vec![
                ("user", "message"),
                ("assistant", "reasoning"),
                ("assistant", "message")
            ]
        );
        assert_eq!(chat.turns[0].text, "fix the parser");
        assert_eq!(chat.turns[1].text, "weighing it up");
        assert_eq!(chat.turns[2].text, "on it");
    }

    /// Devin records a call's result on the same step that made it, under
    /// `observation.results` keyed by `source_call_id` — the call and its
    /// outcome still come back as one thing.
    #[test]
    fn a_devin_result_lands_on_the_call_it_made() {
        let chat = sink_devin(
            &[
                r#"{"step_id":3,"source":"agent","timestamp":"t","message":"reading","tool_calls":[{"tool_call_id":"read_1#abc","function_name":"read","arguments":{"file_path":"/a/b.rs"}}],"observation":{"results":[{"source_call_id":"read_1#abc","content":"fn main() {}"}]}}"#,
            ],
            &HashMap::new(),
        );
        assert_eq!(chat.turns.len(), 1);
        let tool = &chat.turns[0].tools[0];
        assert_eq!(tool.name, "read");
        assert_eq!(tool.detail, "/a/b.rs");
        assert_eq!(tool.result.as_deref(), Some("fn main() {}"));
        assert!(!tool.failed);
    }

    /// Two steps are two replies even when neither made a call — the step id
    /// is what stops them being folded into one.
    #[test]
    fn consecutive_devin_steps_stay_separate_replies() {
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"agent","timestamp":"t1","message":"first"}"#,
                r#"{"step_id":2,"source":"agent","timestamp":"t2","message":"second"}"#,
            ],
            &HashMap::new(),
        );
        assert_eq!(chat.turns.len(), 2);
        assert_eq!(chat.turns[0].text, "first");
        assert_eq!(chat.turns[1].text, "second");
    }

    /// The transcript does not say how a call ended; the database's status
    /// map does. A call whose last reported status is not `completed` failed.
    #[test]
    fn a_devin_call_the_database_marked_failed_is_marked() {
        let statuses = HashMap::from([("exec_1#abc".to_string(), "failed".to_string())]);
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"agent","timestamp":"t","tool_calls":[{"tool_call_id":"exec_1#abc","function_name":"exec","arguments":{"command":"cargo test"}}],"observation":{"results":[{"source_call_id":"exec_1#abc","content":"error: no such target"}]}}"#,
            ],
            &statuses,
        );
        let tool = &chat.turns[0].tools[0];
        assert!(tool.failed);
        assert_eq!(tool.result.as_deref(), Some("error: no such target"));
    }

    /// A call still running has an observation pending, so no result at all —
    /// which is what the page shows as in-flight.
    #[test]
    fn a_devin_call_with_no_observation_is_still_running() {
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"agent","timestamp":"t","tool_calls":[{"tool_call_id":"exec_1#abc","function_name":"exec","arguments":{"command":"sleep 60"}}]}"#,
            ],
            &HashMap::new(),
        );
        assert!(chat.turns[0].tools[0].result.is_none());
    }

    /// ATIF has no `structuredPatch`; the patch an `edit` applied is its own
    /// `old_string`/`new_string` arguments.
    #[test]
    fn a_devin_edit_carries_its_diff() {
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"agent","timestamp":"t","tool_calls":[{"tool_call_id":"edit_1#abc","function_name":"edit","arguments":{"file_path":"/a/b.rs","old_string":"old\nlines","new_string":"new\nalso"}}],"observation":{"results":[{"source_call_id":"edit_1#abc","content":"updated"}]}}"#,
            ],
            &HashMap::new(),
        );
        let tool = &chat.turns[0].tools[0];
        assert_eq!((tool.added, tool.removed), (2, 2));
        assert_eq!(tool.diff, vec!["-old", "-lines", "+new", "+also"]);
    }

    /// The system prompt, the rule files and the context blocks re-injected at
    /// every turn are all steps too, but they are the prompt being assembled,
    /// not the conversation. A step the harness injected mid-run — a
    /// backgrounded subagent finishing, the user editing a file — is one.
    #[test]
    fn devin_prompt_assembly_is_not_a_turn() {
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"system","timestamp":"t1","message":"You are Devin…","extra":{"telemetry":{"source":"sysprompt"}}}"#,
                r#"{"step_id":2,"source":"system","timestamp":"t2","message":"<rules>…</rules>","extra":{"telemetry":{"source":"rules"}}}"#,
                r#"{"step_id":3,"source":"system","timestamp":"t3","message":"<available_skills>…</available_skills>","extra":{"telemetry":{"source":"system"}}}"#,
                r#"{"step_id":4,"source":"system","timestamp":"t4","message":"You are powered by SWE-2 High.","extra":{"telemetry":{"source":"system"}}}"#,
                r#"{"step_id":5,"source":"system","timestamp":"t5","message":"<subagent_completion_notification>done</subagent_completion_notification>","extra":{"telemetry":{"source":"system"}}}"#,
            ],
            &HashMap::new(),
        );
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].role, "system");
        // The envelope comes off an event the same as any other harness block.
        assert_eq!(chat.turns[0].text, "done");
    }

    /// The user-edited-a-file block is aimed at the model: a preamble about
    /// when to mention it and a pseudo-diff per file. The turn is the event.
    #[test]
    fn a_devin_user_action_reads_as_the_files_touched() {
        let chat = sink_devin(
            &[
                r#"{"step_id":1,"source":"system","timestamp":"t1","message":"<additional_metadata>\nThe user took the following actions after the last message. ONLY talk about this if it is directly relevant to the user's next request.\n\n<user_actions>\nThe following changes were made by the USER to: /home/flo/src/a.rs.\n[diff_block_start]\n@@ -1 +1 @@\n-old\n+new\n[diff_block_end]\nPlease note that the above snippet only shows the MODIFIED lines.\nThe following changes were made by the USER to: /home/flo/src/b.rs.\n[diff_block_start]\n@@ -2 +2 @@\n-x\n+y\n[diff_block_end]\n</user_actions>\n</additional_metadata>","extra":{"telemetry":{"source":"system"}}}"#,
            ],
            &HashMap::new(),
        );
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(
            chat.turns[0].text,
            "the user edited /home/flo/src/a.rs, /home/flo/src/b.rs"
        );
    }

    /// A transcript that is not the document ATIF says it is reports as
    /// unreadable rather than panicking or looking empty.
    #[test]
    fn a_devin_transcript_that_is_not_json_is_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s1.json");
        std::fs::write(&path, b"not json").unwrap();
        let mut session = Session::new(Provider::Devin, "s1".into());
        session.data_file = Some(path);
        let chat = build(&session, None);
        assert!(!chat.supported);
        assert!(chat.note.is_some_and(|n| n.contains("could not read")));
    }

    /// A turn's `seq` is its place in the whole transcript, not its position
    /// in the window — what `?before=` and the page's turn links count in.
    #[test]
    fn turns_kept_off_the_front_keep_their_sequence_numbers() {
        let mut sink = Sink::default();
        for i in 0..MAX_TURNS + 3 {
            let mut turn = Turn::new("user", "message", "t");
            turn.text = format!("turn {i}");
            sink.push(turn);
        }
        let chat = sink.finish();
        assert_eq!(chat.turns.len(), MAX_TURNS);
        assert_eq!(chat.earlier, 3);
        assert_eq!(chat.turns[0].seq, 3);
        assert_eq!(chat.turns[0].text, "turn 3");
    }

    /// `?before=` ends the window at the boundary rather than the newest
    /// turn, same size and same `earlier` count as the unwindowed one.
    #[test]
    fn a_before_window_ends_where_it_was_asked_to() {
        let mut sink = Sink {
            before: Some(MAX_TURNS + 2),
            ..Sink::default()
        };
        for i in 0..MAX_TURNS + 5 {
            let mut turn = Turn::new("user", "message", "t");
            turn.text = format!("turn {i}");
            sink.push(turn);
        }
        let chat = sink.finish();
        // The three turns past the boundary were numbered — `earlier` and the
        // next window's seqs depend on it — but never kept.
        assert_eq!(chat.turns.len(), MAX_TURNS);
        assert_eq!(chat.earlier, 2);
        assert_eq!(chat.turns[0].seq, 2);
        assert_eq!(chat.turns.last().unwrap().seq, MAX_TURNS + 1);
    }

    /// A result can be written entries after the window's boundary; the call
    /// it answers still has to show as finished.
    #[test]
    fn a_result_past_the_boundary_still_lands_on_its_call() {
        let mut sink = Sink {
            before: Some(1),
            ..Sink::default()
        };
        for line in [
            r#"{"type":"assistant","timestamp":"t1","message":{"content":[{"type":"tool_use","id":"toolu_1","name":"Read","input":{"file_path":"/a"}}]}}"#,
            // Turn 1 — past the boundary, numbered but not kept.
            r#"{"type":"user","timestamp":"t2","message":{"content":"next question"}}"#,
            // The call's result, recorded last of all.
            r#"{"type":"user","timestamp":"t3","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"done"}]}}"#,
        ] {
            sink.claude(&serde_json::from_str(line).unwrap());
        }
        let chat = sink.finish();
        assert_eq!(chat.turns.len(), 1);
        assert_eq!(chat.turns[0].seq, 0);
        assert_eq!(chat.turns[0].tools[0].result.as_deref(), Some("done"));
    }
}
