# Status

Last updated when the design was written and the repository scaffolded.

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

## Not done

**No code exists.** There is no workspace, no crate, no binary, and nothing has
been run. The crate layout in
[../internals/architecture.md](../internals/architecture.md) is a plan, not a
description.

Also absent, and deliberately: `docs/guide/`. There is nothing to install and
nothing to use, and writing those pages now would produce documentation that is
trusted and wrong.

## Open questions

These are real. Several of them can still move the design, and they are ordered
by how much damage a wrong assumption would do.

| | Question | Why it matters |
|---|---|---|
| 1 | Does Wine accept `dosdevices/c:` pointing at an `overlayfs` mount? | If not, the whole overlay design needs rethinking. It should work — Wine sees a directory — but it is the assumption everything else rests on, so it gets tested first. |
| 2 | Is a WIM-applied, never-booted Windows usable as a base? | Its hives are pre-`specialize` and no profile exists. This may be fine, or even preferable, but "may be" is not a foundation. |
| 3 | What happens to reparse points when a WIM is applied to a POSIX filesystem? | Junctions hold a Windows profile together. If they do not survive and Wine does not synthesize them, paths break in ways that look like application bugs. |
| 4 | How thin can the shadow set get? | The research question. See [../internals/shadow-set.md](../internals/shadow-set.md). |
| 5 | Does `overlayfs` accept an `ntfs3` lower layer? | Gates the "bring your own Windows partition" path. The ISO path does not depend on it. |
| 6 | Is `ntsync` picked up, and what does it change here? | `CONFIG_NTSYNC=m` on current kernels; the module exists and is not loaded by default. Cheap to answer, and it should be measured rather than asserted. |
| 7 | Synthetic or locally-generated registry test corpus? | The repository cannot carry Microsoft's hives. Decide before the first test, not after. |

## Carried upstream

One finding that is not Raven's to fix:

`colony_ui::paths` is the org's canonical filesystem helper, and it lives in an
iced crate. A daemon with no user interface cannot use it without pulling in a
GUI toolkit. Eidos worked around this with `eidos-paths`; Raven will need
`raven-paths` for the same reason, which makes it the second duplication rather
than the first.

The fix is a `colony-paths` crate that `colony-ui` re-exports, and it belongs in
Project-Colony-Resources. Raised there, not solved here.
