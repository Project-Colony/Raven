//! Deploying a base, and reading how far it has got.
//!
//! `base deploy` is the only long operation in Raven - minutes over 143 886
//! files - so the GUI spawns `raven base deploy` rather than calling the
//! library, and reads wimlib's progress out of the child's output.
//!
//! No library change was needed for this. `.status()` inherits the parent's
//! stdio, so a piped stdout reaches `wimlib-imagex` unchanged, and wimlib does
//! not suppress progress when its output is not a terminal - verified before
//! this was written.
//!
//! The two shapes below are wimlib's own format strings, read from the
//! `wimlib-imagex` binary:
//!
//! ```text
//! Creating files: %lu of %lu (%u%%) done
//! Extracting file data: %lu %s of %lu %s (%u%%) done
//! ```

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, Stream};

use crate::Message;

/// How far a deployment has got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    /// 0 to 100.
    pub percent: u8,
    /// Which phase wimlib is in, as it names it.
    pub what: String,
}

/// Reads one line of wimlib output. `None` for anything that is not progress.
///
/// Matched by shape rather than by regex: the phase name, then a bracketed
/// percentage, then the word `done`. A stray percentage elsewhere in a line is
/// not progress and must not be read as any.
pub fn parse_progress(line: &str) -> Option<Progress> {
    let line = line.trim();
    let rest = line.strip_suffix(" done")?;
    let (head, pct) = rest.rsplit_once(" (")?;
    let percent: u8 = pct.strip_suffix("%)")?.parse().ok()?;
    let what = head.split_once(':')?.0;
    if what.is_empty() {
        return None;
    }
    Some(Progress {
        percent: percent.min(100),
        what: what.to_string(),
    })
}

/// The one line of a failed child's error output worth putting in a banner.
///
/// The last line is the one that says why - both Raven and `wimlib-imagex`
/// end with the sentence that matters. What follows it is usually blank, or a
/// remnant of a progress line wimlib redrew with `\r`, so empty pieces are
/// skipped rather than shown as an empty message.
pub fn last_meaningful_line(stderr: &str) -> Option<String> {
    stderr
        .split(['\r', '\n'])
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .map(str::to_owned)
}

/// What `raven base deploy` needs, gathered from the bases screen's three
/// text fields since this batch has no file dialog.
///
/// Doubles as the identity `Subscription::run_with` hashes on: while the same
/// deployment is in flight, the stream survives repeated `view` calls rather
/// than being torn down and restarted.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Args {
    pub image: PathBuf,
    pub edition: u32,
    pub name: String,
}

/// Spawns `raven base deploy` and turns its stdout into progress messages.
///
/// The intended target of `Subscription::run_with(args, deploy::run)`. Runs
/// the child on a blocking worker - spawning, reading a pipe and waiting are
/// all blocking calls - and forwards what it reads back through `output`.
///
/// `+ use<>` tells the 2024 capture rules the returned stream does not borrow
/// `args` - it is cloned on the next line - so the opaque type stays a plain
/// `S`, which is what `Subscription::run_with`'s `fn(&D) -> S` needs; without
/// it the type is `for<'a> fn(&'a Args) -> S<'a>`, which cannot coerce there.
pub fn run(args: &Args) -> impl Stream<Item = Message> + use<> {
    let args = args.clone();
    iced::stream::channel(16, async move |mut output| {
        let outcome = {
            let output = output.clone();
            tokio::task::spawn_blocking(move || pump(&args, output))
                .await
                .expect("the blocking task panicked")
        };
        // The outcome comes back as a value rather than being pushed down the
        // channel with the progress updates, so that it can be `await`ed here.
        // Dropping a progress update is right - they arrive faster than anyone
        // reads them. Dropping this one is not: `deploying` and `deploy_job`
        // would stay `Some` for ever, freezing the bar at whatever percentage
        // it had reached with the fields and the Deploy button disabled and no
        // way out of it. Awaiting waits for room instead of giving up.
        let _ = output.send(Message::DeployDone(outcome)).await;
    })
}

/// Runs the child to completion, reporting wimlib's progress as it goes, and
/// returns why it failed if it did.
///
/// wimlib redraws its progress line in place with `\r` rather than emitting
/// one `\n`-terminated line per update, so each chunk read off the pipe is
/// split on both and only the last complete reading is reported - the ones
/// before it were already stale by the time the read returned.
///
/// stderr is piped rather than inherited. A GUI started from an application
/// menu has nowhere to inherit it *to*, so the reason a deployment failed -
/// the one operation here that costs minutes - would go to no terminal at all
/// and the window would have nothing to say but "try it again by hand".
///
/// `Command::new("raven")` resolves through `$PATH`, not to this workspace's
/// own build. That's the right binary to run in the packaged case the GUI
/// ships for: the package installs the GUI and the CLI together at the same
/// version (see `packaging/PKGBUILD`), so whichever `raven` `$PATH` finds is
/// guaranteed to match. It is the wrong binary under `cargo run -p
/// raven-gui`: PATH still resolves to the system's installed `/usr/bin/raven`
/// rather than `target/debug/raven`, so a developer testing a CLI change
/// against a freshly built GUI silently deploys with the old, already-
/// installed CLI and sees no sign that happened.
fn pump(args: &Args, mut output: mpsc::Sender<Message>) -> Result<(), String> {
    let child = Command::new("raven")
        .arg("base")
        .arg("deploy")
        .arg("--image")
        .arg(&args.image)
        .arg("--edition")
        .arg(args.edition.to_string())
        .arg("--name")
        .arg(&args.name)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let mut child = match child {
        Ok(child) => child,
        Err(e) => return Err(format!("`raven base deploy` would not start: {e}")),
    };

    // stderr is drained by its own thread. The child writes to both pipes, and
    // a single reader parked on stdout would deadlock the moment stderr's pipe
    // filled - which is exactly what a failing deployment does.
    let mut stderr = child.stderr.take().expect("stderr was requested piped");
    let errors = std::thread::spawn(move || {
        let mut said = String::new();
        let _ = stderr.read_to_string(&mut said);
        said
    });

    let mut stdout = child.stdout.take().expect("stdout was requested piped");
    let mut buf = [0u8; 4096];
    loop {
        let n = match stdout.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let chunk = String::from_utf8_lossy(&buf[..n]);
        if let Some(progress) = chunk
            .split(['\r', '\n'])
            .filter_map(parse_progress)
            .next_back()
        {
            // Losing one of these is fine and intended: they arrive faster than
            // the window redraws, and the next one is already truer.
            let _ = output.try_send(Message::DeployProgress(progress));
        }
    }

    let success = child.wait().map(|status| status.success()).unwrap_or(false);
    let said = errors.join().unwrap_or_default();
    if success {
        Ok(())
    } else {
        Err(last_meaningful_line(&said)
            .unwrap_or_else(|| "It gave no reason before it stopped.".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_shapes_wimlib_emits_are_both_understood() {
        // Captured from `wimlib-imagex apply` with its output piped, which is
        // how the GUI will always see it.
        let extracting = parse_progress("Extracting file data: 3886 KiB of 3906 KiB (99%) done");
        assert_eq!(
            extracting,
            Some(Progress {
                percent: 99,
                what: "Extracting file data".into()
            })
        );
        let creating = parse_progress("Creating files: 12345 of 143886 (8%) done");
        assert_eq!(
            creating,
            Some(Progress {
                percent: 8,
                what: "Creating files".into()
            })
        );
    }

    #[test]
    fn anything_else_is_not_progress() {
        assert_eq!(parse_progress(""), None);
        assert_eq!(parse_progress("Applying image 1 to /home/x/base"), None);
        // A percentage that is not wimlib's shape must not be mistaken for one.
        assert_eq!(parse_progress("almost (50%) there"), None);
    }

    #[test]
    fn a_failure_is_reported_with_the_line_that_says_why() {
        // Raven's own wording, as `base deploy` writes it to stderr, followed
        // by the blank line a terminating newline leaves behind.
        let said = "Applying image 1 to /home/x/.local/share/raven/bases/win11\n\
                    error: no space left on device while writing Windows/System32\n";
        assert_eq!(
            last_meaningful_line(said).as_deref(),
            Some("error: no space left on device while writing Windows/System32")
        );
    }

    #[test]
    fn a_child_that_said_nothing_yields_nothing_to_show() {
        assert_eq!(last_meaningful_line(""), None);
        // wimlib redraws with \r, so a killed child can leave only whitespace.
        assert_eq!(last_meaningful_line("\r\n  \r\n"), None);
    }

    #[test]
    fn a_carriage_return_stream_yields_the_last_complete_line() {
        // wimlib redraws in place with \r rather than emitting new lines.
        let chunk = "Extracting file data: 1 KiB of 10 KiB (10%) done\rExtracting file data: 5 KiB of 10 KiB (50%) done\r";
        let last = chunk.split('\r').filter_map(parse_progress).next_back();
        assert_eq!(last.map(|p| p.percent), Some(50));
    }
}
