//! Ticket identity: the ULID key + the derived, all-caps display slug.
//!
//! IDs are ULIDs (`docs/02-ticket-storage.md` §2.4) — no central sequence, mintable
//! offline, k-sortable. The human-facing **slug** is derived deterministically from
//! the ULID + the store's prefix, and rendered in ALL CAPS (e.g. `HS-7F3K9Q`).

/// The ticket id type. Re-exported from the `ulid` crate; minting happens through an
/// injected clock/rng in `hotsheet-ticketing`, never in this pure crate.
pub use ulid::Ulid;

/// Crockford base32 alphabet (uppercase; excludes I, L, O, U).
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Derive the 6-character slug code from a ULID and format it as `<PREFIX>-<CODE>`,
/// e.g. `HS-7F3K9Q`. Deterministic and pure — the same ULID + prefix always yields
/// the same slug. (Collisions are resolved at display time by the index, §2.4.)
pub fn derive_slug(id: &Ulid, prefix: &str) -> String {
    // FNV-1a (64-bit) over the 16 ULID bytes — a small, stable, dependency-free hash.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.to_bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // Take the low 30 bits as six Crockford base32 characters.
    let code: String = (0..6)
        .map(|i| {
            let shift = 25 - 5 * i;
            CROCKFORD[((hash >> shift) & 0x1f) as usize] as char
        })
        .collect();
    format!("{}-{}", prefix.to_uppercase(), code)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Ulid {
        Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap()
    }

    #[test]
    fn slug_is_deterministic() {
        let a = derive_slug(&sample(), "HS");
        let b = derive_slug(&sample(), "HS");
        assert_eq!(a, b);
    }

    #[test]
    fn slug_shape_is_prefix_dash_six_uppercase() {
        let slug = derive_slug(&sample(), "hs"); // lowercase prefix is upcased
        let (prefix, code) = slug.split_once('-').expect("has a dash");
        assert_eq!(prefix, "HS");
        assert_eq!(code.len(), 6);
        assert!(
            code.bytes().all(|b| CROCKFORD.contains(&b)),
            "code {code} is Crockford base32"
        );
        assert_eq!(slug, slug.to_uppercase(), "slug is all-caps");
    }

    #[test]
    fn different_prefixes_signal_different_stores() {
        let id = sample();
        assert_ne!(derive_slug(&id, "HS"), derive_slug(&id, "SEC"));
    }
}
