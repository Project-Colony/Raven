# Contributing

Raven follows the Project Colony conventions rather than inventing its own. This
page covers what is specific to Raven; where it points elsewhere, the other
document is authoritative.

## Conventions inherited from the org

| | |
|---|---|
| [repository-layout.md](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/repository-layout.md) | crate naming, workspace shape, one version for the whole workspace, a mandatory `description` per crate |
| [dependencies.md](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/dependencies.md) | every dependency on its latest release, full version pinned in the manifest, `Cargo.lock` committed |
| [filesystem.md](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/filesystem.md) | everything under `Colony/Raven/`, config and data and cache kept apart by sub-directory |
| [documentation.md](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/documentation.md) | README shape, `docs/` sorted by audience, lowercase kebab-case filenames |

**Everything is written in English** — source, identifiers, comments, commit
messages, and these pages. French is a UI locale a program ships, not a
documentation language.

Commits are conventional commits, because release-please reads them.

## What is specific to Raven

### Nothing writes to a base

The base is mounted read-only and that is the invariant the whole design rests
on. A change that gives any code path a writable handle to a base is wrong even
if it passes every test, and the tests should be the ones that catch it — see
[mount-stack.md](mount-stack.md).

### Mounting goes through the backend interface

Raven's mount is unprivileged today — a user namespace and native `overlayfs` —
and that is the only backend implemented. It is not the only one that will
exist: hardened kernels disable unprivileged namespaces, and those systems need
`fuse-overlayfs` or a privileged helper.

So no code calls `unshare` and `mount` directly at the point it happens to need a
filesystem. Acquiring a mount is one interface, and adding a backend must not
require touching anything that consumes it — see
[architecture.md](architecture.md).

If a privileged helper is ever written, it takes **named operations, never
caller-supplied paths**. "Activate the environment called `skyrim`" is
validatable; "mount this onto that" is a service that mounts anything anywhere as
root.

### The library is the API, the CLI is a shell over it

A GUI is a second caller of the same operations. Logic that ends up inside
argument handlers has to be rewritten to add one. The rule costs a few function
signatures now and saves the GUI later.

### Measurements come with their configuration

Results about which libraries can be Microsoft's are worthless without the exact
configuration that produced them: Windows build, Wine version, the full library
set, and the corpus outcome. A number without its configuration does not go in
the repository — see [shadow-set.md](shadow-set.md).

### Generated things are never hand-edited

The registry projection and the shadow set are both derived artifacts with
hand-edited rules behind them. Correcting the output by hand produces something
nobody can reproduce. Fix the rules and regenerate.

## Building and testing

There is no workspace yet, so there is nothing to build. When there is, this
section says how — and it will say it in a form someone can paste, not in prose.

Two things that will be true from the first crate:

- `cargo test` at the workspace root must work on a plain developer machine,
  without root and without a Windows base present. Tests that need either are
  gated behind a feature or a fixture, because a test command people learn to
  avoid is a test suite that stops running.
- The suite never needs root. The mount path is unprivileged by design, so a
  test that requires `sudo` is a sign the design drifted rather than a
  legitimate need. The one genuinely privileged step, `binfmt_misc`
  registration, belongs to packaging and is tested there.
