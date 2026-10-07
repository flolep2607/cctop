import { withToken } from "./config";

// What went wrong, in words worth showing. A cctop error is short text/plain;
// anything else came from something in between — a tunnel's HTML error page —
// and must not land on screen verbatim.
async function problem(response: Response): Promise<string> {
  const kind = (response.headers.get("content-type") || "").split(";")[0].trim();
  const said = kind === "text/plain" ? (await response.text()).trim() : "";
  if (said) return said.length > 400 ? said.slice(0, 400) + "…" : said;
  if (response.status >= 502 && response.status <= 504)
    return "cctop is not answering. The link is up but nothing is behind it — the terminal it runs in may have stopped.";
  if (response.status === 401 || response.status === 403)
    return "This link is no longer authorised. Open a fresh one from the terminal.";
  return "The server answered " + response.status + (response.statusText ? " " + response.statusText : "") + ".";
}

/** A refusal from the server: the sentence to show, and the status it came with. */
export class HttpError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

/** Every request the app makes: the token added, failures turned into sentences. */
export async function ask(path: string, init?: RequestInit, extra?: Record<string, string | number | null | undefined>): Promise<Response> {
  let response: Response;
  try {
    response = await fetch(withToken(path, extra), init);
  } catch {
    throw new Error("cctop is unreachable — the connection dropped.");
  }
  if (!response.ok) throw new HttpError(response.status, await problem(response));
  return response;
}

/** The body as JSON — a 200 from a captive portal is not cctop's JSON. */
export async function getJson<T>(path: string, extra?: Record<string, string | number | null | undefined>): Promise<T> {
  const text = await (await ask(path, undefined, extra)).text();
  try {
    return JSON.parse(text) as T;
  } catch {
    throw new Error("Whatever answered this page, it was not cctop.");
  }
}

/**
 * An action on a session: a POST with a JSON body. The token authorises it; the
 * content type is what stops another origin's page from sending one.
 */
export async function act(verb: string, sessionId: string, body: object = {}): Promise<{ message?: string; [k: string]: unknown }> {
  const response = await ask("/api/act/" + verb + "/" + encodeURIComponent(sessionId), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  const text = (await response.text()).trim();
  try {
    return JSON.parse(text);
  } catch {
    return { message: text };
  }
}

// The harnesses whose permission menus the server knows the keys of (see
// `actions::answer`): Allow presses the first option, Deny presses Esc. For
// any other, a guessed key could mean the opposite, so no button is offered.
export const ANSWERABLE = new Set(["claude", "codex"]);

export async function answerPrompt(sessionId: string, choice: "allow" | "deny"): Promise<string> {
  const done = await act("answer", sessionId, { choice });
  return done.message || (choice === "allow" ? "Allowed" : "Denied");
}

/**
 * Whether an answer was refused because the session had already stopped
 * asking (`actions::answer`'s 409). The server's table is current when it says
 * that, so a page still showing the prompt is the one behind — the caller drops
 * it rather than leave buttons up that can only be refused again.
 */
export function isNotAsking(e: unknown): boolean {
  return e instanceof HttpError && e.status === 409 && /not asking/.test(e.message);
}

/**
 * Switch YOLO for a session: every permission prompt it raises allowed by
 * cctop, until switched off or the session ends (`actions::yolo`). The server
 * answers the prompts — the page never presses Allow on YOLO's behalf, which
 * would be a second press of the same prompt.
 */
export async function setYolo(sessionId: string, on: boolean): Promise<string> {
  const done = await act("yolo", sessionId, { on });
  return done.message || (on ? "YOLO on" : "YOLO off");
}
