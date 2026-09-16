//! Deterministic hashing and reverse complements.
//!
//! The isomorphism algorithm colors the nodes by hashing isomorphism-invariant data. The colors of
//! two graphs are only comparable if the hash function is exactly the same for both, so the
//! functions here are deterministic and fixed. In particular, they never use a randomly seeded
//! hasher such as [`std::collections::hash_map::RandomState`], and they do not depend on
//! [`std::hash::Hasher`] implementations, whose output is not specified across Rust versions.
//!
//! A hash collision merges two colors. Because the same function is applied to both graphs, the
//! same merge happens in both, so the coloring remains an isomorphism invariant and only becomes
//! coarser. A coarser coloring means more search, never a wrong answer, and the final mapping is
//! always verified without using any hash values.

use gbz::Orientation;

use std::cmp::Ordering;

#[cfg(test)]
mod tests;

//-----------------------------------------------------------------------------

/// Complement table for DNA bases.
///
/// Unlike [`gbz::support::COMPLEMENT`], this table is an involution. It preserves case and maps
/// every byte outside `ACGTacgt` to itself, so that `reverse_complement` is its own inverse for
/// arbitrary byte strings.
///
/// This matters because the canonical GFA format treats sequences as case sensitive, as some graph
/// implementations do not normalize them. A non-involutive complement would make the isomorphism
/// relation asymmetric.
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

/// Returns the reverse complement of the sequence.
///
/// See [`COMPLEMENT`] for how this differs from [`gbz::support::reverse_complement`].
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
///
/// This does not allocate.
pub fn equals_reverse_complement(sequence: &[u8], other: &[u8]) -> bool {
    sequence.len() == other.len() &&
        sequence.iter().zip(other.iter().rev()).all(|(&c, &d)| c == COMPLEMENT[d as usize])
}

/// Compares the sequence to its reverse complement.
///
/// Returns [`Ordering::Equal`] if the sequence is its own reverse complement.
/// This does not allocate.
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

/// Returns the reference orientation of the sequence.
///
/// This is [`Orientation::Forward`] if the sequence is lexicographically smaller than or equal to
/// its reverse complement, and [`Orientation::Reverse`] otherwise. Because the two orientations of
/// a node are considered in the same order in both graphs, corresponding nodes always get the same
/// reference orientation.
pub fn reference_orientation(sequence: &[u8]) -> Orientation {
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
pub fn hash_reverse_complement(sequence: &[u8]) -> u64 {
    let mut result = FNV_OFFSET;
    for &c in sequence.iter().rev() {
        result = (result ^ (COMPLEMENT[c as usize] as u64)).wrapping_mul(FNV_PRIME);
    }
    mix(result ^ (sequence.len() as u64))
}

/// Returns a hash value for the canonical form of the sequence.
///
/// If `allow_flips` is `true`, the canonical form is the smaller of the sequence and its reverse
/// complement, so that a node and its reverse complement get the same value. Otherwise the
/// canonical form is the sequence itself.
pub fn hash_canonical_sequence(sequence: &[u8], allow_flips: bool) -> u64 {
    if !allow_flips || compare_to_reverse_complement(sequence) != Ordering::Greater {
        hash_sequence(sequence)
    } else {
        hash_reverse_complement(sequence)
    }
}

//-----------------------------------------------------------------------------
