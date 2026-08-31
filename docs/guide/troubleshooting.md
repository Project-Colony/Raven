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

## Launching feels slower than Proton

It is, by about 95 ms per process. That is the price of case-insensitive path
resolution over a Windows roughly six times larger than Wine's synthetic prefix,
paid once per process.

Invisible for a game started once; twenty seconds for an installer spawning two
hundred processes. Two plausible explanations were measured and destroyed before
the real one was found — [../internals/performance.md](../internals/performance.md)
has the numbers, so nobody repeats them.

## An environment will not delete

It should. `overlayfs` leaves a `work/work` directory with no permissions at all,
which a plain recursive delete cannot enter — and it stops *after* removing the
upper layer, leaving something that can neither be destroyed nor recreated.

`raven env destroy` restores permissions as it descends and handles this. If you
deleted an environment by hand and hit it, `chmod -R u+w` the directory first.

## Anti-cheat refuses to start the game

If the publisher has enabled Easy Anti-Cheat or BattlEye's Linux support, the
game should work — and if it works under Proton but not Raven, that is a bug
worth reporting.

If the publisher has not enabled it — PUBG, Fortnite, Valorant — the game does
not run on Linux at all, under anything. That is a decision at the publisher, and
Raven will not work around it: these systems ban on detecting tampering, so a
tool that tried would get you banned. See
[../project/landscape.md](../project/landscape.md).
