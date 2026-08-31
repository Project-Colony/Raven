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

## Privilege, and the mount backend

The instinct is that mounting filesystems needs root, and therefore that Raven
needs a privileged daemon. That was this design's original shape, and it was
wrong. It is worth stating what was measured, because it removed a whole
component:

| Operation | Needs root? |
|---|---|
| Mount the overlay | **No.** `unshare -Urm` puts it in a user namespace. |
| Does that mount leak to the host? | No — invisible outside the namespace. |
| Join the namespace later | **No.** `nsenter --preserve-credentials`. |
| Register `binfmt_misc` | Yes — but once, at install time. |

So the runtime path is unprivileged end to end: `unshare`, mount the overlay,
`exec` Wine. Child processes inherit the namespace, so a launcher starting a game
works without special handling, and the mount dies with the process tree that
owns it.

`binfmt_misc` is the exception, and it is a **packaging** concern rather than a
runtime one: a file in `/etc/binfmt.d/`, applied by `systemd-binfmt` at boot,
installed by the package manager that already holds root legitimately. Nothing
needs to hold privilege while Raven runs.

### Where this breaks, and the seam that anticipates it

Unprivileged user namespaces are exactly the feature hardened systems turn off.
`linux-hardened` on Arch disables them. Ubuntu restricts them through AppArmor by
default since 23.10. Debian did so for years. On SELinux-enforcing systems the
namespace works but the mount is subject to policy, and overlayfs has real
interactions with SELinux labelling.

None of that is speculative and none of it is fatal — **rootless Podman does
precisely this, on SELinux-enforcing systems, every day.** The known answers are
`fuse-overlayfs` where native overlayfs in a namespace is refused, and the
`context=` mount option for labelling.

Raven therefore does not call `unshare` and `mount` from wherever it happens to
need a filesystem. Acquiring a mounted C: is **one interface with room for three
backings**:

1. **native `overlayfs` in a user namespace** — the primary path, and the only
   one Phase 1 implements
2. **`fuse-overlayfs` in a user namespace** — where policy refuses the native one
3. **a privileged helper** — where unprivileged namespaces are unavailable
   entirely

Only the first exists at first. The seam exists from the first commit, so the
other two are additions rather than a rewrite — and the privileged helper, if it
is ever built, inherits the rule that was going to govern the daemon: it accepts
**named operations, never caller-supplied paths**, because a service that mounts
an arbitrary source onto an arbitrary target as root is a privilege escalation
vector wearing a project's name.

Detection is at runtime, and the diagnostic matters. "This kernel restricts
unprivileged user namespaces; Raven needs one of the following" is a useful
error. A bare `mount: operation not permitted` is not.

## Crate layout

Phase 1 is **one crate**, not a workspace.

[The org rule](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/repository-layout.md)
is that a workspace is for a real boundary — a separate process, a different
build target, a library something else genuinely consumes — and that splitting
by layer buys nothing but a dependency graph. Once the privileged daemon
disappeared, Raven became one program in one process, which is the case the rule
answers with a single crate and subsystems as directories under `src/`.

```
src/
├── main.rs        entry point
├── cli.rs         argument parsing; a thin shell over the library below
├── paths.rs       the Colony filesystem layout for Raven
├── base/          deploying and describing a Windows base
├── env/           the environment model: create, activate, destroy
├── mount/         the mount backends, behind one interface
├── hive/          registry hive reading and Wine .reg projection
└── shadow.rs      the shadow set, loaded from data
```

A directory earns its existence by holding more than one file, which is why
`shadow.rs` and `paths.rs` are files and `hive/` is not.

### The constraint that keeps a GUI possible

Everything above lives as a **library API**, and `cli.rs` is a thin shell over
it. This is not architectural decoration: a GUI is a second caller of the same
operations, and if the logic ends up inside argument handlers, adding one means
rewriting it. The cost of the rule now is a few function signatures; the cost of
skipping it is the GUI.

### When this becomes a workspace

| Split out | When |
|---|---|
| `raven-gui` | there is a model worth showing — it consumes `colony-ui`, so it is cheap once the model exists |
| `raven-daemon`, `raven-proto` | a privileged helper is needed for systems without unprivileged namespaces |
| `raven-launch` | measurement shows CLI start-up cost matters on the `binfmt` path |
| `raven-hive` | the hive corpus and its tests outgrow living alongside the binary |

Each has a trigger, so the split is a decision rather than a drift. None of them
is speculative — they are the four things already known to be coming.

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
