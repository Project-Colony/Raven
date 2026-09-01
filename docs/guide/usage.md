# Using Raven

From an installation image to a running program. Three commands, and then
`./program.exe` works like anything else.

## 1. Deploy a base

A **base** is a real Windows, deployed once, mounted read-only, and shared by
every environment. It is never written to.

```bash
raven base editions --image install.wim
```

```
  1  Windows 11 Home  (build 26200)
  6  Windows 11 Pro  (build 26200)
```

```bash
raven base deploy --image install.wim --edition 6 --name win11-pro
```

This writes about 14 GB and takes a few minutes. No hypervisor is involved and
Windows never boots — booting is what would bind the installation to hardware
that is not there.

## 2. Create an environment

An **environment** is a Wine layer over a base, plus somewhere to put writes. It
is cheap, and it is disposable.

```bash
raven env create games --base win11-pro
```

Twenty seconds, during which Raven builds a Wine prefix, turns its `drive_c`
into a read-only layer above the base, renames that layer to match Windows'
spelling so the two actually merge, points the prefix's C: at the runtime mount,
and projects the real registry into it.

## 3. Run something

```bash
raven run games -- wine 'C:\Windows\System32\notepad.exe'
```

Anything the program writes lands in the environment. The base finishes
byte-identical.

## Making `./program.exe` work

The package already did this: it registered `.exe` with the kernel and masked
Wine's own registration, which otherwise silently wins the same `MZ` magic. On
a source build, `raven binfmt` prints exactly what to install; either way,
`raven doctor` shows who gets a double-clicked `.exe` right now.

Then tell Raven which environment to use for programs that are not inside one:

```bash
raven env default games
```

After that, `./setup.exe` runs like any other executable. A program **inside** an
environment resolves to that environment on its own; the default covers
everything else.

## Giving a program a real disk

Some tools want a device, not files — sector editors, imaging tools, anything
that opens `\\.\PhysicalDriveN`. Wire one in:

```bash
raven env attach games /dev/sdc
```

The device appears as `d:` and `\\.\PhysicalDrive1` — the exact number is
printed; Raven numbers disks the way Wine's own mountmgr does, counting from
1 because PhysicalDrive0 is a stub Wine pre-creates — with **raw read and
write access to its sectors**. Attach a disk whose contents you are prepared
to lose to the program you are about to run. Raven never changes the device
node's permissions; if you cannot open it, `attach` prints the `setfacl`
command that grants it. `raven env detach games` undoes the whole thing, and
the environment must be stopped for either direction.

What this does and does not make visible — tools that *enumerate* disks
instead of opening them by name will still show an empty list — is in
[../internals/device-passthrough.md](../internals/device-passthrough.md).

## Direct3D through Vulkan

Wine translates Direct3D to OpenGL; DXVK reimplements it on Vulkan, and most
games want the second. It is a set of DLLs, not a patch, so Raven installs it
the way it does everything else - from something you already have:

```bash
raven env dxvk games --from ~/Downloads/dxvk-2.7.tar.gz
```

Raven downloads nothing and bundles no version, exactly as `base deploy` takes
an ISO you supply. Point it at an upstream release, at the copy inside a Proton,
at a distribution's package - whichever you trust.

**Updating is the same command with a newer build.** Raven replaces what it
installed and deletes what the new version no longer ships, so an older module
cannot survive beside newer ones - upstream dropped `d3d10.dll` exactly that
way, and a stale one paired with a current `d3d11.dll` is how a mismatch breaks
without saying so.

C: is a real Windows, so `System32` already holds Microsoft's own `d3d11.dll`
and `dxgi.dll`. The DXVK copies land in the environment's writable layer and
shadow them; the base finishes byte-identical, and `raven env dxvk games
--remove` uncovers the real ones again by deleting Raven's rather than restoring
Microsoft's. Raven refuses to overwrite a library it did not install itself, so
a DLL some installer left in the environment is safe.

DXVK has been shown to initialise against a real Windows and reach the GPU -
Microsoft's own `dxdiag.exe` drove it and DXVK enumerated the card. No game has
rendered a frame through it yet, and nothing is benchmarked; see
[../project/status.md](../project/status.md) for exactly how far that goes.

## Starting over

```bash
raven env destroy games
```

Deletes one directory. The base is untouched, which is the point: a broken
install costs twenty seconds, not a re-deployment.

## Changing what the registry carries

Each environment has a `registry-rules.toml` saying which parts of the real
Windows registry cross into it. Edit it, then:

```bash
raven env reproject games
```

What the defaults do and why — including why the COM registry is deliberately
switched off — is in
[../internals/registry-projection.md](../internals/registry-projection.md).
