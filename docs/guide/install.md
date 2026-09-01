# Installing

On Arch, a package builds from [`packaging/PKGBUILD`](../../packaging/PKGBUILD);
everywhere else, build from source. **Installing the package changes what every
`.exe` on the machine does** — the kernel hands them to Raven instead of Wine's
default prefix — and uninstalling reverses it. `wine program.exe` keeps working
either way.

## What the system needs

| | |
|---|---|
| Rust 1.85+ | to build |
| `wine` | the NT-to-Linux bridge — not optional, not replaceable |
| `wimlib` | deploys a Windows base from an ISO without a VM |
| a kernel allowing unprivileged user namespaces | how Raven mounts without root |

`ntfsprogs` is needed only to read an existing Windows partition.
`fuse-overlayfs` is reserved for a fallback backend on hardened kernels that is
designed but not yet built — installing it does nothing today. Neither
is needed for the normal path. Details in
[../internals/system-dependencies.md](../internals/system-dependencies.md).

## The package (Arch)

```bash
git clone https://github.com/Project-Colony/Raven
cd Raven/packaging
makepkg -si
```

This installs `raven` (and the `rvn` alias), registers `.exe` files with the
kernel, masks Wine's own registration so the two never fight over the same
magic, and adds the desktop entry that makes file managers open `.exe` files
through Raven. pacman's own hook applies the registration in the same
transaction; removing the package restores Wine's handler.

## From source (everywhere else)

```bash
sudo pacman -S --needed wine wimlib rust   # or your distribution's equivalent
git clone https://github.com/Project-Colony/Raven
cd Raven
cargo build --release
```

A source build runs fine, but `./program.exe` needs the kernel registration —
`raven binfmt` prints exactly what to install and where.

## Check the machine can run it

```bash
raven doctor
```

It reports whether unprivileged user namespaces are available, whether Wine is
found, whether `/dev/ntsync` is present, what is already deployed — and **who
actually gets a double-clicked `.exe`**: every `binfmt_misc` registration that
claims one, which the kernel will pick, and the fix when it is not Raven's. If
user namespaces are restricted — `linux-hardened`, or Ubuntu's AppArmor policy
— Raven's only implemented mount backend cannot work, and `doctor` says so
rather than failing later with a bare permission error.

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
