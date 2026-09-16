use super::*;

//-----------------------------------------------------------------------------

#[test]
fn complement_is_an_involution() {
    // This must hold for every byte, not just for DNA bases. Otherwise `min(seq, revcomp(seq))` is
    // not well defined and the isomorphism relation is not symmetric.
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
    // Case is preserved, unlike in `gbz::support::reverse_complement`.
    assert_eq!(reverse_complement(b"gatta"), b"taatc".to_vec(), "Case was not preserved");
    // Unknown characters are preserved, unlike in `gbz::support::reverse_complement`.
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
            reference_orientation(sequence), Orientation::Forward,
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
fn reference_orientation_is_consistent() {
    // A sequence and its reverse complement must agree on the canonical form, and disagree on the
    // reference orientation unless the sequence is a palindrome.
    let sequences: Vec<&[u8]> = vec![b"A", b"GATTACA", b"AAAA", b"TTTT", b"acgtt"];
    for sequence in sequences.iter() {
        let rc = reverse_complement(sequence);
        assert_eq!(
            hash_canonical_sequence(sequence, true), hash_canonical_sequence(&rc, true),
            "Different canonical hashes for {} and its reverse complement",
            String::from_utf8_lossy(sequence)
        );
        assert_ne!(
            reference_orientation(sequence), reference_orientation(&rc),
            "Same reference orientation for {} and its reverse complement",
            String::from_utf8_lossy(sequence)
        );
        // Without flips, the canonical form is the sequence itself.
        assert_eq!(
            hash_canonical_sequence(sequence, false), hash_sequence(sequence),
            "Wrong canonical hash without flips for {}", String::from_utf8_lossy(sequence)
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

//-----------------------------------------------------------------------------
