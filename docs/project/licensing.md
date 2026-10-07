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

Raven ships no Microsoft software, but its users run Microsoft's, and the terms
that govern that are Microsoft's. The ones quoted here are the Windows 11
*Microsoft Software License Terms* ("Last updated April 2024", the edition
Microsoft publishes at
[microsoft.com/useterms](https://www.microsoft.com/en-us/useterms), which
covers both preinstalled and retail copies). A volume-licensed installation is
governed by its volume licence agreement instead, as the terms themselves say.

- **Section 2(a)** grants "the right to install and run one instance of the
  software on your device (the licensed device), for use by one person at a
  time". **Section 2(b)** defines a device as "a local hardware system (whether
  physical or virtual)", and **Section 2(d)(iv)** requires a separate licence
  for each virtual device the software is used on.
- **Section 5** is the one that matters most here: "You are authorized to use
  this software only if you are properly licensed and the software has been
  properly activated with a genuine product key or by other authorized method",
  and "You may not bypass or circumvent activation."
- **Section 2(c)(i)** forbids using or virtualising "features of the software
  separately". Running individual Microsoft programs and libraries under Wine,
  which is what Raven does, has not been assessed against this clause.

What follows from that, without overstating it:

- Microsoft distributes Windows ISOs at no charge, and Raven's development
  deploys one without ever booting or activating it. That is how the measurements
  in this repository were made. It does not make such a deployment licensed:
  under Section 5 an unactivated installation is not "properly activated", and
  nothing here should be read as saying otherwise.
- **A Windows the user has already licensed and activated**, for example a
  dual-boot partition on the same machine used as Raven's base, is the case
  that looks most compatible with Sections 2(a) and 5: one instance, on the
  licensed device, for one person. That is **unverified** on both counts.
  Technically, Raven has not yet run against an existing NTFS partition (see
  open question 6 in [status.md](status.md)), and whether activation, which
  "associate[s] it with a certain device", carries over to the same
  installation running under Wine is unknown. Legally, whether that use stays
  within the licence has not been reviewed by anyone qualified to say.
- Whether a given user's deployment is properly licensed is between that user
  and Microsoft. Raven does not decide it, and claiming otherwise in either
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
