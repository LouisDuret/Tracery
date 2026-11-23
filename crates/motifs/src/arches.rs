//! Arcs gothiques (§ 4.4).
//!
//! Notation : portée s, naissances en (±s/2, 0), rayon R, brisure ρ = R/s.
//! Arc brisé à deux centres : arc gauche de centre (R − s/2, 0), arc droit
//! de centre (s/2 − R, 0) ; flèche h = s·√(ρ − 1/4).

use geom::math::*;
use geom::{ConstructionEntity, ConstructionOp, Loop, Seg, Xf};
use serde::{Deserialize, Serialize};

/// Type d'arc.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ArchKind {
    /// Plein cintre (ρ = 1/2).
    Round,
    /// Arc brisé à deux centres : ρ = 1 tiers-point, ρ > 1 lancette,
    /// 1/2 < ρ < 1 surbaissé.
    Pointed { rho: f64 },
    /// Arc Tudor à quatre centres : r1 = r1f·s, raccord à l'angle θ,
    /// R₂ = k·R₂_min (R₂_min : grand centre sur l'axe).
    Tudor { r1f: f64, theta: f64, k: f64 },
    /// Arc en accolade : flèche h = hf·s, courbe basse r₁ = r1f·s/2.
    Ogee { hf: f64, r1f: f64 },
}

impl ArchKind {
    pub fn equilateral() -> Self {
        ArchKind::Pointed { rho: 1.0 }
    }
    pub fn tudor() -> Self {
        ArchKind::Tudor {
            r1f: 0.2,
            theta: PI / 4.0,
            k: 1.6,
        }
    }
    pub fn ogee() -> Self {
        ArchKind::Ogee {
            hf: 0.85,
            r1f: 0.55,
        }
    }
    /// Normalise un arc brisé dégénéré en plein cintre.
    pub fn normalized(self) -> Self {
        match self {
            ArchKind::Pointed { rho } if rho <= 0.5 + 1e-9 => ArchKind::Round,
            k => k,
        }
    }
    pub fn name(&self) -> &'static str {
        match self.normalized() {
            ArchKind::Round => "plein cintre",
            ArchKind::Pointed { rho } if (rho - 1.0).abs() < 1e-9 => "tiers-point",
            ArchKind::Pointed { rho } if rho > 1.0 => "lancette",
            ArchKind::Pointed { .. } => "surbaissé",
            ArchKind::Tudor { .. } => "Tudor",
            ArchKind::Ogee { .. } => "accolade",
        }
    }
}

/// Courbe d'arc de la naissance gauche (−s/2, 0) à la naissance droite
/// (s/2, 0), avec sa flèche et ses entités de construction.
#[derive(Clone, Debug)]
pub struct ArchCurve {
    pub segs: Vec<Seg>,
    pub height: f64,
    pub construction: Vec<ConstructionEntity>,
}

/// Demi-arc gauche (de la naissance gauche jusqu'à la clé).
fn left_half(kind: ArchKind, s: f64) -> (Vec<Seg>, f64, Vec<ConstructionEntity>) {
    let mut cons = Vec::new();
    match kind.normalized() {
        ArchKind::Round => {
            let r = s / 2.0;
            cons.push(ConstructionEntity::point(
                P2::ZERO,
                ConstructionOp::ArchCenter,
            ));
            (vec![Seg::arc(P2::ZERO, r, PI, -FRAC_PI_2)], r, cons)
        }
        ArchKind::Pointed { rho } => {
            let r = rho * s;
            let h = s * (rho - 0.25).sqrt();
            let cl = P2::new(r - s / 2.0, 0.0);
            let a_apex = atan2(h, -(r - s / 2.0));
            cons.push(ConstructionEntity::point(cl, ConstructionOp::ArchCenter));
            cons.push(ConstructionEntity::Circle {
                c: cl,
                r,
                op: ConstructionOp::ArchCenter,
            });
            (vec![Seg::arc(cl, r, PI, a_apex - PI)], h, cons)
        }
        ArchKind::Tudor { r1f, theta, k } => {
            let r1 = r1f * s;
            let a = P2::new(-(s / 2.0 - r1), 0.0);
            let r2_min = r1 + (s / 2.0 - r1) / cos(theta);
            let r2 = k * r2_min;
            let b = a + P2::new(cos(theta), -sin(theta)) * (r2 - r1);
            let h = b.y + (r2 * r2 - b.x * b.x).max(0.0).sqrt();
            let a_apex = atan2(h - b.y, -b.x);
            cons.push(ConstructionEntity::point(a, ConstructionOp::ArchCenter));
            cons.push(ConstructionEntity::point(b, ConstructionOp::ArchCenter));
            (
                vec![
                    Seg::arc(a, r1, PI, -theta),
                    Seg::arc(b, r2, PI - theta, a_apex - (PI - theta)),
                ],
                h,
                cons,
            )
        }
        ArchKind::Ogee { hf, r1f } => {
            let a = s / 2.0;
            let h = hf * s;
            let r1 = r1f * a;
            let r2 = (a * a + h * h) / (2.0 * a) - r1;
            let c1 = P2::new(-(a - r1), 0.0);
            let c2 = P2::new(-r2, h);
            let j = c1 + unit(c2 - c1) * r1;
            let aj1 = angle_of(j - c1);
            let aj2 = angle_of(j - c2);
            cons.push(ConstructionEntity::point(c1, ConstructionOp::ArchCenter));
            cons.push(ConstructionEntity::point(c2, ConstructionOp::ArchCenter));
            (
                vec![
                    Seg::arc(c1, r1, PI, aj1 - PI),
                    Seg::arc(c2, r2, aj2, ccw_delta(aj2, 0.0)),
                ],
                h,
                cons,
            )
        }
    }
}

/// Arc complet de (−s/2, 0) à (s/2, 0) passant par la clé (0, h).
pub fn arch_curve(kind: ArchKind, s: f64) -> ArchCurve {
    let (left, h, mut cons) = left_half(kind, s);
    let m = Xf::mirror_x(0.0);
    let right: Vec<Seg> = left
        .iter()
        .rev()
        .map(|sg| sg.transform(&m).reversed())
        .collect();
    let mirrored: Vec<ConstructionEntity> = cons.iter().map(|c| c.transform(&m)).collect();
    cons.extend(mirrored);
    let mut segs = left;
    segs.extend(right);
    ArchCurve {
        segs,
        height: h,
        construction: cons,
    }
}

/// Baie : piédroits de hauteur `jamb` sous la ligne des naissances (y = 0)
/// et arc au-dessus. Boucle trigonométrique.
pub fn arch_window(kind: ArchKind, s: f64, jamb: f64) -> (Loop, ArchCurve) {
    let curve = arch_curve(kind, s);
    let a = s / 2.0;
    let mut segs = Vec::new();
    if jamb > 0.0 {
        segs.push(Seg::line(P2::new(-a, -jamb), P2::new(a, -jamb)));
        segs.push(Seg::line(P2::new(a, -jamb), P2::new(a, 0.0)));
    } else {
        segs.push(Seg::line(P2::new(-a, 0.0), P2::new(a, 0.0)));
    }
    segs.extend(curve.segs.iter().rev().map(|s| s.reversed()));
    if jamb > 0.0 {
        segs.push(Seg::line(P2::new(-a, 0.0), P2::new(-a, -jamb)));
    }
    (Loop::new(segs), curve)
}

/// Rayon du lobe supérieur d'une tête trilobée : tangent intérieurement aux
/// deux arcs d'un arc brisé de portée `s` et de rayon `r`, et descendant de
/// `delta` sous la ligne des naissances (résolution par dichotomie de
/// √((R − r_t)² − (R − s/2)²) − r_t + δ = 0).
pub fn trefoil_top_lobe(s: f64, r: f64, delta: f64) -> Option<(f64, f64)> {
    let f = |rt: f64| {
        let v = (r - rt) * (r - rt) - (r - s / 2.0) * (r - s / 2.0);
        if v < 0.0 {
            return f64::NAN;
        }
        v.sqrt() - rt + delta
    };
    let (mut lo, mut hi) = (1e-9 * s, s / 2.0);
    if f(lo).is_nan() || f(lo) <= 0.0 {
        return None;
    }
    // f décroissante ; chercher un hi où f < 0
    if f(hi).is_nan() || f(hi) >= 0.0 {
        let mut found = false;
        let mut x = s / 2.0;
        while x > lo {
            if f(x) < 0.0 {
                hi = x;
                found = true;
                break;
            }
            x *= 0.9;
        }
        if !found {
            return None;
        }
    }
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        let v = f(mid);
        if v.is_nan() || v < 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let rt = 0.5 * (lo + hi);
    let yc = ((r - rt) * (r - rt) - (r - s / 2.0) * (r - s / 2.0))
        .max(0.0)
        .sqrt();
    Some((rt, yc))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_curve(kind: ArchKind) {
        let s = 2.0;
        let c = arch_curve(kind, s);
        let first = c.segs[0].start();
        let last = c.segs.last().unwrap().end();
        assert!(dist(first, P2::new(-1.0, 0.0)) < 1e-12, "{kind:?} {first}");
        assert!(dist(last, P2::new(1.0, 0.0)) < 1e-12, "{kind:?} {last}");
        for w in c.segs.windows(2) {
            assert!(dist(w[0].end(), w[1].start()) < 1e-9, "{kind:?}");
        }
        let (l, _) = arch_window(kind, s, 1.0);
        assert!(l.is_ccw());
        assert!(!geom::clip::self_intersects(&l), "{kind:?}");
    }

    #[test]
    fn all_arches_are_closed() {
        for k in [
            ArchKind::Round,
            ArchKind::equilateral(),
            ArchKind::Pointed { rho: 1.3 },
            ArchKind::Pointed { rho: 0.7 },
            ArchKind::tudor(),
            ArchKind::ogee(),
        ] {
            check_curve(k);
        }
    }

    #[test]
    fn heights_match_formula() {
        let s = 2.0;
        assert!((arch_curve(ArchKind::equilateral(), s).height - 3f64.sqrt()).abs() < 1e-12);
        // ρ = 1,2 → h ≈ 0,975·s
        let h = arch_curve(ArchKind::Pointed { rho: 1.2 }, s).height / s;
        assert!((h - 0.975).abs() < 1e-3);
        assert!((arch_curve(ArchKind::Round, s).height - 1.0).abs() < 1e-12);
    }

    #[test]
    fn ogee_tangency_and_vertical_key() {
        let c = arch_curve(ArchKind::ogee(), 2.0);
        // raccord tangent entre courbe et contre-courbe
        let t1 = c.segs[0].tangent_end();
        let t2 = c.segs[1].tangent_start();
        assert!(dist(t1, t2) < 1e-9);
        // tangente verticale à la clé
        let tk = c.segs[1].tangent_end();
        assert!(tk.x.abs() < 1e-9);
    }
}
