# The landscape

What running Windows software on Linux looks like today, and the gap Raven aims at.

## The thing everyone gets wrong first

The instinct on meeting this problem is to reach for emulation: Linux should
*emulate* Windows, the way an emulator runs a console game. It is the wrong
model, and the name of the incumbent says so — **WINE is a recursive acronym for
"Wine Is Not an Emulator."**

A Windows `.exe` contains x86 machine code. Your CPU is an x86 CPU. It executes
that code directly, at full speed, with nothing translating anything. There is
no emulation to add because there is no instruction set gap to bridge.

What a `.exe` also contains is a list of imports: `kernel32.dll`, `user32.dll`,
`ntdll.dll`, `d3d11.dll`. Those are not part of the program — they are part of
the *operating system*. The program's first instruction cannot run until
something resolves them. That is the entire problem, and it has never been a
CPU problem.

So the question is never "how do we emulate Windows." It is "where does the
Windows side of that boundary come from."

## How the existing answers resolve it

### Wine and Proton: reimplement the boundary

Wine provides its own `kernel32`, `user32`, `ntdll` and several hundred more,
written from scratch to behave as Microsoft's do, and translating downward into
Linux syscalls instead of NT ones. Around them it builds a *prefix*: a directory
that looks like a C: drive, plus `system.reg` and `user.reg`, plain text files
standing in for the Windows registry.

This is an enormous achievement and it works far better than it has any right
to. Proton is Valve's Wine, with patches, DXVK and VKD3D translating Direct3D
into Vulkan, and it carries a large fraction of the Windows game catalogue.

What it costs is that the program's world is a reconstruction. The prefix is
something Wine built, not something Windows installed. The registry describes a
machine that was never configured. A library Wine has reimplemented to 95% is
95% correct, and the missing 5% is distributed unevenly across the software that
exists.

### A virtual machine: bring the real boundary, isolated

QEMU/KVM running an actual Windows gives perfect fidelity, because it is not an
approximation of Windows — it *is* Windows, with its own NT kernel, its own
drivers, its own everything.

The cost is that it is a second computer. Separate memory, separate filesystem,
separate GPU arrangement, a separate machine to keep updated, and its display is
a screen rather than an application. Tools like WinApps paper over the last part
with seamless RDP, but the isolation is structural: it is what a hypervisor is
for. For someone who wants their Linux and their Windows software to be *one
system*, isolation is the problem, not the solution.

### Longene: move the boundary into the kernel

Longene implemented NT syscalls as a Linux kernel module, merging Wine's job
into the kernel. It is dead, and the reason is instructive: an out-of-tree module
chasing both NT semantics and Linux internals is a maintenance burden that grows
in two directions at once, and Wine's userspace approach turned out to be more
sustainable for most of what it did.

### ntsync: move *part* of the boundary into the kernel

The counter-example, and the reason "kernel" should not be dismissed outright.

Wine implements NT synchronization objects — mutexes, semaphores, events —
through `wineserver`, a separate process reached by IPC. For a game
synchronizing thousands of times a second, that round trip is a serious cost.
`ntsync`, written by Elizabeth Figura at CodeWeavers, implements those
primitives as a kernel driver, and it was **merged into Linux 6.14**.

The lesson is not "put NT in the kernel." It is that a narrow, well-chosen piece
of NT semantics can belong in the kernel and can land upstream, while the
wholesale version cannot. Raven does not write kernel code; it uses `ntsync`
where it is available, and treats it as evidence about which questions are worth
asking.

## What is left unoccupied

Line the answers up by where the Windows side comes from:

| | The Windows side is | Where it lives |
|---|---|---|
| Wine / Proton | reimplemented | a synthetic prefix |
| Virtual machine | genuine | behind a hypervisor |
| **Raven** | **genuine** | **mounted directly, as the program's C:** |

Nobody occupies the third row, and the reason is not that it is impossible — it
is that Wine explicitly discourages pointing a prefix at a real Windows
installation, because doing it naively breaks in ways that generate
unanswerable bug reports. That warning is correct. It is a warning about doing
it naively.

Doing it deliberately means three things Wine has no reason to build:

1. **The base must be immutable.** Naive attempts write into the Windows
   installation, which is how they corrupt it and how they become
   irreproducible. See [../internals/mount-stack.md](../internals/mount-stack.md).
2. **The registry must be projected, not imported.** A real `HKLM\SYSTEM`
   describes real hardware, real drivers and a real service database. Loading it
   into a prefix overwrites Wine's description of its own synthetic environment
   and breaks everything. See
   [../internals/registry-projection.md](../internals/registry-projection.md).
3. **The library shadow must be exact.** Some libraries must be Wine's for
   reasons of physics, some may be Microsoft's, and nobody has measured where the
   line falls. See [../internals/shadow-set.md](../internals/shadow-set.md).

That third point is the part of Raven that is genuinely new. The first two are
engineering; this one is a question with no published answer.

## What Raven cannot do

Stated here rather than discovered later.

- **Kernel-mode drivers will never load.** A `.sys` file is code for the NT
  kernel, and there is no NT kernel. This excludes driver-based DRM and hardware
  utilities that ship a driver. Using a real Windows installation does not change
  this by even a little: the drivers are present as files and remain unloadable.
  What it means for anti-cheat specifically is more nuanced, and is below.
- **`ntdll` and `win32u` stay Wine's.** They are where Windows issues syscalls.
  Microsoft's versions would issue NT syscall numbers into a Linux kernel. This
  is not a limitation to be engineered around; it is the definition of the
  boundary.
- **Raven is Linux-only.** It mounts `overlayfs` and registers `binfmt_misc`.
  Neither concept exists on the other two platforms the org targets.

## Anti-cheat: a compatibility target, not a problem to solve

Worth stating carefully, because "anti-cheat does not work on Linux" is the
common belief and it is wrong.

**Easy Anti-Cheat has supported Linux and Proton since 2021, and BattlEye since
2022.** Both work. Support is **opt-in per game**, enabled by the developer in
their own dashboard. Where a developer has enabled it, the game runs under
Proton today, and Raven's job is simply not to break that.

Where a developer has *not* enabled it — PUBG, Fortnite, Valorant — the game does
not run on Linux. **That is a business decision at the publisher, not a technical
gap.** No amount of engineering below it changes the answer: not Wine, not
Proton, not Raven, and not an NT kernel written from scratch. The lever is the
publisher's switch.

### The line Raven does not cross

Making a game run anyway would mean convincing its anti-cheat that it is on a
genuine Windows with its kernel driver loaded. That is anti-cheat circumvention —
the same technique cheats use — and Raven will not implement it.

The practical objection is as strong as the principled one: these systems ban on
detecting tampering. A tool that did this would get the people using it banned.

### What Raven does instead

Anti-cheat is treated as a **compatibility target**. A real Windows base is a
more coherent environment than a synthetic prefix — real registry, real system
libraries, a populated side-by-side store — and a user-mode anti-cheat module
performs environment consistency checks that have nothing to do with detecting
Linux. Passing those better than Wine alone is a legitimate gain, on the games
that already permit Linux.

That gives a measurable milestone: take a game whose developer has enabled the
Linux path, run it under Raven, and compare against Proton. Parity means nothing
was broken. Better than parity is an argument worth publishing.
