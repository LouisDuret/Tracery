//! Tracés du canevas découpés au cadre de la vue (ADR-31).
//!
//! À fort zoom, un arc ou un cercle de construction mesure des centaines
//! de milliers de pixels : le tracer en entier (points, tirets, maillage
//! egui) fait ramer chaque image. Ici, chaque segment est découpé au
//! rectangle de la vue (élargi d'une marge) en double précision, et un arc
//! n'est échantillonné que sur la plage d'angles qui peut toucher la vue,
//! au pas voulu pour une flèche d'un quart de pixel : le coût d'une image
//! ne dépend plus du zoom.

use egui::{Pos2, pos2};
use geom::math::*;
use geom::{Loop, Seg};

/// Flèche maximale d'une corde d'arc, en pixels.
const TOL_PX: f64 = 0.25;
/// Nombre maximal de cordes par plage d'arc visible.
const MAX_CHORDS: f64 = 4096.0;

/// Passage document → écran (y vers le bas) et rectangle visible.
#[derive(Clone, Copy, Debug)]
pub struct ScreenMap {
    /// Point du document au centre de l'écran.
    pub center: P2,
    /// Pixels par unité du document.
    pub scale: f64,
    /// Centre de l'écran (pixels).
    pub origin: (f64, f64),
    /// Rectangle visible élargi : x₀, y₀, x₁, y₁ (pixels).
    pub rect: [f64; 4],
}

impl ScreenMap {
    fn to_screen(self, p: P2) -> (f64, f64) {
        (
            self.origin.0 + (p.x - self.center.x) * self.scale,
            self.origin.1 - (p.y - self.center.y) * self.scale,
        )
    }

    /// Disque qui contient le rectangle visible : centre et rayon, en
    /// unités du document.
    fn view_disc(&self) -> (P2, f64) {
        let [x0, y0, x1, y1] = self.rect;
        let c = P2::new(
            self.center.x + (0.5 * (x0 + x1) - self.origin.0) / self.scale,
            self.center.y - (0.5 * (y0 + y1) - self.origin.1) / self.scale,
        );
        let r = 0.5 * ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt() / self.scale;
        (c, r)
    }
}

/// Découpe de Liang-Barsky : portion de [a, b] dans le rectangle, en
/// paramètres (t₀, t₁) ⊂ [0, 1].
fn clip_params(a: (f64, f64), b: (f64, f64), r: &[f64; 4]) -> Option<(f64, f64)> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [
        (-dx, a.0 - r[0]),
        (dx, r[2] - a.0),
        (-dy, a.1 - r[1]),
        (dy, r[3] - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 <= t1).then_some((t0, t1))
}

/// Morceaux visibles d'un tracé, en pixels écran.
pub struct Clipper {
    map: ScreenMap,
    pieces: Vec<Vec<Pos2>>,
    cur: Vec<Pos2>,
}

impl Clipper {
    pub fn new(map: ScreenMap) -> Self {
        Clipper {
            map,
            pieces: Vec::new(),
            cur: Vec::new(),
        }
    }

    /// Termine le morceau en cours.
    fn cut(&mut self) {
        if self.cur.len() >= 2 {
            self.pieces.push(std::mem::take(&mut self.cur));
        } else {
            self.cur.clear();
        }
    }

    /// Segment en pixels (double précision).
    fn seg_px(&mut self, a: (f64, f64), b: (f64, f64)) {
        let Some((t0, t1)) = clip_params(a, b, &self.map.rect) else {
            self.cut();
            return;
        };
        let at = |t: f64| {
            pos2(
                (a.0 + t * (b.0 - a.0)) as f32,
                (a.1 + t * (b.1 - a.1)) as f32,
            )
        };
        let (p, q) = (at(t0), at(t1));
        let joined = t0 == 0.0 && self.cur.last().is_some_and(|l| l.distance(p) < 1e-3);
        if !joined {
            self.cut();
            self.cur.push(p);
        }
        self.cur.push(q);
        if t1 < 1.0 {
            self.cut();
        }
    }

    /// Segment de droite du document.
    pub fn line(&mut self, a: P2, b: P2) {
        let (a, b) = (self.map.to_screen(a), self.map.to_screen(b));
        self.seg_px(a, b);
    }

    /// Arc du document (centre, rayon, angle de départ, ouverture signée),
    /// échantillonné sur sa seule partie qui peut toucher la vue.
    pub fn arc(&mut self, c: P2, r: f64, theta0: f64, sweep: f64) {
        let rp = r * self.map.scale;
        if rp.is_nan() || rp <= 0.0 || sweep == 0.0 {
            let p = c + dir(theta0) * r;
            let q = c + dir(theta0 + sweep) * r;
            self.line(p, q);
            return;
        }
        let (vc, vr) = self.map.view_disc();
        let d = dist(c, vc);
        // le cercle ne coupe pas le disque de la vue
        if d >= r + vr || r >= d + vr {
            self.cut();
            return;
        }
        let s = sweep.signum();
        let span = sweep.abs();
        // plages visibles de ψ = s·(θ − θ₀) dans [0, |ouverture|]
        let mut ranges: Vec<(f64, f64)> = Vec::new();
        if r + d <= vr || d < 1e-12 * r {
            ranges.push((0.0, span));
        } else {
            let cos_a = ((r * r + d * d - vr * vr) / (2.0 * r * d)).clamp(-1.0, 1.0);
            let alpha = acos(cos_a);
            let phi = angle_of(vc - c);
            let pc = (s * (phi - theta0)).rem_euclid(TAU);
            for k in [-1.0, 0.0, 1.0] {
                let lo = (pc - alpha + k * TAU).max(0.0);
                let hi = (pc + alpha + k * TAU).min(span);
                if lo < hi {
                    ranges.push((lo, hi));
                }
            }
            ranges.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        if ranges.is_empty() {
            self.cut();
            return;
        }
        let step = 2.0 * (2.0 * TOL_PX / rp).sqrt();
        let map = self.map;
        let pt = |psi: f64| map.to_screen(c + dir(theta0 + s * psi) * r);
        for (lo, hi) in ranges {
            let n = ((hi - lo) / step).ceil().clamp(1.0, MAX_CHORDS) as usize;
            let mut prev = pt(lo);
            for k in 1..=n {
                let q = pt(lo + (hi - lo) * k as f64 / n as f64);
                self.seg_px(prev, q);
                prev = q;
            }
        }
    }

    pub fn seg(&mut self, s: &Seg) {
        match *s {
            Seg::Line { a, b } => self.line(a, b),
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => self.arc(c, r, theta0, sweep),
        }
    }

    pub fn polyline(&mut self, pts: &[P2]) {
        for w in pts.windows(2) {
            self.line(w[0], w[1]);
        }
    }

    /// Morceaux visibles ; le tracé en cours est terminé.
    pub fn finish(mut self) -> Vec<Vec<Pos2>> {
        self.cut();
        self.pieces
    }
}

/// Morceaux visibles d'un contour fermé, et s'il est vu en entier (un seul
/// morceau qui se referme).
pub fn loop_pieces(l: &Loop, map: ScreenMap) -> (Vec<Vec<Pos2>>, bool) {
    let mut c = Clipper::new(map);
    for s in &l.segs {
        c.seg(s);
    }
    let pieces = c.finish();
    let closed = pieces.len() == 1
        && pieces[0].len() > 2
        && pieces[0][0].distance(*pieces[0].last().unwrap_or(&pieces[0][0])) < 1e-2;
    (pieces, closed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(scale: f64) -> ScreenMap {
        ScreenMap {
            center: P2::ZERO,
            scale,
            origin: (500.0, 400.0),
            rect: [-8.0, -8.0, 1008.0, 808.0],
        }
    }

    fn count(p: &[Vec<Pos2>]) -> usize {
        p.iter().map(|v| v.len()).sum()
    }

    #[test]
    fn segment_decoupe() {
        let mut c = Clipper::new(map(1.0));
        c.line(P2::new(-10_000.0, 0.0), P2::new(10_000.0, 0.0));
        let p = c.finish();
        assert_eq!(p.len(), 1);
        assert!((p[0][0].x - -8.0).abs() < 1e-3 && (p[0][1].x - 1008.0).abs() < 1e-3);
        // entièrement hors champ
        let mut c = Clipper::new(map(1.0));
        c.line(P2::new(-10_000.0, 1000.0), P2::new(10_000.0, 1000.0));
        assert!(c.finish().is_empty());
    }

    #[test]
    fn contour_vu_entier() {
        let l = Loop::circle(P2::ZERO, 100.0);
        let (p, closed) = loop_pieces(&l, map(1.0));
        assert!(closed, "{} morceaux", p.len());
        // flèche ≤ ¼ px : le nombre de cordes suit le rayon
        assert!(count(&p) > 30 && count(&p) < 400, "{}", count(&p));
    }

    /// Un fort zoom ne fait pas exploser le nombre de points : seule la
    /// partie du cercle qui traverse la vue est échantillonnée.
    #[test]
    fn fort_zoom_borne() {
        // cercle de rayon 1000 passant par le centre de la vue
        let l = Loop::circle(P2::new(1000.0, 0.0), 1000.0);
        for scale in [1.0, 100.0, 1e4, 1e6] {
            let (p, _) = loop_pieces(&l, map(scale));
            assert!(!p.is_empty(), "échelle {scale}");
            assert!(count(&p) < 6000, "échelle {scale} : {} points", count(&p));
            for v in &p {
                for q in v {
                    assert!(q.x > -9.0 && q.x < 1009.0 && q.y > -9.0 && q.y < 809.0);
                }
            }
        }
        // vue à l'intérieur du cercle sans le toucher
        let mut c = Clipper::new(map(1e6));
        c.arc(P2::new(0.0, 0.0), 10.0, 0.0, TAU);
        assert!(c.finish().is_empty());
    }

    /// Un arc partiel ne déborde pas de son ouverture.
    #[test]
    fn arc_partiel() {
        let mut c = Clipper::new(map(1.0));
        // quart de cercle haut-gauche, de 90° à 180°, rayon 200
        c.arc(P2::ZERO, 200.0, FRAC_PI_2, FRAC_PI_2);
        let p = c.finish();
        assert_eq!(p.len(), 1);
        for q in &p[0] {
            assert!(q.x <= 500.0 + 1e-3 && q.y <= 400.0 + 1e-3);
        }
        // même arc parcouru dans l'autre sens
        let mut c = Clipper::new(map(1.0));
        c.arc(P2::ZERO, 200.0, PI, -FRAC_PI_2);
        let p = c.finish();
        assert_eq!(p.len(), 1);
        for q in &p[0] {
            assert!(q.x <= 500.0 + 1e-3 && q.y <= 400.0 + 1e-3);
        }
    }
}
