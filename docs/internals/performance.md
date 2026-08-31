# Performance

What running against a real Windows costs, what it does not cost, and what has
been ruled out. Deferred by decision — the numbers are recorded so the work can
start from measurement rather than from guesses.

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
