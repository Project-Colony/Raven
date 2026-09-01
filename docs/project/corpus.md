# The corpus

Every program Raven has actually been run against, what happened, and - when it
failed - **which category** the failure belongs to.

This file exists because of a specific lesson. Raven's Direct3D support was
going to be measured with the one game the project had; DXVK installed
correctly and was then never called, because that game renders through GDI and
imports no Direct3D library at all. Nothing was broken. The corpus was one
program wide, and that program could not answer the question being asked of it.

A feature you cannot name a failing program for is a feature you are guessing
about. So: **the way to "it runs everything" is not more features, it is more
programs tried and every failure given a category.** Twenty entries tell you
where the wall is. Two tell you nothing.

## How to add an entry

Run it, write down what happened, and give any failure one of the categories
below. A row that says "did not work" and stops is worth less than no row - it
looks like knowledge and is not.

```bash
raven env create scratch --base <base>
raven run scratch -- wine /path/to/program.exe
```

Keep the entry even when the program works. A confirmation is evidence too, and
"which programs are known good" is the question every prospective user asks
first.

## Failure categories

Naming the category is the whole point: it turns one broken program into a
statement about a class of them.

| Category | What it means | Fixable in Raven? |
|---|---|---|
| `kernel-driver` | Needs a driver, so needs the NT kernel - kernel-mode anti-cheat above all | **No.** This is the permanent ceiling, and the reason a VM exists. See [../internals/device-passthrough.md](../internals/device-passthrough.md) |
| `com` | Resolves a COM server registered at install time. The projection deliberately does not carry the COM registry | Open question - the refusal is a design decision that could be revisited per-environment |
| `mui` | Strings live in `.mui` satellite files; the program runs mute | Known, unfixed |
| `service` | Expects a Windows service that the real installation had. Wine's Service Control Manager runs and serves ~15 services Wine reimplements, and a program **can** register its own - tested. What is absent is the 2835 service definitions in the base's `SYSTEM` hive, which the projection denies on purpose: they describe a different machine's drivers and hardware | Depends which. A program's own service should work; Microsoft's do not exist |
| `media` | Audio or video missing while the game itself runs. Wine decodes through GStreamer, and a system with GStreamer's libraries but not its plugins fails silently | Not Raven's code, but Raven's job to report: `raven doctor` names the missing package |
| `enumeration` | Discovers hardware through SetupDi device interfaces, which Wine never registers - Rufus is the type specimen | A Wine patch, not a Raven change |
| `drm` | A copy-protection wrapper refuses before the program starts. Steam's is the common one: launched outside a running Steam client it aborts with `Application load error 5:0000065434` | Not Raven's to fix. Test a title without the wrapper, or run the store client itself |
| `d3d` | A Direct3D problem: version unsupported, device creation fails, rendering wrong | Depends. D3D 8-11 is DXVK's; D3D12 needs vkd3d-proton, which Raven does not install |
| `wine` | Fails identically under plain Wine - not Raven's doing | Upstream |
| `raven` | Raven's own bug: the mount, the projection, the shadow set, the layer | **Yes - fix it** |

`wine` matters as much as `raven`: a program that fails the same way under a
plain prefix is not evidence about this project, and recording that stops the
same investigation from being run twice.

## The corpus

| Program | Kind | Result | Category | Notes |
|---|---|---|---|---|
| *N.P.C. Dreams* v1.12 | RPG Maker VX Ace game | **Runs**, but silently mute: `Missing decoder: MPEG-1 Layer 3` | `media` | The end-to-end proof: a real installer wrote 256 MB into the environment and the base finished byte-identical. Renders through `GDI32` only - it imports no Direct3D library, so it cannot exercise DXVK |
| RPG Maker VX Ace RGSS3 runtime | Installer (Inno-style) | **Installs** | — | The one installer framework exercised so far |
| Rufus 4.15 | Disk utility | **Runs and renders**, but finds no devices | `enumeration` | Also a datum in the other direction: it *crashes at startup under plain Wine* and runs under Raven. A real Windows sometimes fixes a program rather than breaking it |
| Fallout New Vegas (Steam) | Game, Direct3D 9 | **Refuses to start**: `Application load error 5:0000065434` | `drm` | Steam's DRM wrapper, aborting before the engine exists. Says nothing about Raven or Direct3D - and is exactly why the corpus records categories: without one, this reads as "Raven cannot run Fallout" |
| *ShineHill* (Steam, GameMaker) | Game, Direct3D 11, 64-bit | **Runs and renders.** DXVK created a D3D11 device on the GPU | — | The first game to draw through Direct3D against a real Windows. Chosen by reading its imports (`d3d11.dll`) and confirming it carries no Steam DRM wrapper - the two things that defeated the previous attempts |
| *ShineHill*, second run onward | Game, Direct3D 11 | **Runs**, and a relaunch immediately after closing now works | — | What sessions bought: before them a relaunch inside three to six seconds was refused outright. The remaining several-second wait is the game's own start-up - plain Wine takes 7.9 s to the same window |
| `dxdiag.exe` (Windows 11 26200) | Microsoft's DirectX diagnostic | **Runs**, drove DXVK to initialise and enumerate the GPU | — | How Direct3D-on-Vulkan was first shown to work at all against a real Windows. Its DirectDraw probe went through WineD3D and its D3D9 probe through DXVK, in one process |

## Experiment: Proton's Wine against a real Windows

Run 2026-09-01 on the branch `experiment/proton-wine`. The hypothesis was
appealing: keep Raven and the real Windows, gain Proton's Wine patches.

**It works.** Proton Experimental's `wine` runs standalone outside the Steam
runtime with no missing libraries, builds a prefix, and Raven creates and runs
an environment with it against the same real Windows base. One change was
needed and is worth keeping regardless: Raven invoked a bare `wineboot`, which
is a wrapper script a distribution's package happens to ship - Proton's `bin/`
has only `wine` and `wineserver`. Going through `wine wineboot` works with
every build.

**It brings nothing here.** Same game, same base, same DXVK 3.1, both using
ntsync:

| | session ready | *ShineHill* to a window |
|---|---|---|
| system Wine 11.16 | **1.8 s** | **10.06 s, 9.90 s** |
| Proton Experimental (branch 11.0, build 20260826) | 10.5 s | 11.18 s, 11.51 s |

Slower to a window, and nearly six times slower to bring a session up. No
behaviour differed.

**Why that is not surprising, in hindsight.** Proton's patches are dominated by
per-title hacks for games it ships with, plus work that compensates for a
*synthetic* Windows being incomplete - forcing builtin DLLs, spoofing Steam
input, faking what a real installation would have provided. Raven has the real
installation, so most of that is either irrelevant or actively opposed to the
premise. What would genuinely transfer - a more mature Media Foundation - this
workload never exercised.

**One constraint the experiment exposed**, unrelated to Proton and true in
general: a session pins its `wineserver`, so whichever Wine starts a session is
the Wine every later launch in it must use. Mixing gives
`wine client error: version mismatch 961/931`. That is inherent to how sessions
work and worth knowing before anyone tries to switch Wine under a live one.

**Verdict: not adopted.** Kept as a documented negative result so nobody spends
an evening rediscovering it.

## What the corpus does not yet contain

Named so the gaps are visible rather than merely absent:

- **A Direct3D 12 title.** D3D12 is vkd3d-proton's, not DXVK's, and Raven
  installs neither. Cyberpunk 2077 is the type specimen sitting untested.
- **A 32-bit Direct3D title.** The x32 libraries are installed and unexercised.
- **Anything using COM**, the category most likely to be broken by a design
  decision rather than a bug.
- **A program whose service must actually *run*, not merely register.**
  `sc create` and `sc query` work; nothing has yet been made to start and stay
  up under Wine's Service Control Manager.
- **A second installer framework.** NSIS, MSI, InstallShield - all untried.
- **Anything with a `.mui` file**, to see how mute "mute" really is.
- **A 32-bit program of any kind other than the RPG Maker game.**
