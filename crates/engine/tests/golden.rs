//! Seeds dorées (§ 5.9, EQ-06) : toute modification involontaire du
//! comportement d'une règle change ces empreintes. Pour régénérer la
//! référence après un changement voulu (et une incrémentation de
//! GENERATOR_VERSION) : `TRACERY_BLESS=1 cargo test -p engine --test golden`.

use engine::refshapes::ref_shape;
use engine::{CancelToken, Family, Genome, StyleParams, generate};
use std::collections::BTreeMap;

fn fnv(bytes: &[u8]) -> String {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    format!("{h:016x}")
}

#[test]
fn golden_seeds() {
    let cases = [
        ("S01", Family::Rayonnant, 1u64),
        ("S01", Family::Perpendiculaire, 2),
        ("S04", Family::Rayonnant, 3),
        ("S05", Family::Geometrique, 4),
        ("S10", Family::Flamboyant, 5),
        ("S13", Family::Plaque, 6),
    ];
    let mut got = BTreeMap::new();
    for (id, fam, seed) in cases {
        let s = ref_shape(id).unwrap();
        let g = Genome::new(seed, StyleParams::for_family(fam));
        let d = generate(&s.region, &g, &CancelToken::new()).unwrap();
        let j = serde_json::to_vec(&d.openings).unwrap();
        got.insert(format!("{id}-{}-{seed}", fam.key()), fnv(&j));
    }
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/golden/seeds.json");
    if std::env::var("TRACERY_BLESS").is_ok() || !std::path::Path::new(path).exists() {
        std::fs::write(path, serde_json::to_string_pretty(&got).unwrap()).unwrap();
        return;
    }
    let want: BTreeMap<String, String> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(got, want, "empreintes des seeds dorées modifiées");
}
