# The mount stack

How a real Windows installation becomes an immutable base with disposable
writable environments on top of it.

## Two ways a base arrives

Raven distinguishes the base it deployed from the base it was handed, because
they carry very different risk.

### Deployed from an ISO — the primary path

An official Microsoft ISO contains `sources/install.wim`. `wimlib-imagex apply`
writes an image out of it to an ordinary directory, from Linux, without a
hypervisor and without booting anything:

```bash
wimlib-imagex apply install.wim <edition-index> <base>/
```

The base then lives on **whatever Linux filesystem you like** — btrfs for
snapshots, ext4 otherwise. No NTFS is involved anywhere, which removes an entire
category of risk before it exists.

What this produces is a Windows that has never run: the hives are in their
pre-`specialize` state, and no user profile has been created. Whether that is a
liability or an advantage is genuinely open — a machine-specific hive is mostly
noise to a projection that deliberately drops the machine-specific keys — and it
is the first thing to establish empirically.

### An existing Windows partition — the secondary path

Pointing Raven at a Windows already installed on an NTFS partition is the case
people ask for, and it is the one that carries the hazards:

- **The volume may be dirty.** Windows Fast Startup does not shut down; it
  hibernates the kernel session. A volume left in that state has a valid
  in-memory view that the on-disk metadata does not reflect, and mounting it
  writable from Linux corrupts it. Read-only mounting is the mitigation, and
  Raven never offers anything else.
- **`overlayfs` over `ntfs3` as a lower layer is unverified.** `overlayfs` places
  real requirements on its layers, and the combination needs to be established by
  experiment rather than assumed. Until it is, the ISO path is the supported one.

## Read-only is not a precaution, it is the design

The base is mounted read-only in every case, deployed or supplied. Four
properties fall out of that single decision:

| | |
|---|---|
| **The base cannot be damaged** | not by a misbehaving installer, not by a program writing where it should not, not by Raven |
| **Environments are disposable** | a broken install is `rm -rf` of one directory, not a re-deployment |
| **One base, many environments** | the expensive artifact is stored once, however many environments sit on it |
| **Snapshots are free on btrfs** | the base is a subvolume; so is each environment's upper layer |

The moment writes reach the base, all four are gone simultaneously. There is no
partial version of this decision.

## The overlay

```
  upper layer   <data>/Colony/Raven/environments/<name>/upper/   ← every write
  work dir      <data>/Colony/Raven/environments/<name>/work/    ← overlayfs internals
  lower layer   <data>/Colony/Raven/bases/<id>/                  ← read-only, shared
  ──────────────────────────────────────────────────────────────
  mounted at    $XDG_RUNTIME_DIR/raven/<name>/c                  ← the program's C:
```

The prefix's `dosdevices/c:` points at the mount. Wine sees one coherent C:
drive; the program sees Windows.

The mount point lives under the runtime directory rather than under data,
because a mount is runtime state. It should not survive a reboot, and finding a
stale mount point in a data directory after a crash is worse than finding
nothing.

`overlayfs` needs an upper layer on a filesystem that supports extended
attributes — ext4, xfs and btrfs all do — which is another reason the deployed
base is the path of least resistance.

## Case sensitivity

Windows software assumes a case-insensitive filesystem, and Linux filesystems are
case-sensitive by default. This is not a new problem and Wine already solves it:
its file layer resolves a Windows path against a case-sensitive directory by
matching case-insensitively. Every Wine prefix on ext4 in the world depends on
this working.

Raven inherits that solution and adds nothing. Where the lookup cost shows up in
profiling, ext4's `casefold` feature is the lever to reach for, applied to the
base at deployment time.

## Reparse points

A real Windows tree is held together by junctions: `C:\Documents and Settings`
points at `C:\Users`, and `Application Data` inside a profile points at
`AppData\Roaming`. Software still follows these paths.

A WIM stores reparse points, and applying one to a POSIX filesystem has to decide
what to turn them into. What survives that translation, and what Wine
synthesizes on its own regardless, is a question for the first spike rather than
an assumption to build on.

## Performing the mount

The overlay is mounted **without root**, inside a user namespace:

```
unshare -Urm  →  mount -t overlay  →  exec wine
```

This was measured rather than assumed. The mount succeeds unprivileged, it is
invisible from outside the namespace, `nsenter --preserve-credentials` can join
it later without root, and the base is provably untouched — a write to a file
that exists in the lower layer produces a modified copy in `upper/` while the
original in the base is unchanged.

Child processes inherit the namespace, so a launcher starting a game needs no
special handling, and the mount is destroyed with the process tree that owns it.
Nothing has to hold privilege while Raven runs.

Two consequences worth stating:

- **Concurrent independent launches into one environment are not supported at
  first.** `overlayfs` does not support two live mounts sharing an `upperdir`,
  and the simple path — mount, exec, exit — gives one mount per process tree.
  If launching two unrelated programs into the same environment turns out to
  matter, the fix is a keeper process holding the namespace with others joining
  by `nsenter`, which is proven to work. It is not built until it is needed.
- **`binfmt_misc` still needs root, once.** It is a file in `/etc/binfmt.d/`
  applied by `systemd-binfmt` at boot — a packaging concern, handled by the
  package manager, not a service that runs.

Where unprivileged namespaces are unavailable — `linux-hardened`, Ubuntu's
AppArmor policy, some enterprise configurations — the mount goes through a
different backend behind the same interface. See
[architecture.md](architecture.md).

## Environment lifecycle

Four verbs, and they are Raven's entire vocabulary:

| | |
|---|---|
| **create** | allocate `upper/` and `work/`, build the Wine prefix, project the registry |
| **activate** | mount the overlay at the runtime path |
| **deactivate** | unmount; the environment persists, nothing is lost |
| **destroy** | unmount, then delete the environment directory; the base is untouched |

Nothing in that list writes to a base. That is checkable, and it should be
checked by a test rather than by reading the code — the test that a write through
the overlay leaves the lower layer byte-identical is the one that guards the
whole design.

## Where files live

Following [the org filesystem
rule](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/filesystem.md),
everything sits under `Colony/Raven/`:

```
~/.config/Colony/Raven/          what the user chose
└── preferences/

~/.local/share/Colony/Raven/     what Raven produced and cannot re-derive
├── bases/<id>/                  a deployed Windows
└── environments/<name>/
    ├── upper/                   the writable layer
    ├── work/
    └── prefix/                  the Wine prefix; dosdevices/c: → the mount

~/.cache/Colony/Raven/           re-derivable; deleting it costs only time
└── projections/                 cached registry projections, keyed by rules hash

$XDG_RUNTIME_DIR/raven/<name>/c  the active mount; never survives a reboot
```

A base is data, not cache, despite being reproducible from an ISO: reproducing
it requires an ISO the user may no longer have, and the rule is that deleting
the cache must cost nothing but time.
