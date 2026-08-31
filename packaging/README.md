# Packaging files

What a package installs beyond the binary. Nothing here is wired into a build
yet; the reasoning behind each file is in
[../docs/internals/packaging.md](../docs/internals/packaging.md).

| File | Installed to | Why |
|---|---|---|
| `raven.conf` | `/etc/binfmt.d/` | makes the kernel hand `.exe` files to Raven, so `./program.exe` runs from a shell |
| `raven.desktop` | `/usr/share/applications/` | makes a **file manager** open them, which `binfmt` alone does not do |

The two are needed for different things and neither replaces the other.
`binfmt_misc` answers "the kernel is asked to execute this file"; a desktop entry
answers "a person double-clicked this file", which a file manager resolves
through MIME types without ever asking the kernel to execute anything.

`NoDisplay=true` keeps Raven out of application menus. It is a handler for a file
type, not something anybody launches on its own.

A package also installs `rvn` as a symlink to `raven`, and must remove
`raven.conf` on uninstall — a registration left pointing at a deleted binary
makes every `.exe` on the machine fail in a way nobody would connect to Raven.
