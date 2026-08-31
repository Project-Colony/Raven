# Licensing

Two separate questions that are easy to run together: what licence Raven is
under, and what Raven is allowed to do with Microsoft's software.

Nothing here is legal advice.

## Raven

GPL-3.0-or-later, matching the rest of the Project Colony organisation. See
[LICENSE](../../LICENSE).

## Raven never distributes Microsoft software

This is the line, and it is not a close call.

Raven ships no Microsoft binary, no Windows image, no library extracted from one,
and no ISO. It contains no mechanism for obtaining Windows on the user's behalf.
It is a tool that operates on a Windows installation **the user supplies and
licenses themselves**.

That is the same posture as the tools already in this space: `winetricks`
downloads Microsoft redistributables from Microsoft's own servers rather than
mirroring them, and Proton ships no Windows components it does not have the
right to ship. The model is well established and Raven does not depart from it.

## What that means in practice

- Microsoft distributes Windows ISOs at no charge, and Windows runs unactivated
  with functional limitations. That makes **development and testing**
  straightforward and legitimate.
- Whether a given user's deployment is properly licensed is between that user
  and Microsoft. Raven changes nothing about it, and claiming otherwise in either
  direction would be both wrong and unhelpful.
- Raven's documentation does not explain how to avoid licensing Windows, and
  will not.

## The registry projection

Worth stating explicitly because it is the least obvious case.

Reading hives from a user's own installation, on that user's own machine, to
configure that user's own Wine prefix, moves nothing off the machine. The
projection output is derived data that stays local and is regenerated rather
than distributed.

What Raven must not do is carry a hive corpus in the repository for testing
purposes — those are Microsoft's files, and a test fixture is distribution. This
is why the test corpus question in [status.md](status.md) is open rather than
answered with "commit a real hive."
