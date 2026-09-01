# Consolidating Raven

What stands between a thing that works on one machine and a thing other people
can install and trust. Ordered by what a wrong answer costs, not by effort.

Every item names how you know it is done. An item without an acceptance test is
a wish.

## Before anything: how to measure

Two of this project's wrong turns came from measuring the wrong quantity
confidently, so this comes first.

- **CPU percentage is not latency.** A process blocked waiting burns *less* CPU
  while running *worse*. The comparison that misled us — 59% under Raven against
  66% under plain Wine — said nothing about which was faster.
- **A trace line count is not a cost.** 112 373 WinSxS lookups looked
  catastrophic and turned out to be free.
- **A ratio between differently-sized workloads measures the workload.** The
  6.6× enumeration gap decomposed into 6.0× more directory entries and ~11%
  actual overhead. Divide by size before reading a ratio as a cost.
- **A control run is not optional, and it goes first.** Both times a fault was
  actually located, it was the plain-Wine control that located it. Both times it
  was run after an hour of guessing.

---

## 1. Robustness — the things that bit us

These are all faults hit in one evening of real use. None is speculative.

### 1.1 `raven doctor` must detect the Wine registration conflict

Wine's package registers `:DOSWin:` for the same `MZ` magic. When both are
present the kernel picks Wine's, every `.exe` runs against `~/.wine`, and the
failure looks like Raven losing the prefix. It cost an hour.

**Done.** `doctor` (and `raven binfmt`) lists every registration claiming a
`.exe` — by `MZ` magic or by extension — names the one the kernel will pick,
and says what to do when it is not Raven's, when a rival is still armed behind
a winning Raven, and when the interpreter was deleted after registration (the
`cargo clean` case: the `F` flag keeps it alive until reboot, then every
`.exe` stops).

Which entry wins was settled by experiment, not documentation: in a sandboxed
`binfmt_misc` mount, of two entries claiming `MZ` the one registered *last*
runs, and the unsorted directory order lists newest-first — so the first
enabled claimant in readdir order is the kernel's choice. All three failure
scenarios were staged in the sandbox and produce the intended diagnosis.

### 1.2 Recovering an environment whose namespace is still alive

Killing a program can leave its `wineserver` and a dozen Wine services alive
inside the mount namespace. The overlay stays busy, and the next `raven run`
fails with `Device or resource busy` — an error naming neither the environment
nor the processes holding it.

`wineserver -k` from outside does not help: the server inside the namespace is a
different one.

**Done when:** `raven env status <name>` reports whether it is mounted and what
holds it, `raven env stop <name>` releases it, and the busy error points at both.

### 1.3 Concurrent launches into one environment

`overlayfs` refuses two live mounts sharing an `upperdir`, so a second program
launched into a running environment fails. Children of an already-running
program are fine — they inherit the namespace — but a second independent launch
is not.

**Done when:** a second launch joins the existing namespace instead of failing,
or refuses with an error that says why. The keeper-plus-`nsenter` pattern is
already proven to work; it is not built.

### 1.4 Uninstall

Installing Raven changes what every `.exe` on the machine does. Removing it must
restore that, and a `binfmt` registration left pointing at a deleted binary
breaks every `.exe` in a way nobody would connect to Raven.

**Done when:** removing the package restores Wine's registration and leaves no
handler pointing at a path that no longer exists.

---

## 2. Performance — measured properly this time

### 2.1 Measure latency, not CPU

**Partly done** — see [performance.md](../internals/performance.md). A fixed
workload, identical in both conditions, wall-clock: process spawn is **2.0×**
(113 → 228 ms), and directory enumeration is 6.6× — of which 6.0× is the real
`System32` holding six times the entries, leaving **~11% per entry** as the
overlay's share. The lesson joined the list above: a ratio between two
differently-sized workloads measures the workload.

**Still open:** a frame-time or input-to-response number for the same program
under Raven and under plain Wine, on the same machine, in the same scene. The
spawn and enumeration numbers are proxies; whether a *running* game feels
anything is still unmeasured.

### 2.2 Test ext4 `casefold`

The one lever identified for the +95 ms per-process cost. Wine builds a
directory cache to resolve Windows paths case-insensitively —
`init_cached_dir_data` runs 809 times against 133 — and a case-insensitive
filesystem would remove the need.

Two unknowns, both cheap to settle: whether Wine detects such a filesystem and
skips its cache at all, and whether it is reachable, since `casefold` is
ext4-only and a btrfs base cannot have it.

**Done when:** a base is deployed onto a `casefold` ext4 filesystem and the
`init_cached_dir_data` count is compared. A negative result closes the line of
attack, which is worth knowing.

### 2.3 Find out what `wineserver` is actually doing

7.75× is a large multiple and nobody has looked at *what* the extra traffic is.
`WINEDEBUG=+server` names every request.

**Done when:** the extra requests are attributed to a cause — file handles,
synchronization, registry — because the fix differs for each.

### 2.4 Not the problem, so nobody re-checks it

Raven's own start-up is **1 ms** in release and 2 ms in debug. It runs once per
`.exe` launch and is not where the 95 ms goes.

---

## 3. A real package

### 3.1 A release profile

**Done.** `lto = "fat"`, `codegen-units = 1`, `strip = "symbols"`. Measured:
1.9 MB → 1.2 MB, start-up unchanged within noise (~1.7 ms before and after).

`panic = "abort"` was considered and **deliberately not set**: a panic in raven
happens between the kernel's `binfmt` hand-off and Wine, where there is no
terminal and no context — the backtrace is the only witness. The ~100 kB it
would save does not buy that back. Revisit only if a measurement shows unwind
tables costing something real.

### 3.2 The package itself

`packaging/` holds the pieces and nothing installs them:

| File | Goes to |
|---|---|
| `raven.conf` | `/etc/binfmt.d/raven.conf` |
| `wine-mask.conf` | `/etc/binfmt.d/wine.conf` — disables Wine's |
| `raven.desktop` | `/usr/share/applications/` |
| the binary | `/usr/bin/raven`, plus `rvn` beside it |

The interpreter path in `raven.conf` must be the installed one. Today the
machine points at a debug binary inside the repository — a `cargo clean` breaks
every `.exe` on the system.

**Done when:** a PKGBUILD installs all four, uninstall reverses them, and the
package description states that installing changes what every `.exe` does.

### 3.3 Releases

The org supplies the machinery and none of it is wired: release-please for
tagging, the release workflow for the four platform assets, `colony.json`
`releaseFiles` for the launcher to find them. Raven is Linux-only, so the asset
matrix is one row, not four.

**Done when:** a tag produces a signed asset that Colony can install.

---

## 4. The shadow set — the actual research

One entry is measured: `Windows\WinSxS` must be hidden, or installers render
without text and ignore every click. That is the whole list.

**Next, in order:**

1. **A corpus.** Inno Setup is one framework. NSIS, InstallShield, MSI and
   Squirrel are each common enough that one of them failing is a class of
   software that does not install.
2. **The COM registry.** 121 256 keys sit behind one line in the rules file,
   deliberately off because a CLSID pointing at a library Wine shadows turns a
   working fallback into a hard failure. Nobody has measured whether that
   actually happens.
3. **The band nobody has touched.** `ole32`, `rpcrt4`, `shell32`, `ws2_32` —
   whether any can be Microsoft's is the question the project exists to answer,
   and the answer is still unmeasured.

**Done when:** the shadow set is a data file with a measurement behind every
entry, and a corpus that regression-tests it.

---

## 5. Coverage — what has never been tried

Stated so nobody mistakes the current evidence for more than it is.

- **No 3D.** The one game run is 2D software-rendered. Nothing here says
  anything about Direct3D, DXVK, or a GPU.
- **One installer framework.**
- **No anti-cheat.** It is a compatibility target, not a feature: a game whose
  publisher enabled the Proton path should behave the same under Raven, and
  parity is the test. See [landscape.md](landscape.md).
- **One Windows build, one edition, one machine.**
