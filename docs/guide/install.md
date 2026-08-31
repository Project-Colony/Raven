# Installing

Raven is not packaged yet. Building from source is the only route.

## What the system needs

| | |
|---|---|
| Rust 1.85+ | to build |
| `wine` | the NT-to-Linux bridge — not optional, not replaceable |
| `wimlib` | deploys a Windows base from an ISO without a VM |
| a kernel allowing unprivileged user namespaces | how Raven mounts without root |

`ntfsprogs` is needed only to read an existing Windows partition, and
`fuse-overlayfs` only where a hardened kernel refuses the native mount. Neither
is needed for the normal path. Details in
[../internals/system-dependencies.md](../internals/system-dependencies.md).

On Arch:

```bash
sudo pacman -S --needed wine wimlib rust
```

## Build

```bash
git clone https://github.com/Project-Colony/Raven
cd Raven
cargo build --release
```

## Check the machine can run it

```bash
./target/release/raven doctor
```

It reports whether unprivileged user namespaces are available, whether Wine is
found, whether `/dev/ntsync` is present, and what is already deployed. If user
namespaces are restricted — `linux-hardened`, or Ubuntu's AppArmor policy —
Raven's only implemented mount backend cannot work, and `doctor` says so rather
than failing later with a bare permission error.

`ntsync` reported as absent is not a problem. It is a Wine performance feature
the distribution arranges, not something Raven needs or should load on your
behalf.

## A Windows to run against

Raven ships no Microsoft software and never will. You supply a Windows and
license it yourself.

An official ISO from Microsoft contains `sources/install.wim`. Extract that one
file — the rest of the ISO is installer scaffolding Raven does not use:

```bash
7z e Win11_x64.iso sources/install.wim -o.
```

Then continue with [usage.md](usage.md).
