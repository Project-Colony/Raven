# Performance

What running against a real Windows costs, what it does not cost, what has
been ruled out — and what has been fixed. The spawn overhead is attributed and
mostly removed (fonts, masked); what stays deferred is the ~20 ms residual and
an in-game frame-time measurement.

## The wall-clock benchmark

An earlier comparison used CPU percentage — `wineserver` at 15.5% under Raven
against 2.0% under plain Wine — and CPU percentage is not latency: a process
blocked waiting burns *less* CPU while running *worse*. This benchmark replaces
it. One fixed script, run identically in both conditions on a quiet machine,
`WINEDEBUG=-all`, wineserver warmed up outside the measurement, wall-clock
nanoseconds around each sample.

**Process spawn** (`wine cmd /c exit`, eight samples each — measured *before*
the fonts mask; the fix it motivated is further down):

|  | plain Wine | Raven | ratio |
|---|---|---|---|
| range | 108–115 ms | 218–240 ms | |
| median | 113 ms | 228 ms | **2.0×** |

About **+115 ms per process** at the time, consistent with the +95 ms measured
earlier by a cruder method — twenty seconds of pure overhead for an installer
spawning two hundred processes. With `Windows/Fonts` masked it is **~135 ms
(1.19×), about +22 ms per process** — some four seconds for the same installer.

**Directory enumeration** (30 × `dir C:\windows\system32` inside one `cmd`,
three samples each):

|  | plain Wine | Raven | ratio |
|---|---|---|---|
| total | 1 491–1 519 ms | 9 739–10 174 ms | **6.6×** |
| minus one process spawn | ~1 392 ms | ~9 763 ms | 7.0× |
| entries enumerated | 817 | 4 877 | 6.0× |
| **cost per entry** | 57 µs | 67 µs | **~1.17×** |

The 6.6× would be alarming if it measured Raven. It mostly measures Windows:
the merged `System32` holds six times the entries of Wine's synthetic one, and
dividing by directory size leaves **~17% per entry** (13–21% across the sample
ranges) as the overlay's actual share. A ratio between two differently-sized
workloads measures the workload.

The spawn subtraction is not pedantry. Each ENUM sample is one `wine cmd`
invocation, which the spawn table above prices at 113 ms and 228 ms — and the
contamination is asymmetric: the fixed cost spreads over 6× more entries under
Raven, so leaving it in *flatters the overlay* (it read ~11% before the
correction). Review caught it; the direction survived, the number did not.

So the honest summary is: per-process launch cost was doubled until the fonts
mask and is now ~19% (113 → 135 ms), and the sustained filesystem path is
modestly slower. What this benchmark does *not*
answer is whether a running game feels any of it — that needs a frame-time or
input-to-response measurement in the same scene, which no one has made yet.

## The wineserver phantom

The 7.75× claim died under attribution. `WINEDEBUG=+server` captures of the
same game reaching the same title screen, 25 seconds each, both conditions on
the same day:

|  | plain Wine | Raven |
|---|---|---|
| total server requests | 490 852 | 490 298 |
| registry (`enum_key_value`) | 8 213 | 8 213 |
| file (`create_file`) | 944 | 887 |
| steady state, dominant | `set_queue_mask` + `get_message` | identical |

**A difference of 0.1%.** Request for request, `wineserver` does the same work
under Raven as under plain Wine. Nothing about the six-times-larger C: appears
in the server traffic, because nothing about it can: file metadata and reads on
C: are in-process unix syscalls inside ntdll, synchronization is `/dev/ntsync`
(nine handles open, verified), and what remains in steady state is the game's
own message pump. The one place filesystem cost *can* land inside the server —
`NtCreateFile`, where wineserver itself `open(2)`s the unix path — is ruled out
by the same data: equal `create_file` counts, and Raven's server CPU read
*lower*, so there is no per-request overlay penalty either.

The original observation — 15.5% against 2.0% — was an instantaneous CPU
reading in a process monitor. On the day of the controlled capture the same
reading went the *other* way (17.8% under Raven, 31–34% under plain Wine)
while the request streams stayed identical. An instantaneous `wineserver` CPU
percentage tracks the game's frame pacing at the moment of the glance, not the
server's workload; it now joins the list of measurements this project does not
trust. There is no wineserver pathology to fix, and the whole launch overhead
lives client-side — in fonts, dissected below.

## The launch cost, dissected

**Launching a process cost about twice as much, and the cause was not what it
looked like.** Measured, three runs each: 111–122 ms against Wine's synthetic
prefix, 207–238 ms against the real Windows. Roughly **+95 ms per process**.

The first suspect was WinSxS. A real Windows carries 28 006 manifests and 17 385
assembly directories where Wine's prefix has 21, and a `+file` trace showed
Wine scanning them with wildcard masks — 112 373 trace lines, to find the 8
manifests that actually match. It was the obvious culprit and it was wrong:
masking the entire store with an opaque overlay directory, so 1 directory and
117 manifests remained visible, **changed the time not at all** (207–231 ms).

The overlay is not the cost either. An overlay carrying only Wine's skeleton
runs in 109–121 ms — identical to no overlay at all.

The third suspect was the case-insensitivity machinery, and it looked
convincing: `init_cached_dir_data` appeared 809 times against the real base and
133 against the skeleton. That attribution was **a counting artifact**. The
function traces one line per *file it lists*, not one per cache built; the real
count is 9 directory enumerations per launch against the real base — mostly one
directory of ~340 files, listed twice. The "809 directories cached" never
existed. Three controlled experiments buried the theory properly: a skeleton
tree with `System32`/`SysWOW64` inflated to real-Windows entry counts spawns in
~120 ms (+7 ms, entry count is nearly free); the same tree renamed to real
Windows casing (`Windows/System32`) also spawns in ~120 ms (the case mismatch
costs nothing measurable); and the same trees on a casefolding tmpfs are no
faster (Wine detects the `casefold` flag — the check is not gated on ext4, read
from the 11.16 source and verified — but keeps building the same caches, and
its non-wildcard listing path *degrades* to full readdir on a case-insensitive
filesystem). **The `casefold` line of attack is closed**, with one genuine
side-benefit recorded below.

What the +105 ms actually was: **fonts.** Per traced launch, 676 of the 841
path resolutions point into `C:\windows\fonts` — win32u re-enumerates and
re-checks every font file at every process start. The real base carries ~340
fonts; Wine's own `Fonts` directory is empty (text renders through the host's
fontconfig), so plain Wine never pays this. One opaque-overlay mask on
`Windows/Fonts`, same measurement protocol:

|  | plain Wine | Raven, fonts visible | Raven, fonts masked |
|---|---|---|---|
| spawn, warm server | ~113 ms | ~227 ms | **~135 ms** |

`Windows/Fonts` is now the shadow set's second measured entry (`layer.rs`),
and the remaining ~20 ms is the overlay plus the rest of the real tree. Fonts
still render — through fontconfig, exactly as under plain Wine — and the game
still reaches its title screen with the mask on. The cost: programs that want
Microsoft's font *files*, not just the faces, will not see them; the corpus
will say whether such a program exists.

The casefold side-benefit worth keeping: on a casefolding filesystem the
lowercase-shadow hazard is *impossible*. On a case-sensitive filesystem, a
`wineboot` prefix update can `mkdir` a literal lowercase `windows` beside a
real-Windows-cased `Windows`, and every later exact-match lookup lands in the
empty shell — observed in an experiment replica, where it broke `kernel32`
loading outright. A casefolding filesystem folds the two names into one
directory and the hazard vanishes. This matters to Raven because an
environment's upper layer is exactly where such a shadow would be born.

**The methodological lesson, which cost three wrong hypotheses:** a trace line
count is not a cost, and it is not even a *count* until the trace format is
read. WinSxS, the overlay, and the directory cache were each plausible,
measurable, and false; the control experiments — an overlay with nothing real
in it, a skeleton inflated to real size, a tree renamed to real casing — are
what separated the variables, and the last one took the attribution down to a
single directory name.

## Built

Everything in [architecture.md](architecture.md) is now a description rather
than a plan: base deployment, environments, the registry projection, the
shadow set as a filesystem layer, `binfmt` registration, and recovery of a
held environment. 82 tests pass, and the one that matters still asserts the
central claim — a write through the overlay leaves the base byte-identical —
and was checked against a deliberately broken mount: with the overlay disabled
it fails. A guarantee whose test cannot fail is not a guarantee.

## Where to start when this is picked up

1. **The spawn cost is attributed and mostly fixed** — fonts, masked, 227 → 135
   ms. The residual ~20 ms over plain Wine has no owner yet; profile it only if
   an installer measurement says it matters.
2. **`casefold` is closed.** Tested on a casefolding tmpfs against identical
   control trees: detected by Wine, no gain, slight regression on directory
   listing — and one real benefit (the lowercase-shadow hazard becomes
   impossible) recorded above for whenever base deployment chooses a
   filesystem.
3. **Do not re-derive the falsified theories.** WinSxS, the overlay, the
   directory cache and the case-mismatch are all ruled out, by experiment, and
   the experiments are described above.

## What was gained anyway

The WinSxS investigation produced a capability even though its theory was wrong:
**opaque overlay directories work unprivileged**. Mounting with the `userxattr`
option and setting `user.overlay.opaque` on a directory in the Wine layer masks
the real Windows's version of it entirely — verified by reducing a 17 385-entry
`WinSxS` to a single directory.

That is a general tool for shaping what a program sees of the base, and it needs
no root. It will be useful for something; it was simply not useful for this.
