//! `cctop tunnel setup | status | remove`: connecting a Cloudflare account so
//! that `--tunnel` comes up on a hostname that stays the same.
//!
//! The work is `cctop_core::cloudflare`'s; this is the command line around
//! it. With `--browser` it logs in the way `cloudflared tunnel login` does:
//! the authorize page opens in a browser — or, over ssh or with none to open,
//! its address is printed to open elsewhere — and the certificate comes back
//! here. Otherwise, on a terminal it walks through the steps with the token typed
//! invisibly. Piped, it reads one token from stdin and takes the suggestions —
//! the first domain, the suggested name — unless `--zone` and `--hostname`
//! say otherwise, so it can be scripted the way `cctop --add-account` can.
//!
//! No token is ever printed, and none is taken as an argument, where `ps` and
//! shell history would keep it.

use std::io::{BufRead, IsTerminal, Write};

use cctop_core::cloudflare::access::{self, Invite, Level};
use cctop_core::cloudflare::login::Login;
use cctop_core::cloudflare::{self, Api, Pasted, access_setup};
use cctop_core::tunnel::{self, Account};

pub const HELP: &str = "\
cctop tunnel — your own Cloudflare tunnel, for a link that stays the same

USAGE:
  cctop tunnel setup --browser [--hostname <NAME>]
  cctop tunnel setup [--zone <DOMAIN>] [--hostname <NAME>] [--access <EMAIL>]
  cctop tunnel status
  cctop tunnel remove
  cctop tunnel access on --owner <EMAIL>
  cctop tunnel access off
  cctop tunnel access links on|off
  cctop tunnel invite add <EMAIL | @DOMAIN> [--full]
  cctop tunnel invite remove <EMAIL | @DOMAIN>
  cctop tunnel invite list

`cctop serve --tunnel` and the dashboard's tunnel use a trycloudflare quick
tunnel: nothing to set up, but a new address every run, no Server-Sent Events
(which the live table is), and at most 200 requests in flight. A tunnel on
your own free Cloudflare account has none of those limits and keeps one
hostname. It needs a domain whose DNS is on Cloudflare.

setup    With --browser, log in to Cloudflare in the browser and pick the
         domain there: nothing to make or paste. Over ssh, the address to
         open is printed. Without it, paste an API token (cctop prints a
         link that makes one with the three permissions it needs) and pick
         a domain. Either way cctop creates the tunnel and its DNS records.
         A tunnel token from the dashboard works too. Piped, one token is
         read from stdin.
status   What is connected.
remove   Delete what setup created on Cloudflare — the DNS records, the
         tunnel and any Access application, by the ids it stored, nothing
         else — and forget it.
access   Put the dashboard's hostname behind Cloudflare Access: you open it
         by logging in with your email (a one-time code is sent to it), and
         no token is needed. `--access <EMAIL>` on setup does the same.
         Needs the API token to have the two Access permissions; a token
         made from the link setup prints has them. Token links keep
         working for people who cannot log in, on a hostname of their own
         beside it (cctop-link.<domain>); `access links off` removes that
         hostname, and then a token gets nothing through the tunnel — only a
         login does. A token on this machine (127.0.0.1) always works.
invite   Who else may log in: an email, or everyone at @a-domain. Read-only
         unless --full. Nobody invited needs a Cloudflare account.

CCTOP_TUNNEL_TOKEN (a tunnel token) and CCTOP_TUNNEL_HOSTNAME connect a tunnel
without a config file, for a service; they win over what setup stored.
";

pub fn run(argv: &[String]) -> anyhow::Result<i32> {
    match argv.first().map(String::as_str) {
        Some("setup") => setup(&argv[1..]),
        Some("status") => Ok(status()),
        Some("remove") => Ok(remove()),
        Some("access") => access(&argv[1..]),
        Some("invite") => invite(&argv[1..]),
        Some("-h" | "--help") | None => {
            print!("{HELP}");
            Ok(0)
        }
        Some(other) => {
            eprintln!("cctop tunnel: unknown command '{other}'\n\n{HELP}");
            Ok(2)
        }
    }
}

fn status() -> i32 {
    let Some(account) = tunnel::account() else {
        println!(
            "No Cloudflare account connected: --tunnel uses a quick tunnel, at a new \
             address each run.\n`cctop tunnel setup` connects one."
        );
        return 0;
    };
    let from = match account.from_env {
        true => "CCTOP_TUNNEL_TOKEN",
        false => "config.toml",
    };
    match &account.hostname {
        Some(host) => println!("Connected: https://{host} (from {from})"),
        None => println!(
            "Connected (from {from}); the hostname comes from the tunnel's configuration \
             when it connects"
        ),
    }
    if let Some(share) = &account.share_hostname {
        println!("Share hostname: https://{share}");
    }
    if let Some(id) = &account.tunnel_id {
        println!("Tunnel: {id}");
    }
    match tunnel::access_settings() {
        Some(access) => {
            println!(
                "Cloudflare Access: on — log in as {} at the dashboard's hostname",
                access.owner
            );
            for line in invite_lines(&access.invites) {
                println!("  {line}");
            }
            match (access.public_links, &access.link_hostname) {
                (true, Some(host)) => {
                    println!("Public token links: on — at https://{host}")
                }
                (true, None) => println!("Public token links: on"),
                (false, _) => println!(
                    "Public token links: off — a token gets nothing through the tunnel \
                     (`cctop tunnel access links on`)"
                ),
            }
        }
        None => println!(
            "Cloudflare Access: off — the dashboard is opened with its token link \
             (`cctop tunnel access on` puts it behind a login)"
        ),
    }
    if tunnel::in_use(&account) {
        println!("A cctop on this machine is serving over it now.");
    }
    0
}

/// The invite list, one line each, or the line saying there is none.
fn invite_lines(invites: &[Invite]) -> Vec<String> {
    if invites.is_empty() {
        return vec!["No one else is invited (`cctop tunnel invite add`).".to_string()];
    }
    invites
        .iter()
        .map(|invite| {
            let who = match invite.is_domain() {
                true => format!("everyone at {}", invite.who),
                false => invite.who.clone(),
            };
            let can = match invite.level {
                Level::Read => "read-only",
                Level::Full => "full",
            };
            format!("{who}: {can}")
        })
        .collect()
}

/// The account cctop can write to, or `None` after saying why on stderr.
fn writable() -> Option<(Account, Api)> {
    if tunnel::account().is_none() {
        eprintln!("cctop: no Cloudflare account is connected; `cctop tunnel setup` first.");
        return None;
    }
    match cloudflare::connected() {
        Ok(found) => Some(found),
        Err(why) => {
            eprintln!("cctop: {why}");
            None
        }
    }
}

/// `cctop tunnel access on --owner <EMAIL>` and `access off`.
fn access(argv: &[String]) -> anyhow::Result<i32> {
    match argv.first().map(String::as_str) {
        Some("on") => {
            let owner = match argv.get(1..) {
                Some([flag, owner]) if flag == "--owner" => owner.clone(),
                _ => anyhow::bail!("usage: cctop tunnel access on --owner <EMAIL>"),
            };
            let Some((account, api)) = writable() else {
                return Ok(1);
            };
            Ok(turn_on(&account, &api, &owner))
        }
        Some("off") => {
            let Some((mut account, api)) = writable() else {
                return Ok(1);
            };
            let Some(settings) = account.access.take() else {
                println!("Cloudflare Access is already off.");
                return Ok(0);
            };
            let left = access_setup::disable(&api, &account, &settings);
            tunnel::save_account(&account)?;
            match left.is_empty() {
                true => {
                    println!("Access is off: the dashboard is opened with its token link again.")
                }
                false => {
                    println!("Access is off, but these are left on Cloudflare to delete by hand:");
                    for item in &left {
                        println!("  - {item}");
                    }
                }
            }
            Ok(0)
        }
        Some("links") => {
            let on = match argv.get(1).map(String::as_str) {
                Some("on") => true,
                Some("off") => false,
                _ => anyhow::bail!("usage: cctop tunnel access links on|off"),
            };
            if tunnel::account().is_none() {
                eprintln!("cctop: no Cloudflare account is connected; `cctop tunnel setup` first.");
                return Ok(1);
            }
            match access_setup::apply(access_setup::Change::PublicLinks(on)) {
                Ok(applied) => {
                    println!("{}", applied.said);
                    for item in &applied.left {
                        println!("  left on Cloudflare to delete by hand: {item}");
                    }
                    Ok(0)
                }
                Err(why) => {
                    eprintln!("cctop: {why}");
                    Ok(1)
                }
            }
        }
        _ => anyhow::bail!(
            "usage: cctop tunnel access on --owner <EMAIL> | access off | access links on|off"
        ),
    }
}

/// Put `account`'s page behind Access for `owner`, keeping any invites, and
/// store it. The exit code.
fn turn_on(account: &Account, api: &Api, owner: &str) -> i32 {
    eprintln!("Putting the dashboard behind Cloudflare Access…");
    let invites = account
        .access
        .as_ref()
        .map(|a| a.invites.clone())
        .unwrap_or_default();
    let settings = match access_setup::enable(api, account, owner, invites) {
        Ok(settings) => settings,
        Err(e) => {
            eprintln!("cctop: {e}");
            return 1;
        }
    };
    let host = account.hostname.clone().unwrap_or_default();
    let owner = settings.owner.clone();
    let link = settings.link_hostname.clone();
    let stored = Account {
        access: Some(Box::new(settings)),
        ..account.clone()
    };
    if let Err(e) = tunnel::save_account(&stored) {
        // Made on Cloudflare but not remembered: say what to delete, since
        // `remove` will not find it.
        eprintln!(
            "cctop: Access is set up on Cloudflare, but cctop could not remember it ({e}); \
             delete the application on {host} in the Zero Trust dashboard"
        );
        return 1;
    }
    eprintln!(
        "Access is on: open https://{host} and log in as {owner} with the code Cloudflare \
         emails you. `cctop tunnel invite add` lets others in."
    );
    if let Some(link) = link {
        eprintln!(
            "Token links go on https://{link} from now on; `cctop tunnel access links off` \
             turns them off."
        );
    }
    0
}

/// `cctop tunnel invite add|remove|list`.
fn invite(argv: &[String]) -> anyhow::Result<i32> {
    const USAGE: &str = "usage: cctop tunnel invite add <EMAIL | @DOMAIN> [--full] | remove <EMAIL | @DOMAIN> | list";
    let verb = argv.first().map(String::as_str);
    if verb == Some("list") {
        match tunnel::access_settings() {
            Some(access) => {
                println!("{} (owner): full", access.owner);
                for line in invite_lines(&access.invites) {
                    println!("{line}");
                }
            }
            None => println!(
                "Cloudflare Access is off; `cctop tunnel access on --owner <EMAIL>` first."
            ),
        }
        return Ok(0);
    }
    let (who, level) = match (verb, argv.get(1..).unwrap_or_default()) {
        (Some("add"), [who]) => (who, Some(Level::Read)),
        (Some("add"), [who, flag]) | (Some("add"), [flag, who]) if flag == "--full" => {
            (who, Some(Level::Full))
        }
        (Some("add"), [who, flag]) | (Some("add"), [flag, who]) if flag == "--read" => {
            (who, Some(Level::Read))
        }
        (Some("remove"), [who]) => (who, None),
        _ => anyhow::bail!(USAGE),
    };
    let Some(normal) = access::parse_who(who) else {
        eprintln!("cctop: {who} is neither an email address nor a domain like @company.com");
        return Ok(1);
    };
    let Some((account, api)) = writable() else {
        return Ok(1);
    };
    let Some(mut settings) = account.access.clone() else {
        eprintln!(
            "cctop: Cloudflare Access is off; `cctop tunnel access on --owner <EMAIL>` first."
        );
        return Ok(1);
    };
    match level {
        Some(level) => {
            settings.invites = access_setup::with_invite(settings.invites, normal.clone(), level)
        }
        None => {
            let before = settings.invites.len();
            settings.invites.retain(|i| i.who != normal);
            if settings.invites.len() == before {
                println!("{normal} was not invited; nothing changed.");
                return Ok(0);
            }
        }
    }
    if let Err(e) = access_setup::update(&api, &account, &settings) {
        eprintln!("cctop: {e}");
        eprintln!("Nothing was changed.");
        return Ok(1);
    }
    tunnel::save_account(&Account {
        access: Some(settings),
        ..account
    })?;
    match level {
        Some(Level::Full) => println!("Invited {normal}, with full access."),
        Some(Level::Read) => println!("Invited {normal}, read-only."),
        None => println!("{normal} can no longer log in."),
    }
    Ok(0)
}

fn remove() -> i32 {
    let Some(account) = tunnel::account() else {
        println!("No Cloudflare account is connected; nothing to remove.");
        return 0;
    };
    if account.from_env {
        eprintln!(
            "cctop: that tunnel comes from CCTOP_TUNNEL_TOKEN, not from setup: unset it \
             to stop using it. Delete the tunnel itself in the Cloudflare dashboard."
        );
        return 1;
    }
    if tunnel::in_use(&account) {
        eprintln!(
            "cctop: a cctop on this machine is serving over this tunnel. Stop it first, \
             then run this again."
        );
        return 1;
    }
    let left = cloudflare::remove(&account);
    if let Err(e) = tunnel::clear_account() {
        eprintln!("cctop: could not update config.toml: {e}");
        return 1;
    }
    match (account.api_token.is_some(), left.0.is_empty()) {
        (false, _) => println!(
            "Forgotten. The tunnel was made in the Cloudflare dashboard, so it is still \
             there; delete it there if you no longer want it."
        ),
        (true, true) => println!("Removed: the tunnel and its DNS records are gone."),
        (true, false) => {
            println!("Forgotten, but these are left on Cloudflare to delete by hand:");
            for item in &left.0 {
                println!("  - {item}");
            }
        }
    }
    0
}

fn setup(argv: &[String]) -> anyhow::Result<i32> {
    let mut zone_given: Option<String> = None;
    let mut hostname_given: Option<String> = None;
    let mut browser = false;
    let mut access_owner: Option<String> = None;
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = || {
            it.next()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--zone" => zone_given = Some(value()?),
            "--hostname" => hostname_given = Some(value()?),
            "--browser" => browser = true,
            "--access" => access_owner = Some(value()?),
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(0);
            }
            other => anyhow::bail!("unknown option '{other}'\n\n{HELP}"),
        }
    }
    if let Some(account) = tunnel::account()
        && !account.from_env
    {
        let at = account
            .hostname
            .as_deref()
            .unwrap_or("its configured hostname");
        eprintln!(
            "cctop: a tunnel is already connected ({at}). `cctop tunnel remove` first to \
             connect another."
        );
        return Ok(1);
    }

    let interactive = std::io::stdin().is_terminal();
    if browser {
        if zone_given.is_some() {
            eprintln!("cctop: with --browser the domain is picked in the browser; drop --zone.");
            return Ok(1);
        }
        let Some(account) = log_in(hostname_given, interactive)? else {
            return Ok(1);
        };
        return connected(&account, access_owner.as_deref());
    }
    let pasted = match interactive {
        true => {
            eprintln!(
                "A tunnel on your own Cloudflare account keeps one hostname, so a link or a \
                 phone shortcut keeps working.\n\n\
                 1. Make an API token (the link fills in the permissions and the name):\n\
                 \x20  {}\n\
                 \x20  It needs: {}.\n\
                 \x20  For logging in with Cloudflare Access later, also: {}.\n\
                 2. Paste it below. A tunnel token from the dashboard works too.\n",
                cloudflare::token_link(),
                cloudflare::PERMISSIONS.join(", "),
                access_setup::PERMISSIONS.join(", ")
            );
            eprint!("Token (not shown as you paste): ");
            read_secret()?
        }
        false => read_line()?,
    };
    if pasted.is_empty() {
        eprintln!("cctop: no token given; nothing written.");
        return Ok(1);
    }

    let account = match cloudflare::classify(&pasted) {
        Pasted::Tunnel(token) => {
            if let Err(e) = cloudflare::check_tunnel_token(&token) {
                eprintln!("cctop: {e}");
                return Ok(1);
            }
            let hostname = match (hostname_given, interactive) {
                (Some(h), _) => Some(h),
                (None, true) => {
                    eprint!(
                        "Hostname the tunnel serves (Enter to take it from the tunnel's \
                         configuration): "
                    );
                    Some(read_line()?).filter(|h| !h.is_empty())
                }
                (None, false) => None,
            };
            Account {
                token,
                hostname,
                ..Account::default()
            }
        }
        Pasted::Api(token) => match create(&token, zone_given, hostname_given, interactive)? {
            Some(account) => account,
            None => return Ok(1),
        },
    };
    connected(&account, access_owner.as_deref())
}

/// Store `account` and say so, then put it behind Access for `owner` when
/// that was asked for. The tunnel is kept whatever Access says: it works
/// without it, and `cctop tunnel access on` can try again.
fn connected(account: &Account, owner: Option<&str>) -> anyhow::Result<i32> {
    tunnel::save_account(account)?;
    match &account.hostname {
        Some(host) => eprintln!("Connected: https://{host}"),
        None => eprintln!("Connected; the hostname is learned when the tunnel first connects."),
    }
    eprintln!("`cctop serve --tunnel`, and t in the dashboard's serve panel, now use it.");
    let Some(owner) = owner else {
        return Ok(0);
    };
    // Through what was just stored, which is what every later command uses.
    let code = match cloudflare::connected() {
        Ok((account, api)) => turn_on(&account, &api, owner),
        Err(why) => {
            eprintln!("cctop: {why}");
            1
        }
    };
    if code != 0 {
        eprintln!(
            "The tunnel is connected; `cctop tunnel access on --owner {owner}` tries Access again."
        );
    }
    Ok(code)
}

/// The browser path: the login, then a name on the domain picked there, then
/// the same creation as a pasted token's. `None` after saying why on stderr.
fn log_in(hostname_given: Option<String>, interactive: bool) -> anyhow::Result<Option<Account>> {
    let login = Login::new();
    // Over ssh the browser `xdg-open` would reach is not the user's, if there
    // is one at all; the address goes where they can see it either way, as
    // cloudflared prints it, and the certificate is fetched from here.
    let opened = !cctop_core::clipboard::over_ssh() && cctop_serve::open_in_browser(login.url());
    match opened {
        true => eprintln!(
            "A browser should have opened Cloudflare's login at:\n\n{}\n\nIf it did not, \
             open that address yourself. Log in and pick the domain for the tunnel.",
            login.url()
        ),
        false => eprintln!(
            "Open this address in a browser, log in to Cloudflare and pick the domain for \
             the tunnel:\n\n{}\n\nLeave this running: the login comes back here.",
            login.url()
        ),
    }
    eprintln!("\nWaiting for the login…");
    let cert = match login.wait(&|| false) {
        Ok(cert) => cert,
        Err(e) => return Ok(refuse(&e)),
    };
    let api = Api::with(&cert.auth());
    let zone = api.login_zone(&cert);
    if let Some(zone) = &zone {
        eprintln!("Logged in: the tunnel goes on {}.", zone.name);
    }
    let machine = cloudflare::machine_label();
    let (zone, hostname) = match (zone, hostname_given) {
        (Some(zone), Some(hostname)) => (zone, hostname),
        (Some(zone), None) => {
            let suggested = match cloudflare::suggest_hostname(&api, &zone, &machine) {
                Ok(name) => name,
                Err(e) => return Ok(refuse(&e)),
            };
            let hostname = match interactive {
                true => {
                    eprint!("Hostname [{suggested}]: ");
                    Some(read_line()?)
                        .filter(|h| !h.is_empty())
                        .unwrap_or(suggested)
                }
                false => suggested,
            };
            (zone, hostname)
        }
        // The domain's name could not be read with the login's token: the
        // user knows it, having just picked it.
        (None, given) => {
            let hostname = match (given, interactive) {
                (Some(hostname), _) => hostname,
                (None, true) => {
                    eprint!(
                        "Logged in. The address for the dashboard, on the domain you picked \
                         (like cctop.example.com): "
                    );
                    read_line()?
                }
                (None, false) => {
                    eprintln!(
                        "cctop: logged in, but the domain's name could not be read; pass \
                         --hostname <NAME> on it."
                    );
                    return Ok(None);
                }
            };
            match cloudflare::login_zone_from(&cert.zone_id, &cert.account_id, &hostname) {
                Ok(zone) => (zone, hostname),
                Err(e) => return Ok(refuse(&e)),
            }
        }
    };
    eprintln!("Creating the tunnel and its DNS records…");
    match cloudflare::create(&api, &zone, &hostname, &machine) {
        Ok(account) => Ok(Some(account)),
        Err(e) => Ok(refuse(&e)),
    }
}

/// The API-token path: verify, pick a domain and a name, create. `None` after
/// saying why on stderr.
fn create(
    token: &str,
    zone_given: Option<String>,
    hostname_given: Option<String>,
    interactive: bool,
) -> anyhow::Result<Option<Account>> {
    let api = Api::new(token);
    eprintln!("Checking the token with Cloudflare…");
    let zones = match api
        .verify()
        .and_then(|()| api.zones())
        .and_then(cloudflare::usable)
    {
        Ok(zones) => zones,
        Err(e) => return Ok(refuse(&e)),
    };
    let zone = match zone_given {
        Some(name) => match zones.iter().find(|z| z.name.eq_ignore_ascii_case(&name)) {
            Some(zone) => zone.clone(),
            None => {
                let names: Vec<&str> = zones.iter().map(|z| z.name.as_str()).collect();
                eprintln!(
                    "cctop: {name} is not an active domain on this account; it has {}",
                    names.join(", ")
                );
                return Ok(None);
            }
        },
        None if zones.len() == 1 || !interactive => zones[0].clone(),
        None => {
            eprintln!("Which domain?");
            for (i, zone) in zones.iter().enumerate() {
                eprintln!("  {}. {}", i + 1, zone.name);
            }
            eprint!("Number [1]: ");
            let answer = read_line()?;
            let pick = match answer.is_empty() {
                true => 0,
                false => answer.parse::<usize>().unwrap_or(0).saturating_sub(1),
            };
            zones.get(pick).unwrap_or(&zones[0]).clone()
        }
    };
    let machine = cloudflare::machine_label();
    let hostname = match hostname_given {
        Some(h) => h,
        None => {
            let suggested = match cloudflare::suggest_hostname(&api, &zone, &machine) {
                Ok(name) => name,
                Err(e) => return Ok(refuse(&e)),
            };
            match interactive {
                true => {
                    eprint!("Hostname [{suggested}]: ");
                    Some(read_line()?)
                        .filter(|h| !h.is_empty())
                        .unwrap_or(suggested)
                }
                false => suggested,
            }
        }
    };
    eprintln!("Creating the tunnel and its DNS records…");
    match cloudflare::create(&api, &zone, &hostname, &machine) {
        Ok(account) => Ok(Some(account)),
        Err(e) => Ok(refuse(&e)),
    }
}

fn refuse(error: &cloudflare::Error) -> Option<Account> {
    eprintln!("cctop: {error}");
    eprintln!("Nothing was written, and --tunnel still uses a quick tunnel.");
    None
}

fn read_line() -> anyhow::Result<String> {
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

/// A line from the terminal with echo off, so a pasted token is not left on
/// screen or in a terminal's scrollback. Echo comes back however this ends.
fn read_secret() -> anyhow::Result<String> {
    struct Restore(libc::termios);
    impl Drop for Restore {
        fn drop(&mut self) {
            // SAFETY: tcsetattr on stdin with a termios read from it.
            unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.0) };
        }
    }
    // SAFETY: a zeroed termios is a valid out-parameter for tcgetattr.
    let mut original: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: tcgetattr on stdin, which is a terminal (the caller checked).
    let restore = match unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut original) } {
        0 => {
            let mut quiet = original;
            quiet.c_lflag &= !libc::ECHO;
            // SAFETY: as above, with a termios derived from the one read.
            unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &quiet) };
            Some(Restore(original))
        }
        _ => None,
    };
    let line = read_line();
    drop(restore);
    eprintln!();
    let _ = std::io::stderr().flush();
    line
}
