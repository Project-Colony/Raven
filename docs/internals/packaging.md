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
to sign and one to update. Help output and diagnostics say `raven` under both
names — the short name is a convenience, not a second identity.

## `binfmt_misc` registration

Making `./program.exe` run like any other binary means registering the PE magic
with the kernel, and writing to `/proc/sys/fs/binfmt_misc/register` needs root.

It is registered **once, at install time**, through `/etc/binfmt.d/raven.conf`
applied by `systemd-binfmt` — the same mechanism the `wine` package uses. The
package manager already holds root legitimately; Raven does not need to, and
adding a privileged service to do at runtime what a config file does at boot
would be trading a file for an attack surface.

Uninstalling removes the file. A registration left behind pointing at a deleted
binary would keep working on the kernel's open handle until the next reboot
(the `F` flag) and then make every `.exe` on the machine fail in a way nobody
would connect to Raven; `raven doctor` detects and reports both states.

## `ntsync` is not Raven's business

Arch's `wine` package depends on `ntsync-autoload`, whose entire content is a
`modules-load.d` entry loading the module at boot. The distribution already
arranges it.

Raven **detects and reports** — `raven doctor` says whether `/dev/ntsync` is
present — and does not load modules on a user's behalf. A tool that quietly
modifies kernel module state is doing something the person running it did not
ask for.
