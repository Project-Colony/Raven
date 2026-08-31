# Registry projection

Reading a real Windows registry and giving Wine the parts that apply, without
giving it the parts that would destroy the prefix.

## Two formats, and they do not meet

Windows keeps the registry in **binary hive files**:

| Hive | Mounts at | Holds |
|---|---|---|
| `System32\config\SYSTEM` | `HKLM\System` | hardware, drivers, the service database, control sets |
| `System32\config\SOFTWARE` | `HKLM\Software` | per-machine software: install paths, COM registrations, file associations |
| `System32\config\SAM`, `SECURITY` | — | local accounts and security policy |
| `System32\config\DEFAULT` | `HKU\.DEFAULT` | the profile template |
| `Users\<u>\NTUSER.DAT` | `HKCU` | that user's per-user software state |
| `…\AppData\Local\Microsoft\Windows\UsrClass.dat` | `HKCU\Software\Classes` | per-user COM and associations |

Wine keeps the registry as **text files** in the prefix — `system.reg`,
`user.reg`, `userdef.reg` — and cannot read a hive.

So a bridge is needed. The question is what should cross it.

## Why importing everything destroys the prefix

The tempting version of this feature is "mount the hives, dump them, import the
lot." It produces a prefix that does not work, and the reason is `HKLM\System`.

That hive is a description of a **specific physical machine**:
`CurrentControlSet\Services` is the NT driver and service database;
`CurrentControlSet\Enum` enumerates devices that were present; `Control`
describes disk layout, class GUIDs, and the boot configuration.

Wine populates its own `HKLM\System` with a description of *its* synthetic
environment — the drives it presents, the minimal services it emulates, the
devices it pretends to have. Overwriting that with the description of a real
machine replaces a true account of the running environment with a true account
of a different, absent one. Every subsequent lookup gets a confident wrong
answer, which is worse than an empty key.

`SAM` and `SECURITY` are worse still and have no upside: they describe accounts
that do not exist here, and they are the two hives it is least appropriate to
copy around.

## What actually crosses

Deny by default. A subtree is projected only if it is on the allow list.

**Projected** — the keys that describe *software*, which is the thing that
genuinely carries over:

- `HKLM\Software\<vendor>\…` — where a program installed itself, its options,
  its licence state
- `HKLM\Software\Classes\CLSID`, `Interface`, `TypeLib` — COM registrations
  pointing at libraries that are present in the base
- `HKCU\Software\<vendor>\…` from `NTUSER.DAT` — per-user settings
- `HKCU\Software\Classes` from `UsrClass.dat` — per-user COM and associations

**Never projected** — the keys that describe a *machine*:

- the whole of `HKLM\System`
- `SAM` and `SECURITY`, entirely
- the subtrees of `HKLM\Software\Microsoft\Windows NT\CurrentVersion` that record
  which physical installation this was

**Rewritten or dropped** — values that name storage by device rather than by
drive letter. `C:\Program Files\…` is correct, because C: *is* the base.
`\Device\HarddiskVolume2\…` names a volume that does not exist and must not
survive the projection.

## How it is written

Two decisions that keep this maintainable.

**Emit standard `.reg`, import through `wine regedit`.** Wine's `system.reg` is
an internal format that Wine is free to change; the `.reg` format that `regedit`
consumes is a documented interface with a stable definition, and importing
through Wine's own tool means Wine writes its own files in whatever shape it
currently wants. Writing `system.reg` directly would be reaching into another
project's private state.

**Read with a pure-Rust hive parser.** Read-only is sufficient — Raven never
writes a hive, only reads one and emits `.reg` — and read-only pure-Rust hive
parsers exist. That keeps the whole path free of C and free of FFI.

## The rules are the artifact

The allow list is a data file, reviewed like source, not a constant buried in
Rust. What crosses from a real Windows into a prefix is a security-relevant and
correctness-relevant decision, and it should be readable by someone who does not
read Rust.

This mirrors the shape Project-Colony-Resources already uses for design tokens:
a hand-edited source of truth, a generator, and output that is never hand-edited.
The same discipline applies for the same reason — a projection someone corrected
by hand is a projection nobody can reproduce.

Projection must therefore be **idempotent**: same base, same rules, same output,
every time. That is a property a test can assert, and it is the property that
makes a cached projection safe to reuse.

## Testing

- **Round-trip.** A known hive corpus projects to a known `.reg`, byte for byte.
- **The deny list holds.** No output line falls under a denied subtree — asserted
  against the output, not the intent, so a rules edit that widens the allow list
  too far fails a test rather than shipping.
- **Idempotency.** Projecting twice produces identical output.
- **Device paths do not survive.** No `\Device\` reference reaches the prefix.

The corpus is the awkward part: hives are Microsoft's, and the repository cannot
carry a real one. Either the corpus is generated synthetically, or it is produced
locally from a deployed base and kept out of the repository — a choice to make
before the first test is written, not after.
