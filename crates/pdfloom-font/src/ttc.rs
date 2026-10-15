//! TrueType Collection (TTC) handling: extract one face as a standalone
//! TrueType font program. PDF FontFile2 must contain a single sfnt, so a
//! loaded collection face is sliced out at load time and everything
//! downstream (metrics, embedding) uses the standalone program.

/// If `data` is a TrueType Collection, extract face `index` into a new
/// standalone sfnt byte vector. Non-collection data returns None; an out
/// of range index returns None.
///
/// The extracted font keeps the original table offsets (tables in a TTC
/// are already absolute within the collection), so only the table records
/// and checksums are rebuilt — table data is referenced by absolute
/// offsets into the original buffer, which is copied wholesale.
#[must_use]
pub fn extract_face(data: &[u8], index: u32) -> Option<Vec<u8>> {
    if data.len() < 12 || &data[0..4] != b"ttcf" {
        return None;
    }
    let num_fonts = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    if index >= num_fonts {
        return None;
    }
    let off_pos = 12 + (index as usize) * 4;
    if off_pos + 4 > data.len() {
        return None;
    }
    let face_off = u32::from_be_bytes([
        data[off_pos],
        data[off_pos + 1],
        data[off_pos + 2],
        data[off_pos + 3],
    ]) as usize;
    if face_off + 12 > data.len() {
        return None;
    }

    // Table directory of the chosen face.
    let num_tables = u16::from_be_bytes([data[face_off + 4], data[face_off + 5]]) as usize;
    let dir_start = face_off + 12;
    let dir_len = num_tables * 16;
    if dir_start + dir_len > data.len() {
        return None;
    }

    // Find the end of all table data in this face.
    let mut data_end = 0usize;
    for t in 0..num_tables {
        let rec = dir_start + t * 16;
        let len = u32::from_be_bytes([
            data[rec + 12],
            data[rec + 13],
            data[rec + 14],
            data[rec + 15],
        ]) as usize;
        let off = u32::from_be_bytes([data[rec + 8], data[rec + 9], data[rec + 10], data[rec + 11]])
            as usize;
        data_end = data_end.max(off.saturating_add(len));
    }
    if data_end > data.len() {
        return None;
    }

    // Rebuild: sfnt header + table directory (with fresh checksums for the
    // modified records) + a copy of all bytes from 0..data_end (the
    // offsets stay absolute, so the header/directory region of the
    // original collection is simply included).
    let mut out = Vec::with_capacity(data_end);
    out.extend_from_slice(&data[..data_end]);

    // Overwrite the sfnt header region: the face's own header is at
    // face_off. We need to write the standard TrueType header into out at
    // position 0, then the table directory at 12. But positions 0..12 in
    // the original data are the TTC header — replace them with the face's
    // sfnt header.
    out[..12].copy_from_slice(&data[face_off..face_off + 12]);
    // Table directory at offset 12 (matching the face's numTables).
    out[12..12 + dir_len].copy_from_slice(&data[dir_start..dir_start + dir_len]);

    // Fix entrySelector / searchRange / rangeShift in the copied header.
    crate::sfnt::write_directory_search(&mut out, num_tables);

    // head.checkSumAdjustment must match the new whole-font checksum:
    // find the head table's offset in the copied directory.
    let mut head_off = None;
    for t in 0..num_tables {
        let rec = 12 + t * 16;
        if &out[rec..rec + 4] == b"head" {
            let off = u32::from_be_bytes([out[rec + 8], out[rec + 9], out[rec + 10], out[rec + 11]])
                as usize;
            head_off = Some(off);
        }
    }
    if let Some(h) = head_off {
        crate::sfnt::fix_head_adjustment(&mut out, h);
    }
    Some(out)
}
