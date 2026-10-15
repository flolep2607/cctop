# Telemetry

cctop sends a small usage report at most once a day to
`https://cctop-releases.ecorsiste.workers.dev/telemetry`. It is on by default,
and nothing is sent until a one-time notice has been shown on an interactive
run. `cctop hook` and `cctop yolo-hook` never send anything. A report holds
only:

- `id`: 128 random bits, made at the first report; `cctop telemetry reset-id`
  replaces it.
- `version`, `os`, `arch`: cctop's version, and e.g. `linux`, `x86_64`.
- `features`: how many times each of `tui`, `serve`, `workspace`, `tunnel`,
  `yolo` and `update` ran since the last report.
- `agents`: the most agents of each kind seen running at once.
- `crash`, only if cctop panicked: the file and line in cctop's source, and
  the panic message with paths, URLs, emails, user and host names, IPs and long
  tokens removed, cut to 200 characters.

It never sends prompts, session names, working directories, account names,
hostnames or IP addresses. `cctop telemetry status` shows the next report.
To turn it off, any one of: `cctop telemetry off` (also deletes the ID and the
counts), `telemetry = false` under `[settings]` in `config.toml` or the toggle
in settings (`,`), or `CCTOP_TELEMETRY=0` / `DO_NOT_TRACK=1`.
