//! Seeds : SplitMix64, seeds hiérarchiques (§ 5.9), affichage Crockford
//! base32 (EF-40).

/// SplitMix64 (Steele, Lea, Flood).
pub fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Empreinte 64 bits d'un chemin de nœud (indépendante de la plateforme).
pub fn hash64(path: &[u16]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325 ^ path.len() as u64;
    for &i in path {
        h = splitmix64(h ^ (i as u64).wrapping_mul(0x1000_0000_01B3));
    }
    h
}

/// seed(nœud) = SplitMix64(seed_globale ⊕ hash64(chemin) ⊕ sel(chemin)).
pub fn node_seed(global: u64, path: &[u16], salt: u32) -> u64 {
    let sel = if salt == 0 {
        0
    } else {
        splitmix64(0x5A17_0000 ^ salt as u64)
    };
    splitmix64(global ^ hash64(path) ^ sel)
}

/// Seed dérivée pour un usage secondaire (jumelage, mutation…).
pub fn derived(seed: u64, tag: u64) -> u64 {
    splitmix64(seed ^ splitmix64(tag))
}

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Seed en Crockford base32, 13 caractères groupés par 4 : `7QK3-M2XD-9A1P-F`.
pub fn to_base32(seed: u64) -> String {
    let mut chars = [b'0'; 13];
    let mut v = seed as u128;
    for i in (0..13).rev() {
        chars[i] = ALPHABET[(v & 31) as usize];
        v >>= 5;
    }
    let s: String = chars.iter().map(|&c| c as char).collect();
    format!("{}-{}-{}-{}", &s[0..4], &s[4..8], &s[8..12], &s[12..13])
}

/// Forme compacte (sans tirets).
pub fn to_base32_compact(seed: u64) -> String {
    to_base32(seed).replace('-', "")
}

fn decode_char(c: char) -> Option<u64> {
    let c = match c.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        c => c,
    };
    ALPHABET
        .iter()
        .position(|&a| a as char == c)
        .map(|p| p as u64)
}

/// Décodage strict d'une seed base32.
pub fn from_base32(s: &str) -> Option<u64> {
    let clean: String = s
        .chars()
        .filter(|c| *c != '-' && !c.is_whitespace())
        .collect();
    if clean.is_empty() || clean.len() > 13 {
        return None;
    }
    let mut v: u128 = 0;
    for c in clean.chars() {
        v = (v << 5) | decode_char(c)? as u128;
    }
    if v > u64::MAX as u128 {
        return None;
    }
    Some(v as u64)
}

/// Hachage d'une seed textuelle (mot ou phrase), FNV-1a puis SplitMix64.
pub fn hash_text(s: &str) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    splitmix64(h)
}

/// Interprétation d'une seed saisie : entier décimal, base32 (avec tirets
/// ou lettres) ou texte libre haché.
pub fn parse_seed(s: &str) -> u64 {
    let t = s.trim();
    if !t.is_empty()
        && t.chars().all(|c| c.is_ascii_digit())
        && let Ok(v) = t.parse::<u64>()
    {
        return v;
    }
    if let Some(v) = from_base32(t) {
        return v;
    }
    hash_text(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_roundtrip() {
        for s in [0u64, 1, 42, u64::MAX, 0x0123_4567_89AB_CDEF] {
            let e = to_base32(s);
            assert_eq!(e.len(), 16);
            assert_eq!(from_base32(&e), Some(s));
        }
        assert_eq!(
            parse_seed("7QK3-M2XD-9A1P-F"),
            from_base32("7QK3M2XD9A1PF").unwrap()
        );
        assert_eq!(parse_seed("42"), 42);
        assert_ne!(parse_seed("rose de Chartres"), parse_seed("rose de Reims"));
    }

    #[test]
    fn node_seeds_are_independent() {
        let a = node_seed(7, &[0, 1], 0);
        let b = node_seed(7, &[0, 2], 0);
        let c = node_seed(7, &[0, 1], 1);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, node_seed(7, &[0, 1], 0));
    }
}
