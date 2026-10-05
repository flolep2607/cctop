// The shapes cctop's routes answer with — as much of them as the app reads.

export type State = "working" | "waiting" | "asking" | "error" | "idle" | string;

export interface Session {
  session_id: string;
  provider: string;
  harness?: string;
  state: State;
  running: boolean;
  project?: string;
  title?: string | null;
  model?: string | null;
  branch?: string | null;
  profile?: string | null;
  last_active?: string;
  started_at?: string;
  asking_for?: string | null;
  cost?: { available: boolean; included: boolean; total: string | number | null; today?: number; this_hour?: number };
  tokens?: { input: number; output: number; total: number };
  context?: { used: number; max: number; compacted?: boolean } | null;
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
}

export interface Turn {
  seq: number;
  role: "user" | "assistant" | "system" | string;
  kind: "message" | "reasoning" | "compaction" | string;
  text?: string;
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
