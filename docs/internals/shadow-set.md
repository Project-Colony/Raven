# The shadow set

Which libraries must be Wine's, which may be Microsoft's, and how to find out
where the line actually falls.

This is the part of Raven that is a research question rather than an engineering
task. Everything else in the design is known to be buildable; this is the part
nobody has published an answer to.

## The problem, stated precisely

Once a real Windows is mounted as C:, Microsoft's `System32` is *present*. Every
library a program imports exists on disk, in its genuine form.

Some of them will work. Some of them will fail in ways that produce a crash with
no explanation. And a small number will fail in the specific way that matters
most: by appearing to work.

The **shadow set** is the list of libraries Raven forces to Wine's
implementation despite Microsoft's being right there. Every name on that list is
a place where the program gets a reimplementation instead of the real thing.
Shrinking it is the whole point.

## The floor, which is not negotiable

`ntdll.dll` and `win32u.dll` are where Windows crosses into its kernel. On
Windows they issue `syscall` instructions carrying NT syscall numbers. Those
numbers mean unrelated things to Linux, and the numbering is not even stable
across Windows builds.

These are not "hard to make work." They are the definition of the boundary
between the two operating systems, and Wine's implementations of them are what
Wine fundamentally *is*. They stay Wine's under every possible design.

## The coupled tier

`kernel32`, `kernelbase`, `user32`, `gdi32` and `advapi32` are user-mode
libraries with no syscalls of their own, so in principle they are candidates.
In practice they are coupled to two things Wine owns:

- **`ntdll`'s internals.** They call into it constantly, and not only through the
  documented surface. Microsoft's `kernel32` is built against Microsoft's
  `ntdll`, including behaviour that was never contracted.
- **`wineserver`.** Wine implements NT's object model — processes, threads,
  handles, sections, synchronization — in a separate server process. Wine's
  `kernel32` talks to it. Microsoft's talks to a kernel that is absent.

The expectation is that these stay Wine's. That expectation should still be
tested rather than assumed, because "obviously impossible" and "actually
impossible" have historically not been the same list.

## The band that is genuinely open

`ole32`, `oleaut32`, `rpcrt4`, `shell32`, `shlwapi`, `comctl32`, `comdlg32`,
`ws2_32`, and their neighbours.

Wine reimplements all of them. They are large, they are old, they are used by
almost everything, and their Wine implementations are the usual location of
"works, except." Microsoft's versions are sitting in the mounted base.

Whether they can be used is unknown. It is unknown per library, and it is unknown
per *combination* of libraries, which is the part that makes this a real
experiment rather than a checklist.

## The top, which is already settled

Microsoft's `ucrtbase` and `msvcr*`, the DirectX redistributables, `.NET`, media
codecs, `xinput` — these work under Wine today. This is not a hypothesis:
`winetricks` has been installing exactly these, as genuine Microsoft
redistributables into Wine prefixes, for over fifteen years, and it is the
standard fix for a large class of problems.

Raven's contribution here is not making it work. It is that these arrive from a
coherent Windows installation rather than from fifteen years of accumulated
per-application workarounds.

## The mechanism, revised by measurement

The two mechanisms below were the plan. Running Wine against a real deployed
Windows found a third, and it is better than either.

Wine refuses to run against a bare real Windows at all: finding Microsoft's
`C:\windows` where its own belongs, it decides the prefix needs rebuilding and
runs `wineboot` instead of the program.

`overlayfs` takes **multiple lower layers**, leftmost winning. So:

```
lowerdir=<wine-skeleton>:<real-windows>
```

gives Wine's files precedence wherever they exist and Microsoft's everywhere
else — and that *is* the shadow set, expressed as a filesystem layer. Measured:
`ntdll.dll` read through such a mount is Wine's 770 139 bytes rather than
Microsoft's 2 522 008.

This is better than an environment variable because it is inspectable: the
shadow set becomes "what the Wine layer contains", which can be listed, diffed
and reviewed, rather than a string that has to be believed.

**It does not work yet.** Wine's skeleton uses `windows` and `users`; Microsoft's
uses `Windows` and `Users`; `overlayfs` merges on the exact byte path, so the
trees stay separate and the mount shows both. Wine's own case-insensitivity acts
a layer above the filesystem and cannot help here. Normalising the skeleton's
casing to Microsoft's is the obvious next step and is untested.

## The first measured entry, and it is not a library

Running a real Windows installer found the first thing the base must not
provide, and it turned out to be an entire assembly store rather than a DLL.

**`Windows\WinSxS` is shadowed.** A real Windows carries a populated
side-by-side store. An installer whose manifest asks for
`Microsoft.Windows.Common-Controls` version 6.0 — which Inno Setup, and a large
share of Windows installers, do — gets Microsoft's `comctl32` out of it. That
library loads, and then does not work against Wine's `user32`.

The symptom is precise and thoroughly misleading:

| | |
|---|---|
| The window | drawn, correct size, correct frame |
| Its bitmaps | drawn correctly |
| Every control | created, and positioned |
| Text in any of them | **absent** |
| Response to a click | **none at all** |

Nothing errors. Nothing appears in a log. The installer simply sits there, whole
and inert, and a person would reasonably conclude the fonts are broken.

**`WINEDLLOVERRIDES` cannot fix this**, which is what makes it worth writing
down. Forcing `comctl32=b` changes nothing, because side-by-side resolution goes
through the activation context rather than the loader search path the override
governs. The override was the obvious remedy and it was measured to be useless.

Hiding the store fixes it completely: the wizard renders its text and its buttons
answer clicks. The mask sits in the read-only layer, so an installer that
registers its *own* assemblies into the environment is unaffected — only the
base's store disappears.

Two earlier findings turn out to have been the same thing seen from different
angles. The 112 373 WinSxS manifest lookups measured during the performance
investigation were Wine genuinely resolving activation contexts against the real
store; and the reason a bare game exited while its dialog flashed was the same
machinery failing earlier.

## The two mechanisms

**`WINEDLLOVERRIDES`** asks Wine which implementation to prefer, per library:

```
WINEDLLOVERRIDES="ole32,oleaut32=n,b;comctl32=b"
```

`n` selects native — Microsoft's file, the one in the base. `b` selects Wine's
builtin. `n,b` means try native and fall back. This is Wine's own supported
interface and it is the right tool for everything above the floor.

**Physically shadowing the file** in the overlay's upper layer replaces the file
Wine would find at all. It is heavier, it is visible in the filesystem rather
than in an environment variable, and it is the fallback for cases where the
override is not respected.

Prefer the override. Reach for the shadow only with a recorded reason, because a
shadowed file is a divergence from the base that is easy to forget about.

## Measuring it

The naive experiment — flip each library to native, one at a time, see what
breaks — answers the wrong question, because these libraries interact. `ole32`
native with `rpcrt4` builtin is a different system from both-native, and a
per-library result that ignores that will be confidently wrong.

The shape of a real measurement:

1. **Start from the maximal shadow set**, where everything Wine can provide is
   Wine's. That is approximately Wine's behaviour today, with the real files
   merely present, and it is the configuration most likely to work at all. It is
   the baseline.
2. **Fix a corpus** of programs with observable success criteria — starts,
   reaches a known state, produces a known output. A corpus of "seems to work"
   measures nothing.
3. **Move candidates to native in groups that are used together**, not one at a
   time. `ole32` and `oleaut32` and `rpcrt4` are one COM decision, not three.
4. **Record the whole configuration with each result.** The output of this work
   is a table of (Windows build, library set, corpus outcome), and a result
   without its configuration is not a result.
5. **Diagnose failures with `WINEDEBUG` first** — the `loaddll` and `relay`
   channels report what was loaded and what was called, and they are already
   there. Only if those prove insufficient does instrumenting Wine from the
   inside become worth its cost, and that instrument would be a Wine DLL, in C.

The output is a **data file**, keyed by Windows build, not a constant in the
source. It is derived from measurement, it will change as Wine changes, and it is
exactly the kind of table
[the org layout rule](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/repository-layout.md)
says belongs in a data file rather than a `const` array.

## The result that would be disappointing, and is still a result

The middle band may turn out to be thin. It is entirely possible that `ole32` and
its neighbours are coupled to `ntdll` internals tightly enough that Microsoft's
versions cannot be used, and that the honest shadow set is nearly as large as
Wine's default.

That would mean Raven's value rests on the real registry, the real application
libraries and the immutable base — which is still worth having — rather than on
a thin shadow set. It would also be the first published measurement of where
that line falls, which is worth having regardless of which side of it the answer
lands on.

What would make this work worthless is measuring it badly and reporting a number
nobody can reproduce. Hence the insistence on recording configurations.
