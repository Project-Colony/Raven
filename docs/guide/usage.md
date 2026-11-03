# Using Raven

From an installation image to a running program. Three commands, and then
`./program.exe` works like anything else.

## 1. Deploy a base

A **base** is a real Windows, deployed once, mounted read-only, and shared by
every environment. Programs never write to it.

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
Windows never boots - booting is what would bind the installation to hardware
that is not there.

## 2. Create an environment

An **environment** is a Wine layer over a base, plus somewhere to put writes. It
is cheap, and it is disposable.

```bash
raven env create games --base win11-pro
```

Twenty seconds, during which Raven builds a Wine prefix, turns its `drive_c`
into a read-only layer above the base, renames that layer to match the base's
spelling so the two actually merge, points the prefix's C: at the runtime mount,
and projects the real registry into it.

## 3. Run something

```bash
raven run games -- wine 'C:\Windows\System32\notepad.exe'
```

Anything the program writes lands in the environment. The base finishes
byte-identical.

## A command and a menu entry for a program

`raven run` takes the whole command line every time. A shortcut gives a
program a command of its own and an entry in the applications menu, as an
installer would:

```bash
raven env shortcut games mygame -- wine 'C:\Games\MyGame\MyGame.exe'
```

After that, `mygame` in a terminal, or **MyGame** in the menu, starts the
program in `games`. What follows `--` is the command line `raven run` takes,
and arguments given to the command are added after it, so `mygame -windowed`
passes `-windowed` to the program. The menu shows the program's file name
without `.exe` unless `--title` names it. When Raven finds the program in the
environment, it starts in its own directory, `C:\Games\MyGame` here, as a
Windows shortcut starts it; `--cwd` names another, as for `raven run`. Raven
warns when it does not find the program.

Three files make a shortcut:

- `~/.local/bin/mygame`, a shell script. Raven says so if `~/.local/bin` is
  not on your `PATH`.
- `~/.local/share/applications/raven-shortcut-mygame.desktop`, the menu entry.
- `~/.local/share/Colony/Raven/icons/mygame.png`, the program's icon, read out
  of its `.exe`. A program with no icon Raven can read shows Raven's.

The script and the menu entry each carry a line marking them as Raven's, so a
file of yours with the same name is never replaced or deleted. A name another
program already answers to anywhere on your `PATH` is refused, since one would
hide the other, and so are `wine` and `wineserver`, which Raven itself runs by
name. A shortcut of the same name in another environment has to be removed
there first.

Started from a terminal, the program prints there as usual, and
`mygame > out.txt` goes to `out.txt`. Started from the menu, or anywhere with no
terminal at all, the command writes what it prints to
`~/.cache/Colony/Raven/logs/mygame.log`, in a directory only you can read, and
keeps the previous start's as `mygame.log.1`, so the reason for a crash is still
there after the next start.

```bash
raven env shortcut games                  # list the shortcuts of games
raven env shortcut games mygame --remove  # delete the three files
```

A program whose installer puts a shortcut in the Start menu does not need one:
Wine turns that shortcut into a menu entry, and Raven makes the entry start in
the environment. A shortcut is for a program that was copied in, such as a
portable build or a game's own launcher. Destroying an environment leaves its
shortcuts, and says so; `raven env shortcut <name>` still lists and removes
them.

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

Some tools want a device, not files - sector editors, imaging tools, anything
that opens `\\.\PhysicalDriveN`. Wire one in:

```bash
raven env attach games /dev/sdc
```

The device appears as `d:` and `\\.\PhysicalDrive1` - the exact number is
printed; Raven numbers disks the way Wine's own mountmgr does, counting from
1 because PhysicalDrive0 is a stub Wine pre-creates - with **raw read and
write access to its sectors**. Attach a disk whose contents you are prepared
to lose to the program you are about to run. Raven never changes the device
node's permissions; if you cannot open it, `attach` prints the `setfacl`
command that grants it. `raven env detach games` undoes the whole thing, and
the environment must be stopped for either direction.

What this does and does not make visible - tools that *enumerate* disks
instead of opening them by name will still show an empty list - is in
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

An archive is unpacked under `~/.cache/Colony/Raven/unpack/`, in a directory only
you can open, and deleted once its libraries are copied. A library that is a
symbolic link inside the archive is refused; a directory you point at directly is
yours to arrange.

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
Microsoft's own `dxdiag.exe` drove it and DXVK enumerated the card, and
*ShineHill*, a Direct3D 11 game from Steam, then ran and drew through it; see
[../project/status.md](../project/status.md) for exactly how far that goes.

### Direct3D 12

DXVK stops at Direct3D 11. Twelve is a different project, **vkd3d-proton**, and
it installs beside DXVK rather than instead of it:

```bash
raven env vkd3d games --from ~/Downloads/vkd3d-proton-3.0.1.tar.zst
```

A game wanting D3D11 and a game wanting D3D12 are different games, and one
environment serves both. Removing either leaves the other alone. No D3D12 title
has been run through it yet, though - vkd3d-proton is installed and unproven
where DXVK is not.

Neither command has an opinion about whose build you use, and that is not
politeness: CachyOS's Proton and Valve's both carry these two projects as
**unpatched upstream submodules**. What a Proton distribution forks is Wine.
There is no distinct "CachyOS DXVK" to prefer.

### DLSS and Reflex on an NVIDIA card

DXVK reports an NVIDIA card to games as an AMD one, so they offer neither DLSS
nor Reflex. Both go through NVIDIA's NVAPI, which Wine does not have.
**dxvk-nvapi** provides it on top of DXVK and vkd3d-proton, and installs the same
way:

```bash
raven env nvapi games --from ~/Downloads/dxvk-nvapi-v0.9.3.tar.gz
```

A dxvk-nvapi archive has no directory of its own around `x64/`, so the build is
recorded under the archive's name.

DLSS also loads `nvngx.dll` and `_nvngx.dll` from `System32`. The NVIDIA driver
ships them for Wine, in `/usr/lib/nvidia/wine` on Arch, and Raven links them
there from the driver's directory instead of copying them, so a driver update
reaches the environment without reinstalling. It looks in
`/usr/lib/nvidia/wine`, `/usr/lib64/nvidia/wine` and
`/usr/lib/x86_64-linux-gnu/nvidia/wine`. If the driver is elsewhere, the install
says so and only Reflex can work.

Proton enables nvapi by default in the same way, except that it copies the two
driver files and its own opt-out is `PROTON_DISABLE_NVAPI=1`.

Once it is installed, every program in the environment starts with
`DXVK_ENABLE_NVAPI=1`, which makes DXVK report the card as what it is, and
`NVIDIA_WINE_DLL_DIR` pointing at the driver's directory, where NVIDIA's
libraries look for the rest of the driver. A value you set yourself is kept, so
a game that misbehaves with nvapi can be started without it:

```bash
DXVK_ENABLE_NVAPI=0 raven run games -- wine game.exe
```

Under Raven this has been checked down to the variables a Windows program sees;
no game has shown DLSS through it yet.

## Why the second launch is faster

The first program you run in an environment starts a **session**: Raven mounts
C: once, starts Wine's server and services, and keeps all of them until you stop
the session, so every later launch joins what is already there instead of
building a world of its own. Measured with a trivial program, the first launch
of the day costs about two seconds and every later one about 0.16 s, after a
pause as much as straight after the previous launch, against plain Wine's
0.12 s once its own server is warm. On a base from an earlier release a later
launch costs about a quarter of a second until the base is converted (below). A
real program adds its own start-up on top: a Direct3D 11 game took 7.7 to 9.5
seconds to show its window, against 7.9 under plain Wine. The figures are in
[../internals/performance.md](../internals/performance.md).

You can also bring an environment up before you need it:

```bash
raven env start games
```

Mounting costs four hundredths of a second. `start` also brings Wine's services
up, which costs about 1.7 seconds, so even the first launch joins them. While
the session is up they use about 50 MiB of memory.

The session holds C: until you release it:

```bash
raven env stop games
```

You need that only to change the environment itself - installing DXVK, attaching
a device, re-running the projection - because those cannot safely happen under a
live Wine. They will tell you so, and name this command.

The mount lives inside the session's own namespace, so a crash leaves nothing
on the host and nothing stale for the next boot to trip over. If the anchor
dies, the session's `wineserver` and Wine's services keep C: mounted inside
that namespace until the next launch ends them, `raven env stop` does, or the
machine restarts.

The session's `wineserver` does not exit on its own, so `wineserver -w`, which
waits for it to exit, waits until `raven env stop`. winetricks runs it after
most verbs and stalls there. To let such a script go on once its installer has
finished, run `raven run games -- wineserver -k` from another terminal. That
also closes every program open in the environment, and the session has no
persistent server until it is stopped and started again.

## Converting a base from an earlier release

Raven deploys a base with `windows`, `system32`, `fonts`, `users` and 24 other
directories spelled the way Wine looks them up. A base deployed by an earlier
release still spells them `Windows`, `System32` and so on. It works, but every
file lookup under `C:\windows` makes Wine list a directory, and a process
start costs about 240 ms instead of 160. `raven doctor` names such a base.

Stop every environment built on it, then convert it:

```bash
raven env stop games
raven base respell win11-pro
```

This renames those directories in the base and in the layers of each of its
environments, all together; no file's content changes, and the programs and
saves in the environments stay where they are. It takes well under a second.

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

What the defaults do and why - including why the COM registry is deliberately
switched off - is in
[../internals/registry-projection.md](../internals/registry-projection.md).

The file starts as a copy of
[`config/registry-rules.toml`](../../config/registry-rules.toml), with the
reason for every rule beside it.

## Language

Raven speaks English and French: its output, its errors and `--help`. It follows
the locale, read the way every other program reads it: the first of `LC_ALL`,
`LC_MESSAGES` and `LANG` that is set decides, and a value starting with `fr`
means French. Anything else, `C` included, means English.

```bash
LANG=fr_FR.UTF-8 raven doctor     # in French
LC_ALL=C raven doctor             # in English, whatever LANG says
```

Commands, program names and paths read the same in both. Errors that come from
the system itself, such as "Permission denied", are worded by the system.

The window has its own choice: Preferences, under General, sets its language
to the system's, English or French, and the change applies at once.

## The window

`raven-gui` shows environments, bases and diagnostics, and acts on them through
the same library as the command line. Ctrl+1, Ctrl+2 and Ctrl+3 switch between
the three screens, on any keyboard layout.

Click **Raven**, top left, to open Preferences; click it again, or Close, to
return to the screen you were on. Preferences hold the window's language, its
theme and accent, the text size, and the accessibility switches: high
contrast, a dyslexia-friendly font, reduced motion and a second text size that
multiplies the first. There is no Save button. Each change applies and is
written as it is made, to `~/.config/Colony/Raven/preferences/preferences.toml`,
and a value the window does not understand falls back to its default rather
than stopping it from opening.
