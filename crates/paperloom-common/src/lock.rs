//! The optional password in front of the Hidden section.
//!
//! It keeps people who pick up your computer out of the Hidden tab; it does
//! not encrypt anything — hidden wallpapers are ordinary files on disk. The
//! password is stored only as a salted, repeatedly hashed value.

use sha2::{Digest, Sha256};

const ROUNDS: u32 = 100_000;

fn digest(salt: &[u8], password: &str) -> [u8; 32] {
    let mut hash: [u8; 32] = Sha256::new().chain_update(salt).chain_update(password.as_bytes()).finalize().into();
    for _ in 0..ROUNDS {
        hash = Sha256::new().chain_update(hash).chain_update(salt).chain_update(password.as_bytes()).finalize().into();
    }
    hash
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok()).collect()
}

/// A fresh random salt (the standard library's per-process random seed,
/// mixed with the clock).
fn salt() -> [u8; 16] {
    use std::hash::{BuildHasher, Hasher};
    let mut out = [0u8; 16];
    for chunk in out.chunks_mut(8) {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
        chunk.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    out
}

/// What to store for `password`.
pub fn make(password: &str) -> String {
    let salt = salt();
    format!("v1${}${}", hex(&salt), hex(&digest(&salt, password)))
}

pub fn verify(stored: &str, password: &str) -> bool {
    let mut parts = stored.split('$');
    let (Some("v1"), Some(salt), Some(expected), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else { return false };
    let (Some(salt), Some(expected)) = (unhex(salt), unhex(expected)) else { return false };
    let actual = digest(&salt, password);
    // Compare every byte, whatever the first mismatch.
    actual.len() == expected.len() && actual.iter().zip(&expected).fold(0u8, |difference, (a, b)| difference | (a ^ b)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_the_right_password() {
        let stored = make("correct horse");
        assert!(verify(&stored, "correct horse"));
        assert!(!verify(&stored, "correct horse "));
        assert!(!verify(&stored, ""));
        assert!(!verify("garbage", "correct horse"));
        assert_ne!(make("same"), make("same"), "each password gets its own salt");
    }
}
