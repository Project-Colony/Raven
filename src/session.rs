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
//! The crash property survives intact. The mount is still owned by the anchor's
//! process tree; if the anchor dies the mount goes with it, and nothing is left
//! behind for the next boot to trip over.
//!
//! Joining needs no privilege, which is the fact this whole design rests on.
//! Kernel uids are absolute and a namespace only supplies a *view* of them: the
//! anchor maps `0 <uid> 1`, so a process of the same user that joins is seen as
//! uid 0 inside, exactly as the anchor is. Owning the user namespace grants
//! full capabilities within it, which is in turn what permits joining the mount
//! namespace. Hence the order below - user first, mount second - which is not
//! interchangeable.

use std::io::{BufRead, BufReader};
use std::os::fd::AsFd;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::{Error, env::Environment};

/// How long to wait for an anchor to mount and report itself.
const READY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

impl Environment {
    /// Where the anchor's pid is recorded.
    pub fn session_file(&self) -> PathBuf {
        self.root.join("session.pid")
    }

    /// The live anchor's pid, if a session is running.
    ///
    /// A recorded pid is not evidence on its own - it survives a reboot and
    /// pids are reused. The check that matters is whether that process is
    /// actually holding *this* environment's upper layer, which is the same
    /// test `holders` performs and is immune to both.
    pub fn session(&self) -> Option<u32> {
        let pid: u32 = std::fs::read_to_string(self.session_file())
            .ok()?
            .trim()
            .parse()
            .ok()?;
        self.holders().iter().any(|h| h.pid == pid).then_some(pid)
    }

    /// The pid of a session to join, starting one if none is running.
    pub fn ensure_session(&self) -> Result<u32, Error> {
        if let Some(pid) = self.session() {
            return Ok(pid);
        }
        // A dead session's file would otherwise sit there looking alive to a
        // reader that trusts it.
        let _ = std::fs::remove_file(self.session_file());

        // No session, but something else still holds the upper layer -
        // typically the Wine services of a session whose anchor has just died,
        // which outlive it by a few seconds. overlayfs will not mount the same
        // upper twice, so a new anchor would fail with a bare EBUSY. Name the
        // real cause instead: this is the pre-session error, and it is still
        // the right one here.
        self.ensure_not_running()?;
        self.start_session()
    }

    /// Starts an anchor and waits for it to report that the mount is up.
    fn start_session(&self) -> Result<u32, Error> {
        let exe = std::env::current_exe().map_err(|e| Error::Tool("raven", e))?;
        let mut cmd = Command::new(exe);
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
            .ok_or_else(|| Error::SessionFailed("the anchor produced no output".into()))?;
        let (tx, rx) = std::sync::mpsc::channel();
        // Read on a thread so a wedged anchor cannot hang the launch for ever.
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = tx.send(line);
        });

        match rx.recv_timeout(READY_TIMEOUT) {
            Ok(line) => match line.trim().strip_prefix("ready ") {
                Some(pid) => pid
                    .parse()
                    .map_err(|_| Error::SessionFailed(format!("unreadable anchor reply {line:?}"))),
                None if line.trim().is_empty() => {
                    let _ = child.wait();
                    Err(Error::SessionFailed(
                        "the anchor exited before mounting; run the same command with \
                         `raven env status` to see whether something still holds the layer"
                            .into(),
                    ))
                }
                None => Err(Error::SessionFailed(line.trim().to_string())),
            },
            Err(_) => {
                let _ = child.kill();
                Err(Error::SessionFailed(
                    "the anchor did not finish mounting in time".into(),
                ))
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
            let f =
                std::fs::File::open(&path).map_err(|e| Error::Layer(PathBuf::from(&path), e))?;
            move_into_link_name_space(f.as_fd(), Some(kind))
                .map_err(|e| Error::SessionFailed(format!("could not join {name}: {e}")))?;
        }
        Ok(())
    }

    /// Forgets a session's record. The processes are `stop`'s business.
    pub fn clear_session(&self) {
        let _ = std::fs::remove_file(self.session_file());
    }
}

#[cfg(test)]
mod tests {
    use crate::env::{Environment, Manifest};

    fn fake(name: &str) -> Environment {
        let root = std::env::temp_dir().join(format!("raven-sess-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("upper")).unwrap();
        Environment {
            name: name.into(),
            manifest: Manifest {
                base: "none".into(),
            },
            root,
        }
    }

    #[test]
    fn no_file_means_no_session() {
        let e = fake("none");
        assert_eq!(e.session(), None);
        let _ = std::fs::remove_dir_all(&e.root);
    }

    #[test]
    fn a_recorded_pid_that_holds_nothing_is_not_a_session() {
        // The exact hazard the holders check exists for: this process is alive
        // and has a valid pid, but holds no layer of ours. A reader trusting
        // the file alone would try to join a namespace that is not there.
        let e = fake("stale");
        std::fs::write(e.session_file(), format!("{}\n", std::process::id())).unwrap();
        assert_eq!(e.session(), None);
        let _ = std::fs::remove_dir_all(&e.root);
    }

    #[test]
    fn nonsense_in_the_file_is_not_a_session() {
        let e = fake("junk");
        std::fs::write(e.session_file(), "not a pid\n").unwrap();
        assert_eq!(e.session(), None);
        let _ = std::fs::remove_dir_all(&e.root);
    }
}
