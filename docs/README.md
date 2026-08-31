# Raven documentation

Sorted by who is reading, not by subject.

Raven is at the design stage, so there is no `guide/` yet. Documenting an
install procedure for software that does not run would be the exact failure
[the org convention](https://github.com/Project-Colony/Project-Colony-Resources/blob/main/design/documentation.md)
warns about: a page that is trusted and wrong. `guide/` arrives with the first
milestone that a person can actually run.

## Reading the code

| | |
|---|---|
| [internals/architecture.md](internals/architecture.md) | the whole design, and why each part is shaped that way |
| [internals/mount-stack.md](internals/mount-stack.md) | how a real Windows becomes an immutable base with writable overlays |
| [internals/registry-projection.md](internals/registry-projection.md) | reading binary hives, and why importing them wholesale destroys the prefix |
| [internals/shadow-set.md](internals/shadow-set.md) | which libraries must be Wine's, which may be Microsoft's, and how we find out |
| [internals/contributing.md](internals/contributing.md) | building, testing, and the conventions inherited from the org |

## Why it exists

| | |
|---|---|
| [project/landscape.md](project/landscape.md) | what Wine, Proton and virtual machines do, and the gap between them |
| [project/status.md](project/status.md) | the done/remaining ledger, and the open questions that could still move the design |
| [project/licensing.md](project/licensing.md) | Raven's licence, and the line it does not cross around Microsoft's |
