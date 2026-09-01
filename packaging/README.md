# Packaging files

What the package installs beyond the binary, wired together by
[`PKGBUILD`](PKGBUILD); the reasoning behind each file is in
[../docs/internals/packaging.md](../docs/internals/packaging.md).

| File | Installed to | Why |
|---|---|---|
| `raven.conf` | `/usr/lib/binfmt.d/` | makes the kernel hand `.exe` files to Raven, so `./program.exe` runs from a shell — pacman's `systemd-binfmt` hook applies it in the same transaction |
| `raven.desktop` | `/usr/share/applications/` | makes a **file manager** open them, which `binfmt` alone does not do |
| `wine-mask.conf` | `/etc/binfmt.d/wine.conf` | disables Wine's own `.exe` registration, which otherwise wins — an `/etc` file shadows Wine's `/usr/lib` one by name without touching a file the wine package owns |

The two are needed for different things and neither replaces the other.
`binfmt_misc` answers "the kernel is asked to execute this file"; a desktop entry
answers "a person double-clicked this file", which a file manager resolves
through MIME types without ever asking the kernel to execute anything.

## Raven cannot share the registration with Wine

Wine's own package installs `/usr/lib/binfmt.d/wine.conf`, registering `:DOSWin:`
for the same `MZ` magic. Two handlers for one magic is not a merge — the kernel
silently picks the most recently registered one (verified by experiment:
last-registered wins), so a Wine package update can take every `.exe` back at
any time. When Wine's wins, every double-clicked `.exe` runs against the
default `~/.wine` prefix, in an environment that has none of what Raven set up,
and the failure looks like a Raven bug rather than a registration conflict.
`raven doctor` names every claimant and the winner.

`wine-mask.conf` is installed as `/etc/binfmt.d/wine.conf`. systemd takes a file
in `/etc` over one of the same name in `/usr/lib`, so an empty one disables
Wine's registration without editing a package-owned file. Deleting it restores
Wine's.

The consequence is worth stating in a package description rather than
discovering: **installing Raven changes what every `.exe` on the machine does**.
`wine program.exe` still works, and `./program.exe` stops using `~/.wine`.

`NoDisplay=true` keeps Raven out of application menus. It is a handler for a file
type, not something anybody launches on its own.

The package also installs `rvn` as a symlink to `raven`, and removing it
removes `raven.conf` with it — a registration left pointing at a deleted binary
makes every `.exe` on the machine fail in a way nobody would connect to Raven.
`raven doctor` diagnoses exactly that state, and the package makes it
impossible by ownership: the registration lives and dies with the binary.
