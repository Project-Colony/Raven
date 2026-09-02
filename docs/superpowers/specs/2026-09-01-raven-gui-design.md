# raven-gui: the administration window

Design for Raven's first graphical interface. Approved 2026-09-01.

## Why now, and why this shape

The architecture reserved a `raven-gui` crate with a stated trigger: *"there is
a model worth showing - it consumes `colony-ui`, so it is cheap once the model
exists"*. Until today the trigger was not met, because the thing a GUI would
show was mostly unproven. That changed: Direct3D 11 renders through DXVK
against a real Windows, sessions exist, vkd3d installs beside DXVK, and there
is a corpus. There is now a model.

The argument that decided it, though, is not any of that. **The corpus only
grows through users, and users who will not type commands never arrive.** Seven
programs tested is too few to know anything, and no feature widens that number.
An interface does.

What a GUI cannot do is close the gap the README concedes to Lutris and
Bottles. Their advantage is not their window - it is years of per-title
recipes, runner management, winetricks, store integration. Raven has none of
that and this design does not chase it.

## Scope

**In:** environments, bases with real deployment progress, the Direct3D
runtimes, and doctor.

**Deferred to their own specs**, agreed at design time: the launch/loading
window for a double-clicked `.exe`, and the block-device picker for `attach`.
Both are separable, and shipping the administration window first means
something is usable at the end of the first batch.

**Out, permanently:** anything the CLI already does well. Creating an
environment is one command; listing them is one command. Implementing either
twice costs more than it returns.

## The foundation

`raven-gui` links the `raven` library and calls it directly. This is what the
architecture always intended - *"a GUI is a second caller of the same
operations"* - and it means no parsing of Raven's own human-facing output to
discover state.

Two operations cannot go through the library, and both reasons are structural:

- **Running a program.** `Environment::join_session` performs `setns` and the
  caller then `exec`s, replacing the process. A GUI calling that would kill
  itself. It spawns `raven run <env> -- wine <exe>` as a child instead.
- **Deploying a base.** Minutes of work over 143 886 files. It spawns
  `raven base deploy` as a child so the window stays responsive and progress
  can be read from it.

Everything else - listing, status, DXVK and vkd3d install/remove, attach and
detach, reproject, stop, start - is a library call.

### One library addition

`base::deploy` currently runs `wimlib-imagex apply` with `.status()`, which
inherits the terminal. The GUI needs its output. Add a variant that captures
it and reports progress; leave the existing function untouched so the CLI is
unaffected.

## Workspace and packaging

Raven is one crate today (`src/lib.rs` plus `src/main.rs`). It becomes:

```
Cargo.toml            workspace
crates/raven/         the library and the CLI binary, unchanged
crates/raven-gui/     the new binary
```

`colony-ui` is published on crates.io, so the dependency is a plain version:

```toml
colony-ui = "0.1.4"
```

This changed during the design. It was a git dependency on a tag when the
question was first asked, and the recommendation then was to leave it that way
until a second consumer had exercised the API. It was published instead, and
Eidos - its first consumer - moved to `colony-ui = "0.1.1"`. Raven follows the
convention that now exists rather than the one that did.

`colony-ui` 0.1.4 requires `iced ^0.14.0`, which is the version Colony already
uses, so the org stays on one iced rather than each repo drifting - the
outcome the Resources workspace comment asks for.

Naming the exact version is for the reader, not for Cargo: a `0.x` requirement
already admits any later `0.1.y`, so `"0.1.3"` would have resolved to 0.1.4 on
its own. The spec says which version it was written against so a future reader
knows what was true.

**What this touches outside the GUI**, which is the real cost of the split:

| | |
|---|---|
| `packaging/PKGBUILD` | builds and installs two binaries, plus a second desktop entry for `raven-gui`. The existing `raven.desktop` serves the `.exe` double-click and must not become the GUI launcher |
| `[profile.release]` | `lto = "fat"`, `codegen-units = 1` now apply to an iced binary too - much slower to compile, and to be accepted or adjusted deliberately |
| `release-please` | tracks `.` as one package. **One version for both**: the GUI is unusable against a library of a different age |
| the signing job | signs `raven-linux`; it must sign both binaries |

**The CLI does not change.** No command disappears, no behaviour moves. The GUI
is a second caller, not a replacement - losing the interface that works today
to bet on one that does not exist yet would be a poor trade.

## Screens

A sidebar in the Colony convention: the logo at 28x28 with the program name,
as `design/navigation.md` describes.

### Environments - the home screen

One card per environment: name, base, whether a session holds it, and what it
contains. **Start and Stop are on the card itself**, because starting is the
most frequent action and `env start` is what makes launches instant.

Selecting a card opens its detail:

- **Session** - running or not, which processes hold it, start and stop.
- **Direct3D** - DXVK and vkd3d-proton side by side: installed build, install
  from a file, remove. They are separate runtimes, not versions of one.
- **Devices** - what is attached, and detach. Attaching is deferred to the
  picker spec; until then the detail explains how and shows the CLI command.
- **Registry** - the environment's rules, and Reproject.

### Bases

The deployed Windows installations, and deployment - the only long operation in
Raven, and the reason a real progress bar exists at all.

### Doctor

The checks, with their consequences written out. Not green and red lights: the
CLI already reports *"absent - Wine falls back to wineserver for NT
synchronization"* rather than "no", and the window keeps that.

## Data flow

**Nothing runs on the interface thread.** Reading an environment's state opens
`/proc` for every process on the machine and stats dozens of files - enough to
stutter a window. Every library call goes through an iced `Task`.

**Refresh follows actions, not a clock.** One exception: while the environments
screen is open, a two-second poll shows who holds a session. Polling `/proc`
continuously for a window nobody is looking at would be waste.

**Long operations report through a channel.** `raven base deploy` runs as a
child; its `wimlib-imagex` output is parsed for a percentage and fed to the
window through an iced subscription. The parser gets a test pinned to real
wimlib output, following the precedent already set in `base.rs`, where a change
in wimlib's output shape fails a test rather than silently breaking.

## Error handling

Raven's error messages are written for a terminal and carry commands:

> `environment "games" has a live session holding its C:, which is what makes
> launches fast — release it: raven env stop games`

Telling someone using a window to open a terminal is an admission of failure.
**Errors with an obvious action become that action**: this one renders as "A
session is holding this environment" with a *Stop the session* button. Errors
without one show their message unchanged - the text is good, and inventing a
worse GUI-flavoured paraphrase would lose information.

One exception is deliberate and stays. `attach` prints a `setfacl` command it
refuses to run, because granting raw disk write access is the user's decision
and not Raven's. The window shows the same command with a *Copy* button and
does not run it either.

## Testing

Testing an iced widget tree is not attempted, and pretending otherwise would
produce tests that assert nothing. What is tested:

- the wimlib progress parser, against real captured output;
- the error-to-action mapping, which is a pure function over `raven::Error`;
- the state-to-view-model conversion.

The business logic is already covered by the library's 97 tests, and the GUI
adds none of its own.

## What this design deliberately does not decide

- **Memory cost of the window.** Raven's own footprint is 5 MB for an idle
  session and about 9 MB once Wine's services are up (measured; Wine's own
  services account for 220 MB either way). iced pulls in wgpu and will cost
  more than that. It should be measured once the window exists rather than
  guessed at now - and it does not affect launching a game, since the GUI need
  not be open for that.
- **Whether the GUI ever gains a program library.** Raven knows no programs
  today; inventing a shortcut registry is a separate question with its own
  design.
