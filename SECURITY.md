# Security Policy

Raven sits between the kernel and Windows programs: it is the handler the
kernel runs for every `.exe`, it mounts filesystems inside user namespaces, it
parses binary registry hives and unpacks archives it did not write, and it can
hand a program raw access to a disk. A bug in any of those is a security bug,
and reports are welcome.

## Supported versions

Only the **latest release** receives security fixes. There is no backport
branch: the fix for a vulnerability is the next release, which Colony and the
release page deliver.

| Version | Supported |
| --- | --- |
| latest release | yes |
| anything older | no |

## Reporting a vulnerability

Please report vulnerabilities **privately** via
[GitHub Security Advisories](https://github.com/Project-Colony/Raven/security/advisories/new)
("Report a vulnerability"). Do not open a public issue for exploitable bugs.

Include what you have:

- what an attacker controls, and what they get;
- the version (`raven --version`), distribution and kernel version;
- the steps, or the file (archive, hive, `.exe`, environment name) that
  reproduces it.

The report stays private until a fixed release is out, and you are credited in
the release notes if you want to be.

## Scope

Reports of particular interest:

- **The binfmt handler.** Once registered, the kernel runs Raven for every
  `.exe` that anyone on the machine executes. Anything that lets a crafted
  `.exe` or path make Raven do more than launch it in the environment it
  belongs to.
- **The mount code.** Raven mounts overlayfs inside an unprivileged user and
  mount namespace. An escape from that namespace, a write that reaches the
  read-only Windows base, or an environment name or path that resolves outside
  `~/.local/share/Colony/Raven/` or `$XDG_RUNTIME_DIR/raven/`.
- **The registry hive parser.** Hives come from a Windows image the user
  supplies and are parsed as untrusted input. A crash, a hang, or a key that
  escapes the allow-listed subtrees into the projected registry.
- **DXVK and vkd3d archive unpacking.** Archives are untrusted. Path traversal,
  symlink or hard-link escapes, files that land outside the private unpack
  directory, or a race that swaps libraries between unpacking and the copy into
  the environment.
- **Device attach.** `raven env attach` links a block device into an
  environment. Anything that attaches a device the user did not name, or that
  changes a device node's permissions.
- **Release signing.** Every release asset is signed with the organisation's
  ed25519 key, as described in the README's
  [code signing policy](README.md#code-signing-policy). An asset that verifies
  but was not built from this repository, or a `.meta` that does not match its
  asset, is critical.

Out of scope: what Windows programs do once launched (Raven does not sandbox
them, and says so in the README's [privacy section](README.md#privacy)), bugs
in Wine, DXVK or vkd3d-proton themselves (report those upstream), and raw disk
access that the user granted through `raven env attach` and the device node's
permissions.
