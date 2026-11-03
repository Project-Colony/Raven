//! The icon a Windows program shows, as a PNG a Linux desktop can use.
//!
//! Explorer shows a program's first icon group: the first `RT_GROUP_ICON` in
//! its resource directory, which lists named entries before numbered ones,
//! each in order. A group lists one icon at several sizes, each image an
//! `RT_ICON` resource of its own. Since Windows Vista the large images are
//! usually stored as PNG already; the others are device-independent bitmaps
//! followed by an AND mask, which this turns into RGBA and writes out as PNG.
//!
//! Written here rather than taken from a crate: reading the resources is a
//! walk through a few tables, and a PNG is valid with its data stored
//! uncompressed, which costs a larger file and no encoder.

/// `RT_ICON`: one image of an icon.
const RT_ICON: u32 = 3;
/// `RT_GROUP_ICON`: the list of an icon's images.
const RT_GROUP_ICON: u32 = 14;
const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
/// Larger than any icon Windows draws. An image claiming more is not one.
const MAX_SIDE: usize = 1024;
/// A PNG image is copied as it is, and the desktop decodes it each time it
/// draws the menu: four times what a 256-pixel RGBA image takes unpacked.
const MAX_PNG: usize = 4 << 20;

/// The first icon in the PE file `pe`, as a PNG: its largest image that can be
/// read, or `None` when it has none.
pub fn from_exe(pe: &[u8]) -> Option<Vec<u8>> {
    let resources = Resources::of(pe)?;
    let group = resources.first(RT_GROUP_ICON)?;
    // A 16-bit bitmap or a damaged image is no reason to show no icon when a
    // smaller one of the same group reads. Each try walks the directory
    // again, and a group can list 65535 images: a real one lists a dozen.
    by_size(group)
        .into_iter()
        .take(16)
        .find_map(|id| to_png(resources.find(RT_ICON, id)?))
}

/// The resource IDs of the images a group lists, largest then deepest first.
///
/// A group is a `GRPICONDIR`: six bytes, the count at offset 4, then one
/// 14-byte entry per image. A width of 0 means 256.
fn by_size(group: &[u8]) -> Vec<u32> {
    let count = u16_at(group, 4).map_or(0, usize::from);
    let mut images: Vec<((u32, u16), u32)> = (0..count)
        .filter_map(|i| {
            let entry = 6 + i * 14;
            let width = match *group.get(entry)? {
                0 => 256,
                w => u32::from(w),
            };
            let bits = u16_at(group, entry + 6)?;
            let id = u16_at(group, entry + 12)?;
            Some(((width, bits), u32::from(id)))
        })
        .collect();
    images.sort_by_key(|&(size, _)| std::cmp::Reverse(size));
    images.into_iter().map(|(_, id)| id).collect()
}

/// One image as PNG: as it is when it is one already, decoded from a bitmap
/// otherwise.
fn to_png(image: &[u8]) -> Option<Vec<u8>> {
    if image.starts_with(PNG_SIGNATURE) {
        // The IHDR chunk comes first: its length and type, then the size.
        let side = |at| {
            u32_be(image, at)
                .and_then(|v| usize::try_from(v).ok())
                .is_some_and(|v| (1..=MAX_SIDE).contains(&v))
        };
        let sane = image.len() <= MAX_PNG && image.get(12..16)? == b"IHDR" && side(16) && side(20);
        return sane.then(|| image.to_vec());
    }
    let (width, height, rgba) = bitmap(image)?;
    Some(png(width, height, &rgba))
}

/// An icon image stored as a bitmap: its size and its pixels as RGBA, top row
/// first.
///
/// A `BITMAPINFOHEADER` whose height counts two images, the colours and the
/// AND mask; a palette for 8 bits a pixel or fewer; the colour rows, bottom
/// first, each padded to four bytes; then the mask rows, one bit a pixel, a
/// set bit marking a transparent one. A 32-bit image carries its own alpha,
/// and the mask counts only when every alpha is zero, as in icons made before
/// Windows XP read alpha.
fn bitmap(b: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let header = usize::try_from(u32_at(b, 0)?).ok()?;
    let width = i32::from_le_bytes(b.get(4..8)?.try_into().ok()?);
    let doubled = i32::from_le_bytes(b.get(8..12)?.try_into().ok()?);
    let bits = usize::from(u16_at(b, 14)?);
    if header < 40 || u32_at(b, 16)? != 0 {
        return None;
    }
    let w = usize::try_from(width)
        .ok()
        .filter(|w| (1..=MAX_SIDE).contains(w))?;
    let h = usize::try_from(doubled / 2)
        .ok()
        .filter(|h| (1..=MAX_SIDE).contains(h))?;
    let colours = match bits {
        1 | 4 | 8 => match usize::try_from(u32_at(b, 32)?).ok()? {
            n if n == 0 || n > 1 << bits => 1 << bits,
            n => n,
        },
        24 | 32 => 0,
        _ => return None,
    };
    let stride = (w * bits).div_ceil(32) * 4;
    let mask_stride = w.div_ceil(32) * 4;
    // Both cut out before the loop: a bitmap shorter than it says it is fails
    // here, before its pixels are allocated.
    let palette = b.get(header..header + colours * 4)?;
    let pixels = b.get(header + colours * 4..)?;
    let (colour, mask) = (pixels.get(..stride * h)?, &pixels[stride * h..]);

    let mut rgba = vec![0u8; w * h * 4];
    let mut has_alpha = false;
    for y in 0..h {
        let row = &colour[(h - 1 - y) * stride..][..stride];
        for x in 0..w {
            let pixel = match bits {
                32 => [row[x * 4 + 2], row[x * 4 + 1], row[x * 4], row[x * 4 + 3]],
                24 => [row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 255],
                _ => {
                    let per_byte = 8 / bits;
                    let shift = 8 - bits * (x % per_byte + 1);
                    let index = usize::from(row[x / per_byte] >> shift) & ((1 << bits) - 1);
                    let c = &palette[index.min(colours - 1) * 4..][..4];
                    [c[2], c[1], c[0], 255]
                }
            };
            has_alpha |= bits == 32 && pixel[3] != 0;
            rgba[(y * w + x) * 4..][..4].copy_from_slice(&pixel);
        }
    }
    if !has_alpha {
        for y in 0..h {
            let start = (h - 1 - y) * mask_stride;
            // A mask cut short leaves the rest opaque rather than the icon
            // unreadable.
            let row = mask.get(start..start + mask_stride);
            for x in 0..w {
                let clear = row.is_some_and(|r| r[x / 8] >> (7 - x % 8) & 1 == 1);
                rgba[(y * w + x) * 4 + 3] = if clear { 0 } else { 255 };
            }
        }
    }
    Some((w as u32, h as u32, rgba))
}

/// `rgba`, `width` by `height`, as a PNG: 8-bit RGBA, no filtering, and the
/// zlib stream in stored blocks.
pub(crate) fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    debug_assert!(width > 0 && height > 0 && rgba.len() == (width * height * 4) as usize);
    let row = width as usize * 4;
    let mut raw = Vec::with_capacity((row + 1) * height as usize);
    for line in rgba.chunks(row) {
        raw.push(0);
        raw.extend_from_slice(line);
    }
    // 0x78 0x01: deflate with a 32 KiB window, no dictionary, and the check
    // bits that make the pair a multiple of 31.
    let mut zlib = vec![0x78, 0x01];
    let mut blocks = raw.chunks(0xffff).peekable();
    while let Some(block) = blocks.next() {
        zlib.push(u8::from(blocks.peek().is_none()));
        let len = block.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    // Bit depth 8, colour type 6 (RGBA), standard compression and filter, no
    // interlace.
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);

    let mut out = PNG_SIGNATURE.to_vec();
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 == 1 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// A PE file's resource directory.
struct Resources<'a> {
    pe: &'a [u8],
    sections: Vec<Section>,
    /// The file offset of the root directory.
    root: usize,
}

struct Section {
    address: u32,
    size: u32,
    raw: u32,
    raw_size: u32,
}

impl<'a> Resources<'a> {
    fn of(pe: &'a [u8]) -> Option<Self> {
        if pe.get(..2)? != b"MZ" {
            return None;
        }
        let nt = usize::try_from(u32_at(pe, 0x3c)?).ok()?;
        if pe.get(nt..nt.checked_add(4)?)? != b"PE\0\0" {
            return None;
        }
        let coff = nt + 4;
        let count = usize::from(u16_at(pe, coff + 2)?);
        let optional = coff + 20;
        // The data directories follow the fixed part of the optional header,
        // which is longer in a 64-bit image. The resource table is the third.
        let directories = match u16_at(pe, optional)? {
            0x10b => optional + 96,
            0x20b => optional + 112,
            _ => return None,
        };
        if u32_at(pe, directories - 4)? < 3 {
            return None;
        }
        let rva = u32_at(pe, directories + 16)?;
        let table = optional + usize::from(u16_at(pe, coff + 16)?);
        let sections = (0..count)
            .map(|i| {
                let s = table + i * 40;
                Some(Section {
                    size: u32_at(pe, s + 8)?,
                    address: u32_at(pe, s + 12)?,
                    raw_size: u32_at(pe, s + 16)?,
                    raw: u32_at(pe, s + 20)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let mut resources = Resources {
            pe,
            sections,
            root: 0,
        };
        resources.root = resources.offset(rva)?;
        Some(resources)
    }

    /// Where the address `rva` is in the file.
    fn offset(&self, rva: u32) -> Option<usize> {
        let s = self
            .sections
            .iter()
            .find(|s| rva >= s.address && rva - s.address < s.size.max(s.raw_size))?;
        let into = rva - s.address;
        // In usize: a hostile PointerToRawData near u32::MAX overflows a u32.
        (into < s.raw_size).then(|| s.raw as usize + into as usize)
    }

    /// The entries of the directory `dir` bytes into the resources: each one's
    /// name or ID, and where it leads, a set high bit marking a directory.
    fn entries(&self, dir: usize) -> Option<Vec<(u32, u32)>> {
        let at = self.root.checked_add(dir)?;
        let named = usize::from(u16_at(self.pe, at + 12)?);
        let numbered = usize::from(u16_at(self.pe, at + 14)?);
        (0..named + numbered)
            .map(|i| {
                let e = at + 16 + i * 8;
                Some((u32_at(self.pe, e)?, u32_at(self.pe, e + 4)?))
            })
            .collect()
    }

    /// The data an entry leads to, through the first entry of each directory
    /// below it: the first language, where there are several.
    fn data(&self, mut to: u32) -> Option<&'a [u8]> {
        for _ in 0..3 {
            let at = (to & 0x7fff_ffff) as usize;
            if to & 0x8000_0000 == 0 {
                let leaf = self.root.checked_add(at)?;
                let start = self.offset(u32_at(self.pe, leaf)?)?;
                let size = usize::try_from(u32_at(self.pe, leaf + 4)?).ok()?;
                return self.pe.get(start..start.checked_add(size)?);
            }
            to = self.entries(at)?.first()?.1;
        }
        None
    }

    /// The directory of the resources of type `kind`.
    fn of_type(&self, kind: u32) -> Option<usize> {
        self.entries(0)?
            .into_iter()
            .find(|&(id, to)| id == kind && to & 0x8000_0000 != 0)
            .map(|(_, to)| (to & 0x7fff_ffff) as usize)
    }

    fn first(&self, kind: u32) -> Option<&'a [u8]> {
        self.data(self.entries(self.of_type(kind)?)?.first()?.1)
    }

    fn find(&self, kind: u32, id: u32) -> Option<&'a [u8]> {
        let (_, to) = self
            .entries(self.of_type(kind)?)?
            .into_iter()
            .find(|&(n, _)| n == id)?;
        self.data(to)
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn u32_be(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 64-bit PE file whose only section holds a resource directory with
    /// one icon group, listing `images` as `RT_ICON` 1, 2, … with the given
    /// group widths.
    fn exe_with_icon(images: &[(u8, &[u8])]) -> Vec<u8> {
        exe(images, false)
    }

    /// The same, as a 32-bit PE file, whose optional header is shorter.
    fn exe(images: &[(u8, &[u8])], pe32: bool) -> Vec<u8> {
        const SECTION_RVA: u32 = 0x1000;
        const SECTION_RAW: usize = 0x200;
        let mut rsrc = Vec::new();
        // Root: two types, each a directory of one or more names, each a
        // directory of one language, each a leaf.
        let dir = |out: &mut Vec<u8>, entries: &[(u32, u32)]| {
            out.extend_from_slice(&[0; 12]);
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
            for (id, to) in entries {
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&to.to_le_bytes());
            }
        };
        let n = images.len() as u32;
        let dir_size = |entries: u32| 16 + 8 * entries;
        // Layout: root, icon type dir, group type dir, one language dir per
        // resource, one leaf per resource, then the data.
        let root = 0;
        let icon_dir = root + dir_size(2);
        let group_dir = icon_dir + dir_size(n);
        let lang_dirs = group_dir + dir_size(1);
        let leaves = lang_dirs + dir_size(1) * (n + 1);
        let data = leaves + 16 * (n + 1);

        let mut group = Vec::new();
        group.extend_from_slice(&[0, 0, 1, 0]);
        group.extend_from_slice(&(n as u16).to_le_bytes());
        for (i, (width, image)) in images.iter().enumerate() {
            // The depth a group states is a hint; this one is the bitmap's.
            let bits = image
                .get(14)
                .copied()
                .filter(|_| !image.starts_with(PNG_SIGNATURE));
            group.extend_from_slice(&[*width, *width, 0, 0, 1, 0, bits.unwrap_or(32), 0]);
            group.extend_from_slice(&(image.len() as u32).to_le_bytes());
            group.extend_from_slice(&(i as u16 + 1).to_le_bytes());
        }
        let mut blobs: Vec<&[u8]> = images.iter().map(|(_, b)| *b).collect();
        blobs.push(&group);

        dir(
            &mut rsrc,
            &[
                (RT_ICON, 0x8000_0000 | icon_dir),
                (RT_GROUP_ICON, 0x8000_0000 | group_dir),
            ],
        );
        let icon_entries: Vec<(u32, u32)> = (0..n)
            .map(|i| (i + 1, 0x8000_0000 | (lang_dirs + dir_size(1) * i)))
            .collect();
        dir(&mut rsrc, &icon_entries);
        dir(
            &mut rsrc,
            &[(1, 0x8000_0000 | (lang_dirs + dir_size(1) * n))],
        );
        for i in 0..=n {
            dir(&mut rsrc, &[(0x409, leaves + 16 * i)]);
        }
        let mut at = data;
        for blob in &blobs {
            rsrc.extend_from_slice(&(SECTION_RVA + at).to_le_bytes());
            rsrc.extend_from_slice(&(blob.len() as u32).to_le_bytes());
            rsrc.extend_from_slice(&[0; 8]);
            at += blob.len() as u32;
        }
        for blob in &blobs {
            rsrc.extend_from_slice(blob);
        }

        let mut pe = vec![0u8; SECTION_RAW];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        pe[0x40..0x44].copy_from_slice(b"PE\0\0");
        let coff = 0x44;
        // The fixed part of the optional header, then 16 data directories.
        let (magic, fixed) = if pe32 { (0x10bu16, 96) } else { (0x20b, 112) };
        let optional_size = fixed + 16 * 8;
        pe[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        pe[coff + 16..coff + 18].copy_from_slice(&(optional_size as u16).to_le_bytes());
        let optional = coff + 20;
        pe[optional..optional + 2].copy_from_slice(&magic.to_le_bytes());
        pe[optional + fixed - 4..optional + fixed].copy_from_slice(&16u32.to_le_bytes());
        let resource_dir = optional + fixed + 16;
        pe[resource_dir..resource_dir + 4].copy_from_slice(&SECTION_RVA.to_le_bytes());
        pe[resource_dir + 4..resource_dir + 8].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        let section = optional + optional_size;
        pe[section..section + 5].copy_from_slice(b".rsrc");
        pe[section + 8..section + 12].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        pe[section + 12..section + 16].copy_from_slice(&SECTION_RVA.to_le_bytes());
        pe[section + 16..section + 20].copy_from_slice(&(rsrc.len() as u32).to_le_bytes());
        pe[section + 20..section + 24].copy_from_slice(&(SECTION_RAW as u32).to_le_bytes());
        pe.extend_from_slice(&rsrc);
        pe
    }

    /// A bitmap icon image: `w` by `h`, `bits` deep, the given colour rows
    /// (bottom first, already padded) and mask rows.
    fn bitmap_image(
        w: i32,
        h: i32,
        bits: u16,
        palette: &[[u8; 4]],
        rows: &[u8],
        mask: &[u8],
    ) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&w.to_le_bytes());
        b.extend_from_slice(&(h * 2).to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&[0; 12]);
        b.extend_from_slice(&(palette.len() as u32).to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        for c in palette {
            b.extend_from_slice(c);
        }
        b.extend_from_slice(rows);
        b.extend_from_slice(mask);
        b
    }

    /// The pixels of a PNG this module wrote, read back through its stored
    /// blocks.
    fn pixels(png: &[u8]) -> (u32, u32, Vec<u8>) {
        assert!(png.starts_with(PNG_SIGNATURE));
        let (w, h) = (u32_be(png, 16).unwrap(), u32_be(png, 20).unwrap());
        let mut at = 8;
        let mut zlib = Vec::new();
        while at < png.len() {
            let len = u32_be(png, at).unwrap() as usize;
            let kind = &png[at + 4..at + 8];
            let body = &png[at + 8..at + 8 + len];
            let crc = u32_be(png, at + 8 + len).unwrap();
            assert_eq!(crc, crc32(&png[at + 4..at + 8 + len]), "chunk CRC");
            if kind == b"IDAT" {
                zlib.extend_from_slice(body);
            }
            at += 12 + len;
        }
        assert_eq!(((u16::from(zlib[0]) << 8) | u16::from(zlib[1])) % 31, 0);
        let mut raw = Vec::new();
        let mut i = 2;
        loop {
            let last = zlib[i] & 1 == 1;
            let len = usize::from(u16::from_le_bytes([zlib[i + 1], zlib[i + 2]]));
            raw.extend_from_slice(&zlib[i + 5..i + 5 + len]);
            i += 5 + len;
            if last {
                break;
            }
        }
        assert_eq!(u32_be(&zlib, i).unwrap(), adler32(&raw));
        let rgba = raw
            .chunks(w as usize * 4 + 1)
            .flat_map(|row| {
                assert_eq!(row[0], 0, "no filter");
                row[1..].to_vec()
            })
            .collect();
        (w, h, rgba)
    }

    #[test]
    fn the_checksums_match_their_references() {
        // IEND's CRC is in every PNG ever written; Adler-32 of "Wikipedia" is
        // the worked example of its definition.
        assert_eq!(crc32(b"IEND"), 0xae42_6082);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
    }

    #[test]
    fn a_png_image_is_taken_as_it_is() {
        let inner = png(256, 256, &vec![7; 256 * 256 * 4]);
        let small = bitmap_image(1, 1, 32, &[], &[1, 2, 3, 255], &[0, 0, 0, 0]);
        let exe = exe_with_icon(&[(16, &small), (0, &inner), (32, &small)]);
        let icon = from_exe(&exe).expect("an icon");
        assert_eq!(icon, inner, "0 means 256, the largest");
    }

    #[test]
    fn a_png_image_that_is_not_an_icon_is_refused() {
        let small = bitmap_image(1, 1, 32, &[], &[1, 2, 3, 255], &[0, 0, 0, 0]);
        let mut huge = png(1, 1, &[0; 4]);
        huge[16..20].copy_from_slice(&65535u32.to_be_bytes());
        let mut untyped = png(1, 1, &[0; 4]);
        untyped[12..16].copy_from_slice(b"tEXt");
        for bad in [huge, untyped] {
            // Refused, so the next largest image is used instead.
            let exe = exe_with_icon(&[(0, &bad), (1, &small)]);
            assert_eq!(pixels(&from_exe(&exe).unwrap()).0, 1);
        }
    }

    #[test]
    fn an_unreadable_largest_image_falls_back_to_a_smaller_one() {
        // 16 bits a pixel is not read; the 1-pixel 32-bit image is.
        let deep = bitmap_image(48, 48, 16, &[], &[0; 48 * 96], &[0; 48 * 4]);
        let small = bitmap_image(1, 1, 32, &[], &[1, 2, 3, 255], &[0, 0, 0, 0]);
        let icon = from_exe(&exe_with_icon(&[(48, &deep), (1, &small)])).unwrap();
        assert_eq!(pixels(&icon).2, [3, 2, 1, 255]);
    }

    #[test]
    fn a_32_bit_file_is_read_too() {
        let small = bitmap_image(1, 1, 32, &[], &[1, 2, 3, 255], &[0, 0, 0, 0]);
        let icon = from_exe(&exe(&[(1, &small)], true)).unwrap();
        assert_eq!(pixels(&icon).2, [3, 2, 1, 255]);
    }

    #[test]
    fn a_section_placed_past_four_gigabytes_is_not_followed() {
        // PointerToRawData near u32::MAX once overflowed into a panic.
        let small = bitmap_image(1, 1, 32, &[], &[1, 2, 3, 255], &[0, 0, 0, 0]);
        let mut exe = exe_with_icon(&[(1, &small)]);
        let section = 0x44 + 20 + 112 + 128;
        exe[section + 20..section + 24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(from_exe(&exe).is_none());
    }

    #[test]
    fn a_32_bit_bitmap_keeps_its_alpha_and_is_turned_upright() {
        // Two rows, bottom first: blue half-transparent, then red opaque.
        let rows = [255, 0, 0, 128, 0, 0, 255, 255];
        let image = bitmap_image(1, 2, 32, &[], &rows, &[0; 8]);
        let icon = from_exe(&exe_with_icon(&[(1, &image)])).unwrap();
        let (w, h, rgba) = pixels(&icon);
        assert_eq!((w, h), (1, 2));
        assert_eq!(rgba, [255, 0, 0, 255, 0, 0, 255, 128]);
    }

    #[test]
    fn without_alpha_the_mask_makes_pixels_transparent() {
        // 24-bit, 2 by 1: green then white, the second masked out.
        let rows = [0, 255, 0, 255, 255, 255, 0, 0];
        let mask = [0b0100_0000, 0, 0, 0];
        let image = bitmap_image(2, 1, 24, &[], &rows, &mask);
        let icon = from_exe(&exe_with_icon(&[(2, &image)])).unwrap();
        assert_eq!(pixels(&icon).2, [0, 255, 0, 255, 255, 255, 255, 0]);
    }

    #[test]
    fn a_32_bit_bitmap_with_no_alpha_at_all_uses_its_mask() {
        // Icons made before Windows XP leave every alpha at zero.
        let rows = [10, 20, 30, 0, 40, 50, 60, 0];
        let mask = [0b0100_0000, 0, 0, 0];
        let image = bitmap_image(2, 1, 32, &[], &rows, &mask);
        let icon = from_exe(&exe_with_icon(&[(2, &image)])).unwrap();
        assert_eq!(pixels(&icon).2, [30, 20, 10, 255, 60, 50, 40, 0]);
    }

    #[test]
    fn a_mask_cut_short_leaves_the_rest_opaque() {
        let rows = [10, 20, 30, 40, 50, 60, 0, 0];
        let image = bitmap_image(2, 1, 24, &[], &rows, &[]);
        let icon = from_exe(&exe_with_icon(&[(2, &image)])).unwrap();
        assert_eq!(pixels(&icon).2, [30, 20, 10, 255, 60, 50, 40, 255]);
    }

    #[test]
    fn a_paletted_bitmap_is_read_through_its_palette() {
        // 4 bits a pixel, 3 by 1: indices 1, 0, 1.
        let palette = [[0, 0, 0, 0], [255, 128, 0, 0]];
        let rows = [0x10, 0x10, 0, 0];
        let image = bitmap_image(3, 1, 4, &palette, &rows, &[0; 4]);
        let icon = from_exe(&exe_with_icon(&[(3, &image)])).unwrap();
        assert_eq!(
            pixels(&icon).2,
            [0, 128, 255, 255, 0, 0, 0, 255, 0, 128, 255, 255]
        );
    }

    #[test]
    fn one_and_eight_bit_bitmaps_are_read_through_their_palette() {
        // 1 bit, 3 by 1: 1, 0, 1.
        let two = [[0, 0, 0, 0], [255, 255, 255, 0]];
        let image = bitmap_image(3, 1, 1, &two, &[0b1010_0000, 0, 0, 0], &[0; 4]);
        let icon = from_exe(&exe_with_icon(&[(3, &image)])).unwrap();
        assert_eq!(
            pixels(&icon).2,
            [255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 255, 255]
        );
        // 8 bits, 2 by 1: index 2, then index 9, past the three colours the
        // palette has, which is read as its last.
        let three = [[1, 1, 1, 0], [2, 2, 2, 0], [3, 3, 3, 0]];
        let image = bitmap_image(2, 1, 8, &three, &[2, 9, 0, 0], &[0; 4]);
        let icon = from_exe(&exe_with_icon(&[(2, &image)])).unwrap();
        assert_eq!(pixels(&icon).2, [3, 3, 3, 255, 3, 3, 3, 255]);
    }

    #[test]
    fn a_file_with_no_icon_has_none() {
        assert!(from_exe(b"").is_none());
        assert!(from_exe(b"MZ not a program").is_none());
        let mut exe = exe_with_icon(&[(1, b"neither a PNG nor a bitmap")]);
        assert!(from_exe(&exe).is_none());
        exe.truncate(0x300);
        assert!(
            from_exe(&exe).is_none(),
            "a truncated file is not read past its end"
        );
    }

    #[test]
    fn a_large_image_spans_several_stored_blocks() {
        let rgba: Vec<u8> = (0..300 * 300 * 4).map(|i| (i % 251) as u8).collect();
        let (w, h, back) = pixels(&png(300, 300, &rgba));
        assert_eq!((w, h), (300, 300));
        assert_eq!(back, rgba);
    }
}
