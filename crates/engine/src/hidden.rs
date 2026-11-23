//! Parties vues d'une porte (ADR-24) : ordre de peinture des ornements et
//! traits débarrassés des parties cachées.
//!
//! Chaque relief a une profondeur `(niveau, rang)` : un niveau plus haut
//! est plus proche de l'observateur, et à niveau égal l'ornement posé après
//! passe devant (le chou d'un crochet devant la flèche, le nœud de l'épi
//! devant sa tige). Un creux est taillé dans le relief qui le porte (son
//! hôte, le relief le plus en avant sous lui) et prend la profondeur de cet
//! hôte, juste devant lui.
//!
//! Un point du contour d'un ornement est caché dès qu'il tombe strictement
//! dans un relief plus en avant : les tracés ne se croisent plus. Les bords
//! communs restent vus, puisqu'ils sont au bord du relief de devant.

use crate::design::{Design, OrnKind};
use geom::math::*;
use geom::{BBox, Region};

/// Dessin d'une porte : ordre de peinture des ornements (du fond vers
/// l'avant) et traits vus (polylignes, unités du document).
#[derive(Clone, Debug, Default)]
pub struct DoorDrawing {
    pub order: Vec<usize>,
    pub lines: Vec<Vec<P2>>,
}

/// Rang d'un relief : niveau, puis ordre de pose.
fn relief_key(level: u8, i: usize) -> f64 {
    level.min(3) as f64 * 1e6 + i as f64
}

/// Profondeur du relief le plus en avant qui contient strictement `p`.
fn top_key(reliefs: &[(f64, &Region, BBox)], p: P2, eps: f64) -> f64 {
    reliefs
        .iter()
        .rev()
        .find(|(_, r, bb)| bb.dist(p) <= 0.0 && r.sdf(p) > eps)
        .map_or(-1.0, |(k, _, _)| *k)
}

/// Points intérieurs d'une région, juste en deçà de son contour.
fn inner_samples(r: &Region, n: usize, h: f64) -> Vec<P2> {
    let l = &r.outer;
    let len = l.length();
    let mut v = Vec::new();
    for k in 0..n {
        let (p, t) = l.point_at_length(len * (k as f64 + 0.5) / n as f64);
        let nrm = perp(unit(t));
        let mut d = h;
        for _ in 0..6 {
            let q = if r.sdf(p + nrm * d) > r.sdf(p - nrm * d) {
                p + nrm * d
            } else {
                p - nrm * d
            };
            if r.sdf(q) > 0.3 * d {
                v.push(q);
                break;
            }
            d *= 0.4;
        }
    }
    v
}

/// Ordre de peinture et traits vus des ornements de la porte `d`.
pub fn door_drawing(d: &Design) -> DoorDrawing {
    let orn = &d.ornaments;
    if orn.is_empty() {
        return DoorDrawing::default();
    }
    let diag = d.bbox().diag().max(1e-9);
    let eps = 2e-5 * diag;
    // pas des tests de visibilité, tolérance de corde des arcs
    let test_step = diag / 1500.0;
    let tol = 4e-5 * diag;

    // reliefs rangés du fond vers l'avant
    let mut reliefs: Vec<(f64, &Region, BBox)> = orn
        .iter()
        .enumerate()
        .filter(|(_, o)| o.kind == OrnKind::Relief)
        .map(|(i, o)| (relief_key(o.level, i), &o.region, o.region.bbox()))
        .collect();
    reliefs.sort_by(|a, b| a.0.total_cmp(&b.0));

    // profondeur de chaque ornement ; un creux prend celle de son hôte
    let keys: Vec<f64> = orn
        .iter()
        .enumerate()
        .map(|(i, o)| match o.kind {
            OrnKind::Relief => relief_key(o.level, i),
            OrnKind::Creux => {
                let host = inner_samples(&o.region, 12, 2e-3 * diag)
                    .into_iter()
                    .map(|p| top_key(&reliefs, p, eps))
                    .fold(f64::INFINITY, f64::min);
                let host = if host.is_finite() { host } else { -1.0 };
                host + 0.5 + i as f64 * 1e-9
            }
        })
        .collect();
    let mut order: Vec<usize> = (0..orn.len()).collect();
    order.sort_by(|&a, &b| keys[a].total_cmp(&keys[b]));

    // chaque ornement indépendamment (en parallèle), dans l'ordre
    let per: Vec<Vec<Vec<P2>>> = par_map(orn.len(), |i| {
        let o = &orn[i];
        let k = keys[i];
        let bb = o.region.bbox();
        let front: Vec<&(f64, &Region, BBox)> = reliefs
            .iter()
            .filter(|(kj, _, bj)| *kj > k && bj.overlaps(&bb, eps))
            .collect();
        let hidden = |p: P2| {
            front
                .iter()
                .any(|(_, r, bj)| bj.dist(p) <= 0.0 && r.contains(p) && r.boundary_dist(p) > eps)
        };
        visible_polylines(&o.region, &hidden, test_step, tol, diag)
    });
    let lines: Vec<Vec<P2>> = per.into_iter().flatten().collect();
    DoorDrawing { order, lines }
}

/// `f(0..n)` en parallèle (ordre conservé).
fn par_map<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync + Send) -> Vec<T> {
    use rayon::prelude::*;
    (0..n).into_par_iter().map(f).collect()
}

/// Polylignes vues du contour de `r` : la visibilité est testée au pas
/// `step` et chaque frontière vue / cachée est affinée par dichotomie. Un
/// segment vu ne garde que ses extrémités ; un arc est découpé à la
/// tolérance de corde `tol`.
fn visible_polylines(
    r: &Region,
    hidden: &impl Fn(P2) -> bool,
    step: f64,
    tol: f64,
    diag: f64,
) -> Vec<Vec<P2>> {
    let mut lines: Vec<Vec<P2>> = Vec::new();
    for l in r.loops() {
        let mut cur: Vec<P2> = Vec::new();
        let flush = |cur: &mut Vec<P2>, lines: &mut Vec<Vec<P2>>| {
            if cur.len() >= 2 {
                lines.push(std::mem::take(cur));
            }
            cur.clear();
        };
        for s in &l.segs {
            let n = ((s.length() / step).ceil() as usize).max(if s.is_arc() { 4 } else { 1 });
            // intervalles vus [t0, t1] du segment
            let mut spans: Vec<(f64, f64)> = Vec::new();
            let mut start: Option<f64> = None;
            let mut prev = (0.0, false);
            for j in 0..=n {
                let t = j as f64 / n as f64;
                let vis = !hidden(s.point_at(t));
                let edge = if j > 0 && prev.1 != vis {
                    let (mut a, mut b) = (prev.0, t);
                    for _ in 0..16 {
                        let m = 0.5 * (a + b);
                        if !hidden(s.point_at(m)) == prev.1 {
                            a = m;
                        } else {
                            b = m;
                        }
                    }
                    0.5 * (a + b)
                } else {
                    t
                };
                match (start, vis) {
                    (None, true) => start = Some(edge),
                    (Some(t0), false) => {
                        spans.push((t0, edge));
                        start = None;
                    }
                    _ => {}
                }
                prev = (t, vis);
            }
            if let Some(t0) = start {
                spans.push((t0, 1.0));
            }
            if spans.first().is_none_or(|sp| sp.0 > 0.0) {
                flush(&mut cur, &mut lines);
            }
            let last = spans.len().saturating_sub(1);
            for (k, &(t0, t1)) in spans.iter().enumerate() {
                if k > 0 {
                    flush(&mut cur, &mut lines);
                }
                let m = match *s {
                    geom::Seg::Arc { r, sweep, .. } => {
                        let da = 2.0 * (1.0 - tol / r.max(tol * 2.0)).clamp(-1.0, 1.0).acos();
                        ((sweep.abs() * (t1 - t0) / da.max(1e-4)).ceil() as usize).clamp(1, 512)
                    }
                    geom::Seg::Line { .. } => 1,
                };
                for q in 0..=m {
                    let p = s.point_at(t0 + (t1 - t0) * q as f64 / m as f64);
                    if cur.last().is_none_or(|&lp| dist(lp, p) > 1e-9 * diag) {
                        cur.push(p);
                    }
                }
                if k == last && t1 < 1.0 {
                    flush(&mut cur, &mut lines);
                }
            }
        }
        flush(&mut cur, &mut lines);
    }
    lines
}
