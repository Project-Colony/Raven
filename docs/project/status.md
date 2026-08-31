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

**Case blocked it, and normalising fixed it.** Wine's skeleton uses `windows` and
`users`; Microsoft's uses `Windows` and `Users`; `overlayfs` merges byte-identical
paths only, so the trees stayed separate and the mount showed both. Renaming 338
paths in the skeleton to Microsoft's casing merged them: no duplicates,
`System32` holds 4877 entries where Wine alone has 852 and Microsoft alone 4617,
`ntdll.dll` reads as Wine's 770 139 bytes, and Microsoft-only files show through.

### The premise is proven

With that stack in place, Wine loads and executes genuine Microsoft binaries out
of the mounted base:

```
Loaded L"C:\Windows\System32\choice.exe"   at 0000000140000000: native
Loaded L"C:\Windows\System32\forfiles.exe" at 0000000140000000: native
```

`native` means the PE came from the base, not from Wine's own directory. A real
Windows, deployed from an ISO without a hypervisor and without ever booting,
mounted as C:, running its own binaries under Wine — with the base still
byte-identical afterwards.

**A caution that cost time and is worth writing down:** `whoami.exe` and
`certutil.exe` appeared to work first, and both are Wine builtins. Wine ships
several hundred `.exe` files, so a program producing correct output proves
nothing about where it came from. **Only the `+loaddll` trace settles
provenance**, and any future claim about a Microsoft binary running needs to cite
it.

### Two limits found by running it

**Console utilities print nothing, and the cause is MUI.** `choice.exe` and
`forfiles.exe` load, run, and exit zero with empty output. Modern Windows keeps
program strings in separate `.mui` resource files — the base holds 10 416 of
them, and `choice.exe` has only a 2 KB `.rsrc` section, far too small for its own
help text. Traced with `WINEDEBUG=+file`: Wine opens **zero** `.mui` files. So
`LoadString` finds nothing and the program prints nothing.

Launching a process against the real Windows costs about **+95 ms** more than
against Wine's synthetic prefix. The cause is measured, two plausible theories
about it were falsified, and the whole investigation is in
[../internals/performance.md](../internals/performance.md).

## Built

Eleven commands. The path from an installation image to `./program.exe` is
complete.

| | |
|---|---|
| `raven doctor` | namespaces, Wine, `ntsync`, and what is deployed |
| `raven base editions` / `deploy` / `list` | the immutable Windows installations |
| `raven env create` / `list` / `destroy` | environments, cheap and disposable |
| `raven env default` / `reproject` | which environment is used by default; re-run the projection |
| `raven binfmt` | what to install so the kernel recognises `.exe` |
| `raven launch` / `run` / `exec` | running a program, at three levels of explicitness |

Measured against the real Windows 11 base:

- `./program.exe` typed at a shell runs, and the loader reports Microsoft's PE
  loading as **native**.
- `raven env create` takes twenty seconds, including the registry projection.
- The projection carries **1 894 keys** in 130 ms, every refusal holding through
  to the prefix, and no `X:` left anywhere.
- After a full cycle the base holds **143 886 files, none modified**.

**69 tests pass** and `clippy -D warnings` is clean. The ones carrying the
design: base immutability under a real write (checked against a sabotaged mount,
so it can fail), layer precedence with two read-only layers, removal of a mounted
environment that `remove_dir_all` cannot delete, PE recognition by magic bytes
rather than by extension, and eight projection tests against hives built by a
**different implementation** from the reader.

## Not built

No package. `raven binfmt` prints what to install rather than installing it, and
the `rvn` alias is decided but has nowhere to be installed from — see
[../internals/packaging.md](../internals/packaging.md).

No application has been installed into an environment, and no game has been run.
Everything above is measured on Microsoft's own utilities, which is a much easier
case than software that was never expecting any of this.

## A real program, end to end

The first third-party software run against Raven, and the chain completed:

1. A real Windows 11 Pro, deployed from an official ISO, never booted.
2. Mounted as C:, with Wine's layer above it.
3. **A real Windows installer ran** — Inno Setup, 32-bit — and wrote 850 files
   and 256 MB into `Program Files (x86)`, plus its own registry key at
   `HKLM\Software\Wow6432Node\Enterbrain\RGSS3\RTP`.
4. A 32-bit RPG Maker game then found its runtime and reached its title screen.

**The base finished with 143 886 files and none modified.**

Getting there required the first measured entry in the shadow set, and it was
not a library: the base's `Windows\WinSxS` must be hidden, or installers render
without text and ignore every click. The full account is in
[../internals/shadow-set.md](../internals/shadow-set.md).

Two things this does *not* show. The game is 2D and makes no 3D calls, so
nothing here says anything about Direct3D. And one installer working is one data
point — Inno Setup is common, but so are half a dozen other installer
frameworks, and none has been tried.

## Open questions

Ordered by how much damage a wrong assumption would do.

| | Question | Why it matters |
|---|---|---|
| 1 | Can Wine be made to resolve `.mui` resources? | Without it every real Windows console utility is mute, and any program that keeps its strings in MUI — which is the modern default — shows blank text. This is now the largest known gap. |
| 2 | Does a case-insensitive filesystem remove Wine's directory-cache cost? | It is the only lever identified for the +95 ms per process. `casefold` is ext4-only, so a btrfs base cannot use it, and whether Wine even detects it is unknown. |
| 3 | Which Wine files must be in the upper lower-layer? | The shadow set, now expressed as "which paths does the Wine layer need to contain". See [../internals/shadow-set.md](../internals/shadow-set.md). |
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
