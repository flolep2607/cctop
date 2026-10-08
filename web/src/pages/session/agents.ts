import { createContext, useContext } from "react";
import type { AgentCall, Turn } from "@/lib/types";

// What a turn needs to know about the conversation's subagents to link to them:
// which turn started each one, and when each reported back. Shared through
// context rather than props because the turns are memoised one by one, and a
// hand-back's link depends on a call several turns above it.
export interface Agents {
  /** The session the agents belong to, for reading an agent's own turns. */
  session: string;
  /** agent id → the call that started it and the turn it is in. */
  calls: Map<string, { seq: number; agent: AgentCall }>;
  /** hand-back seq → its time, for "reported back at". */
  reported: Map<number, string>;
  /** Whether agent hand-backs are drawn in the timeline at all. */
  everything: boolean;
  /** Walk to a main-conversation turn and mark it. */
  jump: (seq: number) => void;
}

export const AgentsContext = createContext<Agents>({
  session: "",
  calls: new Map(),
  reported: new Map(),
  everything: true,
  jump: () => {},
});

export const useAgents = () => useContext(AgentsContext);

/** The agents a run of turns started, keyed by agent id. */
export function agentCalls(turns: Turn[]): Agents["calls"] {
  const out: Agents["calls"] = new Map();
  for (const turn of turns)
    for (const tool of turn.tools ?? []) if (tool.agent) out.set(tool.agent.id, { seq: turn.seq, agent: tool.agent });
  return out;
}

/** `Explore — map the parser`. */
export const agentTitle = (a: AgentCall) => (a.description ? `${a.type} — ${a.description}` : a.type);

/** Whether a turn is about a subagent rather than said in the main conversation. */
export const aboutAgent = (t: Turn) => t.kind === "agent-message" || (t.role === "system" && !!t.agent);
