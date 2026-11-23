//! Motifs flamboyants : mouchette et soufflets (§ 4.7).

use geom::constr::inter_circle_circle;
use geom::math::*;
use geom::{Circle, ConstructionEntity, ConstructionOp, Loop, Seg, Tol};

/// Mouchette : lunule D(O₁, R₁) ∖ D(O₂, R₂) dont une corne devient la queue
/// et l'autre extrémité est arrondie par un cercle de tête (h, ρ) tangent
/// intérieurement au grand cercle et extérieurement au petit :
///   |h − O₁| = R₁ − ρ,   |h − O₂| = R₂ + ρ,
/// avec ρ ≤ ρ_max = (R₁ − R₂ + |O₁O₂|)/2. `rho_frac` = ρ/ρ_max.
pub fn mouchette(
    o1: P2,
    r1: f64,
    o2: P2,
    r2: f64,
    rho_frac: f64,
) -> Option<(Loop, Vec<ConstructionEntity>)> {
    let d = dist(o1, o2);
    if !(d > (r1 - r2).abs() && d < r1 + r2 && r1 > r2) {
        return None;
    }
    let tol = Tol::new(r1 * 4.0);
    let c1 = Circle::new(o1, r1);
    let c2 = Circle::new(o2, r2);
    let horns = inter_circle_circle(&c1, &c2, &tol);
    if horns.len() != 2 {
        return None;
    }
    // point le plus épais de la lunule (sur C₁, à l'opposé de O₂)
    let f = o1 + unit(o1 - o2) * r1;
    let af = angle_of(f - o1);
    let a0 = angle_of(horns[0] - o1);
    let a1 = angle_of(horns[1] - o1);
    // queue : la corne d'où l'on atteint F avant l'autre corne en tournant
    // dans le sens trigonométrique
    let (tail, other) = if ccw_delta(a0, af) < ccw_delta(a0, a1) {
        (horns[0], horns[1])
    } else {
        (horns[1], horns[0])
    };
    let rho_max = (r1 - r2 + d) / 2.0;
    let rho = rho_frac.clamp(0.05, 0.999) * rho_max;
    let hs = inter_circle_circle(&Circle::new(o1, r1 - rho), &Circle::new(o2, r2 + rho), &tol);
    let side = |p: P2| cross(o2 - o1, p - o1).signum();
    let h = *hs
        .iter()
        .find(|p| side(**p) == side(other))
        .or(hs.first())?;
    let a1p = o1 + unit(h - o1) * r1;
    let a2p = o2 + unit(h - o2) * r2;
    let segs = vec![
        Seg::arc_from_to(o1, tail, a1p, true),
        Seg::arc_from_to(h, a1p, a2p, true),
        Seg::arc_from_to(o2, a2p, tail, false),
    ];
    let cons = vec![
        ConstructionEntity::Circle {
            c: o1,
            r: r1,
            op: ConstructionOp::Mouchette,
        },
        ConstructionEntity::Circle {
            c: o2,
            r: r2,
            op: ConstructionOp::Mouchette,
        },
        ConstructionEntity::Circle {
            c: h,
            r: rho,
            op: ConstructionOp::Mouchette,
        },
        ConstructionEntity::point(o1, ConstructionOp::Mouchette),
        ConstructionEntity::point(o2, ConstructionOp::Mouchette),
        ConstructionEntity::point(h, ConstructionOp::Mouchette),
    ];
    Some((Loop::new(segs), cons))
}

/// Soufflet en cœur allongé : deux arcs de rayon R passent par la pointe T
/// et sont tangents intérieurement au cercle de tête (h, ρ) ; le centre c
/// de chaque arc vérifie |c − T| = R et |c − h| = R − ρ.
pub fn soufflet_heart(tip: P2, h: P2, rho: f64, r: f64) -> Option<(Loop, Vec<ConstructionEntity>)> {
    let tol = Tol::new(r * 4.0);
    let cs = inter_circle_circle(&Circle::new(tip, r), &Circle::new(h, r - rho), &tol);
    if cs.len() != 2 || r <= rho {
        return None;
    }
    let u = unit(h - tip);
    // centre à gauche de l'axe → arc du côté droit
    let (cl, cr) = if cross(u, cs[0] - tip) > 0.0 {
        (cs[0], cs[1])
    } else {
        (cs[1], cs[0])
    };
    let a_right = cl + unit(h - cl) * r;
    let a_left = cr + unit(h - cr) * r;
    let segs = vec![
        Seg::arc_from_to(cl, tip, a_right, true),
        Seg::arc_from_to(h, a_right, a_left, true),
        Seg::arc_from_to(cr, a_left, tip, true),
    ];
    let cons = vec![
        ConstructionEntity::Circle {
            c: h,
            r: rho,
            op: ConstructionOp::Soufflet,
        },
        ConstructionEntity::point(cl, ConstructionOp::Soufflet),
        ConstructionEntity::point(cr, ConstructionOp::Soufflet),
        ConstructionEntity::point(h, ConstructionOp::Soufflet),
    ];
    let l = Loop::new(segs);
    if !l.is_ccw() {
        return None;
    }
    Some((l, cons))
}

/// Soufflet à deux pointes (0, ±H) : arcs convexes de centres (∓d, 0) et de
/// rayon R, raccordés à des contre-courbes de centres (±r₂, ±H) :
///   r₂ = (d² + H² − R²) / (2(R − d)),  R > d,  d² + H² > R².
/// Le motif est construit autour de l'origine, axe vertical.
pub fn soufflet_two_points(d: f64, r: f64, hh: f64) -> Option<(Loop, Vec<ConstructionEntity>)> {
    if !(r > d && d * d + hh * hh > r * r) {
        return None;
    }
    let r2 = (d * d + hh * hh - r * r) / (2.0 * (r - d));
    let cl = P2::new(-d, 0.0); // centre de l'arc convexe droit
    let cr = P2::new(d, 0.0); // centre de l'arc convexe gauche
    let q_br = P2::new(r2, -hh);
    let q_tr = P2::new(r2, hh);
    let q_tl = P2::new(-r2, hh);
    let q_bl = P2::new(-r2, -hh);
    let t_br = cl + unit(q_br - cl) * r;
    let t_tr = cl + unit(q_tr - cl) * r;
    let t_tl = cr + unit(q_tl - cr) * r;
    let t_bl = cr + unit(q_bl - cr) * r;
    let bot = P2::new(0.0, -hh);
    let top = P2::new(0.0, hh);
    let segs = vec![
        Seg::arc_from_to(q_br, bot, t_br, false),
        Seg::arc_from_to(cl, t_br, t_tr, true),
        Seg::arc_from_to(q_tr, t_tr, top, false),
        Seg::arc_from_to(q_tl, top, t_tl, false),
        Seg::arc_from_to(cr, t_tl, t_bl, true),
        Seg::arc_from_to(q_bl, t_bl, bot, false),
    ];
    let mut cons = Vec::new();
    for c in [cl, cr, q_br, q_tr, q_tl, q_bl] {
        cons.push(ConstructionEntity::point(c, ConstructionOp::Soufflet));
    }
    Some((Loop::new(segs), cons))
}

/// Courbe en S de la double mouchette arrondie, inscrite dans le cercle
/// (o, r) : deux demi-cercles de rayons a·r et (1 − a)·r centrés sur le
/// diamètre de direction `theta`. Chacun est tangent intérieurement au grand
/// cercle à une extrémité du diamètre (P = o − r·u, Q = o + r·u), et ils se
/// raccordent tangentiellement en J = P + 2a·r·u : les barreaux naissent
/// tangentiellement de la forme englobante.
pub fn s_curve_round(o: P2, r: f64, theta: f64, a: f64) -> [Seg; 2] {
    let u = dir(theta);
    let p = o - u * r;
    let r1 = a.clamp(0.05, 0.95) * r;
    let r2 = r - r1;
    let c1 = p + u * r1;
    let c2 = p + u * (2.0 * r1 + r2);
    [
        Seg::arc(c1, r1, theta + PI, -PI),
        Seg::arc(c2, r2, theta + PI, PI),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouchette_tangencies() {
        let (o1, r1, o2, r2) = (P2::new(0.0, 0.0), 3.0, P2::new(1.0, 0.8), 2.4);
        let (l, cons) = mouchette(o1, r1, o2, r2, 0.95).expect("mouchette");
        assert!(l.is_ccw());
        assert!(!geom::clip::self_intersects(&l));
        let h = match cons[2] {
            ConstructionEntity::Circle { c, r, .. } => (c, r),
            _ => unreachable!(),
        };
        assert!((dist(h.0, o1) - (r1 - h.1)).abs() < 1e-9);
        assert!((dist(h.0, o2) - (r2 + h.1)).abs() < 1e-9);
        // raccords G1 tête / côtés
        assert_eq!(l.joints[1], geom::Joint::Tangent);
        assert_eq!(l.joints[2], geom::Joint::Tangent);
    }

    #[test]
    fn soufflet_heart_ok() {
        let (l, _) =
            soufflet_heart(P2::new(0.0, -3.0), P2::new(0.0, 1.0), 1.0, 4.0).expect("soufflet");
        assert!(l.is_ccw());
        assert!(!geom::clip::self_intersects(&l));
        assert_eq!(l.joints[1], geom::Joint::Tangent);
    }

    #[test]
    fn soufflet_two_points_ok() {
        let (l, _) = soufflet_two_points(0.6, 1.2, 2.0).expect("soufflet");
        assert!(l.is_ccw(), "{}", l.signed_area());
        assert!(!geom::clip::self_intersects(&l));
        for j in &l.joints[1..3] {
            assert_eq!(*j, geom::Joint::Tangent);
        }
    }
    #[test]
    fn s_curve_round_g1() {
        let [a1, a2] = s_curve_round(P2::new(1.0, 2.0), 3.0, 0.7, 0.4);
        assert!(dist(a1.end(), a2.start()) < 1e-12);
        assert!(dist(a1.tangent_end(), a2.tangent_start()) < 1e-12);
        // tangence intérieure au grand cercle aux extrémités
        let o = P2::new(1.0, 2.0);
        assert!((dist(a1.start(), o) - 3.0).abs() < 1e-12);
        assert!((dist(a2.end(), o) - 3.0).abs() < 1e-12);
        assert!(dot(a1.tangent_start(), a1.start() - o).abs() < 1e-12);
    }
}
