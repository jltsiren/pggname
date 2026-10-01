//! Deterministic hashing for node coloring in graph isomorphism.

use gbz::Orientation;

use std::cmp::Ordering;

//-----------------------------------------------------------------------------

/// Complement table for DNA bases.
///
/// Unlike [`gbz::support::COMPLEMENT`], this preserves case.
/// We need this, since the naming scheme currently assumes that sequences are case sensitive.
pub const COMPLEMENT: [u8; 256] = generate_complement_table();

const fn generate_complement_table() -> [u8; 256] {
    let mut result: [u8; 256] = [0; 256];
    let mut i = 0;
    while i < 256 {
        result[i] = i as u8;
        i += 1;
    }
    result[b'A' as usize] = b'T'; result[b'T' as usize] = b'A';
    result[b'C' as usize] = b'G'; result[b'G' as usize] = b'C';
    result[b'a' as usize] = b't'; result[b't' as usize] = b'a';
    result[b'c' as usize] = b'g'; result[b'g' as usize] = b'c';
    result
}

/// Returns the case sensitive reverse complement of the sequence.
///
/// # Examples
///
/// ```
/// use pggname::isomorphism::hashing;
///
/// assert_eq!(hashing::reverse_complement(b"GATTA"), b"TAATC".to_vec());
/// // The complement preserves case and is an involution.
/// let sequence = b"gattaca*N";
/// let once = hashing::reverse_complement(sequence);
/// assert_eq!(hashing::reverse_complement(&once), sequence.to_vec());
/// ```
pub fn reverse_complement(sequence: &[u8]) -> Vec<u8> {
    sequence.iter().rev().map(|&c| COMPLEMENT[c as usize]).collect()
}

/// Returns `true` if the first sequence is the reverse complement of the second.
pub fn equals_reverse_complement(sequence: &[u8], other: &[u8]) -> bool {
    sequence.len() == other.len() &&
        sequence.iter().zip(other.iter().rev()).all(|(&c, &d)| c == COMPLEMENT[d as usize])
}

/// Compares the sequence to its reverse complement.
///
/// Returns [`Ordering::Equal`] if the sequence is its own reverse complement.
pub fn compare_to_reverse_complement(sequence: &[u8]) -> Ordering {
    let len = sequence.len();
    for i in 0..len {
        let complement = COMPLEMENT[sequence[len - 1 - i] as usize];
        match sequence[i].cmp(&complement) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// Returns `true` if the sequence is its own reverse complement.
pub fn is_palindrome(sequence: &[u8]) -> bool {
    compare_to_reverse_complement(sequence) == Ordering::Equal
}

/// Returns the canonical orientation of the sequence.
///
/// The canonical orientation is that of the smaller of the sequence and its reverse complement.
/// Returns [`Orientation::Forward`] if the sequence is a palindrome.
pub fn canonical_orientation(sequence: &[u8]) -> Orientation {
    match compare_to_reverse_complement(sequence) {
        Ordering::Greater => Orientation::Reverse,
        _ => Orientation::Forward,
    }
}

//-----------------------------------------------------------------------------

// Initial state for hashing byte sequences (FNV-1a offset basis).
const FNV_OFFSET: u64 = 0xcbf29ce484222325;

// Multiplier for hashing byte sequences (FNV-1a prime).
const FNV_PRIME: u64 = 0x100000001b3;

// Golden ratio in fixed point, used for combining values.
const GOLDEN: u64 = 0x9e3779b97f4a7c15;

/// Mixes the bits of the value (the SplitMix64 finalizer).
///
/// This is a bijection, so it never loses information.
#[inline]
pub const fn mix(value: u64) -> u64 {
    let mut result = value;
    result ^= result >> 30;
    result = result.wrapping_mul(0xbf58476d1ce4e5b9);
    result ^= result >> 27;
    result = result.wrapping_mul(0x94d049bb133111eb);
    result ^= result >> 31;
    result
}

/// Combines a hash value with another value.
#[inline]
pub const fn combine(hash: u64, value: u64) -> u64 {
    mix(hash.rotate_left(23) ^ value.wrapping_mul(GOLDEN))
}

/// Returns a hash value for the sequence.
pub fn hash_sequence(sequence: &[u8]) -> u64 {
    let mut result = FNV_OFFSET;
    for &c in sequence.iter() {
        result = (result ^ (c as u64)).wrapping_mul(FNV_PRIME);
    }
    mix(result ^ (sequence.len() as u64))
}

/// Returns a hash value for the reverse complement of the sequence, without building it.
///
/// This is equivalent to calling [`hash_sequence`] on the reverse complement of the sequence.
pub fn hash_reverse_complement(sequence: &[u8]) -> u64 {
    let mut result = FNV_OFFSET;
    for &c in sequence.iter().rev() {
        result = (result ^ (COMPLEMENT[c as usize] as u64)).wrapping_mul(FNV_PRIME);
    }
    mix(result ^ (sequence.len() as u64))
}

/// Returns a hash value for the canonical form of the sequence.
///
/// The canonical form is the smaller of the sequence and its reverse complement, so that a node and
/// its reverse complement get the same value.
pub fn hash_canonical_sequence(sequence: &[u8]) -> u64 {
    if compare_to_reverse_complement(sequence) != Ordering::Greater {
        hash_sequence(sequence)
    } else {
        hash_reverse_complement(sequence)
    }
}

//-----------------------------------------------------------------------------

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn complement_is_an_involution() {
        for value in 0..=255u8 {
            let complement = COMPLEMENT[value as usize];
            assert_eq!(
                COMPLEMENT[complement as usize], value,
                "The complement of byte {} is not an involution", value
            );
        }
    }

    #[test]
    fn reverse_complement_is_an_involution() {
        let sequences: Vec<&[u8]> = vec![
            b"", b"A", b"AT", b"GATTACA", b"gattaca", b"AcGt", b"NNNN", b"*", b"ACGTN*acgtn",
        ];
        for sequence in sequences.iter() {
            let once = reverse_complement(sequence);
            assert_eq!(
                reverse_complement(&once), sequence.to_vec(),
                "Reverse complement is not an involution for {}", String::from_utf8_lossy(sequence)
            );
        }
    }

    #[test]
    fn reverse_complement_values() {
        assert_eq!(reverse_complement(b""), b"".to_vec(), "Wrong reverse complement for an empty sequence");
        assert_eq!(reverse_complement(b"GATTA"), b"TAATC".to_vec(), "Wrong reverse complement");
        assert_eq!(reverse_complement(b"gatta"), b"taatc".to_vec(), "Case was not preserved");
        assert_eq!(reverse_complement(b"A*N"), b"N*T".to_vec(), "Unknown characters were not preserved");
    }

    #[test]
    fn palindromes() {
        let palindromes: Vec<&[u8]> = vec![b"", b"AT", b"GC", b"ACGT", b"NN", b"N", b"*", b"GATATC"];
        for sequence in palindromes.iter() {
            assert!(
                is_palindrome(sequence),
                "{} should be a palindrome", String::from_utf8_lossy(sequence)
            );
            assert_eq!(
                canonical_orientation(sequence), Orientation::Forward,
                "Wrong reference orientation for palindrome {}", String::from_utf8_lossy(sequence)
            );
        }

        let others: Vec<&[u8]> = vec![b"A", b"AA", b"GATTACA", b"At"];
        for sequence in others.iter() {
            assert!(
                !is_palindrome(sequence),
                "{} should not be a palindrome", String::from_utf8_lossy(sequence)
            );
        }
    }

    #[test]
    fn canonical_orientation_is_consistent() {
        // A sequence and its reverse complement must agree on the canonical form, and disagree on the
        // reference orientation unless the sequence is a palindrome.
        let sequences: Vec<&[u8]> = vec![b"A", b"GATTACA", b"AAAA", b"TTTT", b"acgtt"];
        for sequence in sequences.iter() {
            let rc = reverse_complement(sequence);
            assert_eq!(
                hash_canonical_sequence(sequence), hash_canonical_sequence(&rc),
                "Different canonical hashes for {} and its reverse complement",
                String::from_utf8_lossy(sequence)
            );
            assert_ne!(
                canonical_orientation(sequence), canonical_orientation(&rc),
                "Same reference orientation for {} and its reverse complement",
                String::from_utf8_lossy(sequence)
            );
            // The canonical form is the smaller of the sequence and its reverse complement.
            let smaller: &[u8] = if **sequence <= rc[..] { sequence } else { &rc };
            assert_eq!(
                hash_canonical_sequence(sequence), hash_sequence(smaller),
                "Wrong canonical hash for {}", String::from_utf8_lossy(sequence)
            );
        }
    }

    #[test]
    fn hashing_matches_the_built_sequence() {
        let sequences: Vec<&[u8]> = vec![b"", b"A", b"GATTACA", b"acgtN*"];
        for sequence in sequences.iter() {
            assert_eq!(
                hash_reverse_complement(sequence), hash_sequence(&reverse_complement(sequence)),
                "Wrong reverse complement hash for {}", String::from_utf8_lossy(sequence)
            );
        }
    }

    #[test]
    fn hashing_separates_sequences() {
        // Sequences of the same length, of different lengths, and permutations of each other.
        let sequences: Vec<&[u8]> = vec![b"", b"A", b"C", b"AC", b"CA", b"AAA", b"ACGT", b"acgt"];
        let mut hashes: Vec<u64> = sequences.iter().map(|s| hash_sequence(s)).collect();
        hashes.sort_unstable();
        hashes.dedup();
        assert_eq!(hashes.len(), sequences.len(), "Hash values are not distinct");
    }

    #[test]
    fn mixing_is_a_bijection_on_samples() {
        let mut values: Vec<u64> = (0..1000u64).map(mix).collect();
        values.sort_unstable();
        values.dedup();
        assert_eq!(values.len(), 1000, "Mixing is not injective on small values");

        // Combining must depend on both arguments and on their order.
        assert_ne!(combine(1, 2), combine(2, 1), "Combining is symmetric");
        assert_ne!(combine(0, 0), combine(0, 1), "Combining ignores the value");
        assert_ne!(combine(0, 0), combine(1, 0), "Combining ignores the hash");
    }
}

//-----------------------------------------------------------------------------
