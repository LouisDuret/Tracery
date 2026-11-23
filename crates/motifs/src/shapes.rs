//! Primitives 2D (§ 3.2, § 4.8) : toutes construites à la règle et au compas.

use geom::constr::{biarc, inter_circle_circle, tangents_point_circle};
use geom::math::*;
use geom::{Circle, Loop, Region, Seg, Tol};

pub fn rectangle(c: P2, w: f64, h: f64) -> Loop {
    let (a, b) = (w / 2.0, h / 2.0);
    Loop::polygon(&[
        c + P2::new(-a, -b),
        c + P2::new(a, -b),
        c + P2::new(a, b),
        c + P2::new(-a, b),
    ])
}

/// Polygone régulier à n côtés, un sommet en haut.
pub fn regular_polygon(c: P2, r: f64, n: usize) -> Loop {
    let pts: Vec<P2> = (0..n)
        .map(|i| c + dir(FRAC_PI_2 + TAU * i as f64 / n as f64) * r)
        .collect();
    Loop::polygon(&pts)
}

/// Triangle curviligne : côtés en arcs de rayon ρ·s centrés sur la
/// médiatrice de chaque côté, du côté du sommet opposé (ρ = 1 : triangle
/// de Reuleaux, centres aux sommets). Un sommet en haut.
pub fn curved_triangle(c: P2, s: f64, rho: f64) -> Loop {
    let rc = s / 3f64.sqrt();
    let v: Vec<P2> = (0..3)
        .map(|i| c + dir(FRAC_PI_2 + TAU * i as f64 / 3.0) * rc)
        .collect();
    let r = (rho * s).max(s / 2.0 + 1e-12);
    let segs = (0..3)
        .map(|i| {
            let a = v[(i + 1) % 3];
            let b = v[(i + 2) % 3];
            let m = (a + b) * 0.5;
            let toward = unit(v[i] - m);
            let off = (r * r - s * s / 4.0).max(0.0).sqrt();
            let center = m + toward * off;
            Seg::arc_from_to(center, a, b, true)
        })
        .collect();
    Loop::new(segs)
}

/// Vesica piscis verticale : D(A, R) ∩ D(B, R), |AB| = R.
pub fn vesica(c: P2, r: f64) -> Loop {
    let a = c + P2::new(-r / 2.0, 0.0);
    let b = c + P2::new(r / 2.0, 0.0);
    let hh = r * 3f64.sqrt() / 2.0;
    let top = c + P2::new(0.0, hh);
    let bot = c + P2::new(0.0, -hh);
    Loop::new(vec![
        Seg::arc_from_to(a, bot, top, true),
        Seg::arc_from_to(b, top, bot, true),
    ])
}

/// Lunule (croissant) D(O₁, R₁) ∖ D(O₂, R₂).
pub fn lunule(o1: P2, r1: f64, o2: P2, r2: f64) -> Option<Loop> {
    let tol = Tol::new(r1 * 4.0);
    let h = inter_circle_circle(&Circle::new(o1, r1), &Circle::new(o2, r2), &tol);
    if h.len() != 2 {
        return None;
    }
    let f = o1 + unit(o1 - o2) * r1;
    let (af, a0, a1) = (angle_of(f - o1), angle_of(h[0] - o1), angle_of(h[1] - o1));
    let (p, q) = if ccw_delta(a0, af) < ccw_delta(a0, a1) {
        (h[0], h[1])
    } else {
        (h[1], h[0])
    };
    Some(Loop::new(vec![
        Seg::arc_from_to(o1, p, q, true),
        Seg::arc_from_to(o2, q, p, false),
    ]))
}

/// Ovale à quatre centres de demi-axes a > b, petit rayon r < b :
/// R = (a² + b² − 2ar) / (2(b − r)).
pub fn oval4(c: P2, a: f64, b: f64, r: f64) -> Loop {
    let big = (a * a + b * b - 2.0 * a * r) / (2.0 * (b - r));
    let sr = c + P2::new(a - r, 0.0);
    let sl = c + P2::new(-(a - r), 0.0);
    let bt = c + P2::new(0.0, -(big - b));
    let bb = c + P2::new(0.0, big - b);
    let j_tr = bt + unit(sr - bt) * big;
    let j_tl = bt + unit(sl - bt) * big;
    let j_bl = bb + unit(sl - bb) * big;
    let j_br = bb + unit(sr - bb) * big;
    Loop::new(vec![
        Seg::arc_from_to(sr, j_br, j_tr, true),
        Seg::arc_from_to(bt, j_tr, j_tl, true),
        Seg::arc_from_to(sl, j_tl, j_bl, true),
        Seg::arc_from_to(bb, j_bl, j_br, true),
    ])
}

/// Couronne : cercle avec trou concentrique.
pub fn annulus(c: P2, r_out: f64, r_in: f64) -> Region {
    Region::new(Loop::circle(c, r_out), vec![Loop::circle(c, r_in)])
}

/// Secteur annulaire entre les angles a0 et a1 (trigonométrique).
pub fn annular_sector(c: P2, r_in: f64, r_out: f64, a0: f64, a1: f64) -> Loop {
    let sw = ccw_delta(a0, a1);
    Loop::new(vec![
        Seg::line(c + dir(a0) * r_in, c + dir(a0) * r_out),
        Seg::arc(c, r_out, a0, sw),
        Seg::line(c + dir(a1) * r_out, c + dir(a1) * r_in),
        Seg::arc(c, r_in, a1, -sw),
    ])
}

/// Courbe fermée lisse (G1) passant par des points, par biarcs ; tangentes
/// de type Catmull-Rom.
pub fn smooth_closed(pts: &[P2]) -> Loop {
    let n = pts.len();
    let tangents: Vec<P2> = (0..n)
        .map(|i| unit(pts[(i + 1) % n] - pts[(i + n - 1) % n]))
        .collect();
    let mut segs = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        segs.extend(biarc(pts[i], tangents[i], pts[j], tangents[j]));
    }
    let l = Loop::new(segs);
    l.oriented(true)
}

/// Larme : cercle (c, r) prolongé par deux tangentes vers la pointe `tip`.
pub fn teardrop(c: P2, r: f64, tip: P2) -> Option<Loop> {
    let t = tangents_point_circle(tip, &Circle::new(c, r));
    if t.len() != 2 {
        return None;
    }
    // ordre trigonométrique : pointe → contact droit → arc → contact gauche
    let (p1, p2) = if cross(c - tip, t[0] - tip) < 0.0 {
        (t[0], t[1])
    } else {
        (t[1], t[0])
    };
    Some(
        Loop::new(vec![
            Seg::line(tip, p1),
            Seg::arc_from_to(c, p1, p2, true),
            Seg::line(p2, tip),
        ])
        .oriented(true),
    )
}

/// Contour polylobé (n lobes sur un cercle), utile comme forme d'entrée.
pub fn foil_outline(c: P2, ri: f64, n: usize, k: f64) -> Loop {
    crate::foils::Foil {
        c,
        ri,
        n,
        k,
        phase: FRAC_PI_2,
    }
    .to_loop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_are_valid() {
        let loops = vec![
            rectangle(P2::ZERO, 4.0, 2.0),
            regular_polygon(P2::ZERO, 2.0, 6),
            curved_triangle(P2::ZERO, 3.0, 1.0),
            curved_triangle(P2::ZERO, 3.0, 1.4),
            vesica(P2::ZERO, 2.0),
            lunule(P2::ZERO, 3.0, P2::new(1.0, 0.5), 2.6).unwrap(),
            oval4(P2::ZERO, 3.0, 2.0, 1.0),
            annular_sector(P2::ZERO, 1.0, 2.0, 0.2, 1.4),
            smooth_closed(&[
                P2::new(0.0, 0.0),
                P2::new(3.0, -0.5),
                P2::new(4.0, 1.5),
                P2::new(1.0, 2.5),
            ]),
            teardrop(P2::ZERO, 1.0, P2::new(0.0, -5.0)).unwrap(),
        ];
        for (i, l) in loops.iter().enumerate() {
            assert!(l.is_ccw(), "primitive {i}");
            assert!(!geom::clip::self_intersects(l), "primitive {i}");
            for (k, s) in l.segs.iter().enumerate() {
                let nx = &l.segs[(k + 1) % l.segs.len()];
                assert!(dist(s.end(), nx.start()) < 1e-9, "primitive {i} seg {k}");
            }
        }
    }

    #[test]
    fn reuleaux_area() {
        // aire du triangle de Reuleaux : (π − √3)·s²/2
        let s = 2.0;
        let l = curved_triangle(P2::ZERO, s, 1.0);
        assert!((l.area() - (PI - 3f64.sqrt()) * s * s / 2.0).abs() < 1e-9);
    }

    #[test]
    fn oval_is_g1() {
        let l = oval4(P2::ZERO, 3.0, 2.0, 1.0);
        assert!(l.joints.iter().all(|j| *j == geom::Joint::Tangent));
    }
}
