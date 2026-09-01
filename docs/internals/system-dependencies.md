# System dependencies

What Raven needs from the operating system, and a record of what its development
has installed on a machine.

Two things live here on purpose. The first is documentation — a contributor needs
to know what to install. The second is a log: this project installs system
packages while investigating, and a package that arrived without being written
down is a change nobody can audit or undo.

Package names are Arch's. Other distributions ship the same software under
different names, and that mapping is not written down yet because Raven has only
been developed on Arch.

## What Raven needs

### At runtime

| Package | Why |
|---|---|
| `wine` | the NT-to-Linux syscall bridge; not optional and not replaceable |
| `wimlib` | deploys a Windows base from an official ISO, with no VM and no boot |

### For the secondary "bring your own Windows partition" path

| Package | Why |
|---|---|
| `ntfsprogs`, `libntfs-3g` | reading an existing NTFS Windows installation |

### Where unprivileged `overlayfs` is refused

| Package | Why |
|---|---|
| `fuse-overlayfs` | the fallback mount backend on hardened kernels and under SELinux policy — see [architecture.md](architecture.md) |

### For development only

| Package | Why |
|---|---|
| `hivex` | reference implementation for registry hives; used to check Raven's own pure-Rust parser, never shipped as a dependency |

## Installation log

### 2026-08-31 — unblocking the Wine and WIM spikes

```
pacman -S --needed --noconfirm wine wimlib hivex fuse-overlayfs
```

Seven packages installed, nothing upgraded:

| Package | Version | How it arrived |
|---|---|---|
| `wine` | 11.16-2 | requested |
| `wimlib` | 1.14.5-3 | requested |
| `hivex` | 1.3.24-8 | requested |
| `fuse-overlayfs` | 1.18-1 | requested |
| `ntsync-autoload` | 1-1 | dependency of `wine` |
| `ntfsprogs` | 2026.7.7-1 | dependency of `wimlib` |
| `libntfs-3g` | 2026.7.7-1 | dependency of `ntfsprogs` |

Nothing else on the machine was touched. The system upgrade in the same day's
`pacman.log` — the 7.1 to 7.2 kernel jump and the rest — was not this project's,
and is not claimed here.

### 2026-08-31 — the Windows image for the base

Not a package, but it is the largest thing this project has pulled onto a
machine and it belongs in the same log.

| | |
|---|---|
| Image | Windows 11, version 25H2, multi-edition x64 ISO, English (United States) |
| File | `Win11_25H2_English_x64_v2.iso` |
| Size | 8 471 603 200 bytes |
| Source | `software.download.prss.microsoft.com`, reached from Microsoft's own download page |
| SHA256 | `768984706B909479417B2368438909440F2967FF05C6A9195ED2667254E465E3`, matching the value Microsoft publishes |
| Stored at | `<data>/Colony/Raven/iso/` |

Windows 11 rather than Windows 10, and the reasoning is worth keeping because it
is not the obvious choice: **Windows 10 reached end of support on 14 October
2025**. The shadow set is keyed by Windows build, so a measurement taken against
a build nobody can license fresh has a short shelf life. Windows 11 is also what
a user supplying their own Windows will have.

The consumer ISO was used rather than the Enterprise or LTSC evaluation, because
the evaluation route requires submitting a form with a name, email address and
company. The consumer download requires no account and no personal details —
only a language and edition selection — and publishes a SHA256 for every
language, which is what made verification possible.

Raven does not ship this image and never will; see
[../project/licensing.md](../project/licensing.md).

## What the dependencies revealed

Two findings that came out of reading what the package manager pulled in, rather
than from testing anything.

**`ntsync` is the distribution's job, not Raven's.** `ntsync-autoload` is a hard
dependency of Arch's `wine` package, and its entire content is
`/usr/lib/modules-load.d/10-ntsync.conf` — a line telling systemd to load the
module at boot. So on Arch, installing Wine already arranges for NT
synchronization primitives to be available, and Raven has no business managing
that. It should **detect** `/dev/ntsync` and report whether Wine is getting it,
not load modules on a user's behalf.

On the day of installation the module was not yet loaded — `modules-load.d`
is applied at boot. After the next boot `/dev/ntsync` appeared, and Wine
demonstrably uses it: nine open ntsync handles were counted on a running game
during the wineserver investigation.

**The NTFS tooling arrived for free.** `wimlib` depends on `ntfsprogs`, so the
secondary path — mounting an existing Windows partition — has its dependencies
present already. That does not make the path work; it removes one reason it
might not.
