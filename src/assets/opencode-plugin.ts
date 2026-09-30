// Written by cctop, which watches coding agents. Safe to delete: removing
// this file is all it takes to stop reporting.
//
// Reports the moments a transcript cannot show — a turn finishing, a session
// starting or ending — to whatever cctop is running. Every failure is
// swallowed on purpose: this runs inside OpenCode, and a monitor must never be
// the reason a session breaks.
const CCTOP = "$CCTOP"

import { spawn } from "node:child_process"

// One child per moment, left to run on its own: nothing waits for it, so the
// agent's own loop is never blocked on a monitor.
const report = (type, sessionID, directory, extra) => {
  try {
    if (!sessionID) return
    const payload = JSON.stringify({ type, sessionID, directory: directory ?? "", ...(extra ?? {}) })
    spawn(CCTOP, ["hook", "opencode", payload], {
      stdio: "ignore",
      detached: true,
    }).unref()
  } catch {
    // A monitor is never worth an exception in somebody else's agent.
  }
}


// ---------------------------------------------------------------------------
// OpenCode 1: a plugin is a function that returns the hooks it wants
// ---------------------------------------------------------------------------

// Only the events cctop has something to say about. Anything else is ignored
// here rather than spawning a process to be dropped at the other end.
const REPORTED = new Set(["session.idle","session.created","session.deleted","session.compacted","permission.asked","tool.execute.before","tool.execute.after"])

const cctopV1 = async ({ directory, worktree }) => {
  return {
    event: async ({ event }) => {
      try {
        const type = event?.type
        if (!type || !REPORTED.has(type)) return
        const props = event.properties ?? {}
        const sessionID = props.sessionID ?? props.info?.id ?? props.sessionId
        report(type, sessionID, directory ?? worktree ?? "")
      } catch {
        // A monitor is never worth an exception in somebody else's agent.
      }
    },
  }
}


// The named export is what 1.x has always loaded, and the only thing 1.14 knows.
// 1.18.29 and newer prefer `server` below, which is this same function.
export const cctop = cctopV1

// ---------------------------------------------------------------------------
// OpenCode 2: a default export with an `id` and a `setup`
// ---------------------------------------------------------------------------

// The same four, from the stream. The asking is an API and the tool moments are
// hooks; both are below.
const STREAMED = new Set(["session.idle","session.created","session.deleted","session.compacted"])

// How often to ask the server what is waiting on the user, and how long a
// session that has said nothing is still worth asking about. A second is the gap
// between a prompt going up and cctop saying so, which is the same gap as any
// other harness's hook. Two minutes of quiet is a session nobody is waiting on
// — unless something is pending, which outlasts any amount of quiet.
const POLL_MS = 1000
const FORGET_MS = 120000

// What a pending request is asking to do, in the shape `cctop hook` reads a
// permission prompt in. A question the agent wrote out is passed on as it was
// said; anything else is the action and the resource, under the key that action
// uses, because that is the field cctop looks in.
const askOf = (request) => {
  const action = typeof request?.action === "string" ? request.action : ""
  const said = typeof request?.message === "string" ? request.message : ""
  const resources = Array.isArray(request?.resources) ? request.resources : []
  const resource = resources.find((one) => typeof one === "string" && one !== "*")
  if (said) return { tool_name: action, message: said }
  const key =
    action === "shell"
      ? "command"
      : action === "webfetch"
        ? "url"
        : action === "websearch"
          ? "query"
          : action === "grep"
            ? "pattern"
            : "file_path"
  return { tool_name: action, tool_input: resource ? { [key]: resource } : {} }
}

const setup = async (ctx) => {
  // An OpenCode 1 that knows this shape hands over a context with none of it —
  // 1.18.29 and newer call setup() as well as server() — so half of this file is
  // dead code on the other version, and dead code must not throw. Everything is
  // optional and the whole body is guarded for that reason alone.
  try {
    const here = ctx?.location?.directory ?? ""
    // Every session this project has been heard from, and when — the poll asks
    // about those and nothing else, so a quiet project costs nothing at all.
    const seen = new Map()
    // Which requests have already been reported, so a prompt is said once and
    // an answer is said once.
    const waiting = new Map()

    // The subscription is the whole of the event side, and the AbortController
    // is what stops it: a plugin that outlives its OpenCode would keep spawning
    // children for a server that is no longer there.
    const controller = new AbortController()
    void (async () => {
      try {
        for await (const event of ctx.event?.subscribe?.({ signal: controller.signal }) ?? []) {
          try {
            const props = event?.data ?? {}
            const sessionID = props.sessionID ?? props.info?.id
            if (sessionID) seen.set(sessionID, Date.now())
            const type = event?.type
            if (!type || !STREAMED.has(type)) continue
            report(type, sessionID, event.location?.directory ?? here)
          } catch {
            // A monitor is never worth an exception in somebody else's agent.
          }
        }
      } catch {
        // Nor is a dropped stream: the hooks and the poll below still report.
      }
    })()

    // What is waiting, which is the one thing the stream does not say. Read
    // rather than inferred, so a permission prompt and an agent's question are
    // both the fact they are — 2 counts a question as a request whose action is
    // `question`, so one read covers both — and neither has to be recognised
    // from a line of text on somebody's screen.
    //
    // ponytail: a session is known only once it has made a sound, because
    // OpenCode 2 will not list its sessions to a plugin and the stream does not
    // replay. A prompt that was already up when this file was written is
    // therefore missed until the session next does something. That is the one
    // case the screen behind cctop covers and this does not, and a prompt raised
    // after startup — which is every prompt in normal use — is caught within a
    // second of the tool call that raised it.
    const poll = async () => {
      try {
        if (typeof ctx.permission?.list !== "function") return
        const now = Date.now()
        for (const [sessionID, at] of seen) {
          const pending0 = waiting.get(sessionID)
          // Quiet for two minutes is a session nobody is waiting on, and it
          // stops being asked about. Quiet with something pending is the
          // opposite: a prompt left up is still up however long ago it went up,
          // and cctop's claim that this session is blocked has to keep being
          // true — including when the answer arrives in the agent's own terminal
          // rather than in cctop, which is the usual way it arrives.
          if (now - at > FORGET_MS && !pending0?.size) {
            seen.delete(sessionID)
            waiting.delete(sessionID)
            continue
          }
          let pending
          try {
            pending = await ctx.permission.list({ sessionID })
          } catch {
            continue
          }
          const requests = (Array.isArray(pending) ? pending : []).filter((one) => one?.id)
          const ids = new Set(requests.map((one) => one.id))
          const told = waiting.get(sessionID)
          const same = told && told.size === ids.size && [...ids].every((id) => told.has(id))
          if (requests.length && !same) {
            for (const request of requests) {
              report("permission.asked", sessionID, here, askOf(request))
            }
            waiting.set(sessionID, ids)
          } else if (!requests.length && told?.size) {
            // The prompt was answered. Nothing in OpenCode says so, and without
            // this the tab keeps asking about a question that is already behind
            // you until the tool call that follows, or the end of the turn.
            report("permission.replied", sessionID, here)
            waiting.set(sessionID, ids)
          }
        }
      } catch {
        // A failed read is a poll that did not happen.
      }
    }
    const timer = setInterval(() => void poll(), POLL_MS)
    if (typeof timer?.unref === "function") timer.unref()

    // A tool call starting is the same moment 1 delivered as an event, and the
    // call coming back is the same moment it delivered as another. Reported in
    // 1's spelling so that everything downstream of here reads one vocabulary.
    if (typeof ctx.tool?.hook === "function") {
      await ctx.tool.hook("execute.before", (event) => {
        if (event?.sessionID) seen.set(event.sessionID, Date.now())
        report("tool.execute.before", event?.sessionID, here)
      })
      await ctx.tool.hook("execute.after", (event) => {
        if (event?.sessionID) seen.set(event.sessionID, Date.now())
        report("tool.execute.after", event?.sessionID, here)
      })
    }

    return () => {
      clearInterval(timer)
      controller.abort()
    }
  } catch {
    // A monitor is never worth an exception in somebody else's agent.
    return () => {}
  }
}


export default { id: "cctop", setup, server: cctopV1 }
