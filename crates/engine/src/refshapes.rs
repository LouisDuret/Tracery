//! Jeu de formes de référence S01–S16 (§ 9.5), en millimètres.

use geom::clip::union_loop;
use geom::math::*;
use geom::{Loop, Region};
use motifs::shapes::*;
use motifs::{ArchKind, arch_window};

pub struct RefShape {
    pub id: &'static str,
    pub name: &'static str,
    pub region: Region,
}

fn pts(v: &[(f64, f64)]) -> Vec<P2> {
    v.iter().map(|&(x, y)| P2::new(x, y)).collect()
}

/// Construit la forme de référence `id` (S01…S16).
pub fn ref_shape(id: &str) -> Option<RefShape> {
    let id = id.to_uppercase();
    let (sid, name, region): (&'static str, &'static str, Region) = match id.as_str() {
        "S01" => (
            "S01",
            "Baie tiers-point, piédroits 1,5 × portée",
            Region::simple(arch_window(ArchKind::equilateral(), 1000.0, 1500.0).0),
        ),
        "S02" => (
            "Baie lancette large (ρ = 1,3)",
            "Baie lancette large (ρ = 1,3)",
            Region::simple(arch_window(ArchKind::Pointed { rho: 1.3 }, 1800.0, 1600.0).0),
        ),
        "S03" => (
            "S03",
            "Baie Tudor surbaissée",
            Region::simple(arch_window(ArchKind::tudor(), 1600.0, 1100.0).0),
        ),
        "S04" => ("S04", "Cercle", Region::circle(P2::ZERO, 1000.0)),
        "S05" => (
            "S05",
            "Triangle curviligne (Reuleaux)",
            Region::simple(curved_triangle(P2::ZERO, 1800.0, 1.0)),
        ),
        "S06" => (
            "S06",
            "Carré",
            Region::simple(rectangle(P2::ZERO, 1600.0, 1600.0)),
        ),
        "S07" => (
            "S07",
            "Bande horizontale 4:1",
            Region::simple(rectangle(P2::ZERO, 3200.0, 800.0)),
        ),
        "S08" => ("S08", "Vesica", Region::simple(vesica(P2::ZERO, 1300.0))),
        "S09" => (
            "S09",
            "Forme libre convexe",
            Region::simple(smooth_closed(&pts(&[
                (0.0, -700.0),
                (900.0, -500.0),
                (1200.0, 200.0),
                (600.0, 800.0),
                (-500.0, 700.0),
                (-1000.0, 0.0),
            ]))),
        ),
        "S10" => (
            "S10",
            "Forme libre en haricot",
            Region::simple(smooth_closed(&pts(&[
                (-1300.0, 150.0),
                (-1000.0, 600.0),
                (-350.0, 520.0),
                (0.0, 330.0),
                (450.0, 560.0),
                (1150.0, 480.0),
                (1350.0, 0.0),
                (900.0, -380.0),
                (100.0, -350.0),
                (-500.0, -650.0),
                (-1150.0, -450.0),
            ]))),
        ),
        "S11" => (
            "S11",
            "Polygone en L",
            Region::simple(Loop::polygon(&pts(&[
                (0.0, 0.0),
                (1600.0, 0.0),
                (1600.0, 650.0),
                (650.0, 650.0),
                (650.0, 1600.0),
                (0.0, 1600.0),
            ]))),
        ),
        "S12" => (
            "S12",
            "Croissant (lunule)",
            Region::simple(lunule(P2::ZERO, 1000.0, P2::new(380.0, 260.0), 800.0)?),
        ),
        "S13" => ("S13", "Couronne", annulus(P2::ZERO, 1000.0, 330.0)),
        "S14" => (
            "S14",
            "Larme très effilée",
            Region::simple(teardrop(P2::ZERO, 420.0, P2::new(0.0, -1900.0))?),
        ),
        "S15" => (
            "S15",
            "Arc en accolade",
            Region::simple(arch_window(ArchKind::ogee(), 1100.0, 1200.0).0),
        ),
        "S16" => {
            let mut r = vec![Region::circle(P2::new(-950.0, 0.0), 650.0)];
            r = union_loop(&r, &Loop::circle(P2::new(950.0, 0.0), 650.0));
            r = union_loop(&r, &rectangle(P2::ZERO, 1900.0, 280.0));
            (
                "S16",
                "Deux lobes reliés par un col étroit",
                r.into_iter().next()?,
            )
        }
        _ => return None,
    };
    let sid = if sid.starts_with('S') { sid } else { "S02" };
    Some(RefShape {
        id: sid,
        name,
        region,
    })
}

pub const REF_IDS: [&str; 16] = [
    "S01", "S02", "S03", "S04", "S05", "S06", "S07", "S08", "S09", "S10", "S11", "S12", "S13",
    "S14", "S15", "S16",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_reference_shapes_valid() {
        for id in REF_IDS {
            let s = ref_shape(id).unwrap_or_else(|| panic!("{id}"));
            assert_eq!(s.id, id);
            assert!(s.region.area() > 0.0, "{id}");
            assert!(s.region.outer.is_ccw(), "{id}");
            assert!(!geom::clip::self_intersects(&s.region.outer), "{id}");
        }
    }
}
