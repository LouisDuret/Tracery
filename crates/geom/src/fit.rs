//! Ajustement d'un tracé à main levée en chaîne d'arcs et de segments
//! (§ 5.12, annexe A.4).
//!
//! 1. Rééchantillonnage à pas constant en longueur d'arc, lissage gaussien.
//! 2. Détection des coins : angle de rotation sur une fenêtre glissante
//!    supérieur à 35°, maximum local.
//! 3. Entre deux coins, ajustement récursif par biarcs G1 (tangentes
//!    estimées sur le tracé lissé) ; scission au point d'erreur maximale si
//!    l'écart dépasse la tolérance.
//! 4. Les jonctions hors coins partagent la même tangente (G1), les coins
//!    restent anguleux.
//! 5. Embellissement : reconnaissance d'un cercle entier.

use crate::constr::biarc;
use crate::math::*;
use crate::seg::{Loop, Seg};

/// Paramètres d'ajustement (unités du document).
#[derive(Clone, Copy, Debug)]
pub struct FitOptions {
    /// Tolérance d'écart (défaut : 1,5 px écran convertis).
    pub tol: f64,
    /// Pas de rééchantillonnage (≈ 2 px écran).
    pub step: f64,
    /// Seuil d'angle des coins (radians).
    pub corner_angle: f64,
    /// Reconnaissance d'un cercle entier.
    pub beautify: bool,
}

impl FitOptions {
    pub fn from_pixel(px: f64) -> Self {
        FitOptions {
            tol: 1.5 * px,
            step: 2.0 * px,
            corner_angle: 35f64.to_radians(),
            beautify: true,
        }
    }
}

fn resample_closed(pts: &[P2], step: f64) -> Vec<P2> {
    let n = pts.len();
    let mut out = vec![pts[0]];
    let mut acc = 0.0;
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let l = dist(a, b);
        if l <= 0.0 {
            continue;
        }
        let mut t = step - acc;
        while t <= l {
            out.push(a + (b - a) * (t / l));
            t += step;
        }
        acc = l - (t - step);
    }
    if out.len() > 1 && dist(out[0], out[out.len() - 1]) < 0.5 * step {
        out.pop();
    }
    out
}

fn smooth_closed(p: &[P2], sigma: f64) -> Vec<P2> {
    let n = p.len();
    let k = (3.0 * sigma).ceil() as i64;
    let w: Vec<f64> = (-k..=k)
        .map(|i| exp(-(i * i) as f64 / (2.0 * sigma * sigma)))
        .collect();
    let ws: f64 = w.iter().sum();
    (0..n)
        .map(|i| {
            let mut s = P2::ZERO;
            for (j, wj) in (-k..=k).zip(&w) {
                let idx = (i as i64 + j).rem_euclid(n as i64) as usize;
                s += p[idx] * *wj;
            }
            s / ws
        })
        .collect()
}

fn tangent(p: &[P2], i: usize, k: usize) -> P2 {
    let n = p.len();
    unit(p[(i + k) % n] - p[(i + n - k) % n])
}

/// Écart maximal des échantillons à une chaîne de segments, et son indice.
fn max_error(samples: &[P2], segs: &[Seg]) -> (f64, usize) {
    let mut best = (0.0, 0);
    for (i, p) in samples.iter().enumerate() {
        let d = segs
            .iter()
            .map(|s| s.dist(*p))
            .fold(f64::INFINITY, f64::min);
        if d > best.0 {
            best = (d, i);
        }
    }
    best
}

/// Ajuste un tracé (fermé) en boucle d'arcs et de segments.
pub fn fit_stroke(points: &[P2], opt: &FitOptions) -> Option<Loop> {
    if points.len() < 3 {
        return None;
    }
    let p = resample_closed(points, opt.step);
    if p.len() < 8 {
        return None;
    }
    let p = smooth_closed(&p, 2.0);
    let n = p.len();
    // embellissement : cercle entier ?
    if opt.beautify {
        let c = p.iter().fold(P2::ZERO, |s, q| s + *q) / n as f64;
        let r = p.iter().map(|q| dist(*q, c)).sum::<f64>() / n as f64;
        let dev = p
            .iter()
            .map(|q| (dist(*q, c) - r).abs())
            .fold(0.0, f64::max);
        if dev < 2.0 * opt.tol.max(0.04 * r) {
            return Some(Loop::circle(c, r));
        }
    }
    // coins
    let win = 4usize.min(n / 4).max(1);
    let turn: Vec<f64> = (0..n)
        .map(|i| {
            let a = unit(p[i] - p[(i + n - win) % n]);
            let b = unit(p[(i + win) % n] - p[i]);
            atan2(cross(a, b), dot(a, b)).abs()
        })
        .collect();
    let mut corners: Vec<usize> = (0..n)
        .filter(|&i| {
            turn[i] > opt.corner_angle
                && (1..=win)
                    .all(|k| turn[i] >= turn[(i + k) % n] && turn[i] >= turn[(i + n - k) % n])
        })
        .collect();
    corners.dedup_by(|a, b| a.abs_diff(*b) <= win);
    let is_corner: Vec<bool> = (0..n).map(|i| corners.contains(&i)).collect();
    // points de rupture initiaux : coins, sinon quatre points équirépartis
    let mut breaks: Vec<usize> = if corners.is_empty() {
        (0..4).map(|k| k * n / 4).collect()
    } else {
        corners.clone()
    };
    breaks.sort_unstable();
    breaks.dedup();
    let tk = 2usize.min(n / 8).max(1);
    let tan_out = |i: usize| -> P2 {
        if is_corner[i] {
            unit(p[(i + tk) % n] - p[i])
        } else {
            tangent(&p, i, tk)
        }
    };
    let tan_in = |i: usize| -> P2 {
        if is_corner[i] {
            unit(p[i] - p[(i + n - tk) % n])
        } else {
            tangent(&p, i, tk)
        }
    };
    let mut segs = Vec::new();
    for k in 0..breaks.len() {
        let i0 = breaks[k];
        let i1 = if k + 1 < breaks.len() {
            breaks[k + 1]
        } else {
            breaks[0] + n
        };
        fit_piece(&p, i0, i1, &tan_out, &tan_in, opt.tol, &mut segs, 0);
    }
    if segs.is_empty() {
        return None;
    }
    let l = Loop::new(segs).oriented(true);
    if crate::clip::self_intersects(&l) {
        return None;
    }
    Some(l)
}

#[allow(clippy::too_many_arguments)]
fn fit_piece(
    p: &[P2],
    i0: usize,
    i1: usize,
    tan_out: &dyn Fn(usize) -> P2,
    tan_in: &dyn Fn(usize) -> P2,
    tol: f64,
    out: &mut Vec<Seg>,
    depth: usize,
) {
    let n = p.len();
    let a = p[i0 % n];
    let b = p[i1 % n];
    if dist(a, b) < 1e-12 {
        return;
    }
    let (t0, t1) = (tan_out(i0 % n), tan_in(i1 % n));
    let chord = unit(b - a);
    // droite si le tronçon est rectiligne et que les tangentes s'y alignent
    // (sinon la continuité G1 serait rompue)
    let aligned =
        cross(t0, chord).abs() < 0.03 && cross(t1, chord).abs() < 0.03 && dot(t0, chord) > 0.0;
    let samples: Vec<P2> = (i0..=i1).map(|i| p[i % n]).collect();
    let line = [Seg::line(a, b)];
    if aligned && max_error(&samples, &line).0 <= tol {
        out.push(Seg::line(a, b));
        return;
    }
    let cand = biarc(a, t0, b, t1);
    let (e, k) = max_error(&samples, &cand);
    if (e <= tol && cand.iter().all(|s| s.is_finite())) || depth > 24 || i1 <= i0 + 3 {
        out.extend(cand.into_iter().filter(|s| s.length() > 0.0));
        return;
    }
    let mut m = i0 + k;
    if m <= i0 + 1 || m >= i1 - 1 {
        m = (i0 + i1) / 2;
    }
    fit_piece(p, i0, m, tan_out, tan_in, tol, out, depth + 1);
    fit_piece(p, m, i1, tan_out, tan_in, tol, out, depth + 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_noisy_circle_as_circle() {
        let pts: Vec<P2> = (0..400)
            .map(|i| {
                let a = TAU * i as f64 / 400.0;
                dir(a) * (100.0 + 0.3 * sin(17.0 * a))
            })
            .collect();
        let l = fit_stroke(&pts, &FitOptions::from_pixel(0.5)).unwrap();
        assert!(l.as_circle(1e-6).is_some());
    }

    #[test]
    fn fits_square_with_corners() {
        let mut pts = Vec::new();
        let c = [
            P2::new(0.0, 0.0),
            P2::new(100.0, 0.0),
            P2::new(100.0, 100.0),
            P2::new(0.0, 100.0),
        ];
        for k in 0..4 {
            for i in 0..100 {
                pts.push(c[k] + (c[(k + 1) % 4] - c[k]) * (i as f64 / 100.0));
            }
        }
        let l = fit_stroke(&pts, &FitOptions::from_pixel(0.5)).unwrap();
        assert!(l.is_ccw());
        assert!(
            l.joints
                .iter()
                .filter(|j| **j == crate::Joint::Corner)
                .count()
                >= 4
        );
        assert!((l.area() - 10000.0).abs() < 400.0, "{}", l.area());
    }

    #[test]
    fn fits_bean_g1() {
        // haricot : courbe lisse non convexe
        let pts: Vec<P2> = (0..600)
            .map(|i| {
                let a = TAU * i as f64 / 600.0;
                let r = 100.0 + 30.0 * cos(2.0 * a) - 20.0 * sin(a);
                dir(a) * r
            })
            .collect();
        let opt = FitOptions::from_pixel(0.5);
        let l = fit_stroke(&pts, &opt).unwrap();
        for q in &pts {
            assert!(l.dist(*q) < 3.0, "écart {}", l.dist(*q));
        }
        assert!(l.joints.iter().all(|j| *j == crate::Joint::Tangent));
    }
}
