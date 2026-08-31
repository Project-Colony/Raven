# Status

A status page describing last quarter is actively misleading, so this one either
stays current or gets deleted.

## Done

- The approach is settled: a real Windows installation as an immutable base, an
  `overlayfs` upper layer per environment, a selective registry projection, and
  the smallest possible set of libraries forced to Wine's implementations.
- The alternatives were examined and rejected for stated reasons —
  [landscape.md](landscape.md).
- The design is written down — [../internals/architecture.md](../internals/architecture.md)
  and the three pages it links.
- The repository follows the org conventions: layout, filesystem, documentation
  and dependency rules from
  [Project-Colony-Resources](https://github.com/Project-Colony/Project-Colony-Resources).

## Measured

Findings established by experiment rather than assumed. Each one removed
something from the design.

| | Finding | Consequence |
|---|---|---|
| `overlayfs` mounts without root | `unshare -Urm` succeeds; the mount is invisible outside the namespace; `nsenter --preserve-credentials` joins it without root | **The privileged daemon was deleted.** The runtime path is unprivileged end to end. |
| The base is provably untouched | writing through the overlay to a file present in the lower layer leaves the original byte-identical | the immutability claim is testable, and that test is the one that guards the design |
| `binfmt_misc` needs root once | write to `/proc/sys/fs/binfmt_misc/register` is refused unprivileged; `systemd-binfmt` is the supported route | registration is a **packaging** concern, not a running service |
| `ntsync` ships in the kernel | `CONFIG_NTSYNC=m`; on Arch, `ntsync-autoload` is a hard dependency of `wine` and ships a `modules-load.d` entry | the distribution already handles it. Raven detects `/dev/ntsync` and reports; it does not load modules on a user's behalf |
| **Wine accepts an `overlayfs` mount as `C:`** | with `dosdevices/c:` pointed at the mount, `wine cmd` listed the drive, wrote `C:\preuve.txt`, and read it back | **the central assumption holds.** The design's riskiest bet is no longer a bet |
| The write landed in the overlay, not the base | `preuve.txt` exists in `upper/`, is absent from the 1896-file base, and the base's contents are unchanged | immutability is real under a genuine Wine write, not just under `cat` |
| The mount dies with its process tree | after the namespace exited, the mount point was empty from the host | no stale mounts to clean up after a crash |

The consequence of the first three together is that Phase 1 is a single crate
rather than a seven-crate workspace with a privileged process in it.

## Not done

**No code exists.** There is no crate, no binary, and nothing has been run. The
layout in [../internals/architecture.md](../internals/architecture.md) is a plan,
not a description.

Also absent, and deliberately: `docs/guide/`. There is nothing to install and
nothing to use, and writing those pages now would produce documentation that is
trusted and wrong.

## Open questions

Ordered by how much damage a wrong assumption would do.

| | Question | Why it matters |
|---|---|---|
| 1 | Is a WIM-applied, never-booted Windows usable as a base? | Its hives are pre-`specialize` and no profile exists. This may be fine, or even preferable, but "may be" is not a foundation. |
| 2 | What happens to reparse points when a WIM is applied to a POSIX filesystem? | Junctions hold a Windows profile together. If they do not survive and Wine does not synthesize them, paths break in ways that look like application bugs. |
| 3 | How thin can the shadow set get? | The research question. See [../internals/shadow-set.md](../internals/shadow-set.md). |
| 4 | What do hardened systems need? | `linux-hardened`, Ubuntu's AppArmor policy and SELinux-enforcing systems all change the mount story. Rootless Podman solves this with `fuse-overlayfs` and `context=` labelling, so the answers exist; which one Raven needs is unmeasured. |
| 5 | Does `overlayfs` accept an `ntfs3` lower layer? | Gates the "bring your own Windows partition" path only. The ISO path does not touch NTFS at all. |
| 6 | Synthetic or locally-generated registry test corpus? | The repository cannot carry Microsoft's hives. Decide before the first test, not after. |

The question that used to sit at the top of this table — whether Wine would
accept an `overlayfs` mount as its C: drive — is answered and has moved to
**Measured**. It was the one that could have invalidated the design.

## Carried upstream

One finding that is not Raven's to fix.

`colony_ui::paths` is the org's canonical filesystem helper, and it lives in an
iced crate. A command-line program with no user interface cannot use it without
pulling in a GUI toolkit to compute `~/.local/share/Colony/Raven/`. Eidos hit
this and worked around it with `eidos-paths`; Raven will carry its own
`src/paths.rs` for the same reason, which makes it the second workaround rather
than the first.

The fix is a `colony-paths` crate that `colony-ui` re-exports, and it belongs in
Project-Colony-Resources. Raised there, not solved here.
