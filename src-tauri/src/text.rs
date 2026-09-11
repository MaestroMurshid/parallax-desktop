//! Rust indexes by UTF-8 byte, JavaScript by UTF-16 code unit. UTF-16 wins,
//! because `slice` is what consumes a span offset and `transcript.length` is
//! what the classification port compares it against: an offset is converted
//! where it is produced, and converted back only to take a Rust slice.

/// A mid-character offset clamps down to the character it sits inside.
pub fn byte_to_utf16(s: &str, byte_offset: usize) -> u32 {
    let mut at = byte_offset.min(s.len());
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    s[..at].encode_utf16().count() as u32
}

/// Clamps to a character boundary: half a surrogate pair is a legal UTF-16 index.
pub fn utf16_to_byte(s: &str, utf16_offset: u32) -> usize {
    let target = utf16_offset as usize;
    let mut units = 0;
    for (at, ch) in s.char_indices() {
        if units == target {
            return at;
        }
        units += ch.len_utf16();
        if units > target {
            return at;
        }
    }
    s.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirrors `String.prototype.slice`, so assertions read in the frontend's units.
    fn js_slice(s: &str, start: u32, end: u32) -> String {
        let units: Vec<u16> = s.encode_utf16().collect();
        let lo = (start as usize).min(units.len());
        let hi = (end as usize).min(units.len()).max(lo);
        String::from_utf16_lossy(&units[lo..hi])
    }

    #[test]
    fn ascii_offsets_are_unchanged() {
        let s = "plain ascii text";
        assert_eq!(byte_to_utf16(s, 0), 0);
        assert_eq!(byte_to_utf16(s, 6), 6);
        assert_eq!(byte_to_utf16(s, s.len()), 16);
    }

    /// The snippet prefix: three bytes, one unit. This is the ASCII skew.
    #[test]
    fn a_three_byte_character_counts_as_one_unit() {
        let s = "… tail";
        assert_eq!(s.len(), 8);
        assert_eq!(byte_to_utf16(s, 4), 2);
    }

    #[test]
    fn an_astral_character_counts_as_two_units() {
        let s = "a🎧b";
        assert_eq!(byte_to_utf16(s, 1), 1);
        assert_eq!(byte_to_utf16(s, 5), 3);
        assert_eq!(byte_to_utf16(s, s.len()), 4);
    }

    #[test]
    fn combining_marks_count_separately() {
        let s = "i\u{0307}x";
        assert_eq!(byte_to_utf16(s, 1), 1);
        assert_eq!(byte_to_utf16(s, 3), 2);
    }

    /// The offset that panicked search.
    #[test]
    fn a_byte_offset_inside_a_character_clamps_down() {
        assert_eq!(byte_to_utf16("i\u{0307}x", 2), 1);
    }

    #[test]
    fn offsets_past_the_end_clamp_to_the_end() {
        assert_eq!(byte_to_utf16("short", 999), 5);
        assert_eq!(utf16_to_byte("short", 999), 5);
    }

    #[test]
    fn utf16_offsets_convert_back_to_bytes() {
        let s = "a🎧b";
        assert_eq!(utf16_to_byte(s, 0), 0);
        assert_eq!(utf16_to_byte(s, 1), 1);
        assert_eq!(utf16_to_byte(s, 3), 5);
        assert_eq!(utf16_to_byte(s, 4), 6);
    }

    #[test]
    fn a_split_surrogate_pair_clamps_to_a_boundary() {
        let s = "a🎧b";
        let at = utf16_to_byte(s, 2);
        assert!(s.is_char_boundary(at));
        assert_eq!(at, 1);
    }

    #[test]
    fn the_conversions_round_trip_on_character_boundaries() {
        let s = "ascii … 🎧 नमस्ते i\u{0307} end";
        for (at, _) in s.char_indices().chain(std::iter::once((s.len(), ' '))) {
            let units = byte_to_utf16(s, at);
            assert_eq!(utf16_to_byte(s, units), at, "byte {at} did not round trip");
        }
    }

    #[test]
    fn converted_offsets_slice_correctly_in_javascript() {
        let s = "… the 🎧 headphones are here";
        let at = s.find("headphones").unwrap();
        let start = byte_to_utf16(s, at);
        let end = byte_to_utf16(s, at + "headphones".len());
        assert_eq!(js_slice(s, start, end), "headphones");
    }
}
