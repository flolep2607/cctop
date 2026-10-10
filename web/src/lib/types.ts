// The shapes cctop's routes answer with — as much of them as the app reads.

export type State = "working" | "waiting" | "asking" | "error" | "idle" | string;

export interface Session {
  session_id: string;
  /** The machine a remote row is on: an ssh `--host`, or a sibling on the Cloudflare account. Absent for this machine's rows. */
  host?: string | null;
  provider: string;
  harness?: string;
  state: State;
  running: boolean;
  project?: string;
  title?: string | null;
  model?: string | null;
  branch?: string | null;
  profile?: string | null;
  /** Launched by `cctop sandbox`: running here, working on `host`'s `path`. */
  sandbox?: { host: string; path: string } | null;
  last_active?: string;
  started_at?: string;
  asking_for?: string | null;
  /** The held prompt is a question with choices, not a permission prompt. */
  asking_question?: boolean;
  /** cctop is allowing every permission prompt this session raises (`crates/core/src/yolo.rs`): since when, and what it has allowed. */
  yolo?: { since: string; allowed: { at: string; ask: string }[] } | null;
  cost?: { available: boolean; included: boolean; total: string | number | null; today?: number; this_hour?: number; last_hour?: number };
  tokens?: { input: number; output: number; total: number };
  context?: { used: number; max: number; compacted?: boolean } | null;
  activity?: { tool_count: number; tool_errors: number };
  conflict?: { level: "file" | "repo" | string } | null;
  user?: string | null;
}

export interface Tab {
  name: string;
  label: string;
  cwd?: string | null;
  state: "working" | "needs-input" | "idle" | string;
  session_id?: string | null;
  tab?: string | null;
}

export interface Tool {
  name: string;
  detail?: string;
  full?: string;
  result?: string | null;
  failed?: boolean;
  added?: number;
  removed?: number;
  diff?: string[];
  /** The harness's id for the call. */
  id?: string;
  /** For a call that started a subagent, the subagent it started. */
  agent?: AgentCall;
}

/** The subagent an `Agent` call started — `AgentCall` in `chat.rs`. */
export interface AgentCall {
  /** What `/api/chat/<id>?agent=` takes. */
  id: string;
  type: string;
  description: string;
  status: "running" | "done" | "failed" | string;
  started_at?: string;
  last_active?: string;
  duration_ms?: number;
  tool_count?: number;
  /** Replies in its own transcript. */
  turns?: number;
  /** Its own requests, in dollars at list price; "incl" on a plan that bundles them. */
  cost?: number;
  /** Every token billed to it. */
  tokens?: number;
  /** Its transcript was purged. */
  ghost?: boolean;
  /** Launched in the background: the call's result is only a launch receipt. */
  background?: boolean;
  /** The seq of its hand-back turn. */
  handback?: number;
  report?: string;
  /** `report` is only its last message: a background agent that never handed back. */
  last_message?: boolean;
}

export interface Turn {
  seq: number;
  role: "user" | "assistant" | "system" | string;
  kind: "message" | "reasoning" | "compaction" | "agent-message" | string;
  text?: string;
  /** An agent-message's sender, by type (`general-purpose`). */
  from?: string;
  /** An agent-message's sender id, which names its subagent. */
  agent?: string;
  ts?: string;
  clipped?: boolean;
  tools?: Tool[];
}

export interface Chat {
  supported: boolean;
  note?: string;
  turns?: Turn[];
  earlier?: number;
  stamp?: string;
  unchanged?: boolean;
}

/** The session report. Loosely typed below the top: it is read, not built. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type Report = Record<string, any>;
