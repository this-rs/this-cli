//! Supervised `this dev` sessions for the MCP server.
//!
//! # Why a child process rather than a refactor
//!
//! `this dev` already knows how to detect the workspace, read `this.yaml`,
//! pick a Rust watcher, run the frontend build and stream both processes with a
//! prefix. Re-implementing that inside an MCP handler would duplicate the whole
//! thing and drift. Instead one session = one supervised `this dev` child
//! process, with its output redirected to a log file.
//!
//! # What this module guarantees
//!
//! - `start` returns immediately: the JSON-RPC loop is never blocked by a
//!   long-lived process (the defect this replaces: the handler called
//!   `dev::run` synchronously and froze the whole transport).
//! - Every session belongs to its own **process group**, so stopping it stops
//!   the grandchildren too (`cargo run`'s binary, `npm`'s node process) and not
//!   just the direct child.
//! - Sessions are killed when the MCP server exits: a closed stdin must not
//!   leave orphan dev servers behind.
//!
//! # Logs
//!
//! Output goes to `<temp>/this-dev-sessions/<id>.log`, opened in truncate mode
//! at start. `logs` reads the tail — bounded, so a chatty `cargo run` cannot
//! exhaust the client's context.

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a fresh session is watched before it is reported as started.
///
/// A `this dev` that cannot even find the workspace fails in milliseconds;
/// waiting here turns that into a clean tool error instead of a session that
/// "started" and was dead on arrival.
const STARTUP_GRACE: Duration = Duration::from_millis(700);

/// How long `stop` waits after SIGTERM before escalating to SIGKILL.
const STOP_GRACE: Duration = Duration::from_secs(5);

/// Default number of log lines `dev_logs` returns.
pub const DEFAULT_LOG_LINES: usize = 80;

/// Hard cap on log lines, whatever the caller asks for.
const MAX_LOG_LINES: usize = 1000;

/// One running `this dev`.
struct DevSession {
    id: String,
    pid: u32,
    child: Child,
    log_path: PathBuf,
    started: Instant,
    cwd: PathBuf,
    command: String,
}

/// The registry of supervised sessions, owned by the MCP tool handler.
pub struct DevSessions {
    sessions: Mutex<BTreeMap<String, DevSession>>,
    next_id: Mutex<u64>,
    /// The executable to spawn. `this` in production; overridable so the module
    /// stays testable without spawning the real CLI.
    exe: Option<PathBuf>,
    /// Test seam: replace the `dev ...` arguments, so a test can supervise a
    /// long-lived child that has nothing to do with this CLI.
    #[cfg(test)]
    args: Option<Vec<String>>,
}

impl Default for DevSessions {
    fn default() -> Self {
        Self::new()
    }
}

impl DevSessions {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(BTreeMap::new()),
            next_id: Mutex::new(0),
            exe: None,
            #[cfg(test)]
            args: None,
        }
    }

    /// Use `exe` instead of the current executable (tests).
    #[cfg(test)]
    pub fn with_exe(exe: PathBuf) -> Self {
        let mut s = Self::new();
        s.exe = Some(exe);
        s
    }

    /// Supervise `exe args...` instead of `this dev ...` (tests).
    #[cfg(test)]
    pub fn with_command(mut self, exe: PathBuf, args: Vec<String>) -> Self {
        self.exe = Some(exe);
        self.args = Some(args);
        self
    }

    fn exe(&self) -> Result<PathBuf> {
        match &self.exe {
            Some(p) => Ok(p.clone()),
            None => std::env::current_exe().context("Failed to locate the `this` executable"),
        }
    }

    fn log_dir() -> Result<PathBuf> {
        let dir = std::env::temp_dir().join("this-dev-sessions");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create {}", dir.display()))?;
        Ok(dir)
    }

    /// Start a supervised `this dev` and return its descriptor.
    ///
    /// Returns as soon as the process exists. If the child dies within
    /// [`STARTUP_GRACE`], its log tail is returned as an error: a session that
    /// never started must not look like a session that did.
    pub fn start(
        &self,
        cwd: &Path,
        api_only: bool,
        no_watch: bool,
        port: Option<u16>,
    ) -> Result<Value> {
        if !cwd.is_dir() {
            bail!("Working directory does not exist: {}", cwd.display());
        }

        let exe = self.exe()?;
        let log_dir = Self::log_dir()?;

        let id = {
            let mut next = self.next_id.lock().expect("dev session id lock");
            *next += 1;
            format!("dev-{}", *next)
        };
        let log_path = log_dir.join(format!("{id}.log"));
        let log_file = std::fs::File::create(&log_path)
            .with_context(|| format!("Failed to create {}", log_path.display()))?;

        #[cfg(test)]
        let injected = self.args.clone();
        #[cfg(not(test))]
        let injected: Option<Vec<String>> = None;

        let args: Vec<String> = match injected {
            Some(args) => args,
            None => {
                let mut args: Vec<String> = vec!["dev".to_string()];
                if api_only {
                    args.push("--api-only".to_string());
                }
                if no_watch {
                    args.push("--no-watch".to_string());
                }
                if let Some(p) = port {
                    args.push("--port".to_string());
                    args.push(p.to_string());
                }
                args
            }
        };

        let command = format!("{} {}", exe.display(), args.join(" "));

        let mut cmd = Command::new(&exe);
        cmd.args(&args)
            .current_dir(cwd)
            // stdin must NOT be inherited: it carries the JSON-RPC protocol.
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file.try_clone()?))
            .stderr(Stdio::from(log_file));

        // Own process group, so a stop reaches `cargo run`'s binary and `npm`'s
        // node process, not just the direct child.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn `{command}`: {e}"))?;
        let pid = child.id();

        // Fail fast: a workspace error surfaces here, not in the client's chat
        // three tool calls later.
        std::thread::sleep(STARTUP_GRACE);
        if let Some(status) = child.try_wait().context("Failed to poll the dev process")? {
            let tail = read_tail(&log_path, DEFAULT_LOG_LINES).unwrap_or_default();
            bail!(
                "`this dev` exited immediately ({}).\n\n{}",
                status,
                if tail.trim().is_empty() {
                    "(no output)".to_string()
                } else {
                    tail
                }
            );
        }

        let mut session = DevSession {
            id: id.clone(),
            pid,
            child,
            log_path: log_path.clone(),
            started: Instant::now(),
            cwd: cwd.to_path_buf(),
            command,
        };
        let descriptor = describe(&mut session);
        self.sessions
            .lock()
            .expect("dev session lock")
            .insert(id, session);

        Ok(json!({
            "status": "started",
            "session_id": descriptor["session_id"],
            "pid": pid,
            "cwd": cwd.to_string_lossy(),
            "log_path": log_path.to_string_lossy(),
            "command": descriptor["command"],
            "note": "Call dev_status / dev_logs / dev_stop with this session_id.",
        }))
    }

    /// Status of one session, or of every session when `id` is `None`.
    pub fn status(&self, id: Option<&str>) -> Result<Value> {
        let mut sessions = self.sessions.lock().expect("dev session lock");

        if let Some(id) = id {
            if !sessions.contains_key(id) {
                return Err(unknown_session(id, sessions.keys()));
            }
            let session = sessions.get_mut(id).expect("checked just above");
            return Ok(describe(session));
        }

        let mut out = Vec::new();
        for session in sessions.values_mut() {
            out.push(describe(session));
        }
        Ok(json!({ "sessions": out }))
    }

    /// Tail of a session's log.
    pub fn logs(&self, id: Option<&str>, lines: usize) -> Result<Value> {
        let sessions = self.sessions.lock().expect("dev session lock");
        let session = match id {
            Some(id) => sessions
                .get(id)
                .ok_or_else(|| unknown_session(id, sessions.keys()))?,
            None => sessions
                .last_key_value()
                .map(|(_, s)| s)
                .ok_or_else(|| anyhow::anyhow!("No dev session is running"))?,
        };

        let wanted = lines.clamp(1, MAX_LOG_LINES);
        let text = read_tail(&session.log_path, wanted).unwrap_or_default();
        Ok(json!({
            "session_id": session.id,
            "log_path": session.log_path.to_string_lossy(),
            "lines_requested": wanted,
            "lines_returned": text.lines().count(),
            "log": text,
        }))
    }

    /// Stop a session (or every session when `id` is `None`).
    ///
    /// SIGTERM the process group, wait up to [`STOP_GRACE`], then SIGKILL.
    pub fn stop(&self, id: Option<&str>) -> Result<Value> {
        let mut sessions = self.sessions.lock().expect("dev session lock");
        let ids: Vec<String> = match id {
            Some(id) => {
                if !sessions.contains_key(id) {
                    return Err(unknown_session(id, sessions.keys()));
                }
                vec![id.to_string()]
            }
            None => sessions.keys().cloned().collect(),
        };

        if ids.is_empty() {
            return Ok(json!({ "status": "none", "stopped": [] }));
        }

        let mut stopped = Vec::new();
        for id in ids {
            if let Some(mut session) = sessions.remove(&id) {
                let outcome = terminate(&mut session);
                stopped.push(json!({
                    "session_id": session.id,
                    "pid": session.pid,
                    "outcome": outcome,
                }));
            }
        }
        Ok(json!({ "status": "stopped", "stopped": stopped }))
    }

    /// Kill every session. Called when the MCP server's stdin closes: nothing
    /// this process started may outlive it.
    pub fn shutdown_all(&self) {
        let mut sessions = self.sessions.lock().expect("dev session lock");
        for (_, mut session) in std::mem::take(&mut *sessions) {
            let _ = terminate(&mut session);
        }
    }
}

/// Describe a session, refreshing its liveness first.
fn describe(session: &mut DevSession) -> Value {
    let exit = session.child.try_wait().ok().flatten();
    let running = exit.is_none();
    json!({
        "session_id": session.id,
        "pid": session.pid,
        "running": running,
        "exit_status": exit.map(|s| s.to_string()),
        "uptime_secs": session.started.elapsed().as_secs(),
        "cwd": session.cwd.to_string_lossy(),
        "command": session.command,
        "log_path": session.log_path.to_string_lossy(),
    })
}

fn unknown_session<'a>(id: &str, known: impl Iterator<Item = &'a String>) -> anyhow::Error {
    let known: Vec<&str> = known.map(|s| s.as_str()).collect();
    anyhow::anyhow!(
        "Unknown dev session '{}'. Known sessions: {}",
        id,
        if known.is_empty() {
            "none".to_string()
        } else {
            known.join(", ")
        }
    )
}

/// Stop a session's whole process group: TERM, wait, then KILL.
fn terminate(session: &mut DevSession) -> &'static str {
    if session.child.try_wait().ok().flatten().is_some() {
        return "already_exited";
    }

    signal_group(session.pid, false);

    let deadline = Instant::now() + STOP_GRACE;
    while Instant::now() < deadline {
        match session.child.try_wait() {
            Ok(Some(_)) => return "terminated",
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break,
        }
    }

    signal_group(session.pid, true);
    let _ = session.child.kill();
    let _ = session.child.wait();
    "killed"
}

/// Signal the process group led by `pgid` (`pid` of a child started with
/// `process_group(0)`).
///
/// # Why `kill(2)` and not `sh -c "kill ..."`
///
/// The first version shelled out. It worked on macOS and failed on the Ubuntu
/// runner: dash's `kill` builtin does not accept the `--` separator before a
/// negative pid, so the SIGTERM was silently dropped, the grace expired, and
/// every stop degraded to SIGKILL. `kill(2)` with a negative pid *is* the
/// "signal the group" call — no shell, no dialect.
fn signal_group(pgid: u32, force: bool) {
    #[cfg(unix)]
    {
        // `kill(2)` with a negative pid signals the whole process group. The
        // return value is ignored on purpose: a group that is already gone
        // reports ESRCH, which is not an error here.
        let signal = if force { libc::SIGKILL } else { libc::SIGTERM };
        // SAFETY: `kill` is a plain syscall wrapper with no memory contract; the
        // only requirement is that the pid namespace is the caller's, which it is.
        unsafe {
            libc::kill(-(pgid as i32), signal);
        }
    }
    #[cfg(windows)]
    {
        // `/T` reaches the whole tree, the closest Windows equivalent of a
        // process group.
        let mut args = vec!["/PID".to_string(), pgid.to_string(), "/T".to_string()];
        if force {
            args.push("/F".to_string());
        }
        let _ = Command::new("taskkill")
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Last `lines` lines of a file, read from the end in one pass.
fn read_tail(path: &Path, lines: usize) -> Result<String> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let all: Vec<&str> = content.lines().collect();
    // `0` still yields the last line: an empty tail hides the very error
    // that made someone ask for logs.
    let start = all.len().saturating_sub(lines.max(1));
    let mut out = all[start..].join("\n");
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_tail_returns_the_last_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log");
        std::fs::write(&path, "a\nb\nc\nd\n").unwrap();

        assert_eq!(read_tail(&path, 2).unwrap(), "c\nd\n");
        assert_eq!(read_tail(&path, 99).unwrap(), "a\nb\nc\nd\n");
        assert_eq!(read_tail(&path, 0).unwrap(), "d\n");
    }

    #[test]
    fn unknown_session_names_the_known_ones() {
        let sessions = DevSessions::new();
        let err = sessions.status(Some("dev-42")).unwrap_err().to_string();
        assert!(err.contains("dev-42"), "got: {err}");
        assert!(err.contains("none"), "got: {err}");
    }

    #[test]
    fn stop_without_any_session_reports_none() {
        let sessions = DevSessions::new();
        let out = sessions.stop(None).unwrap();
        assert_eq!(out["status"], "none");
    }

    #[test]
    fn start_rejects_a_missing_working_directory() {
        let sessions = DevSessions::new();
        let err = sessions
            .start(
                Path::new("/definitely/not/a/real/directory"),
                false,
                false,
                None,
            )
            .unwrap_err()
            .to_string();
        assert!(err.contains("does not exist"), "got: {err}");
    }

    /// A `true` that exits 0 at once: the session must not be reported started.
    ///
    /// The path is probed rather than hardcoded — macOS ships `/usr/bin/true`
    /// and no `/bin/true`, which is exactly how this test first failed.
    #[cfg(unix)]
    #[test]
    fn start_reports_a_child_that_dies_immediately() {
        let exe = ["/usr/bin/true", "/bin/true"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
            .expect("a `true` binary is needed to simulate an immediate exit");
        let sessions = DevSessions::with_exe(exe);
        let dir = tempfile::tempdir().unwrap();
        let err = sessions
            .start(dir.path(), false, false, None)
            .expect_err("a child that exits immediately is not a started session");
        let msg = err.to_string();
        assert!(msg.contains("exited immediately"), "got: {msg}");
        // Nothing must be registered.
        assert_eq!(
            sessions.status(None).unwrap()["sessions"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }
    /// The property the whole module exists for: `start` returns while the
    /// child keeps running. The old handler called `dev::run` synchronously and
    /// froze the JSON-RPC loop until the dev servers exited.
    #[cfg(unix)]
    #[test]
    fn start_returns_while_the_child_keeps_running_and_stop_ends_it() {
        let sleep = ["/bin/sleep", "/usr/bin/sleep"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
            .expect("a `sleep` binary is needed for a long-lived child");
        let sessions = DevSessions::new().with_command(sleep, vec!["30".to_string()]);
        let dir = tempfile::tempdir().unwrap();

        let began = Instant::now();
        let started = sessions
            .start(dir.path(), false, false, None)
            .expect("the child stays up, so the session must start");
        let elapsed = began.elapsed();

        // STARTUP_GRACE is 700 ms; anything near the child's own 30 s lifetime
        // would mean the call blocked.
        assert!(
            elapsed < Duration::from_secs(3),
            "start must return promptly, took {elapsed:?}"
        );

        let id = started["session_id"].as_str().unwrap().to_string();
        assert_eq!(started["status"], "started");

        let status = sessions.status(Some(&id)).unwrap();
        assert_eq!(
            status["running"], true,
            "the child should be alive: {status}"
        );

        // Logs are readable even when the child wrote nothing.
        let logs = sessions.logs(Some(&id), 10).unwrap();
        assert_eq!(logs["session_id"], id);

        let stopped = sessions.stop(Some(&id)).unwrap();
        assert_eq!(stopped["status"], "stopped");
        assert_eq!(stopped["stopped"][0]["outcome"], "terminated");

        // The session is gone, and its process with it.
        assert!(sessions.status(Some(&id)).is_err());
        assert_eq!(
            sessions.status(None).unwrap()["sessions"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    /// A closed MCP server must not leave a dev server behind.
    #[cfg(unix)]
    #[test]
    fn shutdown_all_ends_every_session() {
        let sleep = ["/bin/sleep", "/usr/bin/sleep"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
            .expect("a `sleep` binary is needed for a long-lived child");
        let sessions = DevSessions::new().with_command(sleep, vec!["30".to_string()]);
        let dir = tempfile::tempdir().unwrap();

        sessions.start(dir.path(), false, false, None).unwrap();
        sessions.start(dir.path(), false, false, None).unwrap();
        assert_eq!(
            sessions.status(None).unwrap()["sessions"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        sessions.shutdown_all();
        assert_eq!(
            sessions.status(None).unwrap()["sessions"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }
    /// `logs` / `stop` without a session id address the whole registry: the most
    /// recent session for logs, every session for stop.
    #[cfg(unix)]
    #[test]
    fn logs_and_stop_default_to_the_whole_registry() {
        let sleep = ["/bin/sleep", "/usr/bin/sleep"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
            .expect("a `sleep` binary is needed for long-lived children");
        let sessions = DevSessions::new().with_command(sleep, vec!["30".to_string()]);
        let dir = tempfile::tempdir().unwrap();

        let first = sessions.start(dir.path(), false, false, None).unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        let second = sessions.start(dir.path(), false, false, None).unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();
        assert_ne!(first, second, "each session gets its own id");

        // `logs` with no id reads the most recently started session.
        let logs = sessions.logs(None, 5).unwrap();
        assert_eq!(logs["session_id"], second);

        // An unknown id names the sessions that do exist.
        let err = sessions.status(Some("dev-999")).unwrap_err().to_string();
        assert!(
            err.contains(&first) && err.contains(&second),
            "the error should list the known sessions, got: {err}"
        );

        // `stop` with no id stops every session.
        let stopped = sessions.stop(None).unwrap();
        assert_eq!(stopped["status"], "stopped");
        assert_eq!(stopped["stopped"].as_array().unwrap().len(), 2);
        assert_eq!(
            sessions.status(None).unwrap()["sessions"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
    }

    /// A child that dies *after* the startup grace: `status` must report it as
    /// stopped, with its exit status, and `stop` must not pretend it killed it.
    #[cfg(unix)]
    #[test]
    fn status_reports_a_session_whose_child_died_after_start() {
        let sleep = ["/bin/sleep", "/usr/bin/sleep"]
            .iter()
            .map(PathBuf::from)
            .find(|p| p.exists())
            .expect("a `sleep` binary is needed for a long-lived child");
        let sessions = DevSessions::new().with_command(sleep, vec!["1".to_string()]);
        let dir = tempfile::tempdir().unwrap();

        let id = sessions.start(dir.path(), false, false, None).unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_string();

        // The child lives one second; let it exit on its own.
        std::thread::sleep(Duration::from_millis(1600));

        let status = sessions.status(Some(&id)).unwrap();
        assert_eq!(status["running"], false, "the child exited: {status}");
        assert!(
            status["exit_status"].is_string(),
            "an exited session reports how it exited: {status}"
        );

        let stopped = sessions.stop(Some(&id)).unwrap();
        assert_eq!(
            stopped["stopped"][0]["outcome"], "already_exited",
            "stop must not claim to have killed a process that was already gone"
        );
    }
}
