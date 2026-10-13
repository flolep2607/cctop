import { createContext } from "react";

/**
 * What the session page's event stream says about the conversation: the
 * server's mark for it, which changes when the transcript (or a subagent's)
 * may have, and whether the stream is getting through at all.
 *
 * A context rather than props because the readers are a conversation and the
 * subagent conversations nested inside its turns, several components down.
 * Outside a session page there is no stream, which reads as "not live" — and a
 * conversation that is not hearing from a stream polls instead.
 */
export const ChatSignal = createContext<{ mark: string | null; live: boolean }>({ mark: null, live: false });
