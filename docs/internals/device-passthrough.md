# Device passthrough

What it would take for a Windows disk utility to see and use a real block
device, established by reading Wine 11.16 and Rufus 4.15 source rather than by
guessing. The short version: **visibility is a Wine patch, writing is a
research project, and neither is configuration.**

## What Wine builds from configuration

Wine's `mountmgr.sys` creates device *objects* from three inputs:

- `dosdevices/x::` — a symlink to a unix block device becomes the raw device
  behind drive `x:`. Opening `\\.\X:` reaches the real device: genuine sector
  reads and writes.
- `HKLM\Software\Wine\Drives` — **HKLM, not HKCU** — sets the drive type.
  The values are treacherous: `"hd"` produces a volume device with **no**
  `PhysicalDrive` alias; the only registry route to a real
  `\Device\Harddisk` + `\\.\PhysicalDriveN` is `"floppy"` on a letter ≥ 2,
  which a hard-coded exception promotes to a hard disk.
- UDisks2 over the system D-Bus — a *removable* drive gets a device object
  and an auto-assigned letter. Fixed disks only ever become volumes.

One genuinely useful trick falls out of the path resolution: a symlink
`dosdevices/physicaldrive3 → /dev/sdX` makes `CreateFile("\\.\PhysicalDrive3")`
open the **real block device** — actual raw I/O, bypassing mountmgr's fake
disk object entirely.

## Why none of that is visible to Rufus

Rufus has exactly one enumerator: `SetupDiGetClassDevs(GUID_DEVINTERFACE_DISK,
… DIGCF_DEVICEINTERFACE)`. That API reads device *interfaces* — registry
entries under `DeviceClasses\{53f56307-…}` written by
`IoRegisterDeviceInterface` + `IoSetDeviceInterfaceState`. In all of Wine
11.16, only the HID and Bluetooth drivers ever register a device interface;
mountmgr's disks are plain device objects with no PnP identity, so the disk
interface class is **empty**, and every SetupDi-based tool sees zero disks —
regardless of any configuration. (The `Windows VDS is unavailable` notice
Rufus prints is cosmetic; VDS is its formatting backend, not its enumerator.
Rufus's author has declined Wine support outright — rufus#1411.)

## The tiers

| Goal | Tier | What it takes |
|---|---|---|
| Raw sector I/O for tools that open `\\.\PhysicalDriveN` or `\\.\X:` directly | **Configuration** | the links above, plus rw access to the device node. Works today; a candidate `raven env attach` feature. |
| Appearing in a SetupDi enumeration (Rufus's dropdown) | **Wine patch, upstreamable** | make mountmgr's disks PnP-enumerated PDOs (a synthetic storage bus, structurally like `winebus.sys` for HID), register `GUID_DEVINTERFACE_DISK`, expose `USBSTOR` as enumerator and a removable policy, report `BusTypeUsb` instead of the hard-coded `BusTypeScsi`. Nothing in wine or wine-staging does any of this today. |
| Flashing a bootable USB end to end | **Out of reach** | the mountmgr disk device has no read/write path at all, `IOCTL_DISK_SET_DRIVE_LAYOUT_EX` / `CREATE_DISK` / `UPDATE_PROPERTIES` are unimplemented on every path, volume lock/dismount are success-only stubs, and there is no VDS or `FormatEx`. Use the native tool. |

## What Raven does with this

Raven's floor rule applies: the mount and the world are Raven's; the API
surface is Wine's. The configuration tier can become a Raven feature —
explicit, per-environment, loudly labelled dangerous. The patch tier is an
upstream contribution to Wine, not a Raven component. The last tier is what
[troubleshooting.md](../guide/troubleshooting.md) already says: a hardware
utility needs the machine, and the native tool is the answer.
