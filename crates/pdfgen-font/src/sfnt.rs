//! Shared sfnt (TrueType/OTF container) helpers used by the TTC
//! extractor and the subsetter: directory-entry math, table checksums
//! and the head checkSumAdjustment fix.

/// Largest power of two <= n (directory searchRange math).
pub fn max_pow2(mut n: usize) -> usize {
    let mut p = 1usize;
    while p * 2 <= n {
        p *= 2;
    }
    n = p;
    n
}

/// searchRange / entrySelector / rangeShift for a table directory with
/// `num_tables` 16-byte entries, written big-endian into `out[6..12]`.
pub fn write_directory_search(out: &mut [u8], num_tables: usize) {
    let p = max_pow2(num_tables);
    let search_range = (p * 16) as u16;
    out[6..8].copy_from_slice(&search_range.to_be_bytes());
    out[8..10].copy_from_slice(&(p.trailing_zeros() as u16).to_be_bytes());
    out[10..12].copy_from_slice(&((num_tables * 16) as u16 - search_range).to_be_bytes());
}

/// Big-endian u32 sum of `bytes` zero-padded to a 4-byte boundary (the
/// sfnt table checksum).
pub fn sum32(bytes: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let mut i = 0;
    let padded = bytes.len().wrapping_add(3) & !3;
    while i + 4 <= padded {
        let mut word = [0u8; 4];
        let end = (i + 4).min(bytes.len());
        word[..end - i].copy_from_slice(&bytes[i..end]);
        sum = sum.wrapping_add(u32::from_be_bytes(word));
        i += 4;
    }
    sum
}

/// Whole-font checksum adjustment: zero head.checkSumAdjustment, sum the
/// file as big-endian u32 words, store 0xB1B0AFBA - sum at `head_off+8`.
pub fn fix_head_adjustment(out: &mut [u8], head_off: usize) {
    if head_off + 12 > out.len() {
        return;
    }
    out[head_off + 8..head_off + 12].copy_from_slice(&0u32.to_be_bytes());
    let sum = sum32(out);
    let adjust = 0xB1B0_AFBAu32.wrapping_sub(sum);
    out[head_off + 8..head_off + 12].copy_from_slice(&adjust.to_be_bytes());
}
