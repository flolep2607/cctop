// The shapes cctop's routes answer with. Generated from the Rust structs that
// serialise them (`web/src/lib/gen/`, written by `ts_types` in cctop-serve), so
// a field the server renames or makes optional fails `tsc` here rather than
// reading as `undefined` on a page. Only the names the app uses are below.

export type { JsonSession as Session } from "./gen/JsonSession";
export type { Tab } from "./gen/Tab";
export type { ToolUse as Tool } from "./gen/ToolUse";
export type { AgentCall } from "./gen/AgentCall";
export type { Turn } from "./gen/Turn";
export type { Conversation as Chat } from "./gen/Conversation";
export type { Report } from "./gen/Report";
export type { Access } from "./gen/Access";
export type { FileRef } from "./gen/FileRef";
export type { Analytics } from "./gen/Analytics";
export type { AnalyticsSession } from "./gen/AnalyticsSession";
export type { ProviderStatus } from "./gen/ProviderStatus";
export type { PageDocument } from "./gen/PageDocument";
export type { SearchHit } from "./gen/SearchHit";
export type { Address } from "./gen/Address";
export type { AddressDone } from "./gen/AddressDone";

/** A session's state, as `JsonSession.state` carries it — open-ended, since a newer server may add one. */
export type State = "working" | "waiting" | "asking" | "error" | "idle" | (string & {});
