# Raven documentation

Sorted by who is reading, not by subject.

## Using Raven

| | |
|---|---|
| [guide/install.md](guide/install.md) | building it, and checking your machine can run it |
| [guide/usage.md](guide/usage.md) | from an installation image to `./program.exe` |
| [guide/troubleshooting.md](guide/troubleshooting.md) | failure modes that have actually happened, and what they mean |

## Reading the code

| | |
|---|---|
| [internals/architecture.md](internals/architecture.md) | the whole design, and why each part is shaped that way |
| [internals/mount-stack.md](internals/mount-stack.md) | how a real Windows becomes an immutable base with writable overlays |
| [internals/registry-projection.md](internals/registry-projection.md) | reading binary hives, and why importing them wholesale destroys the prefix |
| [internals/shadow-set.md](internals/shadow-set.md) | which libraries must be Wine's, which may be Microsoft's, and how we find out |
| [internals/packaging.md](internals/packaging.md) | how it is installed, and the two steps that need root |
| [internals/performance.md](internals/performance.md) | what a real Windows costs at launch, and the theories already ruled out |
| [internals/system-dependencies.md](internals/system-dependencies.md) | what to install, and a log of what development has installed |
| [internals/contributing.md](internals/contributing.md) | building, testing, and the conventions inherited from the org |

## Why it exists

| | |
|---|---|
| [project/landscape.md](project/landscape.md) | what Wine, Proton and virtual machines do, and the gap between them |
| [project/consolidation.md](project/consolidation.md) | what stands between this and something other people can install |
| [project/status.md](project/status.md) | the done/remaining ledger, and the open questions that could still move the design |
| [project/licensing.md](project/licensing.md) | Raven's licence, and the line it does not cross around Microsoft's |
