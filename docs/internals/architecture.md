# Architecture

Why the design is what it is. The problem this design answers is in
[../project/landscape.md](../project/landscape.md); read that first if the
question "why not just use Wine" has not already been settled for you.

## The one constraint everything else follows from

A Windows program's instructions run natively on the CPU. Its *calls into the
operating system* have to land somewhere, and on Linux there is no NT kernel for
them to land in.

`ntdll.dll` is the library where that transition happens. On Windows it issues
`syscall` instructions into the NT kernel. Wine's `ntdll` implements the same
interface and issues Linux syscalls instead. `win32u.dll` is the same boundary
for the graphics and window-management half of the kernel.

Microsoft's `ntdll` on Linux would issue NT syscall numbers at a kernel that
assigns those numbers to entirely unrelated operations. There is no configuration,
no compatibility shim and no amount of engineering that changes this. **The
bottom of the stack is Wine's, permanently.**

Everything Raven does is above that line. The design question is not whether to
use Wine — it is what Wine should be looking at when it looks up.

## The layer model

```
   the program's own code          native x86, nothing between it and the CPU
   ────────────────────────────────────────────────────────────────────────
   application libraries           Microsoft's, from the real installation
   framework libraries             Microsoft's — ucrtbase, msvcr*, .NET, D3DX
   ────────────────────────────────────────────────────────────────────────
   the negotiable middle           ole32, rpcrt4, shell32, ws2_32, comctl32
                                   — some Microsoft's, some Wine's, MEASURED
   ────────────────────────────────────────────────────────────────────────
   coupled to the NT object model  kernel32, kernelbase, user32, gdi32,
                                   advapi32 — Wine's; they talk to wineserver
   the syscall boundary            ntdll, win32u — Wine's, by physics
   ────────────────────────────────────────────────────────────────────────
   NT semantics in the kernel      ntsync, where the kernel provides it
   the Linux kernel                the only kernel present
```

Two of these bands are settled by the constraint above. One is settled by
fifteen years of `winetricks` evidence that Microsoft's redistributable
frameworks work fine under Wine. **The middle band is unmeasured**, and
narrowing it is the research content of the project — see
[shadow-set.md](shadow-set.md).

## The five components

### 1. The base — a real Windows, deployed without a VM

An official Microsoft ISO contains `sources/install.wim`, a filesystem image.
`wimlib-imagex apply` writes it to a directory from Linux, with no hypervisor
and no boot:

```bash
wimlib-imagex apply install.wim 1 <base>/
```

The result is a genuine Windows tree — `Windows\System32` with Microsoft's
libraries, and `Windows\System32\config\` with the real registry hives. That it
never boots is the point: booting is what would bind it to hardware that is not
there.

A base is content-addressed by its Windows edition and build, stored once, and
shared by every environment built on it.

### 2. The mount stack — immutable base, disposable environments

The base is mounted **read-only**, always. Writes go to an `overlayfs` upper
layer, one per environment.

This is not caution for its own sake. It buys four properties at once: the base
cannot be corrupted by a program, environments are disposable by `rm -rf`, many
environments share one base without duplicating it, and a base on btrfs snapshots
for free. Full reasoning and the lifecycle in
[mount-stack.md](mount-stack.md).

### 3. The registry projection — hives in, Wine registry out

Windows stores the registry as binary hive files. Wine stores it as text. Raven
reads the former and writes the latter, **selectively**: the keys describing
installed software are projected; the keys describing hardware, drivers and the
NT service database are not, because importing them overwrites Wine's account of
its own synthetic environment and breaks the prefix outright.

The projection is derived, idempotent and driven by a rules file that is
reviewable — it is never hand-edited output. This is the same
source-of-truth-plus-generator shape the org already uses for design tokens, for
the same reason: a generated artifact someone edited by hand is an artifact
nobody can regenerate. See
[registry-projection.md](registry-projection.md).

### 4. The shadow set — the libraries Wine must win

The prefix must resolve the bottom two bands to Wine's implementations even
though Microsoft's are physically present in the mounted base. Wine's
`WINEDLLOVERRIDES` mechanism selects builtin-versus-native per library, and the
overlay's upper layer can shadow a file outright where the override is not
enough.

The shadow set is a data file, not a constant in the source. It is the thing the
project exists to shrink. See [shadow-set.md](shadow-set.md).

### 5. Launch — the kernel recognises `.exe`

`binfmt_misc` registers the PE magic (`MZ`) against a Raven handler, so
`./program.exe` executes like any other binary. The handler resolves which
environment the path belongs to, then execs Wine with that environment's prefix
and shadow set.

`binfmt_misc` is already mounted on a normal systemd machine. Wine has shipped
its own registration for years; Raven's differs only in pointing at a resolver
rather than at `wine` directly.

## The privilege boundary

Mounting filesystems, loading kernel modules and writing to
`/proc/sys/fs/binfmt_misc/register` all require `CAP_SYS_ADMIN`. Everything else
Raven does — parsing hives, computing a shadow set, deploying a WIM into a
user-owned directory, launching a program — requires nothing.

That asymmetry is the reason for a daemon, and it dictates the daemon's shape:

> **The daemon accepts named operations, never raw paths.** "Activate the
> environment called `skyrim`" is a request it can validate against its own
> registry of known environments. "Mount *this* over *that*" is a service that
> mounts anything anywhere as root, which is a privilege escalation vector
> wearing a project's name.

The daemon is therefore deliberately small and deliberately boring: it owns the
list of environments, and the only verbs are create, activate, deactivate and
destroy. Path construction, hive parsing, PE inspection and every other
interesting operation happen unprivileged, on the other side of the socket.

This is an attack surface, so the repository warrants a `SECURITY.md` before it
has users.

## Crate layout

Prefix `raven`, following
[the org layout rules](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/repository-layout.md).
A workspace is right here rather than a single crate, because the boundaries are
real ones: a privileged daemon is a separate process, and the launch handler is
on the hot path of every program start.

| Crate | `description` |
|---|---|
| `raven-paths` | Colony filesystem layout for Raven's bases, environments and cache |
| `raven-log` | Logging setup, shared so every Raven binary logs identically |
| `raven-proto` | Wire types for the Raven CLI-to-daemon protocol |
| `raven-core` | Environment model, shadow-set rules and configuration for Raven |
| `raven-hive` | Windows registry hive reader and Wine registry projector |
| `raven-daemon` | Privileged service performing Raven's mounts and binfmt registration |
| `raven` | The Raven command-line interface |

Seven crates, and each one earns it: `-paths` and `-log` are the two things every
binary needs identically, `-proto` crosses a process boundary, `-hive` wraps an
external format with its own test corpus, `-daemon` is the privileged process,
`-core` is the domain logic with the fewest dependencies, and `raven` is the
binary.

### Deliberately deferred

Each of these has a stated trigger, so adding it is a decision rather than a
drift:

| Crate | Added when |
|---|---|
| `raven-client` | a second consumer talks to the daemon; until then the CLI owns that code |
| `raven-launch` | measurement shows the CLI's start-up cost matters on the `binfmt` path |
| `raven-pe` | Raven needs to inspect a PE itself, rather than letting Wine decide |
| `raven-gui` | there is something worth showing; it consumes `colony-ui` and is Phase 3 |

### A note on `raven-paths`

The org's canonical path helper is `colony_ui::paths`, and using it would be the
correct instinct. It lives in `colony-ui`, which is an iced crate — so a
privileged daemon with no user interface would pull a GUI toolkit to compute
`~/.local/share/Colony/Raven/`.

Eidos hit this and solved it with its own `eidos-paths`. Raven does the same,
which means the ecosystem now has the helper duplicated twice. **The real fix is
a `colony-paths` crate that `colony-ui` re-exports**, and that belongs upstream
in Project-Colony-Resources rather than here. Recorded so it is a known
duplication rather than an accidental one.

## Language

Rust throughout. Phase 1 needs no C at all, which was not obvious in advance:

| Job | How |
|---|---|
| Reading binary registry hives | pure-Rust crate (`nt-hive` / `notatin`); read-only is sufficient, because Raven *writes* Wine's text registry, never a hive |
| Mount, overlay, namespaces | `rustix` / `nix` over the Linux syscalls |
| `binfmt_misc` registration | writing to `/proc`; the standard library |
| Deploying the WIM | the `wimlib-imagex` command, as a subprocess |
| PE inspection, when needed | `goblin` / `pelite`, pure Rust |

`wimlib` is a C library, but it ships a well-behaved command-line tool, and
binding it through FFI would buy nothing over invoking it — the interface is
"apply this image to this directory," which a subprocess expresses exactly.

The one place C could still appear is the shadow-set investigation: if
narrowing the middle band requires instrumenting Wine from the inside, that
instrument is a Wine DLL, and Wine is C. That is a research tool rather than a
shipped component, and it stays isolated from the workspace if it happens at all.

Dependency versions are not pinned in this document deliberately — the org rule
is that every dependency sits on its latest release at the moment it is added,
and a version written into prose is a version that starts rotting immediately.
