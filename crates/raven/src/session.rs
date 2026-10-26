//! A persistent environment session: one namespace that launches join.
//!
//! Raven's mount lives in an unprivileged user namespace and dies with the
//! process tree that made it. That is the property which guarantees no stale
//! mounts after a crash - and it is also, measured, the largest thing between
//! Raven and feeling instant. Plain Wine keeps its `wineserver` alive between
//! launches and starts a program in 0.12 s; Raven tore the whole world down
//! after every run and paid ~2 s again. See
//! `docs/internals/performance.md`.
//!
//! So: the first launch starts an **anchor** - a Raven process that creates the
//! namespace, mounts the overlay and then does nothing at all except stay
//! alive. Later launches `setns` into that anchor's namespaces and run there,
//! sharing the mount *and* the wineserver already inside it.
//!
//! The crash property survives intact. The mount belongs to the namespace, and
//! the namespace lives as long as any process inside it: the anchor, the
//! session's persistent wineserver, Wine's services, the user's programs. If
//! the anchor dies, the server keeps the mount up until the next launch finds
//! it orphaned and stops it; nothing outlives a reboot.
//!
//! Joining needs no privilege, which is the fact this whole design rests on.
//! Kernel uids are absolute and a namespace only supplies a *view* of them: the
//! anchor maps `0 <uid> 1`, so a process of the same user that joins is seen as
//! uid 0 inside, exactly as the anchor is. Owning the user namespace grants
//! full capabilities within it, which is in turn what permits joining the mount
//! namespace. Hence the order below - user first, mount second - which is not
//! interchangeable.

use std::io::Read;
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::i18n::{t, tf};
use crate::{Error, env::Environment, paths};

/// How long to wait for an anchor to mount and report itself.
const READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

impl Environment {
    /// Where the anchor's pid is recorded: beside the mount point, in the
    /// runtime directory, so the record goes at reboot with the mount it
    /// describes.
    pub fn session_file(&self) -> Result<PathBuf, Error> {
        Ok(paths::runtime_dir(&self.name)?.join("session.pid"))
    }

    /// Where releases up to 0.5.0 recorded the pid, in the data directory.
    fn legacy_session_file(&self) -> PathBuf {
        self.root.join("session.pid")
    }

    /// The live anchor's pid, if a session is running.
    ///
    /// A recorded pid is not evidence on its own - it outlives an anchor that
    /// was killed, and pids are reused. The check that matters is whether that
    /// process is actually holding *this* environment's upper layer, which is
    /// the same test `holders` performs and is immune to both.
    pub fn session(&self) -> Option<u32> {
        // The legacy record is read too, so a launch right after an upgrade
        // joins the session the older release started instead of refusing
        // until it is stopped. A reboot ends every such session.
        let text = std::fs::read_to_string(self.session_file().ok()?)
            .or_else(|_| std::fs::read_to_string(self.legacy_session_file()))
            .ok()?;
        let pid: u32 = text.trim().parse().ok()?;
        self.holds(pid).then_some(pid)
    }

    /// Where the anchor records the pid of the wineserver it started.
    fn server_file(&self) -> Result<PathBuf, Error> {
        Ok(paths::runtime_dir(&self.name)?.join("wineserver"))
    }

    /// The session's persistent wineserver, if the anchor started one and it
    /// is still running. A recorded pid that is no longer a wineserver is a
    /// server that died or was killed, and the pid may be anybody's by now.
    fn server(&self) -> Option<u32> {
        let text = std::fs::read_to_string(self.server_file().ok()?).ok()?;
        let pid: u32 = text.trim().parse().ok()?;
        is_wineserver(pid).then_some(pid)
    }

    /// Whether this session's wineserver stays up between programs. Older
    /// releases started none, and starting one can fail; either way the
    /// session works as before and its services leave a few seconds after
    /// the last program.
    pub fn server_persistent(&self) -> bool {
        self.server().is_some()
    }

    /// Whether this session's wineserver is not the one a `wine` started
    /// here would use: replaced on disk since it started, as pacman does, or
    /// another Wine first on `PATH` now. A client cannot talk to a server of
    /// another version, and a persistent one never leaves to make room.
    pub fn server_replaced(&self) -> bool {
        self.server().is_some_and(|pid| {
            runs_a_deleted_binary(pid)
                || wineserver_binary().is_some_and(|ours| runs_another_binary(pid, &ours))
        })
    }

    /// The pid of a session to join, starting one if none is running.
    pub fn ensure_session(&self) -> Result<u32, Error> {
        if let Some(pid) = self.session()
            && !self.server_replaced()
        {
            return Ok(pid);
        }

        // Everything after this point is check-then-act, and two cold
        // launches arriving together - a file manager double-click registering
        // twice, a script starting two programs - would both see no session
        // and both start an anchor. Only one can mount, and the loser died on
        // a bare EBUSY. O_EXCL on a lock file settles which of them gets to
        // try. It sits with the record, in a directory the first launch has
        // to create.
        let run = paths::runtime_dir(&self.name)?;
        std::fs::create_dir_all(&run).map_err(|e| Error::Layer(run.clone(), e))?;
        let lock = run.join("session.lock");
        let held = loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
            {
                Ok(_) => break LockGuard(lock.clone()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Somebody else is starting one. Wait for their session
                    // rather than racing it, and fall back to trying
                    // ourselves if their attempt died without cleaning up.
                    if let Some(pid) = self.wait_for_session(&lock) {
                        return Ok(pid);
                    }
                    let _ = std::fs::remove_file(&lock);
                }
                Err(e) => return Err(Error::Layer(lock.clone(), e)),
            }
        };
        let started = self.replace_session();
        drop(held);
        started
    }

    /// Clears whatever stands in the way and starts a session. Called with
    /// the start lock held, so nothing seen here changes under it: before,
    /// a launch could stop a session another had just started, or tidy away
    /// the services of one still coming up.
    fn replace_session(&self) -> Result<u32, Error> {
        // Somebody may have started one while this launch waited for the lock.
        if let Some(pid) = self.session() {
            if !self.server_replaced() {
                return Ok(pid);
            }
            // Wine was upgraded under a live session. Its server is
            // persistent, so this is the normal state after any pacman run
            // and not a rare race. At rest the session is only Raven's and
            // Wine's own processes, and replacing it costs one cold start;
            // with a program of the user's in it, stopping would end that
            // program, so the choice is left to them.
            if self.holders().iter().all(crate::env::Holder::is_session) {
                self.stop()?;
            } else {
                return Err(Error::WineChanged(self.name.clone()));
            }
        }
        // A dead session's file would otherwise sit there looking alive to a
        // reader that trusts it.
        self.clear_session();

        // Its persistent server, too, outlives an anchor that was killed, and
        // never leaves on its own. With nothing of the user's among them,
        // its services are leftovers and only stand in the way of the mount.
        if crate::env::orphaned(&self.holders()) {
            self.stop()?;
        }

        // No session, but something else still holds the upper layer - a
        // program of the user's that outlived its session's anchor, say.
        // overlayfs will not mount the same upper twice, so a new anchor
        // would fail with a bare EBUSY. Name the real cause instead: this is
        // the pre-session error, and it is still the right one here.
        self.ensure_not_running()?;

        self.start_session()
    }

    /// Waits for somebody else's anchor to come up, or for them to give up.
    ///
    /// Both endings matter. Watching only for the session meant that when the
    /// holder failed - a renamed base, a kernel that refuses the mount - it
    /// dropped its lock within milliseconds and the waiter still sat out the
    /// full thirty seconds before trying anything itself. The lock going away
    /// is the holder saying it is no longer starting one, and it is the same
    /// signal that clears a lock left behind by a launcher that was killed.
    fn wait_for_session(&self, lock: &std::path::Path) -> Option<u32> {
        let deadline = std::time::Instant::now() + READY_TIMEOUT;
        while std::time::Instant::now() < deadline {
            // A session whose server is out of date is one the lock holder is
            // about to replace, not one to join.
            if let Some(pid) = self.session().filter(|_| !self.server_replaced()) {
                return Some(pid);
            }
            if !lock.exists() {
                return None;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        None
    }

    /// Starts an anchor and waits for it to report that the mount is up.
    fn start_session(&self) -> Result<u32, Error> {
        let mut cmd = Command::new(helper()?);
        cmd.arg("session-anchor")
            .arg(&self.name)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // Its own session, so closing the terminal that happened to start the
        // first game does not take the mount down with it.
        //
        // SAFETY: `pre_exec` runs between fork and exec, where only
        // async-signal-safe calls are permitted. `setsid` is one of them.
        unsafe {
            use std::os::unix::process::CommandExt as _;
            cmd.pre_exec(|| {
                rustix::process::setsid()
                    .map(|_| ())
                    .map_err(std::io::Error::from)
            });
        }
        let mut child = cmd.spawn().map_err(|e| Error::Tool("raven", e))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::SessionFailed(t("anchor_no_output").into()))?;

        // Read the readiness line WITHOUT a helper thread. This process is
        // about to `setns` into the anchor's user namespace, and the kernel
        // refuses `setns(CLONE_NEWUSER)` from a multi-threaded process with
        // EINVAL. A reader thread made that a race - it usually finished
        // exiting first, and when it did not the launch failed with a bare
        // "Invalid argument" that named nothing. Non-blocking reads and a
        // deadline keep the process single-threaded and still bounded.
        let line = read_line_before(&stdout, READY_TIMEOUT);
        drop(stdout);

        match line.trim().strip_prefix("ready ") {
            Some(pid) => pid.parse().map_err(|_| {
                Error::SessionFailed(tf("anchor_unreadable", &[("line", &format!("{line:?}"))]))
            }),
            None if line.trim().is_empty() => {
                // Either it died, or it never spoke. Reap it so it cannot
                // linger as a zombie, and let the caller see which.
                let _ = child.kill();
                let _ = child.wait();
                Err(Error::SessionFailed(t("anchor_silent").into()))
            }
            None => {
                let _ = child.wait();
                Err(Error::SessionFailed(line.trim().to_string()))
            }
        }
    }

    /// Moves **this process** into a session's namespaces.
    ///
    /// Irreversible: on return the process is inside namespaces it cannot
    /// leave, so callers are expected to be about to `exec`. The order is
    /// fixed - the user namespace grants the capability that admits us to the
    /// mount namespace, so it must come first.
    pub fn join_session(&self, anchor: u32) -> Result<(), Error> {
        use rustix::thread::{LinkNameSpaceType, move_into_link_name_space};
        for (kind, name) in [
            (LinkNameSpaceType::User, "user"),
            (LinkNameSpaceType::Mount, "mnt"),
        ] {
            let path = format!("/proc/{anchor}/ns/{name}");
            // Not Error::Layer: "preparing the layer failed" describes the
            // overlay, and the usual way to get here is a session stopped from
            // another terminal between the check and the join.
            let f = std::fs::File::open(&path).map_err(|e| {
                Error::SessionFailed(tf("session_vanished", &[("path", &path), ("why", &e)]))
            })?;
            move_into_link_name_space(f.as_fd(), Some(kind)).map_err(|e| {
                Error::SessionFailed(tf("session_no_join", &[("name", &name), ("why", &e)]))
            })?;
        }
        Ok(())
    }

    /// Brings Wine's services up, so the first launch does not pay for them.
    ///
    /// Mounting is the cheap half - hundredths of a second. The seconds a
    /// first launch pays are `services.exe` and the rest starting, and they
    /// only start when something runs, so the smallest possible program is
    /// run. The anchor's wineserver is persistent, so they then stay up until
    /// the session is stopped.
    ///
    /// Reports whether they came up. A failure is not the caller's problem:
    /// the mount is up either way and the next launch simply pays what this
    /// would have.
    pub fn warm_up(&self) -> bool {
        Command::new(match helper() {
            Ok(exe) => exe,
            Err(_) => return false,
        })
        .args(["run", &self.name, "--", "wine", "cmd", "/c", "exit"])
        .env("WINEDEBUG", "-all")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
    }

    /// Forgets a session's record. The processes are `stop`'s business.
    pub fn clear_session(&self) {
        if let Ok(file) = self.session_file() {
            let _ = std::fs::remove_file(file);
        }
        if let Ok(file) = self.server_file() {
            let _ = std::fs::remove_file(file);
        }
        let _ = std::fs::remove_file(self.legacy_session_file());
    }
}

/// The binary to re-run for the work the library cannot do in this process.
///
/// Two things are done by re-running Raven: holding a session open, and
/// mounting for a registry import. Both need a process that will be replaced
/// or will sleep for ever, so they cannot happen here - and both used to run
/// `current_exe()`, which is right for the launcher and wrong for the window.
/// The window can answer `session-anchor`, but `exec` would have it mount and
/// exec over itself, so it refuses; running the CLI beside it instead means
/// neither verb depends on which binary the user happened to start.
pub(crate) fn helper() -> Result<PathBuf, Error> {
    let exe = std::env::current_exe().map_err(|e| Error::Tool("raven", e))?;
    Ok(helper_binary(&exe))
}

/// `raven` next to `exe` if there is one, else `exe` itself.
///
/// The package installs both binaries into `/usr/bin` and a cargo build puts
/// both in `target/<profile>`, so the sibling exists in every layout Raven
/// ships or is developed in. Where it does not, re-running ourselves is the
/// old behaviour and still correct for the launcher.
fn helper_binary(exe: &std::path::Path) -> PathBuf {
    let sibling = exe.with_file_name("raven");
    if sibling.is_file() {
        sibling
    } else {
        exe.to_path_buf()
    }
}

/// Removes the start lock however the attempt ends.
struct LockGuard(PathBuf);
impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Reads one line from a pipe, giving up after `limit`, without a thread.
///
/// The fd is put in non-blocking mode and polled. A helper thread would make
/// this process multi-threaded, and `setns(CLONE_NEWUSER)` - which the caller
/// performs moments later - is refused outright to a multi-threaded process
/// with EINVAL. That was a race the first time this was written: the thread
/// usually finished exiting before the join, and when it did not the launch
/// failed with a bare "Invalid argument" that named nothing.
fn read_line_before(pipe: &std::process::ChildStdout, limit: std::time::Duration) -> String {
    use std::os::fd::AsFd as _;
    let _ = rustix::io::ioctl_fionbio(pipe.as_fd(), true);
    let deadline = std::time::Instant::now() + limit;
    let mut out = String::new();
    let mut buf = [0u8; 256];
    // SAFETY: the File is wrapped in ManuallyDrop, so it never closes the fd
    // it borrows - ownership stays with the ChildStdout the caller holds.
    let mut fd = unsafe {
        use std::os::fd::{AsRawFd as _, FromRawFd as _};
        std::mem::ManuallyDrop::new(std::fs::File::from_raw_fd(pipe.as_raw_fd()))
    };
    while std::time::Instant::now() < deadline {
        match fd.read(&mut buf) {
            Ok(0) => break, // the anchor exited and closed the pipe
            Ok(n) => {
                out.push_str(&String::from_utf8_lossy(&buf[..n]));
                if out.contains('\n') {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }
    out
}

/// Holds a namespace open so later launches can join it.
///
/// Mounts, reports itself on stdout, then does nothing for as long as it is
/// wanted. It must stay single-threaded until the mount is done - the kernel
/// refuses `CLONE_NEWUSER` to a threaded process - which is why the readiness
/// line is written only afterwards, and why a binary has to reach this before
/// starting any runtime of its own.
///
/// In the library rather than the launcher because either binary may end up
/// running it: `start_session` prefers the `raven` beside it, and falls back
/// to whichever binary asked. The window asks too, and until it could answer,
/// its Start started a second window instead, said nothing on the pipe, and
/// was killed thirty seconds later with a message blaming the
/// kernel.
pub fn anchor(name: &str) -> ! {
    use crate::mount::MountBackend as _;
    use std::io::Write as _;
    // Every failure has to leave through stdout: the launcher reads that pipe
    // and nothing else, and the anchor's stderr goes to /dev/null because it
    // outlives the terminal that started it. An error on stderr would reach
    // nobody, and the launcher would report only silence.
    let report = |what: std::fmt::Arguments<'_>| -> ! {
        println!("{what}");
        let _ = std::io::stdout().flush();
        std::process::exit(1);
    };
    let e = match crate::env::Environment::open(name) {
        Ok(e) => e,
        Err(err) => report(format_args!("{}", crate::i18n::error_message(&err))),
    };
    let spec = match e.spec() {
        Ok(s) => s,
        Err(err) => report(format_args!("{}", crate::i18n::error_message(&err))),
    };
    let record = match e.session_file() {
        Ok(f) => f,
        Err(err) => report(format_args!("{}", crate::i18n::error_message(&err))),
    };
    if let Err(err) = std::fs::create_dir_all(&spec.target) {
        report(format_args!(
            "{}",
            tf(
                "anchor_no_mount_point",
                &[("path", &spec.target.display()), ("why", &err)]
            )
        ));
    }
    // Bring the layer's opaque markers in line with the current shadow set
    // before mounting. An environment created when Windows/Fonts was masked
    // would otherwise stay masked for ever, and a user should not have to
    // rebuild to receive a fix. The same goes for Wine's mscoree.dll and
    // .winmd files, which older environments still carry. The base is the
    // second lower layer, under the Wine layer.
    if let Err(err) = crate::layer::reconcile(&e.layer(), &e.upper(), &spec.lower[1]) {
        report(format_args!(
            "{}",
            tf("anchor_no_reconcile", &[("why", &because(&err))])
        ));
    }
    if !crate::mount::UserNsOverlay::is_available() {
        report(format_args!("{}", t("userns_restricted")));
    }
    if let Err(err) = crate::mount::UserNsOverlay.mount(&spec) {
        report(format_args!("{}: {}", t("could_not_mount"), because(&err)));
    }
    // Before the record: a launch that finds the record joins at once, and
    // one that ran a program before the server existed would have started an
    // ordinary server, which `-p` cannot then make persistent. Until the
    // record exists a launch waits on the start lock instead.
    start_server(&e);
    let pid = std::process::id();
    // Written only after the mount exists, so a reader of this file never
    // sees a session that cannot be joined. Its directory is the mount
    // point's parent, created above.
    if let Err(err) = std::fs::write(&record, format!("{pid}\n")) {
        report(format_args!(
            "{}",
            tf(
                "anchor_no_record",
                &[("path", &record.display()), ("why", &err)]
            )
        ));
    }
    println!("ready {pid}");
    let _ = std::io::stdout().flush();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

/// Starts the session's wineserver, persistent, before any program does.
///
/// Started by a program, wineserver exits a few seconds after the last one,
/// and Wine's services go with it: a launch after a pause paid their whole
/// start again, about 1.9 s against 0.25 s. `-p` keeps the server, and so the
/// services, for as long as the session lives; `env stop` ends it with the
/// rest. That costs about 50 MiB while the session is up.
///
/// Best effort. If it cannot start, the first program starts an ordinary
/// server and the session behaves as it did before. The same goes for a
/// server that dies later, killed by the OOM killer or by a `wineserver -k`
/// run inside the session: the next program starts an ordinary one, and the
/// session stays without persistence until it is stopped. stdout is the
/// launcher's readiness pipe, so the server must not inherit it.
fn start_server(e: &Environment) {
    let Some(bin) = wineserver_binary() else {
        return;
    };
    let started = Command::new(&bin)
        .arg("-p")
        .env("WINEPREFIX", e.prefix())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !started {
        return;
    }
    // The server forks into the background, so the pid to keep is not the
    // one just spawned. It is the only one in this new namespace.
    let server = e.holders().into_iter().find(|h| h.comm == "wineserver");
    if let (Some(server), Ok(file)) = (server, e.server_file()) {
        let _ = std::fs::write(file, format!("{}\n", server.pid));
    }
}

/// The wineserver of the `wine` on `PATH`, when it can be told for certain.
///
/// Wine starts the server from its own install tree, so the server is the
/// one beside the real `wine` binary. A `wine` that is a wrapper script has no
/// server beside it, and guessing from `WINESERVER` or `PATH` could start a
/// server of another version, which every program would then refuse to talk
/// to for as long as the session lives. Then there is no persistent server,
/// and the first program starts its own as before.
fn wineserver_binary() -> Option<PathBuf> {
    let wine = std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join("wine"))
        .find(|p| p.is_file())?;
    Some(
        std::fs::canonicalize(wine)
            .ok()?
            .with_file_name("wineserver"),
    )
    .filter(|p| p.is_file())
}

fn is_wineserver(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "wineserver")
}

/// Whether a process runs another file than `ours`. The kernel's link names
/// the file it resolved, so `ours` is resolved too: a `wineserver` that is a
/// symlink, as some distributions install it, is still the same server.
fn runs_another_binary(pid: u32, ours: &std::path::Path) -> bool {
    let Ok(ours) = std::fs::canonicalize(ours) else {
        return false;
    };
    std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|exe| exe != ours)
}

/// Whether a process runs a binary that has since been deleted. pacman
/// installs a new file rather than rewriting the old one, and the kernel marks
/// the link of a process still running the old one.
fn runs_a_deleted_binary(pid: u32) -> bool {
    use std::os::unix::ffi::OsStrExt as _;
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .is_ok_and(|exe| exe.as_os_str().as_bytes().ends_with(b" (deleted)"))
}

/// An error and the causes underneath it, as one line.
///
/// `Error::Layer` reads "preparing the layer failed at <path>" and keeps the
/// errno as a source, which anyhow prints for the CLI and `Display` drops.
/// An anchor's line is all the launcher ever shows, and "permission denied"
/// is the difference between a bug and a chmod.
fn because(e: &(dyn std::error::Error + 'static)) -> String {
    let mut out = crate::i18n::describe(e);
    let mut source = e.source();
    while let Some(s) = source {
        // Some messages already name their cause (a mount error's names the
        // errno), and appending it again prints it twice.
        let cause = s.to_string();
        if !out.contains(&cause) {
            out.push_str(&format!(": {cause}"));
        }
        source = s.source();
    }
    out
}

#[cfg(test)]
mod server_tests {
    use super::{is_wineserver, runs_a_deleted_binary, runs_another_binary};

    /// Runs a binary just copied, as a stand-in server. Another test's fork
    /// can still hold the copy's write descriptor for a moment, and exec
    /// then fails with ETXTBSY; and spawn can return a moment before the
    /// kernel renames the child after its binary. Both are the test harness,
    /// not the code, so both are waited out.
    fn start(bin: &std::path::Path) -> std::process::Child {
        for _ in 0..100 {
            match std::process::Command::new(bin).arg("30").spawn() {
                Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {}
                other => {
                    let child = other.unwrap();
                    for _ in 0..100 {
                        if is_wineserver(child.id()) {
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    return child;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("{} stayed busy", bin.display());
    }

    #[test]
    fn a_server_whose_binary_was_replaced_is_told_apart() {
        // A persistent server outlives a Wine upgrade, and a client of the new
        // Wine cannot talk to it ("version mismatch"). pacman removes the old
        // file, which is what has to be caught. A copy of sleep named
        // wineserver stands in for the server.
        let dir = std::env::temp_dir().join(format!("raven-ws-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("wineserver");
        std::fs::copy("/usr/bin/sleep", &bin).unwrap();
        let mut server = start(&bin);
        let pid = server.id();

        assert!(is_wineserver(pid));
        assert!(!runs_a_deleted_binary(pid), "unchanged");
        std::fs::remove_file(&bin).unwrap();
        assert!(runs_a_deleted_binary(pid), "removed like an upgrade");

        let _ = server.kill();
        let _ = server.wait();
        assert!(!is_wineserver(pid), "gone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_server_started_through_a_symlink_is_still_ours() {
        // The kernel reports the file it resolved, wineserver64 here, while
        // the server was started, and is looked up, as wineserver.
        let dir = std::env::temp_dir().join(format!("raven-wsl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy("/usr/bin/sleep", dir.join("wineserver64")).unwrap();
        std::os::unix::fs::symlink("wineserver64", dir.join("wineserver")).unwrap();
        std::fs::copy("/usr/bin/sleep", dir.join("other")).unwrap();
        let mut server = start(&dir.join("wineserver"));
        let pid = server.id();

        assert!(is_wineserver(pid));
        assert!(!runs_another_binary(pid, &dir.join("wineserver")));
        assert!(runs_another_binary(pid, &dir.join("other")), "another Wine");

        let _ = server.kill();
        let _ = server.wait();
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod because_tests {
    use super::because;

    #[test]
    fn a_cause_the_message_already_gives_is_not_repeated() {
        // The mount error's own message names the errno, and so does its
        // source. Printed twice, the one line a launcher shows read
        // "Device or resource busy (os error 16): Device or resource busy
        // (os error 16)".
        let busy = crate::mount::MountError::Mount(std::io::Error::from_raw_os_error(16));
        let line = because(&busy);
        assert_eq!(line.matches("os error 16").count(), 1, "{line}");
    }

    #[test]
    fn a_cause_the_message_leaves_out_is_still_given() {
        let denied = crate::Error::Layer("/tmp/x".into(), std::io::Error::from_raw_os_error(13));
        let line = because(&denied);
        assert!(line.contains("os error 13"), "{line}");
    }
}

#[cfg(test)]
mod helper_tests {
    use super::helper_binary;

    #[test]
    fn the_helper_is_the_cli_beside_us_when_there_is_one() {
        // The library re-runs its own binary for `session-anchor` and for the
        // `exec` behind a registry import. From the window that would be the
        // window - which answers neither well - so a `raven` next to it is
        // preferred. The package installs both into /usr/bin and a cargo
        // build puts both in target/<profile>, so the sibling is there in
        // every layout Raven ships or is developed in.
        let dir = std::env::temp_dir().join(format!("raven-helper-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let gui = dir.join("raven-gui");
        std::fs::write(&gui, b"").unwrap();

        // No sibling yet: the only thing to re-run is ourselves.
        assert_eq!(helper_binary(&gui), gui);

        let cli = dir.join("raven");
        std::fs::write(&cli, b"").unwrap();
        assert_eq!(
            helper_binary(&gui),
            cli,
            "the CLI beside us answers both verbs"
        );
        // The CLI itself is its own helper, not a self-reference through a
        // path that happens to have the same name.
        assert_eq!(helper_binary(&cli), cli);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tests {
    use crate::env::{Environment, Manifest};
    use crate::paths;

    /// An environment whose name no real one has, so its runtime directory,
    /// which these tests write to, is theirs alone.
    fn fake(name: &str) -> Environment {
        let name = format!("raven-sess-{name}-{}", std::process::id());
        let root = std::env::temp_dir().join(&name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("upper")).unwrap();
        Environment {
            name,
            manifest: Manifest {
                base: "none".into(),
            },
            root,
        }
    }

    fn record(e: &Environment, text: &str) {
        let file = e
            .session_file()
            .expect("XDG_RUNTIME_DIR is set wherever Raven can run");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    fn remove(e: &Environment) {
        let _ = std::fs::remove_dir_all(&e.root);
        if let Ok(run) = paths::runtime_dir(&e.name) {
            let _ = std::fs::remove_dir_all(run);
        }
    }

    #[test]
    fn no_file_means_no_session() {
        let e = fake("none");
        assert_eq!(e.session(), None);
        remove(&e);
    }

    #[test]
    fn a_recorded_pid_that_holds_nothing_is_not_a_session() {
        // The exact hazard the holders check exists for: this process is alive
        // and has a valid pid, but holds no layer of ours. A reader trusting
        // the file alone would try to join a namespace that is not there.
        let e = fake("stale");
        record(&e, &format!("{}\n", std::process::id()));
        assert_eq!(e.session(), None);
        remove(&e);
    }

    #[test]
    fn nonsense_in_the_file_is_not_a_session() {
        let e = fake("junk");
        record(&e, "not a pid\n");
        assert_eq!(e.session(), None);
        remove(&e);
    }

    #[test]
    fn the_record_is_runtime_state_beside_the_mount() {
        // A session dies at reboot with its mount, so its record goes with
        // them: under the runtime directory, never in the data directory
        // where it would outlive both.
        let e = fake("where");
        let file = e
            .session_file()
            .expect("XDG_RUNTIME_DIR is set wherever Raven can run");
        assert_eq!(file.parent(), paths::mount_point(&e.name).unwrap().parent());
        assert!(!file.starts_with(paths::data_dir().unwrap()));
        remove(&e);
    }

    #[test]
    fn clearing_forgets_the_legacy_record_too() {
        let e = fake("legacy");
        record(&e, "1\n");
        let legacy = e.root.join("session.pid");
        std::fs::write(&legacy, "1\n").unwrap();
        e.clear_session();
        assert!(!e.session_file().unwrap().exists());
        assert!(!legacy.exists());
        remove(&e);
    }
}
