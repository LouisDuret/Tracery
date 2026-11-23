//! Adaptateur décalages / booléens au-dessus de `cavalier_contours` (§ 4.2).
//!
//! La bibliothèque traite nativement les arcs (représentation « bulge »,
//! b = tan(sweep/4)) avec la limite |bulge| ≤ 1 : les arcs plus longs qu'un
//! demi-cercle sont scindés à la conversion et refusionnés au retour.
//! Toute la dépendance est confinée dans ce module.

use crate::math::*;
use crate::seg::{BBox, Loop, Region, Seg};
use cavalier_contours::polyline::{
    BooleanOp, BooleanResultInfo, PlineSource, PlineSourceMut, PlineVertex, Polyline,
};
use cavalier_contours::shape_algorithms::{Shape, ShapeOffsetOptions};

/// Aire minimale (relative à L²) en dessous de laquelle une pièce est jetée.
const MIN_AREA_REL: f64 = 1e-10;

/// Conversion boucle → polyligne fermée à bulges.
pub fn loop_to_pline(l: &Loop) -> Polyline<f64> {
    let mut p = Polyline::new_closed();
    for s in &l.segs {
        for piece in s.split_arc(PI) {
            let st = piece.start();
            let bulge = match piece {
                Seg::Line { .. } => 0.0,
                Seg::Arc { sweep, .. } => tan(sweep / 4.0),
            };
            p.add(st.x, st.y, bulge);
        }
    }
    p
}

/// Conversion polyligne → boucle ; les arcs scindés de même centre et même
/// rayon sont refusionnés.
pub fn pline_to_loop(p: &Polyline<f64>) -> Loop {
    let n = p.vertex_count();
    let mut segs: Vec<Seg> = Vec::with_capacity(n);
    for i in 0..n {
        let v1 = p.at(i);
        let v2 = p.at((i + 1) % n);
        let a = P2::new(v1.x, v1.y);
        let b = P2::new(v2.x, v2.y);
        if dist(a, b) < 1e-12 {
            continue;
        }
        let s = bulge_seg(a, b, v1.bulge);
        if let Some(last) = segs.last_mut()
            && let Some(m) = merge(last, &s)
        {
            *last = m;
            continue;
        }
        segs.push(s);
    }
    if segs.len() > 1 {
        let first = segs[0];
        let last = *segs.last().unwrap_or(&first);
        if let Some(m) = merge(&last, &first) {
            segs.pop();
            segs[0] = m;
        }
    }
    Loop::new(segs)
}

fn bulge_seg(a: P2, b: P2, bulge: f64) -> Seg {
    if bulge.abs() < 1e-12 {
        return Seg::line(a, b);
    }
    let c = dist(a, b);
    let u = (b - a) / c;
    let m = (a + b) * 0.5;
    let d = 0.5 * c * (1.0 - bulge * bulge) / (2.0 * bulge);
    let center = m + perp(u) * d;
    let sweep = 4.0 * atan(bulge);
    let r = 0.5 * (dist(center, a) + dist(center, b));
    Seg::Arc {
        c: center,
        r,
        theta0: angle_of(a - center),
        sweep,
    }
}

fn merge(a: &Seg, b: &Seg) -> Option<Seg> {
    match (*a, *b) {
        (
            Seg::Arc {
                c: c1,
                r: r1,
                theta0: t1,
                sweep: s1,
            },
            Seg::Arc {
                c: c2,
                r: r2,
                sweep: s2,
                ..
            },
        ) => {
            let tol = 1e-9 * r1.max(1.0);
            if dist(c1, c2) < tol
                && (r1 - r2).abs() < tol
                && s1 * s2 > 0.0
                && (s1 + s2).abs() <= TAU + 1e-9
            {
                Some(Seg::Arc {
                    c: c1,
                    r: r1,
                    theta0: t1,
                    sweep: s1 + s2,
                })
            } else {
                None
            }
        }
        (Seg::Line { a: a1, b: b1 }, Seg::Line { a: _, b: b2 }) => {
            let d1 = unit(b1 - a1);
            let d2 = unit(b2 - b1);
            if cross(d1, d2).abs() < 1e-12 && dot(d1, d2) > 0.0 {
                Some(Seg::line(a1, b2))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn ccw_pline(l: &Loop) -> Polyline<f64> {
    loop_to_pline(&l.oriented(true))
}

fn shape_opts(scale: f64) -> ShapeOffsetOptions<f64> {
    let mut o = ShapeOffsetOptions::new();
    o.pos_equal_eps = 1e-5 * scale;
    o.offset_dist_eps = 1e-4 * scale;
    o.slice_join_eps = 1e-4 * scale;
    o
}

fn ref_scale(bb: &BBox) -> f64 {
    (bb.diag() / 100.0).max(1e-6)
}

/// Assigne des trous (boucles horaires) à des contours extérieurs.
fn assemble(outers: Vec<Loop>, holes: Vec<Loop>, min_area: f64) -> Vec<Region> {
    let mut regs: Vec<Region> = outers
        .into_iter()
        .filter(|o| o.area() > min_area)
        .map(Region::simple)
        .collect();
    // les plus petits contours d'abord pour l'affectation
    for h in holes {
        if h.area() <= min_area {
            continue;
        }
        let p = h.segs[0].point_at(0.5);
        let mut best: Option<(usize, f64)> = None;
        for (i, r) in regs.iter().enumerate() {
            if r.outer.contains(p) {
                let a = r.outer.area();
                if best.is_none_or(|(_, ba)| a < ba) {
                    best = Some((i, a));
                }
            }
        }
        if let Some((i, _)) = best {
            regs[i].holes.push(h.oriented(false));
        }
    }
    regs
}

/// Décalage d'une région : `d > 0` rétrécit (offset intérieur), `d < 0`
/// dilate. Les arcs restent des arcs (offset concentrique).
pub fn offset_region(r: &Region, d: f64) -> Vec<Region> {
    if d == 0.0 {
        return vec![r.clone()];
    }
    let bb = r.bbox();
    let sc = ref_scale(&bb);
    let mut plines = vec![loop_to_pline(&r.outer.oriented(true))];
    for h in &r.holes {
        plines.push(loop_to_pline(&h.oriented(false)));
    }
    let shape = Shape::from_plines(plines);
    let res = shape.parallel_offset(d, &shape_opts(sc));
    let outers: Vec<Loop> = res
        .ccw_plines
        .iter()
        .map(|p| pline_to_loop(&p.polyline))
        .collect();
    let holes: Vec<Loop> = res
        .cw_plines
        .iter()
        .map(|p| pline_to_loop(&p.polyline))
        .collect();
    let min_area = MIN_AREA_REL * (bb.diag() + d.abs()).powi(2);
    assemble(outers, holes, min_area)
}

/// Décalage d'un ensemble de régions.
pub fn offset_all(rs: &[Region], d: f64) -> Vec<Region> {
    rs.iter().flat_map(|r| offset_region(r, d)).collect()
}

fn bool_pl(
    a: &Polyline<f64>,
    b: &Polyline<f64>,
    op: BooleanOp,
) -> (BooleanResultInfo, Vec<Loop>, Vec<Loop>) {
    let res = a.boolean(b, op);
    let pos = res
        .pos_plines
        .iter()
        .map(|p| pline_to_loop(&p.pline))
        .filter(|l| !l.segs.is_empty())
        .collect();
    let neg = res
        .neg_plines
        .iter()
        .map(|p| pline_to_loop(&p.pline))
        .filter(|l| !l.segs.is_empty())
        .collect();
    (res.result_info, pos, neg)
}

/// Retire une boucle simple d'un ensemble de régions.
pub fn subtract_loop(regions: &[Region], cutter: &Loop) -> Vec<Region> {
    let cbb = cutter.bbox();
    let mut out = Vec::new();
    for r in regions {
        let rbb = r.bbox();
        if !rbb.overlaps(&cbb, 0.0) {
            out.push(r.clone());
            continue;
        }
        let min_area = MIN_AREA_REL * rbb.diag().powi(2);
        // fusion du cutter avec les trous qu'il chevauche
        let mut cut = ccw_pline(cutter);
        let mut rest_holes: Vec<Loop> = Vec::new();
        let mut inside_hole = false;
        for h in &r.holes {
            if !h.bbox().overlaps(&cutter.bbox(), 0.0) {
                rest_holes.push(h.clone());
                continue;
            }
            let hp = ccw_pline(h);
            let (info, pos, _) = bool_pl(&hp, &cut, BooleanOp::Or);
            match info {
                BooleanResultInfo::Disjoint => rest_holes.push(h.clone()),
                BooleanResultInfo::Pline1InsidePline2 => {}
                BooleanResultInfo::Pline2InsidePline1 => {
                    inside_hole = true;
                    rest_holes.push(h.clone());
                }
                _ => {
                    if let Some(big) = pos.into_iter().max_by(|a, b| a.area().total_cmp(&b.area()))
                    {
                        cut = ccw_pline(&big);
                    }
                }
            }
        }
        if inside_hole {
            out.push(r.clone());
            continue;
        }
        let op = ccw_pline(&r.outer);
        let (info, pos, neg) = bool_pl(&op, &cut, BooleanOp::Not);
        match info {
            BooleanResultInfo::Disjoint => {
                out.push(Region::new(r.outer.clone(), rest_holes));
            }
            BooleanResultInfo::Pline1InsidePline2 => {}
            BooleanResultInfo::Pline2InsidePline1 => {
                let mut holes = rest_holes;
                holes.push(pline_to_loop(&cut).oriented(false));
                out.push(Region::new(r.outer.clone(), holes));
            }
            BooleanResultInfo::InvalidInput => out.push(r.clone()),
            _ => {
                let mut holes: Vec<Loop> = neg;
                holes.extend(rest_holes);
                out.extend(assemble(pos, holes, min_area));
            }
        }
    }
    out
}

/// Retire une région (avec trous éventuels) d'un ensemble de régions.
pub fn subtract(regions: &[Region], cutter: &Region) -> Vec<Region> {
    let mut res = subtract_loop(regions, &cutter.outer);
    if !cutter.holes.is_empty() {
        for h in &cutter.holes {
            let back = intersect_loop(regions, h);
            res.extend(back);
        }
    }
    res
}

/// Intersection d'un ensemble de régions avec une boucle simple.
pub fn intersect_loop(regions: &[Region], clip: &Loop) -> Vec<Region> {
    let cbb = clip.bbox();
    let cp = ccw_pline(clip);
    let mut out = Vec::new();
    for r in regions {
        if !r.bbox().overlaps(&cbb, 0.0) {
            continue;
        }
        let min_area = MIN_AREA_REL * r.bbox().diag().powi(2);
        let op = ccw_pline(&r.outer);
        let (info, pos, neg) = bool_pl(&op, &cp, BooleanOp::And);
        let pieces: Vec<Region> = match info {
            BooleanResultInfo::Disjoint | BooleanResultInfo::InvalidInput => vec![],
            BooleanResultInfo::Pline1InsidePline2 => vec![Region::simple(r.outer.clone())],
            BooleanResultInfo::Pline2InsidePline1 => vec![Region::simple(clip.oriented(true))],
            _ => assemble(pos, neg, min_area),
        };
        let mut pieces = pieces;
        for h in &r.holes {
            pieces = subtract_loop(&pieces, h);
        }
        out.extend(pieces);
    }
    out
}

/// Intersection de deux régions.
pub fn intersect(a: &[Region], b: &Region) -> Vec<Region> {
    let mut res = intersect_loop(a, &b.outer);
    for h in &b.holes {
        res = subtract_loop(&res, h);
    }
    res
}

/// Union d'un ensemble de régions avec une boucle simple.
pub fn union_loop(regions: &[Region], add: &Loop) -> Vec<Region> {
    let abb = add.bbox();
    let mut merged = Region::simple(add.oriented(true));
    let mut out = Vec::new();
    for r in regions {
        if !r.bbox().overlaps(&merged.bbox(), 0.0) {
            out.push(r.clone());
            continue;
        }
        let rp = ccw_pline(&r.outer);
        let mp = ccw_pline(&merged.outer);
        let (info, pos, neg) = bool_pl(&rp, &mp, BooleanOp::Or);
        match info {
            BooleanResultInfo::Disjoint | BooleanResultInfo::InvalidInput => out.push(r.clone()),
            BooleanResultInfo::Pline1InsidePline2 => {
                // r est contenue dans le contour de merged : elle y est
                // absorbée (ses trous disparaissent), sauf si elle tombe
                // tout entière dans un trou de merged, où c'est une île
                if inside_some_hole(&merged.holes, &rp) {
                    out.push(r.clone());
                }
            }
            BooleanResultInfo::Pline2InsidePline1 if inside_some_hole(&r.holes, &mp) => {
                // merged tombe tout entière dans un trou de r : c'est une
                // île, disjointe de r (sinon elle disparaîtrait)
                out.push(r.clone());
            }
            BooleanResultInfo::Pline2InsidePline1 => {
                // merged dans r : on retire merged des trous de r
                let mut holes = Vec::new();
                for h in &r.holes {
                    let hp = ccw_pline(h);
                    let (hi, hpos, _) = bool_pl(&hp, &mp, BooleanOp::Not);
                    match hi {
                        BooleanResultInfo::Disjoint | BooleanResultInfo::InvalidInput => {
                            holes.push(h.clone())
                        }
                        BooleanResultInfo::Pline1InsidePline2 => {}
                        BooleanResultInfo::Pline2InsidePline1 => holes.push(h.clone()),
                        _ => holes.extend(hpos.into_iter().map(|l| l.oriented(false))),
                    }
                }
                holes.extend(merged.holes.iter().cloned());
                merged = Region::new(r.outer.clone(), holes);
            }
            _ => {
                let outer = pos
                    .into_iter()
                    .max_by(|a, b| a.area().total_cmp(&b.area()))
                    .unwrap_or_else(|| r.outer.clone());
                let mut holes: Vec<Loop> = neg;
                // un trou de r reste trou hors de merged, et un trou de
                // merged hors de r (et non hors de merged lui-même : il y
                // serait toujours inclus et disparaîtrait)
                let other = |i: usize| if i < r.holes.len() { &mp } else { &rp };
                for (i, h) in r.holes.iter().chain(merged.holes.iter()).enumerate() {
                    let hp = ccw_pline(h);
                    let (hi, hpos, _) = bool_pl(&hp, other(i), BooleanOp::Not);
                    match hi {
                        BooleanResultInfo::Disjoint | BooleanResultInfo::InvalidInput => {
                            holes.push(h.clone())
                        }
                        BooleanResultInfo::Pline1InsidePline2 => {}
                        BooleanResultInfo::Pline2InsidePline1 => holes.push(h.clone()),
                        _ => holes.extend(hpos),
                    }
                }
                merged = Region::new(outer, holes);
            }
        }
    }
    let _ = abb;
    out.push(merged);
    out
}

/// La polyligne `p` est-elle tout entière dans l'un des trous `holes` ?
fn inside_some_hole(holes: &[Loop], p: &Polyline<f64>) -> bool {
    holes.iter().any(|h| {
        matches!(
            bool_pl(&ccw_pline(h), p, BooleanOp::Not).0,
            BooleanResultInfo::Pline2InsidePline1
        )
    })
}

/// Union d'un ensemble de régions avec une région.
pub fn union(regions: &[Region], add: &Region) -> Vec<Region> {
    let mut res = union_loop(regions, &add.outer);
    for h in &add.holes {
        // les trous de `add` ne sont conservés que s'ils ne sont pas couverts
        let covered = regions.iter().any(|r| r.contains(h.segs[0].point_at(0.5)));
        if !covered {
            res = subtract_loop(&res, h);
        }
    }
    res
}

/// La boucle s'auto-intersecte-t-elle ?
pub fn self_intersects(l: &Loop) -> bool {
    let p = loop_to_pline(l);
    p.scan_for_self_intersect()
}

/// Inverse le sens d'une polyligne (utilitaire de test).
pub fn reversed_pline(p: &Polyline<f64>) -> Polyline<f64> {
    let mut q = p.clone();
    q.invert_direction_mut();
    q
}

#[allow(dead_code)]
fn vertex(x: f64, y: f64, b: f64) -> PlineVertex<f64> {
    PlineVertex::new(x, y, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(c: P2, h: f64) -> Loop {
        Loop::polygon(&[
            c + P2::new(-h, -h),
            c + P2::new(h, -h),
            c + P2::new(h, h),
            c + P2::new(-h, h),
        ])
    }

    #[test]
    fn roundtrip_circle() {
        let l = Loop::circle(P2::new(1.0, 2.0), 3.0);
        let p = loop_to_pline(&l);
        let back = pline_to_loop(&p);
        assert!((back.area() - l.area()).abs() < 1e-9);
        assert_eq!(back.segs.len(), 1);
    }

    #[test]
    fn offset_square_with_hole() {
        let r = Region::new(square(P2::ZERO, 10.0), vec![square(P2::ZERO, 2.0)]);
        let o = offset_region(&r, 1.0);
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].holes.len(), 1);
        // aire : 18² − aire(carré 4 dilaté de 1 avec coins arrondis)
        let hole = 16.0 + 4.0 * 4.0 + PI;
        assert!(
            (o[0].area() - (324.0 - hole)).abs() < 1e-6,
            "{}",
            o[0].area()
        );
    }

    #[test]
    fn subtract_circle_inside_gives_hole() {
        let r = vec![Region::simple(square(P2::ZERO, 10.0))];
        let c = Loop::circle(P2::ZERO, 2.0);
        let res = subtract_loop(&r, &c);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].holes.len(), 1);
        assert!((res[0].area() - (400.0 - 4.0 * PI)).abs() < 1e-6);
    }

    #[test]
    fn subtract_band_splits() {
        let r = vec![Region::simple(square(P2::ZERO, 10.0))];
        let band = Loop::polygon(&[
            P2::new(-1.0, -20.0),
            P2::new(1.0, -20.0),
            P2::new(1.0, 20.0),
            P2::new(-1.0, 20.0),
        ]);
        let res = subtract_loop(&r, &band);
        assert_eq!(res.len(), 2);
        let tot: f64 = res.iter().map(|r| r.area()).sum();
        assert!((tot - 360.0).abs() < 1e-9);
    }

    #[test]
    fn area_identity_union_inter() {
        let a = Region::circle(P2::ZERO, 3.0);
        let b = Loop::circle(P2::new(2.0, 0.0), 2.0);
        let u: f64 = union_loop(std::slice::from_ref(&a), &b)
            .iter()
            .map(|r| r.area())
            .sum();
        let i: f64 = intersect_loop(std::slice::from_ref(&a), &b)
            .iter()
            .map(|r| r.area())
            .sum();
        assert!((u - (a.area() + b.area() - i)).abs() < 1e-6);
    }
}
