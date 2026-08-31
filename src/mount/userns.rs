//! Mounting the overlay without root, inside an unprivileged user namespace.
//!
//! This is the primary backend and the only one implemented. It works because a
//! process that creates a user namespace holds full capabilities *inside* it,
//! which is enough to mount overlayfs — while holding nothing extra outside.
//! Nothing has to run as root, and no privileged service has to exist.
//!
//! The mount is only visible inside the namespace, which is the intended
//! behaviour rather than a limitation: child processes inherit it, so a launcher
//! starting a game needs no special handling, and the mount is destroyed with the
//! process tree that owns it.

use std::ffi::CString;
use std::fs;
use std::io::Write;

use rustix::mount::{MountFlags, MountPropagationFlags};
use rustix::thread::UnshareFlags;

use super::{MountBackend, MountError, OverlaySpec};

/// Native `overlayfs` inside an unprivileged user namespace.
pub struct UserNsOverlay;

impl MountBackend for UserNsOverlay {
    fn is_available() -> bool {
        // A kernel with unprivileged user namespaces disabled reports zero here.
        // `linux-hardened` and several distribution policies do exactly that.
        match fs::read_to_string("/proc/sys/user/max_user_namespaces") {
            Ok(s) => s.trim().parse::<u64>().unwrap_or(0) > 0,
            // The file is absent on kernels built without the feature at all.
            Err(_) => false,
        }
    }

    fn name(&self) -> &'static str {
        "overlayfs in an unprivileged user namespace"
    }

    /// Enters a new user and mount namespace, then mounts the overlay.
    ///
    /// This changes the calling process irreversibly: on return, the process is
    /// in namespaces it cannot leave. Callers are expected to be about to `exec`
    /// the program being launched.
    ///
    /// The process must be single-threaded — the kernel refuses `CLONE_NEWUSER`
    /// otherwise — which is why this is called early, before anything spawns a
    /// thread.
    fn mount(&self, spec: &OverlaySpec) -> Result<(), MountError> {
        spec.check()?;

        let uid = rustix::process::getuid().as_raw();
        let gid = rustix::process::getgid().as_raw();

        // SAFETY: `unshare` with CLONE_NEWUSER is only sound in a
        // single-threaded process — the kernel refuses it outright otherwise,
        // and rustix deprecates the safe wrapper for that reason. Raven calls
        // this from `main` before anything spawns a thread, and the doc comment
        // above states that requirement for any other caller.
        unsafe { rustix::thread::unshare_unsafe(UnshareFlags::NEWUSER | UnshareFlags::NEWNS) }
            .map_err(|e| MountError::NoUserNamespace(e.into()))?;

        // Order matters and the kernel enforces it: setgroups must be denied
        // before gid_map may be written by an unprivileged process.
        write_once("/proc/self/setgroups", "deny")?;
        write_once("/proc/self/uid_map", &format!("0 {uid} 1"))?;
        write_once("/proc/self/gid_map", &format!("0 {gid} 1"))?;

        // Without this, the mount could propagate back to the host namespace,
        // which would defeat the point of doing it in a namespace at all.
        rustix::mount::mount_change(
            "/",
            MountPropagationFlags::PRIVATE | MountPropagationFlags::REC,
        )
        .map_err(|e| MountError::Mount(e.into()))?;

        // rustix wants the option string as a C string, and a path containing
        // an interior NUL cannot reach the kernel at all.
        let options = CString::new(spec.options()).map_err(|_| {
            MountError::Mount(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a layer path contains a NUL byte",
            ))
        })?;

        rustix::mount::mount(
            "overlay",
            &spec.target,
            "overlay",
            MountFlags::empty(),
            options.as_c_str(),
        )
        .map_err(|e| MountError::Mount(e.into()))?;

        Ok(())
    }
}

/// The uid/gid map files accept exactly one write and reject a second one, so
/// this deliberately opens, writes and closes rather than holding a handle.
fn write_once(path: &str, contents: &str) -> Result<(), MountError> {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(MountError::IdMap)?;
    f.write_all(contents.as_bytes()).map_err(MountError::IdMap)
}
