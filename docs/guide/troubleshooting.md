# When something looks wrong

Failure modes that have actually happened, and what they mean. Anything not
listed here has not been seen yet — if you hit it, it is new.

## `raven doctor` says user namespaces are unavailable

The kernel restricts them. `linux-hardened` disables them outright, Ubuntu
restricts them through AppArmor by default, and some hardened configurations
turn them off deliberately.

Raven's only implemented mount backend needs them. Two other backends are
designed for exactly this case — `fuse-overlayfs`, and a privileged helper — and
neither is built yet. Until one is, Raven cannot run on such a kernel.

## A program starts, exits cleanly, and prints nothing

Expected, for now, and it is not your setup.

Modern Windows keeps program strings in separate `.mui` resource files rather
than inside the executable. A base holds around ten thousand of them, and Wine
does not open them. `LoadString` finds nothing, so the program prints an empty
message and exits zero. It ran — it just has nothing to say.

This is the largest known gap. See
[../project/status.md](../project/status.md).

## A program appears to work, and you want to be sure it was Microsoft's

Wine ships several hundred `.exe` files of its own, so correct output proves
nothing about where the program came from. Only the loader says:

```bash
WINEDEBUG=+loaddll raven run games -- wine 'C:\Windows\System32\where.exe'
```

`native` means the binary came from your Windows base. `builtin` means it was
Wine's own.

## Every `.exe` runs against `~/.wine` instead of your environment

Wine's package registers a handler for the same `MZ` magic, and when both are
present the kernel picks one silently — the failure looks like Raven losing
your prefix, and it once cost an hour. The package prevents it by masking
Wine's registration; if you assembled things by hand:

```bash
raven doctor
```

lists every registration claiming a `.exe`, names the one the kernel will
pick, and prints the masking fix when it is not Raven's.

## A second launch fails, or a program will not start again

Closing a program's window can leave `wineserver` and a handful of Wine
services alive inside the mount namespace, holding the environment busy.

```bash
raven env status games
```

names the processes holding it, and

```bash
raven env stop games
```

terminates them and releases the environment. Launches into a held environment
refuse with exactly these two commands rather than a bare
`Device or resource busy`.

## Launching feels slower than Proton

Slightly — about 20 ms per process over plain Wine (135 ms against 113,
measured). It used to be 2× worse until the cause was found: Wine re-checks
every font file in the base's `C:\windows\fonts` at every process start, and
masking that directory removed 92 of the 105 milliseconds. Four plausible
explanations were measured and destroyed before that one —
[../internals/performance.md](../internals/performance.md) has the numbers, so
nobody repeats them.

## An environment will not delete

If `destroy` refused because the environment is running, that is deliberate —
it named the processes holding the mount, and `raven env stop <name>` releases
it. For anything else: `overlayfs` leaves a `work/work` directory with no permissions at all,
which a plain recursive delete cannot enter — and it stops *after* removing the
upper layer, leaving something that can neither be destroyed nor recreated.

`raven env destroy` restores permissions as it descends and handles this. If you
deleted an environment by hand and hit it, `chmod -R u+w` the directory first.

## A disk or hardware utility finds no devices

Seen with Rufus 4.15: the interface runs, and reports `Windows VDS is
unavailable` then `0 devices found`, with the USB key plugged in and visible
to Linux.

Expected, under any Wine, forever. Tools like Rufus enumerate drives through
the Virtual Disk Service and raw physical-drive handles
(`\\.\PhysicalDrive0`), and Wine implements neither — your block devices are
simply not part of the Windows world it presents. This is the same category
as kernel drivers: a hardware utility needs the machine, not just the API
surface, and Raven deliberately gives programs a Windows *world*, not
Windows *hardware*. Use the native tool for the job instead — for a bootable
USB stick, that is Ventoy, GNOME Disks, or `dd`.

The same Rufus is still a useful data point: under plain Wine it crashes at
startup before showing a window; against a Raven environment it runs and
renders. Real-Windows worlds sometimes fix programs, not just break them.

## Anti-cheat refuses to start the game

If the publisher has enabled Easy Anti-Cheat or BattlEye's Linux support, the
game should work — and if it works under Proton but not Raven, that is a bug
worth reporting.

If the publisher has not enabled it — PUBG, Fortnite, Valorant — the game does
not run on Linux at all, under anything. That is a decision at the publisher, and
Raven will not work around it: these systems ban on detecting tampering, so a
tool that tried would get you banned. See
[../project/landscape.md](../project/landscape.md).
