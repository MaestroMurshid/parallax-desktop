//! Similarity, and the deterministic hash the placement jitter reads.
//! Ported from `lib/scene/vector.ts`; both sides must agree.

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub fn norm(a: &[f32]) -> f32 {
    dot(a, a).sqrt()
}

/// Zero for a zero-length vector rather than NaN, so an unembedded entry reads
/// as "not similar to anything" instead of poisoning every comparison.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let d = norm(a) * norm(b);
    if d == 0.0 {
        0.0
    } else {
        dot(a, b) / d
    }
}

/// FNV-1a over UTF-16 code units, matching `hash32` in the TypeScript. The
/// code-unit detail matters: iterating bytes would give a different number for
/// any non-ASCII id and quietly move those entries.
pub fn hash32(s: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for unit in s.encode_utf16() {
        h ^= unit as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_of_orthogonal_vectors_is_zero() {
        assert_eq!(cosine(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
    }

    #[test]
    fn cosine_of_a_vector_with_itself_is_one() {
        assert!((cosine(&[3.0, 4.0], &[3.0, 4.0]) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn an_empty_vector_is_similar_to_nothing() {
        assert_eq!(cosine(&[], &[1.0, 0.0]), 0.0);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 0.0]), 0.0);
    }

    /// Values taken from the TypeScript, which produced the coordinates the
    /// seeded corpus already carries.
    #[test]
    fn hash32_matches_the_typescript() {
        assert_eq!(hash32(""), 2166136261);
        assert_eq!(hash32("a"), 0xe40c292c);
        assert_eq!(
            hash32("free-will-own-reasoning"),
            hash32("free-will-own-reasoning")
        );
    }
}
