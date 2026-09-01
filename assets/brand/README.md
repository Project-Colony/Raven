# Raven brand assets

The mark is a **raven's head cut clean through three strata**.

The three bands are the mount stack: an immutable base with overlays above it.
The bird is not drawn on top of them - it is the void they leave behind, so the
head only exists because the layers do. That is the program's own architecture,
drawn: Raven is a real Windows seen through the layers Raven puts around it.

The head is faceted rather than drawn - straight cuts only, the same vocabulary
as the bands it is cut from, so the bird and the strata read as one object made
of one operation. The long wedge bill breaking toward the right edge is what
fixes it as a bird at every size.

## Which file to use

| | |
|---|---|
| `raven-logo.svg` | the lockup - mark plus wordmark, pale ink. **Dark backgrounds only.** |
| `raven-logo-light.svg` | the same lockup in dark ink (`#1a1727`), for light backgrounds |
| `raven-icon.svg` | square icon, the three bands separate - **64 px and above** |
| `raven-icon-small.svg` | square icon, the bands fused into one slab - **48 px and below** |

The two icons are not redundant. The band seams are 8 units on a 256 grid; below
48 px they fall under a pixel and rasterise into grey mush that reads as a
printing fault. The simplified one fuses the three bands into a single slab, so
the silhouette - which is what actually identifies the mark - survives the size
instead of dissolving at it. The `png/` exports already apply this rule:
`raven-icon-64.png` and up come from the full icon, `48` and below from the
simplified one.

`png/raven-app-1024-on-dark.png` is the one to hand to a third party (a store
listing, a forum avatar) - square, dark ground baked in, so it renders as
designed rather than depending on where it lands.

## The wordmark

`RAVEN` is drawn as paths, never set as type. A mark that depends on a font
installed on the reader's machine is not a mark. The letterforms are geometric
at a 64-unit cap height and a 14-unit stem, built from straight edges except for
the R's bowl - the same discipline as the icon, so the two halves of the lockup
agree.

## The application icon

`png/raven-icon-<size>-on-dark.png` is what gets installed into the icon theme.
Dark ground baked in on purpose: the mark's ink is pale, and a taskbar is not
somewhere you get to choose the background.

Three things have to agree or the icon silently does not appear:

| | |
|---|---|
| `Icon=` in `packaging/raven.desktop` | `raven` |
| the installed desktop file | `raven.desktop` |
| the installed icon | `hicolor/<size>/apps/raven.png` |

`packaging/PKGBUILD` installs all three. Before that it installed only the first
two, which is why the desktop entry has always asked for an icon that was not
there.

## Colours

| role | dark | light |
|---|---|---|
| ink | `#eae7f2` | `#1a1727` |
| ground | `#14121c` | `#f4f3f7` |

A violet bias rather than a neutral grey, from the sheen on a raven's feathers -
the one colour a black bird actually has. There is no Raven variant in the
Colony Stellar Blade theme family yet; if one is authored, its `text_primary`
and `bg_primary` should be these, so the mark and the program agree.

## Rules

- **Do not distort.** Rescale the `viewBox`; never change width and height
  independently.
- **Do not recolour** beyond the pair above. It is a one-ink mark - the pale
  areas are ground showing through, not a second colour.
- **Do not put the pale-ink icon on a light ground.** The bands vanish and the
  bird disappears with them. Use the light-ink file or the `-on-dark` export.
- Keep clear space of one band-height (54 units) on every side.
- Geometry, for anyone editing the paths: the frame is 208 units square inside a
  24-unit margin; three bands 64 units deep, separated by 8-unit seams; the eye
  is an 18-unit square at (96, 104); the bill runs from the brow at x=130 out to
  the tip at x=216. Every edge of the head is a straight line - there is not one
  curve in the mark, and adding one would break it.

The icon masters keep `currentColor` so a caller can ink them. The logo pair
ships with the ink baked in, because the README header block picks between them
by `prefers-color-scheme` and cannot pass a colour.

## Regenerating the exports

```sh
cd assets/brand
for s in 512 256 128 64; do rsvg-convert -w $s -h $s raven-icon.svg -o png/raven-icon-$s.png; done
for s in 48 32 16;        do rsvg-convert -w $s -h $s raven-icon-small.svg -o png/raven-icon-$s.png; done
rsvg-convert -w 1024 raven-logo.svg -o png/raven-logo-1024.png
```

Add `-b '#14121c'` for the `-on-dark` variants.

Original work, GPL-3.0-or-later with the rest of Raven. The program is named
after a character from Stellar Blade; this mark depicts a bird, and borrows
nothing from that game.
