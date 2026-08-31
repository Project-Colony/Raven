<div align="center">

**A real Windows installation, mounted as C:, with its programs launched from Linux.**

</div>

[![License: GPL-3.0-or-later](https://img.shields.io/badge/License-GPL--3.0--or--later-blue.svg)](LICENSE)
[![Colony app](https://img.shields.io/badge/Colony-system-purple)](https://github.com/Project-Colony/Colony)
[![Platform](https://img.shields.io/badge/platform-linux-lightgrey)](#installation)

Wine and Proton run Windows programs against a *synthetic* Windows: a prefix Wine
created, a registry Wine wrote as a text file, and reimplementations of the
libraries a program expects to find. It works remarkably well, and it is why the
prefix is disposable. It also means the program's whole world is a reconstruction.

Raven keeps Wine where Wine is irreplaceable — translating NT calls into Linux
syscalls — and replaces everything above it with a genuine Windows installation
that you deploy, mount read-only, and write to through an overlay. The registry
comes from real hives. The libraries are Microsoft's, except for the precise set
that physically cannot be.

> **Status:** design only. Nothing is implemented, nothing runs, and no code has
> been written. This repository currently contains the architecture and the
> reasoning behind it — see [docs/](docs/). The open questions that could still
> change the design are listed in
> [docs/project/status.md](docs/project/status.md), and they are real questions,
> not formalities.

## Why Raven

What you would otherwise reach for, and where it stops:

- **Wine / Proton** — the program runs at native speed, but inside an invented
  Windows. Software that reads its own installation state, resolves COM servers
  it registered at install time, or expects a library Wine has only partly
  reimplemented, finds an environment that does not quite add up.
- **A virtual machine** — perfect fidelity, because it is really Windows. It is
  also a second computer: its own RAM, its own filesystem, its own GPU story, and
  a window that is a screen rather than an application. That is isolation, which
  is the opposite of what Raven is for.
- **Bottles, Lutris, umu** — excellent at managing Wine prefixes. They manage the
  synthetic Windows better; they do not replace it.

Raven's position is the one nobody occupies:

```
                    program's own code   →  runs natively on your CPU
                    ─────────────────────────────────────────────────
  Wine / Proton     invented Windows     →  Wine's prefix, text registry
  Virtual machine   real Windows         →  behind a hypervisor, isolated
  Raven             real Windows         →  mounted directly, as your C:
                    ─────────────────────────────────────────────────
                    NT → Linux syscalls  →  Wine, in every case; no alternative
```

## What it does

- **Deploys a real Windows without a VM and without booting it.** An official
  Microsoft ISO carries `sources/install.wim`; `wimlib` applies it straight to a
  directory from Linux. No hypervisor, no installer, no first-boot.
- **Mounts that installation read-only, and writes through an overlay.** The base
  is immutable and shared; every environment is an `overlayfs` upper layer on top
  of it. Discarding an environment is deleting a directory, and the base is
  incapable of being damaged by anything a program does.
- **Projects the real registry.** Windows keeps the registry in binary hives;
  Wine keeps it as text. Raven reads the hives and projects the parts that
  describe *software* into the prefix, deliberately leaving out the parts that
  describe *hardware and drivers* that do not exist here.
- **Shadows only the libraries it must.** `ntdll` and `win32u` are the boundary
  where Windows talks to a kernel that is not present; those are Wine's, and no
  design can change that. How far above them Microsoft's own libraries can be
  used is an open measurement, and it is the question Raven exists to answer.
- **Registers `.exe` with the kernel.** `binfmt_misc` makes `./program.exe` an
  executable like any other, resolved to the environment it belongs to.

## Installation

Not released. There is no binary, no package, and no source to build yet.

When there is, this section will lead with Colony, then the AUR, then a direct
download, following [the org convention](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/documentation.md).

## Documentation

Full documentation is in [docs/](docs/) — start at [docs/README.md](docs/README.md).

The two pages that carry the argument are
[project/landscape.md](docs/project/landscape.md), for why this is worth
building at all, and [internals/architecture.md](docs/internals/architecture.md),
for how it is put together.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

Raven never distributes Microsoft software. It operates on a Windows installation
that you supply and license yourself; see
[docs/project/licensing.md](docs/project/licensing.md).
