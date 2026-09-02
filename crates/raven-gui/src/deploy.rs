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

use iced::futures::Stream;
use iced::futures::channel::mpsc;

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
    iced::stream::channel(16, async move |output| {
        tokio::task::spawn_blocking(move || pump(&args, output))
            .await
            .expect("the blocking task panicked");
    })
}

/// Runs the child to completion, reporting wimlib's progress as it goes.
///
/// wimlib redraws its progress line in place with `\r` rather than emitting
/// one `\n`-terminated line per update, so each chunk read off the pipe is
/// split on both and only the last complete reading is reported - the ones
/// before it were already stale by the time the read returned.
fn pump(args: &Args, mut output: mpsc::Sender<Message>) {
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
        .spawn();

    let mut child = match child {
        Ok(child) => child,
        Err(_) => {
            let _ = output.try_send(Message::DeployDone(false));
            return;
        }
    };

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
            let _ = output.try_send(Message::DeployProgress(progress));
        }
    }

    let success = child.wait().map(|status| status.success()).unwrap_or(false);
    let _ = output.try_send(Message::DeployDone(success));
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

    // The brief's exact `.last()` triggers clippy::double_ended_iterator_last
    // (it would be `.next_back()` in code that ships); kept verbatim since the
    // brief's test text is authoritative, and allowed rather than edited.
    #[allow(clippy::double_ended_iterator_last)]
    #[test]
    fn a_carriage_return_stream_yields_the_last_complete_line() {
        // wimlib redraws in place with \r rather than emitting new lines.
        let chunk = "Extracting file data: 1 KiB of 10 KiB (10%) done\rExtracting file data: 5 KiB of 10 KiB (50%) done\r";
        let last = chunk.split('\r').filter_map(parse_progress).last();
        assert_eq!(last.map(|p| p.percent), Some(50));
    }
}
