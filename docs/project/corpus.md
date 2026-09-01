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
| `service` | Expects a Windows service to be running. None are | Open question |
| `enumeration` | Discovers hardware through SetupDi device interfaces, which Wine never registers - Rufus is the type specimen | A Wine patch, not a Raven change |
| `d3d` | A Direct3D problem: version unsupported, device creation fails, rendering wrong | Depends. D3D 8-11 is DXVK's; D3D12 needs vkd3d-proton, which Raven does not install |
| `wine` | Fails identically under plain Wine - not Raven's doing | Upstream |
| `raven` | Raven's own bug: the mount, the projection, the shadow set, the layer | **Yes - fix it** |

`wine` matters as much as `raven`: a program that fails the same way under a
plain prefix is not evidence about this project, and recording that stops the
same investigation from being run twice.

## The corpus

| Program | Kind | Result | Category | Notes |
|---|---|---|---|---|
| *N.P.C. Dreams* v1.12 | RPG Maker VX Ace game | **Runs**, from a double-clicked `.exe` in a file manager to its title screen | — | The end-to-end proof: a real installer wrote 256 MB into the environment and the base finished byte-identical. Renders through `GDI32` only - it imports no Direct3D library, so it cannot exercise DXVK |
| RPG Maker VX Ace RGSS3 runtime | Installer (Inno-style) | **Installs** | — | The one installer framework exercised so far |
| Rufus 4.15 | Disk utility | **Runs and renders**, but finds no devices | `enumeration` | Also a datum in the other direction: it *crashes at startup under plain Wine* and runs under Raven. A real Windows sometimes fixes a program rather than breaking it |
| `dxdiag.exe` (Windows 11 26200) | Microsoft's DirectX diagnostic | **Runs**, drove DXVK to initialise and enumerate the GPU | — | How Direct3D-on-Vulkan was first shown to work at all against a real Windows. Its DirectDraw probe went through WineD3D and its D3D9 probe through DXVK, in one process |

## What the corpus does not yet contain

Named so the gaps are visible rather than merely absent:

- **A game that renders a frame through Direct3D.** DXVK initialises; nothing
  has drawn through it. This is the largest single unknown.
- **Anything using COM**, the category most likely to be broken by a design
  decision rather than a bug.
- **A second installer framework.** NSIS, MSI, InstallShield - all untried.
- **Anything with a `.mui` file**, to see how mute "mute" really is.
- **A 32-bit program of any kind other than the RPG Maker game.**
