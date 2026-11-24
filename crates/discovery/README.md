# cctop-discovery

The coding agents running on this machine — Claude Code, Codex, OpenCode,
Gemini and others — as [cctop](https://github.com/flolep2607/cctop) finds them.

This crate is being split out of `cctop-core` in steps, so that panel applets,
status bars and notifiers can reuse cctop's agent detection without its
terminal, web, TLS and multiplexer dependencies. So far it holds the shared
types (`Provider`, `Surface`, `ActivityState`, `Permission`); the session
parsers and the process match follow.

It is 0.x: expect breaking changes as minor bumps.
