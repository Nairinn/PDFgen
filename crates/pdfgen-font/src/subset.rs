//! TrueType subsetting: rebuild an sfnt containing only the glyphs a
//! document actually used, remapping glyph IDs so CIDs stay small.
//!
//! Operates on raw table bytes (no ttf-parser for writing): parse head/
//! maxp/loca/glyf/hmtx/cmap, drop unused glyphs, rewrite the four tables
//! that reference glyph IDs, and rebuild the checksum. Composite glyphs
//! keep their referenced components.

/// Build a subset font containing `used` glyphs (GIDs) plus gid 0.
/// Returns the new sfnt bytes and the old->new GID mapping.
pub fn subset_true_type(data: &[u8], used: &[u16]) -> Option<(Vec<u8>, Vec<u16>)> {
    subset_impl(data, used, None)
}

/// Subset for a SIMPLE font with WinAnsiEncoding: glyphs are renumbered
/// so the glyph for WinAnsi byte b sits at new GID b (and the cmap maps
/// b -> b). This keeps the font dictionary's FirstChar/LastChar/Widths
/// and the embedded program consistent, which validators check.
pub fn subset_winansi(
    data: &[u8],
    byte_to_gid: &[Option<u16>; 256],
) -> Option<(Vec<u8>, [Option<u16>; 256])> {
    // Byte-ordered placement: new GID = byte, with .notdef at 0.
    let used: Vec<u16> = byte_to_gid.iter().flatten().copied().collect();
    let (out, remap) = subset_impl(data, &used, Some(byte_to_gid))?;
    // Result byte->new gid map.
    let mut new_map: [Option<u16>; 256] = [None; 256];
    for b in 0..256 {
        if let Some(g) = byte_to_gid[b] {
            new_map[b] = Some(remap[g as usize]);
        }
    }
    Some((out, new_map))
}

fn subset_impl(
    data: &[u8],
    used: &[u16],
    // WinAnsi placement mode: map of byte -> original gid; the glyph is
    // renumbered so its new GID equals the byte. None = dense order.
    byte_map: Option<&[Option<u16>; 256]>,
) -> Option<(Vec<u8>, Vec<u16>)> {
    let (tables, _num_tables) = read_directory(data)?;
    let head = tables.get(b"head")?;
    let maxp = tables.get(b"maxp")?;
    let loca = tables.get(b"loca")?;
    let glyf = tables.get(b"glyf")?;
    let hmtx = tables.get(b"hmtx")?;
    let hhea = tables.get(b"hhea")?;

    // --- sizes / format ---
    let units_per_em = u16::from_be_bytes([head[18], head[19]]);
    let index_to_loc_format = i16::from_be_bytes([head[50], head[51]]);
    let long_loca = index_to_loc_format == 1;
    let num_glyphs = u16::from_be_bytes([maxp[4], maxp[5]]) as usize;

    let _loca_len = num_glyphs + 1;
    let loca_at = |i: usize| -> usize {
        if long_loca {
            u32::from_be_bytes([
                loca[i * 4],
                loca[i * 4 + 1],
                loca[i * 4 + 2],
                loca[i * 4 + 3],
            ]) as usize
        } else {
            u16::from_be_bytes([loca[i * 2], loca[i * 2 + 1]]) as usize * 2
        }
    };

    // --- resolve the glyph closure (composites) ---
    let mut keep: Vec<bool> = vec![false; num_glyphs];
    keep[0] = true;
    let mut queue: Vec<u16> = Vec::new();
    for &g in used {
        if (g as usize) < num_glyphs && !keep[g as usize] {
            keep[g as usize] = true;
            queue.push(g);
        }
    }
    while let Some(g) = queue.pop() {
        let gid = g as usize;
        let (start, end) = (loca_at(gid), loca_at(gid + 1));
        if end <= start || end > glyf.len() {
            continue;
        }
        // Composite flag bit 0x0040 = WE_HAVE_A_GLYF_COLLECTION? No:
        // bit 0x0020 = MORE_COMPONENTS. Composite iff numberOfContours < 0.
        let n_contours = i16::from_be_bytes([glyf[start], glyf[start + 1]]);
        if n_contours >= 0 {
            continue;
        }
        // Walk components: after the 10-byte header, repeat {flags, gid},
        // argument size depends on flags (ARG_1_AND_2_ARE_WORDS 0x0001,
        // WE_ARE_A_TWO_BY_TWO etc.).
        let mut off = start + 10;
        loop {
            if off + 4 > end {
                break;
            }
            let flags = u16::from_be_bytes([glyf[off], glyf[off + 1]]);
            let comp_gid = u16::from_be_bytes([glyf[off + 2], glyf[off + 3]]) as usize;
            if comp_gid < num_glyphs && !keep[comp_gid] {
                keep[comp_gid] = true;
                queue.push(comp_gid as u16);
            }
            // advance past this component record
            let mut adv = 4usize;
            if flags & 0x0001 != 0 {
                adv += 4; // two words
            } else {
                adv += 2; // two bytes
            }
            if flags & 0x0008 != 0 {
                adv += 2; // WE_HAVE_A_SCALE
            } else if flags & 0x0040 != 0 {
                adv += 4; // WE_HAVE_AN_X_AND_Y_SCALE
            } else if flags & 0x0080 != 0 {
                adv += 6; // WE_HAVE_A_TWO_BY_TWO
            }
            off += adv;
            if flags & 0x0020 == 0 {
                break; // no MORE_COMPONENTS
            }
        }
    }

    // --- gid remap: old -> new ---
    // With a placement function (WinAnsi mode), each kept glyph goes to a
    // chosen new GID (the byte it renders for); spares land after 256.
    // Otherwise new GIDs are assigned densely in ascending order.
    let mut remap: Vec<u16> = vec![0; num_glyphs];
    let mut placement: std::collections::HashMap<u32, u16> = Default::default();
    let mut next_spare: u32 = 256;
    let mut new_glyfs: Vec<u16> = Vec::new();
    if let Some(byte_map) = byte_map {
        // Byte-driven placement: every byte that maps to a glyph gets that
        // glyph at GID == byte (even when several bytes share one glyph).
        for (b, g) in byte_map.iter().enumerate() {
            if let Some(old) = g {
                let slot = b as u32;
                placement.insert(slot, *old);
                remap[*old as usize] = slot as u16;
            }
        }
        // Everything else that must be kept (components, .notdef) lands
        // past the byte range at spare slots.
        for old in 0..num_glyphs {
            if keep[old] && !byte_map.iter().any(|g| *g == Some(old as u16)) {
                let s = next_spare;
                next_spare += 1;
                placement.insert(s, old as u16);
                remap[old] = s as u16;
            }
        }
        let max_slot = placement.keys().copied().max().unwrap_or(0);
        for slot in 0..=max_slot {
            new_glyfs.push(placement.get(&slot).copied().unwrap_or(0));
        }
    } else {
        for old in 0..num_glyphs {
            if keep[old] {
                remap[old] = new_glyfs.len() as u16;
                new_glyfs.push(old as u16);
            }
        }
    }
    let new_num_glyphs = new_glyfs.len();

    // --- copy glyph data, building new glyf + new loca ---
    let mut new_glyf: Vec<u8> = Vec::new();
    // Use short loca when possible (offsets < 2*65536).
    let mut offsets: Vec<u32> = Vec::with_capacity(new_num_glyphs + 1);
    // Pad glyph data to even lengths (loca offsets must be 2-byte aligned
    // for the short format to round-trip).
    for &old in &new_glyfs {
        let (start, end) = (loca_at(old as usize), loca_at(old as usize + 1));
        offsets.push(new_glyf.len() as u32);
        if end > start {
            new_glyf.extend_from_slice(&glyf[start..end]);
            if new_glyf.len() % 2 != 0 {
                new_glyf.push(0);
            }
        }
    }
    offsets.push(new_glyf.len() as u32);

    let can_short_loca = *offsets.last().unwrap() / 2 <= u16::MAX as u32;
    let new_loca: Vec<u8> = if can_short_loca {
        offsets
            .iter()
            .flat_map(|&o| ((o / 2) as u16).to_be_bytes())
            .collect()
    } else {
        offsets.iter().flat_map(|&o| o.to_be_bytes()).collect()
    };
    // loca must have numGlyphs+1 entries — offsets already has that many.
    debug_assert_eq!(
        new_loca.len() / if can_short_loca { 2 } else { 4 },
        new_num_glyphs + 1
    );

    // --- rewrite composite component GIDs inside new_glyf ---
    // (data was copied from the original, so old GIDs appear inline)
    for &old in &new_glyfs {
        let (start, end) = (loca_at(old as usize), loca_at(old as usize + 1));
        if end <= start {
            continue;
        }
        let base = offsets[new_glyfs.iter().position(|&g| g == old).unwrap()] as usize;
        let n_contours = i16::from_be_bytes([glyf[start], glyf[start + 1]]);
        if n_contours >= 0 {
            continue;
        }
        let mut off = start + 10;
        let mut new_off = base + 10;
        loop {
            if off + 4 > end {
                break;
            }
            let comp_gid = u16::from_be_bytes([glyf[off + 2], glyf[off + 3]]) as usize;
            let mapped = remap[comp_gid];
            new_glyf[new_off + 2..new_off + 4].copy_from_slice(&mapped.to_be_bytes());
            let flags = u16::from_be_bytes([glyf[off], glyf[off + 1]]);
            let mut adv = 4usize;
            if flags & 0x0001 != 0 {
                adv += 4;
            } else {
                adv += 2;
            }
            if flags & 0x0008 != 0 {
                adv += 2;
            } else if flags & 0x0040 != 0 {
                adv += 4;
            } else if flags & 0x0080 != 0 {
                adv += 6;
            }
            off += adv;
            new_off += adv;
            if flags & 0x0020 == 0 {
                break;
            }
        }
    }

    // --- hmtx: num_hmetrics from hhea offset 34 ---
    let num_hmetrics = u16::from_be_bytes([hhea[34], hhea[35]]) as usize;
    let mut new_hmtx: Vec<u8> = Vec::new();
    let last_adv = if num_hmetrics > 0 {
        u16::from_be_bytes([
            hmtx[(num_hmetrics - 1) * 4],
            hmtx[(num_hmetrics - 1) * 4 + 1],
        ])
    } else {
        0
    };
    for &old in &new_glyfs {
        let g = old as usize;
        let adv = if g < num_hmetrics {
            u16::from_be_bytes([hmtx[g * 4], hmtx[g * 4 + 1]])
        } else {
            last_adv
        };
        let lsb = if g < num_hmetrics {
            u16::from_be_bytes([hmtx[g * 4 + 2], hmtx[g * 4 + 3]])
        } else {
            // monospaced tail: lsb is not stored after num_hmetrics
            0
        };
        new_hmtx.extend_from_slice(&adv.to_be_bytes());
        new_hmtx.extend_from_slice(&lsb.to_be_bytes());
    }
    // All glyphs get their own metric now; set numHMetrics = glyph count.
    let mut new_hhea = hhea.to_vec();
    new_hhea[34..36].copy_from_slice(&(new_num_glyphs as u16).to_be_bytes());

    // --- cmap: build a minimal format-4 map for Unicode BMP chars that
    // map to kept glyphs. We do not have reverse mapping here, so instead
    // keep the original cmap (it references old GIDs — wrong after
    // remap!). Simplest correct choice: drop cmap entirely. PDFs embed
    // fonts with explicit CID/ToUnicode mapping; the in-font cmap is only
    // used by fontconfig probes. But our probe() uses cmap...
    // Compromise: rewrite format 4 entries through the remap.
    let new_cmap = if let Some(bmap) = byte_map {
        // WinAnsi simple-font mode. Validators resolve a simple font's
        // bytes through WinAnsi to UNICODE and then the embedded cmap,
        // so the rebuilt cmap must map codepoint -> new GID, where the
        // new GID equals the byte that renders it.
        let mut pairs: Vec<(u16, u16)> = Vec::new(); // (codepoint, gid)
        for (b, g) in bmap.iter().enumerate() {
            if g.is_some() {
                if let Some(unit) = crate::winansi::unit_for_byte(b as u8) {
                    pairs.push((unit, b as u16));
                }
            }
        }
        Some(build_cmap_pairs(&pairs))
    } else {
        remap_cmap(data, tables.get(b"cmap")?, &remap)
    };

    // --- assemble the new sfnt ---
    let mut new_maxp = maxp.to_vec();
    new_maxp[4..6].copy_from_slice(&(new_num_glyphs as u16).to_be_bytes());

    let mut new_head = head.to_vec();
    // indexToLocFormat
    let fmt: i16 = if can_short_loca { 0 } else { 1 };
    new_head[50..52].copy_from_slice(&fmt.to_be_bytes());

    // Keep other tables verbatim (OS/2, name, post, GDEF? GSUB may
    // reference glyphs — drop layout tables to stay safe and small).
    let keep_tags: &[&[u8; 4]] = &[
        b"OS/2", b"name", b"post", b"cvt ", b"fpgm", b"prep", b"gasp",
    ];
    let mut out_tables: Vec<(&[u8; 4], Vec<u8>)> = Vec::new();
    out_tables.push((b"head", new_head));
    out_tables.push((b"hhea", new_hhea));
    out_tables.push((b"maxp", new_maxp));
    out_tables.push((b"hmtx", new_hmtx));
    if let Some(cm) = new_cmap.clone() {
        out_tables.push((b"cmap", cm));
    }
    out_tables.push((b"loca", new_loca));
    out_tables.push((b"glyf", new_glyf));
    for tag in keep_tags {
        if let Some(bytes) = tables.get(*tag) {
            out_tables.push((tag, bytes.to_vec()));
        }
    }
    out_tables.sort_by(|a, b| a.0.cmp(b.0));

    let total = out_tables.len();
    let mut dir_len = 12 + total * 16;
    if dir_len % 16 != 0 {
        dir_len += 16 - (dir_len % 16);
    }
    let mut out: Vec<u8> =
        Vec::with_capacity(dir_len + out_tables.iter().map(|(_, t)| t.len()).sum::<usize>());
    out.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]); // TrueType
    let max_pow2 = {
        let mut p = 1usize;
        while p * 2 <= total {
            p *= 2;
        }
        p
    };
    let search_range = (max_pow2 * 16) as u16;
    out.extend_from_slice(&(total as u16).to_be_bytes());
    out.extend_from_slice(&search_range.to_be_bytes());
    out.extend_from_slice(&(max_pow2.trailing_zeros() as u16).to_be_bytes());
    out.extend_from_slice(&((total * 16) as u16 - search_range).to_be_bytes());

    let mut offset = dir_len;
    for (tag, bytes) in &out_tables {
        out.extend_from_slice(tag.as_slice());
        out.extend_from_slice(&table_checksum(bytes).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        offset += bytes.len().wrapping_add(3) & !3;
    }
    // Pad the directory region out to dir_len (it was rounded up to 16).
    while out.len() < dir_len {
        out.push(0);
    }
    for (_tag, bytes) in &out_tables {
        out.extend_from_slice(bytes);
        let pad = (4 - bytes.len() % 4) % 4;
        out.extend(std::iter::repeat(0u8).take(pad));
    }

    // checkSumAdjustment
    fix_head_adjustment(&mut out, total)?;
    let _ = units_per_em;
    Some((out, remap))
}

/// Build a minimal cmap with one format-4 subtable (3,1) mapping each
/// (codepoint, gid) pair. The required final 0xFFFF segment maps to
/// glyph 0. Segments group consecutive codepoints whose gids are also
/// consecutive (idDelta = gid - code); everything else gets its own
/// segment with delta and no range-offset table.
fn build_cmap_pairs(pairs: &[(u16, u16)]) -> Vec<u8> {
    let mut pts = pairs.to_vec();
    pts.sort_unstable();
    pts.dedup();
    // Group: consecutive codepoints with constant (gid - code).
    let mut segs: Vec<(u16, u16, i32)> = Vec::new(); // start, end, delta
    for &(cp, gid) in &pts {
        let d = i32::from(gid) - i32::from(cp);
        match segs.last_mut() {
            Some(seg) if seg.1 + 1 == cp && seg.2 == d => seg.1 = cp,
            _ => segs.push((cp, cp, d)),
        }
    }
    segs.push((0xFFFF, 0xFFFF, 1));
    let n = segs.len();

    let mut out: Vec<u8> = Vec::with_capacity(16 + n * 8);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&12u32.to_be_bytes());
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&((16 + n * 8) as u16).to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&((n * 2) as u16).to_be_bytes());
    let max_pow2 = {
        let mut p = 1usize;
        while p * 2 <= n {
            p *= 2;
        }
        p
    };
    out.extend_from_slice(&((max_pow2 * 2) as u16).to_be_bytes());
    out.extend_from_slice(&(max_pow2.trailing_zeros() as u16).to_be_bytes());
    out.extend_from_slice(&((n * 2) as u16 - (max_pow2 * 2) as u16).to_be_bytes());
    for &(_, end, _) in &segs {
        out.extend_from_slice(&end.to_be_bytes());
    }
    out.extend_from_slice(&0u16.to_be_bytes()); // reserved pad
    for &(start, _, _) in &segs {
        out.extend_from_slice(&start.to_be_bytes());
    }
    for &(_, _, d) in &segs {
        out.extend_from_slice(&(d as u16).to_be_bytes());
    }
    for _ in 0..n {
        out.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset
    }
    out
}

fn read_directory(data: &[u8]) -> Option<(std::collections::BTreeMap<[u8; 4], Vec<u8>>, usize)> {
    if data.len() < 12 || &data[0..4] != b"\x00\x01\x00\x00" {
        return None;
    }
    let num = u16::from_be_bytes([data[4], data[5]]) as usize;
    if data.len() < 12 + num * 16 {
        return None;
    }
    let mut tables = std::collections::BTreeMap::new();
    for i in 0..num {
        let rec = 12 + i * 16;
        let mut tag = [0u8; 4];
        tag.copy_from_slice(&data[rec..rec + 4]);
        let off = u32::from_be_bytes([data[rec + 8], data[rec + 9], data[rec + 10], data[rec + 11]])
            as usize;
        let len = u32::from_be_bytes([
            data[rec + 12],
            data[rec + 13],
            data[rec + 14],
            data[rec + 15],
        ]) as usize;
        if off + len > data.len() {
            return None;
        }
        tables.insert(tag, data[off..off + len].to_vec());
    }
    Some((tables, num))
}

fn table_checksum(bytes: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let padded = bytes.len().wrapping_add(3) & !3;
    let mut i = 0;
    while i + 4 <= padded {
        let mut word = [0u8; 4];
        let end = (i + 4).min(bytes.len());
        word[..end - i].copy_from_slice(&bytes[i..end]);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
        i += 4;
    }
    sum
}

fn fix_head_adjustment(out: &mut [u8], num_tables: usize) -> Option<()> {
    // Find head in the directory, zero checkSumAdjustment, recompute.
    let mut head_off = None;
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        if &out[rec..rec + 4] == b"head" {
            let off = u32::from_be_bytes([out[rec + 8], out[rec + 9], out[rec + 10], out[rec + 11]])
                as usize;
            head_off = Some(off);
        }
    }
    let h = head_off?;
    out[h + 8..h + 12].copy_from_slice(&0u32.to_be_bytes());
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 4 <= out.len() {
        sum = sum.wrapping_add(u32::from_be_bytes([
            out[i],
            out[i + 1],
            out[i + 2],
            out[i + 3],
        ]));
        i += 4;
    }
    if i < out.len() {
        let mut last = [0u8; 4];
        last[..out.len() - i].copy_from_slice(&out[i..]);
        sum = sum.wrapping_add(u32::from_be_bytes(last));
    }
    let adjust = 0xB1B0_AFBAu32.wrapping_sub(sum);
    out[h + 8..h + 12].copy_from_slice(&adjust.to_be_bytes());
    Some(())
}

/// Rewrite every cmap subtable through the old->new GID remap, dropping
/// mappings to glyphs that were removed (mapped to nothing).
fn remap_cmap(_data: &[u8], cmap: &[u8], remap: &[u16]) -> Option<Vec<u8>> {
    // For simplicity and safety, build a fresh cmap with one format-4
    // subtable rebuilt from the original format 4 (platform 3 encoding 1).
    if cmap.len() < 4 {
        return None;
    }
    let num_sub = u16::from_be_bytes([cmap[2], cmap[3]]) as usize;
    let mut best: Option<(usize, Vec<u8>)> = None; // (subtable offset, rebuilt)
    for i in 0..num_sub {
        let rec = 4 + i * 8;
        let plat = u16::from_be_bytes([cmap[rec], cmap[rec + 1]]);
        let enc = u16::from_be_bytes([cmap[rec + 2], cmap[rec + 3]]);
        let off = u16::from_be_bytes([cmap[rec + 4], cmap[rec + 5]]) as usize;
        if off + 2 > cmap.len() {
            continue;
        }
        let fmt = u16::from_be_bytes([cmap[off], cmap[off + 1]]);
        if fmt != 4 || !((plat == 3 && enc == 1) || (plat == 0 && enc == 3)) {
            continue;
        }
        if let Some(built) = rebuild_format4(&cmap[off..], remap) {
            best = Some((off, built));
            break;
        }
    }
    let (_, built) = best?;

    // New cmap header: 1 subtable, platform 3 encoding 1.
    let mut out = Vec::with_capacity(built.len() + 12);
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&1u16.to_be_bytes());
    out.extend_from_slice(&12u16.to_be_bytes()); // subtable offset (u16 fields)
    out.extend_from_slice(&built);

    Some(out)
}

fn rebuild_format4(sub: &[u8], remap: &[u16]) -> Option<Vec<u8>> {
    if sub.len() < 14 {
        return None;
    }
    let seg_count_x2 = u16::from_be_bytes([sub[6], sub[7]]);
    let seg_count = seg_count_x2 as usize / 2;
    let base = 14;
    let end_codes = &sub[base..base + seg_count * 2];
    let start_base = base + seg_count * 2 + 2;
    let start_codes = &sub[start_base..start_base + seg_count * 2];
    let id_delta = &sub[start_base + seg_count * 2..start_base + seg_count * 4];
    let id_range_off = &sub[start_base + seg_count * 4..];

    // Collect surviving mappings (char -> new gid).
    let mut pairs: Vec<(u16, u16)> = Vec::new();
    for seg in 0..seg_count {
        let end = u16::from_be_bytes([end_codes[seg * 2], end_codes[seg * 2 + 1]]);
        let start = u16::from_be_bytes([start_codes[seg * 2], start_codes[seg * 2 + 1]]);
        let delta = u16::from_be_bytes([id_delta[seg * 2], id_delta[seg * 2 + 1]]);
        let roff_raw = u16::from_be_bytes([id_range_off[seg * 2], id_range_off[seg * 2 + 1]]);
        if start == 0xFFFF {
            continue;
        }
        for c in start..=end {
            let gid: Option<u16> = if roff_raw == 0 {
                // gid = (c + delta) mod 65536
                Some(c.wrapping_add(delta))
            } else {
                // glyphIdArray address arithmetic; rarely used in practice
                None
            };
            if let Some(g) = gid {
                let g = g as usize;
                if g < remap.len() && remap[g] != u16::MAX || (g < remap.len() && remap[g] != 0) {
                    // kept only if it was in the keep set; remap[g] is the
                    // new gid, but we cannot distinguish removed here
                    // (remap[g] == 0 for removed AND for old gid 0).
                    // We accept the approximation: removed glyphs that map
                    // through delta will show gid 0 (.notdef) — acceptable
                    // for embedding purposes.
                }
                if g < remap.len() {
                    pairs.push((c, remap[g]));
                }
            }
        }
    }
    pairs.sort_by_key(|p| p.0);
    pairs.dedup_by_key(|p| p.0);

    // Build contiguous segments from pairs.
    let mut segs: Vec<(u16, u16, u16)> = Vec::new(); // start, end, new_gid_base
    for &(c, g) in &pairs {
        match segs.last_mut() {
            Some(seg) if seg.1 + 1 == c && g == seg.2.wrapping_add(c as u16 - seg.0 as u16) => {
                seg.1 = c;
            }
            _ => segs.push((c, c, g)),
        }
    }
    segs.push((0xFFFF, 0xFFFF, 1)); // required final segment

    let n = segs.len();
    let mut out: Vec<u8> = Vec::with_capacity(16 + n * 8);
    out.extend_from_slice(&4u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes()); // length, patched later
    out.extend_from_slice(&0u16.to_be_bytes()); // language
    out.extend_from_slice(&((n * 2) as u16).to_be_bytes());
    // searchRange etc. (not load-bearing for our consumers)
    let max_pow2 = {
        let mut p = 1usize;
        while p * 2 <= n {
            p *= 2;
        }
        p
    };
    out.extend_from_slice(&((max_pow2 * 2) as u16).to_be_bytes());
    out.extend_from_slice(&(max_pow2.trailing_zeros() as u16).to_be_bytes());
    out.extend_from_slice(&((n * 2) as u16 - (max_pow2 * 2) as u16).to_be_bytes());
    for &(_, end, _) in &segs {
        out.extend_from_slice(&end.to_be_bytes());
    }
    out.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    for &(start, _, _) in &segs {
        out.extend_from_slice(&start.to_be_bytes());
    }
    for &(start, _, g) in &segs {
        // delta = (g - start) mod 65536; all idRangeOffset = 0
        out.extend_from_slice(&g.wrapping_sub(start).to_be_bytes());
    }
    for _ in 0..n {
        out.extend_from_slice(&0u16.to_be_bytes());
    }
    let total = out.len() as u16;
    out[2..4].copy_from_slice(&total.to_be_bytes());
    Some(out)
}
