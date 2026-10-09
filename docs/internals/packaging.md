# Packaging

How Raven is installed, and the two things installation has to arrange that the
program cannot do for itself.

The files a package installs live in [packaging/](../../packaging/) and its
`PKGBUILD` wires them together; this page records the reasoning. Both
root-requiring items are arranged at install time, never at runtime.

## The command is `raven`, with `rvn` beside it

The binary is `raven`. The package also installs `rvn` as a symlink to it.

The canonical name matches the program, the organisation and the repository,
which is what everything else in Project Colony does. The short name exists
because it is typed in front of every program launch, and three characters
against five adds up over a day.

`rvn` is a link rather than a second binary, so there is one thing to build, one
to sign and one to update. `--version` and the diagnostics say `raven` under
both names, and only the usage line echoes the name it was invoked as - the
short name is a convenience, not a second identity.

## `binfmt_misc` registration

Making `./program.exe` run like any other binary means registering the PE magic
with the kernel, and writing to `/proc/sys/fs/binfmt_misc/register` needs root.

It is registered **once, at install time**, through
`/usr/lib/binfmt.d/raven.conf` applied by `systemd-binfmt` - the same mechanism
the `wine` package uses. That is the directory for files a package owns; a file
of the same name in `/etc/binfmt.d/` takes precedence over it, which is why
`raven binfmt` prints an `/etc` path for a hand-registration on a build no
package installed. The package manager already holds root legitimately; Raven
does not need to, and adding a privileged service to do at runtime what a config
file does at boot would be trading a file for an attack surface.

Uninstalling removes the file. A registration left behind pointing at a deleted
binary would keep working on the kernel's open handle until the next reboot
(the `F` flag) and then make every `.exe` on the machine fail in a way nobody
would connect to Raven; `raven doctor` detects and reports both states.

## Two binaries, two desktop entries, one version

The package installs `raven-gui` beside `raven`, and `raven-gui.desktop`
beside `raven.desktop`. They stay separate rather than merging into one
launcher, because the two desktop entries mean different things to a file
manager:

`raven.desktop` claims the `.exe` MIME types (`application/x-msdownload` and
friends) and carries `NoDisplay=true`. It is the handler a file manager
reaches for when someone double-clicks a Windows program, launching
`raven launch %f` against that one file - it is not meant to appear in an
application menu on its own.

`raven-gui.desktop` claims no MIME type and has no `NoDisplay`. It appears in
the menu under Raven's `System;Settings;` categories and launches `raven-gui`
with no arguments, opening the administration window for managing bases and
environments.

If the GUI launcher also claimed the `.exe` association, double-clicking a
Windows program would open the manager instead of running the program - the
opposite of what a double-click means. Keeping the association on
`raven.desktop` alone, and never adding it to `raven-gui.desktop`, is what
keeps that from happening. Both entries reuse the single `Icon=raven` already
installed into the `hicolor` theme; there is no second icon to keep in sync.

The window itself has to opt into that entry. A compositor matches a window's
application id against a desktop file's basename, so `raven-gui` sets its
`application_id` to `raven-gui` - without it the entry and the running window
are unrelated as far as a taskbar is concerned, and the `Icon=raven` above
never reaches one. `assets/brand/README.md` records that agreement in full.

`build()` needed no change to produce the second binary: `cargo build
--release` at the workspace root already builds every workspace member, so
`raven-gui` comes out of the same command as `raven`. Only `package()` gained
two more `install` lines, and the release workflow names a second asset.

## Releases

[`.github/workflows/release.yml`](../../.github/workflows/release.yml) is the
organisation's release template with two Raven-specific parts.

Merging the release pull request tags the version and holds the release as a
draft. One Linux job builds the whole workspace with `--locked` and stages
`raven` as `raven-linux` and `raven-gui` as `raven-gui-linux`. It checks
`colony.json` against those two names and runs `--version` on both binaries.
The shared
[sign-and-publish workflow](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/.github/workflows/sign-and-publish.yml)
then writes `.sig`, `.meta` and `.meta.sig` for each asset, verifies them, and
publishes the release. The build job never sees the signing key.

The first Raven-specific part is the lock. `release-type: simple` bumps the
version in `Cargo.toml` and leaves `Cargo.lock` alone, so a `--locked` build of
the tag would fail. The release-please job therefore runs `cargo update
--workspace` on the release branch and commits the lock when it changed. It
then runs `cargo metadata --locked`: a push made with the workflow token starts
no CI, so that is the only check the release branch gets before it merges.

The second is the asset list. Colony installs `raven-linux` alone, because the
Colony naming convention resolves one asset per platform. `raven-gui-linux`
ships signed beside it on the release page for anyone who wants the window.

If a release run fails after the tag exists, `gh workflow run release.yml
--ref vX.Y.Z -f tag=vX.Y.Z` rebuilds that tag and finishes its draft.

## `ntsync` is not Raven's business

Arch's `wine` package depends on `ntsync-autoload`, whose entire content is a
`modules-load.d` entry loading the module at boot. The distribution already
arranges it.

Raven **detects and reports** - `raven doctor` says whether `/dev/ntsync` is
present - and does not load modules on a user's behalf. A tool that quietly
modifies kernel module state is doing something the person running it did not
ask for.
