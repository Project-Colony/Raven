# Performance

What running against a real Windows costs, what it does not cost, and what has
been ruled out. Deferred by decision — the numbers are recorded so the work can
start from measurement rather than from guesses.

## The wall-clock benchmark

An earlier comparison used CPU percentage — `wineserver` at 15.5% under Raven
against 2.0% under plain Wine — and CPU percentage is not latency: a process
blocked waiting burns *less* CPU while running *worse*. This benchmark replaces
it. One fixed script, run identically in both conditions on a quiet machine,
`WINEDEBUG=-all`, wineserver warmed up outside the measurement, wall-clock
nanoseconds around each sample.

**Process spawn** (`wine cmd /c exit`, eight samples each):

|  | plain Wine | Raven | ratio |
|---|---|---|---|
| range | 108–115 ms | 218–240 ms | |
| median | 113 ms | 228 ms | **2.0×** |

About **+115 ms per process**, consistent with the +95 ms measured earlier by a
cruder method. Invisible for a game launched once; twenty seconds of pure
overhead for an installer that spawns two hundred processes.

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

So the honest summary is: per-process launch cost is real and doubled, and the
sustained filesystem path is modestly slower. What this benchmark does *not*
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
own message pump.

The original observation — 15.5% against 2.0% — was an instantaneous CPU
reading in a process monitor. On the day of the controlled capture the same
reading went the *other* way (17.8% under Raven, 31–34% under plain Wine)
while the request streams stayed identical. An instantaneous `wineserver` CPU
percentage tracks the game's frame pacing at the moment of the glance, not the
server's workload; it now joins the list of measurements this project does not
trust. There is no wineserver pathology to fix, and the whole launch overhead
lives client-side, in the directory cache above.

## The launch cost, dissected

**Launching a process costs about twice as much, and the cause is not what it
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

What the operation counts actually show:

| Wine operation | skeleton only | real Windows |
|---|---|---|
| `init_cached_dir_data` | 133 | **809** |
| `get_nt_and_unix_names` | 179 | 841 |
| `append_entry` | 1 027 | 5 802 |

`init_cached_dir_data` is the case-insensitivity machinery. To resolve a Windows
path on a case-sensitive filesystem, Wine reads the whole directory and builds a
lookup cache. The merged `System32` holds 4 617 entries against Wine's 852, and
Wine caches 809 directories per launch instead of 133.

So the cost is not a pathology to be removed. It is the price of case-insensitive
resolution over a Windows that is simply much larger, paid once per process.

Whether a case-insensitive filesystem underneath — ext4's `casefold` — removes
the need for that cache is the obvious question and is **untested**. It also may
not be reachable: `casefold` is an ext4 feature, and a btrfs base cannot have it.

**The methodological lesson, which cost two wrong hypotheses:** a trace line
count is not a cost. Both WinSxS theories were plausible, measurable, and false,
and only the control experiment — an overlay with nothing real in it — separated
the variables.

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

## Where to start when this is picked up

1. **Test `casefold`.** It is the only lever identified. Deploy a base onto an
   ext4 filesystem with the feature enabled, and measure whether Wine's
   `init_cached_dir_data` count drops. If Wine builds its cache regardless of
   the filesystem, this line of attack is closed and that is worth knowing early.
2. **Establish whether it matters.** +95 ms per process is invisible for a game
   launched once and costs twenty seconds for an installer that spawns two
   hundred processes. Measuring a real installer would say whether this is a
   priority at all.
3. **Do not re-derive the falsified theories.** WinSxS and the overlay are both
   ruled out, by experiment, and the experiments are described above.

## What was gained anyway

The WinSxS investigation produced a capability even though its theory was wrong:
**opaque overlay directories work unprivileged**. Mounting with the `userxattr`
option and setting `user.overlay.opaque` on a directory in the Wine layer masks
the real Windows's version of it entirely — verified by reducing a 17 385-entry
`WinSxS` to a single directory.

That is a general tool for shaping what a program sees of the base, and it needs
no root. It will be useful for something; it was simply not useful for this.
