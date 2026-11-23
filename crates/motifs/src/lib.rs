//! Motifs gothiques construits à la règle et au compas : arcs, polylobes,
//! mouchettes, soufflets, roses, redentage (§ 4.4 à 4.8).
#![forbid(unsafe_code)]

pub mod arches;
pub mod cusp;
pub mod flamboyant;
pub mod foils;
pub mod shapes;

pub use arches::{ArchKind, arch_curve, arch_window};

/// Baie canonique à deux lancettes (§ 4.6) : oculus de centre (0, y₀) et de
/// rayon r tangent intérieurement aux deux arcs principaux (portée s, rayon
/// R) et extérieurement aux deux sous-arcs (portée s/2, rayon R′) :
///   r = (R·s − s²/4) / (2(R + R′)),  y₀ = √((R − r)² − (R − s/2)²).
pub fn two_light_oculus(s: f64, r_main: f64, r_sub: f64) -> (f64, f64) {
    let r = (r_main * s - s * s / 4.0) / (2.0 * (r_main + r_sub));
    let y0 = ((r_main - r) * (r_main - r) - (r_main - s / 2.0) * (r_main - s / 2.0)).sqrt();
    (r, y0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_values() {
        // baie tiers-point : r = s/4, y₀ = s·√5/4
        let s = 2.0;
        let (r, y0) = two_light_oculus(s, s, s / 2.0);
        assert!((r - s / 4.0).abs() < 1e-15);
        assert!((y0 - s * 5f64.sqrt() / 4.0).abs() < 1e-14);
    }
}
