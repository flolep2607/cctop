//! cctop's own rmux daemon, built into the cctop binary (#197).
//!
//! Every agent cctop keeps alive lives in an rmux daemon, and that daemon is
//! this one: the rmux server linked into this binary, run as `cctop mux
//! daemon`, on a socket under cctop's own runtime directory, which nothing of
//! the user's knows about. No `rmux` has to be installed, and an `rmux` the
//! user does run never sees cctop's agents, never applies its config to them,
//! and ends none of them with its `kill-server`.
//!
//! What keeps the two apart is that the socket is computed here and nowhere
//! else, and from nothing the user's rmux reads: not `$RMUX`, `$TMUX`,
//! `RMUX_TMPDIR`, `TMUX_TMPDIR` or any `RMUX_SDK_*` variable. A cctop started
//! inside one of the user's rmux panes therefore still reaches its own daemon.
//!
//! The cost of that, said plainly because it lands on upgrade: cctop up to
//! 0.31 kept its agents in the user's own rmux daemon, as `cctop-*` sessions.
//! Those are still running there after an update, and this cctop does not see
//! them — it never lists, attaches to or touches a session of another daemon.
//! `rmux attach -t cctop-…` reaches one by hand, and `cctop doctor` says so.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rmux_proto::{
    CapturePaneRequest, CopyModeRequest, DisplayMessageRequest, HasSessionRequest,
    KillSessionRequest, ListClientsRequest, ListPanesRequest, ListSessionsRequest,
    NewSessionExtRequest, OptionScopeSelector, PaneTarget, Request, Response, SendKeysExtRequest,
    SessionName, SetOptionByNameRequest, SetOptionMode, Target,
};

/// The longest path a unix socket address holds: `sun_path` is 108 bytes on
/// Linux, one of which is the terminating NUL.
const SUN_PATH_MAX: usize = 107;

/// Where cctop's daemon keeps its socket and the config it starts with.
///
/// Under `mux/` so the shim's sweep of `<pid>.sock` files in the directory
/// above never looks at it (see `shim::socket_dir`), and per wire version so a
/// cctop that speaks a newer protocol than the daemon left running by an older
/// one starts its own beside it rather than talking past it.
pub fn dir() -> PathBuf {
    crate::config::runtime_base()
        .join("cctop")
        .join("mux")
        .join(format!("w{}", rmux_proto::RMUX_WIRE_VERSION))
}

/// The socket of cctop's daemon, or why there cannot be one.
///
/// The one function that decides it: every connection in this module, the
/// SDK's included (`rmux::endpoint`), is made to this path and to no other.
pub fn socket() -> Result<PathBuf, String> {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(socket) = TEST_SOCKET.with(|socket| socket.borrow().clone()) {
        return Ok(socket);
    }
    checked(dir().join("default"))
}

/// A socket path the kernel can bind, or a message saying which one could not.
fn checked(path: PathBuf) -> Result<PathBuf, String> {
    let length = path.as_os_str().len();
    if length > SUN_PATH_MAX {
        return Err(format!(
            "cctop's rmux socket would be {} ({length} bytes), longer than the {SUN_PATH_MAX} a \
             unix socket path can hold; set XDG_RUNTIME_DIR to a shorter directory",
            path.display()
        ));
    }
    Ok(path)
}

/// How many lines of an agent's output a pane keeps.
///
/// rmux reports no `history-limit` until one is set, so what a pane keeps
/// unasked is rmux's business and not something to rely on — tmux's answer was
/// 2000, which is a few minutes of a working agent: a pane you can scroll but
/// not scroll *back* to anything. These lines cost nothing until they exist and
/// are gone with the session.
pub const HISTORY_LINES: &str = "50000";

/// The options every session in cctop's daemon starts with, as the config the
/// daemon loads at startup.
///
/// Defaults of the daemon rather than options set on each session after the
/// fact, because the daemon is cctop's to configure and because one of them
/// cannot be set after the fact: a pane's scrollback is allocated when the
/// pane is *made* and never resized, so `history-limit` has to be in force
/// before the agent's pane exists.
///
/// - `history-limit`: [`HISTORY_LINES`].
/// - `mouse on`: rmux is on the alternate screen, so the scrollback of the
///   terminal cctop runs in holds none of the agent's output, and the history
///   that does is reachable only from copy-mode. With the mouse on, the wheel
///   enters copy-mode and scrolls, as it does in a terminal with no rmux.
/// - `status off`: a cctop pane already has a border with the agent's name on
///   it, and rmux's bar carries a clock that repaints every `status-interval`.
///   A pane's fallback idleness test is "has the screen stopped changing", so a
///   ticking clock made every abandoned agent read as busy forever.
/// - `window-size latest`: several cctops may hold a client on one session —
///   that is what sharing tabs means — and at rmux's default the window is
///   sized to the smallest of them. `latest` fits whichever is being used.
/// - `allow-passthrough on`: a harness wraps its desktop notification (OSC 9)
///   in rmux's passthrough sequence, which rmux swallows unless told
///   otherwise, and cctop listens for it in the pane's parser.
/// - `set-clipboard on`: at `external`, clipboard writes from inside a pane are
///   dropped, so a copy in Claude Code went nowhere.
fn defaults() -> String {
    format!(
        "\
set-option -g history-limit {HISTORY_LINES}
set-option -g mouse on
set-option -g status off
set-option -g window-size latest
set-option -g allow-passthrough on
set-option -s set-clipboard on
"
    )
}

/// How long cctop waits for its daemon to come up, or to answer.
const DEADLINE: Duration = Duration::from_secs(30);

/// Run the daemon on `socket` in this process until its last session ends.
///
/// What `cctop mux daemon` runs. The config is written here, by the binary that
/// is about to read it, so a daemon never starts with options from a different
/// cctop than its own.
pub fn serve(socket: &Path) -> Result<(), String> {
    let socket = checked(socket.to_path_buf())?;
    let parent = socket
        .parent()
        .ok_or_else(|| format!("{} has no directory", socket.display()))?;
    let config = parent.join("cctop.conf");
    std::fs::write(&config, defaults())
        .map_err(|e| format!("could not write {}: {e}", config.display()))?;
    let config = rmux_server::DaemonConfig::new(socket)
        // Only cctop's own file: never `~/.rmux.conf` or `~/.tmux.conf`, which
        // belong to the user's own sessions.
        .with_config_files(vec![config], true, None)
        // The shim it would put on a pane's PATH runs an `rmux` binary, which
        // this daemon does not have.
        .without_tmux_shim();
    // The runtime upstream's `rmux-daemon` builds: one worker, and a stack
    // big enough for the command queue's large futures in a debug build.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .thread_stack_size(8 * 1024 * 1024)
        .max_blocking_threads(128)
        .thread_keep_alive(Duration::from_secs(2))
        .enable_io()
        .enable_time()
        .build()
        .map_err(|e| format!("could not start the daemon's runtime: {e}"))?;
    runtime
        .block_on(async move {
            let server = rmux_server::ServerDaemon::new(config).bind().await?;
            server.wait().await
        })
        .map_err(|e| format!("cctop's rmux daemon: {e}"))
}

/// The variables that would point a process at some other multiplexer.
///
/// Stripped from the daemon's environment, so a cctop started inside the user's
/// rmux or tmux does not hand that address down to every agent's pane. The
/// daemon sets `TMUX` for its own panes to its own socket, as tmux does.
const FOREIGN: &[&str] = &[
    "TMUX",
    "TMUX_PANE",
    "TMUX_TMPDIR",
    "RMUX",
    "RMUX_PANE",
    "RMUX_TMPDIR",
];

/// Whether `var` is one of the variables a pane must not inherit from cctop.
fn foreign(var: &std::ffi::OsStr) -> bool {
    var.to_str()
        .is_some_and(|v| FOREIGN.contains(&v) || v.starts_with("RMUX_SDK_"))
}

/// Start cctop's daemon unless one is answering already.
///
/// Only the paths that create a session call this: a tab being launched, a
/// detached start, the web launcher. Everything that only asks questions uses
/// [`ask`], which never starts anything and reads "no daemon" as "no sessions".
///
/// The daemon is this very binary, `cctop mux daemon`, detached into a session
/// of its own so that quitting cctop, or closing the terminal it ran in, leaves
/// the agents running. `/proc/self/exe` rather than a path, so a cctop whose
/// binary was replaced by `--update` still starts the daemon it speaks to.
///
/// ponytail: the daemon stays this process's child, and the SDK does not wait
/// on it, so one that exits (with its last session) while this cctop runs is a
/// zombie until cctop exits: a process-table entry for each daemon that ended
/// while this cctop ran, and nothing else.
pub fn start() -> Result<(), String> {
    let socket = socket()?;
    #[cfg(any(test, feature = "test-support"))]
    if TEST_SOCKET.with(|s| s.borrow().is_some()) {
        return TestDaemon::ensure(&socket);
    }
    #[cfg(any(test, feature = "test-support"))]
    if in_test_harness() {
        return Err("a test that starts an agent needs a mux::TestDaemon".into());
    }
    let mut daemon = std::process::Command::new("/proc/self/exe");
    // Named for `ps` as what it is, rather than as the path it was run by.
    std::os::unix::process::CommandExt::arg0(&mut daemon, "cctop");
    daemon.args(["mux", "daemon", "--socket"]).arg(&socket);
    for (var, _) in std::env::vars_os() {
        if foreign(&var) {
            daemon.env_remove(var);
        }
    }
    daemon
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("could not start a runtime for rmux: {e}"))?;
    runtime.block_on(async move {
        rmux_sdk::Rmux::builder()
            .unix_socket(socket)
            .default_timeout(DEADLINE)
            .connect_or_start_with(daemon)
            .await
            .map(drop)
            .map_err(|e| format!("could not start cctop's rmux daemon: {e}"))
    })
}

/// A connection to cctop's daemon, or `None` when it is not running.
fn connect() -> Option<rmux_client::Connection> {
    let socket = socket().ok()?;
    match rmux_client::connect_or_absent(&socket) {
        Ok(rmux_client::ConnectResult::Connected(connection)) => Some(connection),
        _ => None,
    }
}

/// One request to cctop's daemon, its error turned into a message.
///
/// Never starts the daemon: a question asked of no daemon is answered by the
/// caller as "nothing there".
pub fn ask(request: Request) -> Result<Response, String> {
    let mut connection = connect().ok_or("cctop's rmux daemon is not running")?;
    exchange(&mut connection, &request)
}

fn exchange(
    connection: &mut rmux_client::Connection,
    request: &Request,
) -> Result<Response, String> {
    match connection.roundtrip(request) {
        Ok(Response::Error(error)) => Err(error.error.to_string()),
        Ok(response) => Ok(response),
        Err(error) => Err(error.to_string()),
    }
}

/// What a command printed, as the `rmux` command line would have printed it.
///
/// The formats cctop asks for are rendered by the daemon, so the text is the
/// same as the shell-out's stdout and the parsing in [`crate::rmux`] serves
/// both.
fn printed(response: &Response) -> String {
    response
        .command_output()
        .map(|output| String::from_utf8_lossy(output.stdout()).into_owned())
        .unwrap_or_default()
}

fn session(name: &str) -> Result<SessionName, String> {
    SessionName::new(name.to_string()).map_err(|e| e.to_string())
}

/// Whether a session by exactly this name is alive.
pub fn has_session(name: &str) -> bool {
    let Ok(target) = session(name) else {
        return false;
    };
    matches!(
        ask(Request::HasSession(HasSessionRequest { target })),
        Ok(Response::HasSession(found)) if found.exists
    )
}

/// Create a detached session running `argv`, starting the daemon if need be.
pub fn new_session(
    name: &str,
    cwd: Option<&Path>,
    environment: Vec<String>,
    argv: &[String],
) -> Result<(), String> {
    start()?;
    let request = NewSessionExtRequest {
        session_name: Some(session(name)?),
        working_directory: cwd
            .filter(|d| d.is_dir())
            .map(|d| d.to_string_lossy().into_owned()),
        detached: true,
        size: None,
        environment: (!environment.is_empty()).then_some(environment),
        group_target: None,
        attach_if_exists: false,
        detach_other_clients: false,
        kill_other_clients: false,
        flags: None,
        window_name: None,
        print_session_info: false,
        print_format: None,
        // The form `rmux new-session -- argv` sends, so an agent's argv is read
        // exactly as it was through the command line.
        command: (!argv.is_empty()).then(|| argv.to_vec()),
        process_command: None,
        // None, as the `rmux` command line sends on unix: the pane's
        // environment is the daemon's, plus `environment` above.
        client_environment: None,
        skip_environment_update: false,
    };
    ask(Request::NewSessionExt(Box::new(request))).map(drop)
}

/// End a session, and the agent in it.
pub fn kill_session(name: &str) -> Result<(), String> {
    ask(Request::KillSession(KillSessionRequest {
        target: session(name)?,
        kill_all_except_target: false,
        clear_alerts: false,
        kill_group: false,
    }))
    .map(drop)
}

/// Set a session option — one of the `@cctop_*` records, usually.
pub fn set_session_option(name: &str, option: &str, value: &str) -> Result<(), String> {
    ask(Request::SetOptionByName(Box::new(SetOptionByNameRequest {
        scope: OptionScopeSelector::Session(session(name)?),
        name: option.to_string(),
        value: Some(value.to_string()),
        mode: SetOptionMode::Replace,
        only_if_unset: false,
        unset: false,
        unset_pane_overrides: false,
        format: false,
        format_target: None,
    })))
    .map(drop)
}

/// The panes of one session, rendered through `format`, one line each.
pub fn list_panes(name: &str, format: &str) -> Option<String> {
    let mut connection = connect()?;
    panes_of(&mut connection, session(name).ok()?, format)
}

fn panes_of(
    connection: &mut rmux_client::Connection,
    target: SessionName,
    format: &str,
) -> Option<String> {
    let request = Request::ListPanes(Box::new(ListPanesRequest {
        target,
        target_window_index: None,
        format: Some(format.to_string()),
        filter: None,
        sort_order: None,
        reversed: false,
    }));
    exchange(connection, &request).ok().map(|r| printed(&r))
}

/// Every pane of every session, rendered through `format` — what
/// `list-panes -a -F` prints. `None` when no daemon is running.
///
/// The protocol lists panes per session, so this is one listing of sessions and
/// one of panes for each, over a single connection. A session that ends between
/// the two is skipped rather than failing the lot.
pub fn list_all_panes(format: &str) -> Option<String> {
    let mut connection = connect()?;
    let sessions = exchange(
        &mut connection,
        &Request::ListSessions(ListSessionsRequest {
            format: Some("#{session_name}".to_string()),
            filter: None,
            sort_order: None,
            reversed: false,
        }),
    )
    .ok()?;
    let mut out = String::new();
    for name in printed(&sessions).lines() {
        let Ok(target) = session(name) else { continue };
        if let Some(panes) = panes_of(&mut connection, target, format) {
            out.push_str(&panes);
        }
    }
    Some(out)
}

/// A session's first pane: the one cctop started its agent in.
fn first_pane(connection: &mut rmux_client::Connection, name: &str) -> Option<PaneTarget> {
    let listing = panes_of(
        connection,
        session(name).ok()?,
        "#{session_name}:#{window_index}.#{pane_index}",
    )?;
    pane_target(listing.lines().next()?)
}

/// A pane named the way [`list_all_panes`] can print it, `session:window.pane`.
pub fn pane_target(text: &str) -> Option<PaneTarget> {
    match Target::parse(text.trim()).ok()? {
        Target::Pane(pane) => Some(pane),
        _ => None,
    }
}

/// `display-message -p` against a session's first pane.
pub fn display(name: &str, format: &str) -> Option<String> {
    let mut connection = connect()?;
    let pane = first_pane(&mut connection, name)?;
    let response = exchange(
        &mut connection,
        &Request::DisplayMessage(DisplayMessageRequest {
            target: Some(Target::Pane(pane)),
            print: true,
            message: Some(format.to_string()),
            empty_target_context: false,
        }),
    )
    .ok()?;
    Some(printed(&response))
}

/// The clients attached to a session, rendered through `format`.
pub fn list_clients(name: &str, format: &str) -> Option<String> {
    let response = ask(Request::ListClients(Box::new(ListClientsRequest {
        format: Some(format.to_string()),
        filter: None,
        sort_order: None,
        reversed: false,
        target_session: Some(session(name).ok()?),
    })))
    .ok()?;
    Some(printed(&response))
}

/// How a scroll key reaches a session's first pane: see `rmux::scroll_key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    /// `copy-mode -eu`: enter copy-mode a page up, leaving it at the bottom.
    PageUp,
    /// `copy-mode -e`: enter copy-mode, leaving it at the bottom.
    Enter,
    /// `send-keys -X <command>`: a copy-mode command.
    Command(&'static str),
}

/// Apply one [`Scroll`] to a session's first pane.
pub fn scroll(name: &str, how: Scroll) -> bool {
    let Some(mut connection) = connect() else {
        return false;
    };
    let Some(pane) = first_pane(&mut connection, name) else {
        return false;
    };
    let request = match how {
        Scroll::PageUp | Scroll::Enter => Request::CopyMode(CopyModeRequest {
            target: Some(pane),
            page_down: false,
            exit_on_scroll: true,
            hide_position: false,
            mouse_drag_start: false,
            cancel_mode: false,
            scrollbar_scroll: false,
            source: None,
            page_up: how == Scroll::PageUp,
        }),
        Scroll::Command(command) => Request::SendKeysExt(SendKeysExtRequest {
            target: Some(pane),
            keys: vec![command.to_string()],
            expand_formats: false,
            hex: false,
            literal: false,
            dispatch_key_table: false,
            copy_mode_command: true,
            forward_mouse_event: false,
            reset_terminal: false,
            repeat_count: None,
        }),
    };
    exchange(&mut connection, &request).is_ok()
}

/// Type into a pane: `text` literally when `literal`, else as one named key.
pub fn send_keys(pane: &PaneTarget, key: &str, literal: bool) -> Result<(), String> {
    ask(Request::SendKeysExt(SendKeysExtRequest {
        target: Some(pane.clone()),
        keys: vec![key.to_string()],
        expand_formats: false,
        hex: false,
        literal,
        dispatch_key_table: false,
        copy_mode_command: false,
        forward_mouse_event: false,
        reset_terminal: false,
        repeat_count: None,
    }))
    .map(drop)
}

/// `capture-pane -p`: the visible screen of a pane, `-e` when `escapes` (the
/// colours kept) and `-J` when `join` (wrapped lines kept whole).
pub fn capture(pane: &PaneTarget, escapes: bool, join: bool) -> Option<Vec<u8>> {
    let response = ask(Request::CapturePane(Box::new(CapturePaneRequest {
        target: pane.clone(),
        start: None,
        end: None,
        print: true,
        buffer_name: None,
        alternate: false,
        escape_ansi: escapes,
        escape_sequences: false,
        include_format: false,
        hyperlinks: false,
        line_numbers: false,
        join_wrapped: join,
        use_mode_screen: false,
        preserve_trailing_spaces: false,
        do_not_trim_spaces: false,
        pending_input: false,
        quiet: false,
        start_is_absolute: false,
        end_is_absolute: false,
    })))
    .ok()?;
    match response {
        Response::CapturePane(capture) => Some(capture.output?.stdout),
        _ => None,
    }
}

/// The argv a tab's pane runs to sit on a session of cctop's daemon: this
/// binary as an rmux client, creating the session first when `create` gives
/// the agent to put in it.
///
/// The binary by its path, not `/proc/self/exe`: the pane's command may be run
/// by something other than this process, and `self` would then be that.
pub fn attach_argv(name: &str, create: Option<(&[String], Option<&Path>)>) -> Vec<String> {
    #[cfg(any(test, feature = "test-support"))]
    if in_test_harness() {
        // The harness would read `mux attach …` as test filters and run them in
        // a pane. A program that is not there fails the spawn instead, which is
        // what a pane whose client cannot run reports.
        return vec!["/nonexistent/cctop-mux-in-a-test-binary".to_string()];
    }
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "cctop".to_string());
    attach_argv_of(exe, name, create)
}

fn attach_argv_of(
    exe: String,
    name: &str,
    create: Option<(&[String], Option<&Path>)>,
) -> Vec<String> {
    let mut out = vec![exe, "mux".into(), "attach".into()];
    if let Some((argv, cwd)) = create {
        out.push("--create".into());
        if let Some(dir) = cwd.filter(|d| d.is_dir()) {
            out.push("--cwd".into());
            out.push(dir.to_string_lossy().into_owned());
        }
        out.push(name.to_string());
        out.push("--".into());
        out.extend(argv.iter().cloned());
    } else {
        out.push(name.to_string());
    }
    out
}

/// `cctop mux …`: the daemon itself, and the few commands a human (and the
/// test driver) needs to look inside it. All of them act on cctop's socket and
/// no other. Returns the exit code.
pub fn main(args: &[String]) -> i32 {
    let result = match args.first().map(String::as_str) {
        Some("daemon") => match args.get(1..) {
            Some([flag, socket]) if flag == "--socket" => serve(Path::new(socket)),
            _ => Err("usage: cctop mux daemon --socket <path>".to_string()),
        },
        Some("attach") => attach(&args[1..]),
        Some("ls") => ls(),
        Some("kill-session") => match args.get(1..) {
            Some([flag, name]) if flag == "-t" => kill_session(name),
            _ => Err("usage: cctop mux kill-session -t <name>".to_string()),
        },
        Some("socket") => socket().map(|path| println!("{}", path.display())),
        _ => Err("usage: cctop mux <attach NAME | ls | kill-session -t NAME | socket>".to_string()),
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("cctop mux: {error}");
            1
        }
    }
}

/// `cctop mux ls`: cctop's sessions, as `rmux ls` would list them.
fn ls() -> Result<(), String> {
    let Some(mut connection) = connect() else {
        // No daemon is no sessions, as `running` reads it too.
        return Ok(());
    };
    let response = exchange(
        &mut connection,
        &Request::ListSessions(ListSessionsRequest {
            format: None,
            filter: None,
            sort_order: None,
            reversed: false,
        }),
    )?;
    print!("{}", printed(&response));
    Ok(())
}

/// `cctop mux attach [--create [--cwd DIR] ] NAME [-- ARGV…]`: become the
/// terminal client of a session, the way `rmux attach-session` does, and
/// return when it detaches or ends.
///
/// `--create` is `new-session -A`: the agent is started when the session is
/// not there yet, and attached to as it is otherwise.
fn attach(args: &[String]) -> Result<(), String> {
    let mut create = false;
    let mut cwd = None;
    let mut rest = args;
    loop {
        match rest {
            [flag, tail @ ..] if flag == "--create" => {
                create = true;
                rest = tail;
            }
            [flag, dir, tail @ ..] if flag == "--cwd" => {
                cwd = Some(PathBuf::from(dir));
                rest = tail;
            }
            _ => break,
        }
    }
    let (name, argv) = match rest {
        [name] => (name, &[][..]),
        [name, dash, argv @ ..] if dash == "--" => (name, argv),
        _ => return Err("usage: cctop mux attach [--create [--cwd DIR]] NAME [-- ARGV…]".into()),
    };
    if create && !has_session(name) {
        let env = crate::opencode::launch_env(argv)
            .into_iter()
            .map(|(var, value)| format!("{var}={value}"))
            .collect();
        new_session(name, cwd.as_deref(), env, argv)?;
    }
    let mut connection = connect().ok_or("cctop's rmux daemon is not running")?;
    let target = session(name)?;
    let geometry = connection
        .supports_capability(rmux_proto::CAPABILITY_ATTACH_RESIZE_GEOMETRY)
        .map_err(|e| e.to_string())?;
    let render = connection
        .supports_capability(rmux_proto::CAPABILITY_ATTACH_RENDER)
        .map_err(|e| e.to_string())?;
    let request = rmux_proto::AttachSessionExt2Request {
        target: Some(target),
        target_spec: None,
        detach_other_clients: false,
        kill_other_clients: false,
        read_only: false,
        skip_environment_update: false,
        flags: None,
        working_directory: None,
        client_terminal: rmux_proto::ClientTerminalContext {
            terminal_features: Vec::new(),
            utf8: utf8_locale(),
        },
        client_size: None,
    };
    let transition = match render {
        true => connection.begin_attach_with_capabilities(
            rmux_proto::AttachSessionExt3Request::from_ext2(
                request,
                vec![rmux_proto::CAPABILITY_ATTACH_RENDER.to_string()],
            ),
        ),
        false => connection.begin_attach_with_target_spec(request),
    }
    .map_err(|e| e.to_string())?;
    match transition {
        rmux_client::AttachTransition::Upgraded(upgrade) => {
            let (stream, initial) = upgrade.into_parts();
            match geometry {
                true => rmux_client::attach_terminal_with_initial_bytes_and_resize_geometry(
                    stream, initial,
                ),
                false => rmux_client::attach_terminal_with_initial_bytes(stream, initial),
            }
            .map_err(|e| e.to_string())
        }
        rmux_client::AttachTransition::Rejected(Response::Error(error)) => {
            Err(error.error.to_string())
        }
        rmux_client::AttachTransition::Rejected(other) => {
            Err(format!("the daemon refused to attach: {}", printed(&other)))
        }
    }
}

/// Whether the locale says UTF-8, which is what `rmux` reports for its client
/// too: the first of `LC_ALL`, `LC_CTYPE`, `LANG` that is set decides.
fn utf8_locale() -> bool {
    ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .find_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()))
        .is_some_and(|v| {
            let v = v.to_ascii_lowercase();
            v.contains("utf-8") || v.contains("utf8")
        })
}

/// Whether this process is a test harness rather than cctop: cargo puts those
/// in `target/<profile>/deps/`, and the `cctop` binary a test runs one level up.
///
/// Every agent now lives in this daemon, so a test that opens a tab without a
/// [`TestDaemon`] would otherwise run `/proc/self/exe mux daemon` — the harness,
/// reading `mux` as a filter and running every test that matches, in a loop.
#[cfg(any(test, feature = "test-support"))]
fn in_test_harness() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.file_name()? == "deps"))
        .unwrap_or(false)
}

/// What `cctop doctor` says about the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The rmux version the running daemon was built from.
    pub version: String,
    /// How many sessions it holds — cctop's agents, all of them.
    pub sessions: usize,
}

/// The running daemon's own account of itself, or `None` when none answers on
/// [`socket`]. Never starts one: a doctor that started a daemon to report on
/// it would be reporting on itself.
pub fn status() -> Option<Status> {
    let mut connection = connect()?;
    match connection.daemon_status().ok()? {
        Response::DaemonStatus(status) => Some(Status {
            version: status.rmux_version,
            sessions: status.session_count,
        }),
        _ => None,
    }
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// The socket of this thread's [`TestDaemon`], while one is held.
    static TEST_SOCKET: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// A daemon of a test's own: a private runtime directory, and cctop's daemon
/// running in this process on a socket inside it, which is the one [`socket`]
/// names on this thread while the guard is held.
///
/// In this process rather than as `cctop mux daemon`, because a test binary is
/// not cctop: `/proc/self/exe mux daemon` there would be the test harness
/// reading `mux` as a filter. The daemon is the same code either way
/// ([`serve`]), and it is started again on demand if it exited when its last
/// session ended, as the real one would be.
///
/// On drop it ends the sessions this test made, then the daemon, through its
/// own socket and no other.
#[cfg(any(test, feature = "test-support"))]
pub struct TestDaemon {
    socket: PathBuf,
    _runtime: crate::config::RuntimeBase,
    previous: Option<PathBuf>,
}

#[cfg(any(test, feature = "test-support"))]
impl TestDaemon {
    /// A fresh daemon for the test calling it. `name` is only a label for the
    /// directory, and must be unique to the test.
    pub fn new(name: &str) -> TestDaemon {
        let runtime = crate::config::claim_test_runtime_base(name);
        // Short and private: `sun_path` holds 107 bytes, and the shared
        // runtime root under a long `$TMPDIR` can be most of that already.
        let dir = std::env::temp_dir().join(format!("cm{}-{:x}", std::process::id(), {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            name.hash(&mut h);
            h.finish() as u32
        }));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a socket directory for this test");
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        let socket = dir.join("s");
        let previous = TEST_SOCKET.with(|s| s.replace(Some(socket.clone())));
        TestDaemon {
            socket,
            _runtime: runtime,
            previous,
        }
    }

    /// The socket this test's daemon listens on.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Start the in-process daemon on `socket` unless it is answering.
    fn ensure(socket: &Path) -> Result<(), String> {
        if matches!(
            rmux_client::connect_or_absent(socket),
            Ok(rmux_client::ConnectResult::Connected(_))
        ) {
            return Ok(());
        }
        let _ = std::fs::remove_file(socket);
        let owned = socket.to_path_buf();
        std::thread::spawn(move || {
            if let Err(error) = serve(&owned) {
                eprintln!("test daemon on {}: {error}", owned.display());
            }
        });
        let deadline = std::time::Instant::now() + DEADLINE;
        while std::time::Instant::now() < deadline {
            if let Ok(rmux_client::ConnectResult::Connected(mut connection)) =
                rmux_client::connect_or_absent(socket)
                && matches!(
                    connection.daemon_status(),
                    Ok(Response::DaemonStatus(status)) if !status.config_loading
                )
            {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err(format!(
            "the test daemon on {} never answered",
            socket.display()
        ))
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for TestDaemon {
    fn drop(&mut self) {
        // Every session on this socket is this test's: the daemon is.
        if let Ok(rmux_client::ConnectResult::Connected(mut connection)) =
            rmux_client::connect_or_absent(&self.socket)
        {
            let _ = connection.kill_server();
        }
        TEST_SOCKET.with(|s| *s.borrow_mut() = self.previous.take());
        if let Some(dir) = self.socket.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_lives_under_ctops_runtime_dir_whatever_rmux_variables_say() {
        let _runtime = crate::config::claim_test_runtime_base("mux-socket-path");
        // Not set on the process, which other tests share: these are what the
        // user's rmux would read, and the resolver takes none of them as input,
        // so the assertion is that the path is a function of the runtime dir
        // alone.
        let path = socket().expect("a socket path");
        assert_eq!(
            path,
            crate::config::runtime_base()
                .join("cctop/mux")
                .join(format!("w{}", rmux_proto::RMUX_WIRE_VERSION))
                .join("default")
        );
    }

    #[test]
    fn a_socket_path_too_long_to_bind_says_so() {
        let long = PathBuf::from("/").join("x".repeat(200));
        let error = checked(long).expect_err("too long");
        assert!(error.contains("XDG_RUNTIME_DIR"), "{error}");
    }

    #[test]
    fn the_daemon_inherits_no_other_multiplexers_address() {
        for var in [
            "TMUX",
            "RMUX",
            "RMUX_TMPDIR",
            "TMUX_TMPDIR",
            "RMUX_SDK_ENDPOINT",
        ] {
            assert!(foreign(std::ffi::OsStr::new(var)), "{var}");
        }
        assert!(!foreign(std::ffi::OsStr::new("PATH")));
    }

    #[test]
    fn a_pane_attaches_through_this_binary() {
        let cctop = || "/usr/bin/cctop".to_string();
        let argv = attach_argv_of(cctop(), "cctop-a", Some((&["claude".to_string()], None)));
        assert_eq!(
            argv,
            [
                "/usr/bin/cctop",
                "mux",
                "attach",
                "--create",
                "cctop-a",
                "--",
                "claude"
            ]
        );
        let argv = attach_argv_of(cctop(), "cctop-a", None);
        assert_eq!(argv, ["/usr/bin/cctop", "mux", "attach", "cctop-a"]);
        // A directory that is not there is left off rather than failing the
        // spawn, matching what the pty path does with a stale cwd.
        let gone = attach_argv_of(
            cctop(),
            "cctop-a",
            Some((
                &["claude".to_string()],
                Some(Path::new("/nonexistent/gone")),
            )),
        );
        assert!(!gone.contains(&"--cwd".to_string()));
    }

    /// A test binary is not cctop, and running it as `cctop mux` would run its
    /// own tests in a pane: the guard is what keeps a test that opens a tab
    /// without a daemon of its own from doing that.
    #[test]
    fn a_test_binary_never_runs_itself_as_the_daemon() {
        assert!(in_test_harness());
        assert!(!Path::new(&attach_argv("cctop-a", None)[0]).exists());
        let _runtime = crate::config::claim_test_runtime_base("mux-harness-guard");
        assert!(start().is_err());
    }

    #[test]
    fn a_session_made_on_the_private_daemon_is_listed_and_ended_there() {
        let daemon = TestDaemon::new("mux-roundtrip");
        new_session("cctop-t", None, Vec::new(), &["sleep".into(), "30".into()])
            .expect("session made");
        assert!(daemon.socket().exists());
        assert!(has_session("cctop-t"));
        let listing = list_all_panes("#{session_name}\t#{pane_pid}").expect("listing");
        assert!(
            listing.lines().any(|l| l.starts_with("cctop-t\t")),
            "{listing}"
        );
        set_session_option("cctop-t", "@cctop_label", "hello").expect("option set");
        let label = list_panes("cctop-t", "#{@cctop_label}").expect("panes");
        assert_eq!(label.trim(), "hello");
        let size = display("cctop-t", "#{window_width} #{window_height}").expect("size");
        assert_eq!(size.split_whitespace().count(), 2, "{size}");
        kill_session("cctop-t").expect("killed");
        assert!(!has_session("cctop-t"));
    }
}
