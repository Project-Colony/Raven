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

### Deploying a real Windows

A Windows 11 Pro, build 26200 (25H2), was applied from an official ISO with
`wimlib-imagex` — 143 886 files, 14 GB on disk, no hypervisor and no boot.

| Finding | Consequence |
|---|---|
| It applies cleanly to an ordinary Linux filesystem | the ISO path never touches NTFS, so the `ntfs3` question gates only the secondary path |
| All five hives are present and `hivex` reads them | `SOFTWARE` is 76 MB and carries `Classes`, `WOW6432Node`, `Microsoft`, `OEM` and more — there is real material to project even before first boot |
| `SystemRoot` reads `X:\Windows` and `InstallDate` is zero | a never-booted Windows describes the *setup* environment. The projection has to rewrite this, and that is now a known requirement rather than a surprise |
| The legacy junctions are absent | `Documents and Settings` and `ProgramData\Application Data` are made at first boot. Software that still uses those paths will not find them |
| Reparse points survive as **absolute** symlinks | `Users\All Users` points at `/ProgramData` — the *Linux* root. Only two exist in the whole tree, and deployment must rewrite them relative |
| `wimlib` drops NT security descriptors (131 323 files), DOS attributes, 8.3 names (83 967 files) and xattrs (14 287 files) | whether any of that matters is unmeasured; `--unix-data` mode is the lever if it does |
| Casing is genuinely mixed | `KernelBase.dll`, `Windows`, `Users`. Wine resolves Windows paths case-insensitively, but *overlayfs merges on the exact byte path* — see below |

### Wine will not run against a bare real Windows

The end-to-end attempt found the real obstacle, and it is not a DLL.

Pointing `dosdevices/c:` at a plain deployed Windows makes Wine run `wineboot`
instead of the requested program: finding a real `C:\windows` where its own
belongs, Wine concludes the prefix needs rebuilding. The `+loaddll` trace shows
`wineboot.exe` loading and the requested program never starting.

**`overlayfs` stacks multiple lower layers, and the leftmost wins.** With
`lowerdir=<wine-skeleton>:<real-windows>`, Wine's files take precedence where
they exist and Microsoft's fill in everywhere else. Measured: `ntdll.dll` through
such a mount is 770 139 bytes — Wine's — not Microsoft's 2 522 008.

That is the shadow set expressed as a filesystem layer rather than as an
environment variable, and it is a better mechanism than `WINEDLLOVERRIDES`
because it is inspectable.

**One thing blocks it today: case.** Wine's skeleton uses `windows` and `users`;
Microsoft's uses `Windows` and `Users`. `overlayfs` merges byte-identical paths,
so the two trees do not merge at all — the mount shows both. Wine's
case-insensitivity operates a layer above and cannot help. Normalising the Wine
skeleton to Microsoft's casing at deployment is the obvious fix and is untested.

## Built

The crate exists and its first component is real: the mount backend.

- `raven exec` mounts an overlay over a base in an unprivileged user namespace
  and runs a command inside it. `raven doctor` reports what the running system
  supports.
- Five tests pass, and the one that matters asserts the central claim — a write
  through the overlay leaves the base byte-identical, the new file is absent from
  the base, and it is present in the upper layer.
- That test was checked against a deliberately broken mount: with the overlay
  disabled it fails. A guarantee whose test cannot fail is not a guarantee.

Everything else in [../internals/architecture.md](../internals/architecture.md)
— the base deployment, the registry projection, the shadow set, `binfmt`
registration — is still a plan rather than a description.

Also absent, and deliberately: `docs/guide/`. There is nothing to install and
nothing to use, and writing those pages now would produce documentation that is
trusted and wrong.

## Open questions

Ordered by how much damage a wrong assumption would do.

| | Question | Why it matters |
|---|---|---|
| 1 | Does normalising the Wine skeleton's casing make the two layers merge? | The immediate blocker. Everything else waits on it, and it is a rename rather than a redesign. |
| 2 | Once merged, which Wine files must be in the upper lower-layer? | The shadow set, now expressed as "which paths does the Wine layer need to contain". See [../internals/shadow-set.md](../internals/shadow-set.md). |
| 3 | Does the projection's `X:` to `C:` rewrite cover everything, or is a never-booted hive missing more? | `SystemRoot` was found by looking. What else describes the setup environment is unknown until something reads the whole hive. |
| 4 | What do hardened systems need? | `linux-hardened`, Ubuntu's AppArmor policy and SELinux-enforcing systems all change the mount story. Rootless Podman solves this with `fuse-overlayfs` and `context=` labelling, so the answers exist; which one Raven needs is unmeasured. |
| 5 | Does `overlayfs` accept an `ntfs3` lower layer? | Gates the "bring your own Windows partition" path only. The ISO path does not touch NTFS at all. |
| 6 | Synthetic or locally-generated registry test corpus? | The repository cannot carry Microsoft's hives. Decide before the first test, not after. |
| 7 | Do the dropped NTFS attributes matter? | 131 323 security descriptors, 83 967 short names and 14 287 xattr sets were discarded on deployment. Nothing is known to need them yet, and `--unix-data` is the lever if something does. |

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
