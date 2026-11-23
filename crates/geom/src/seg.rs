//! Types de base : segments, arcs, boucles, régions, cercles (§ 4.1).
//!
//! INV-1 : toute courbe est un segment de droite ou un arc de cercle dont le
//! centre et le rayon sont stockés explicitement ; les extrémités en sont
//! dérivées.

use crate::math::*;
use serde::{Deserialize, Serialize};

/// Boîte englobante alignée sur les axes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub min: P2,
    pub max: P2,
}

impl BBox {
    pub const EMPTY: BBox = BBox {
        min: P2::new(f64::INFINITY, f64::INFINITY),
        max: P2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    };
    pub fn from_pts(a: P2, b: P2) -> Self {
        BBox {
            min: a.min(b),
            max: a.max(b),
        }
    }
    pub fn add(&mut self, p: P2) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }
    pub fn union(&self, o: &BBox) -> BBox {
        BBox {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }
    pub fn overlaps(&self, o: &BBox, margin: f64) -> bool {
        self.min.x <= o.max.x + margin
            && o.min.x <= self.max.x + margin
            && self.min.y <= o.max.y + margin
            && o.min.y <= self.max.y + margin
    }
    pub fn diag(&self) -> f64 {
        dist(self.min, self.max)
    }
    pub fn size(&self) -> P2 {
        self.max - self.min
    }
    pub fn center(&self) -> P2 {
        (self.min + self.max) * 0.5
    }
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }
    pub fn grow(&self, m: f64) -> BBox {
        BBox {
            min: self.min - P2::splat(m),
            max: self.max + P2::splat(m),
        }
    }
    /// Distance minimale d'un point à la boîte (0 à l'intérieur).
    pub fn dist(&self, p: P2) -> f64 {
        let dx = (self.min.x - p.x).max(0.0).max(p.x - self.max.x);
        let dy = (self.min.y - p.y).max(0.0).max(p.y - self.max.y);
        (dx * dx + dy * dy).sqrt()
    }
}

/// Similitude du plan : p ↦ M·p + t, avec M = s·R(angle)·diag(1, ±1).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Xf {
    /// Matrice 2×2 (ligne par ligne) : [m00, m01, m10, m11].
    pub m: [f64; 4],
    pub t: P2,
    pub scale: f64,
    /// Angle de rotation (appliqué après la symétrie éventuelle).
    pub angle: f64,
    /// Symétrie par rapport à l'axe x appliquée en premier.
    pub mirror: bool,
}

impl Xf {
    pub const IDENTITY: Xf = Xf {
        m: [1.0, 0.0, 0.0, 1.0],
        t: P2::ZERO,
        scale: 1.0,
        angle: 0.0,
        mirror: false,
    };
    pub fn translate(t: P2) -> Xf {
        Xf { t, ..Xf::IDENTITY }
    }
    pub fn scale(s: f64) -> Xf {
        Xf {
            m: [s, 0.0, 0.0, s],
            scale: s,
            ..Xf::IDENTITY
        }
    }
    /// Rotation d'angle `a` autour de `c`.
    pub fn rotate_about(c: P2, a: f64) -> Xf {
        let (s, co) = (sin(a), cos(a));
        let m = [co, -s, s, co];
        let rc = P2::new(m[0] * c.x + m[1] * c.y, m[2] * c.x + m[3] * c.y);
        Xf {
            m,
            t: c - rc,
            scale: 1.0,
            angle: a,
            mirror: false,
        }
    }
    /// Symétrie par rapport à la droite verticale x = x0 (matrice exacte).
    pub fn mirror_x(x0: f64) -> Xf {
        Xf {
            m: [-1.0, 0.0, 0.0, 1.0],
            t: P2::new(2.0 * x0, 0.0),
            scale: 1.0,
            angle: PI,
            mirror: true,
        }
    }
    #[inline]
    pub fn apply(&self, p: P2) -> P2 {
        P2::new(
            self.m[0] * p.x + self.m[1] * p.y + self.t.x,
            self.m[2] * p.x + self.m[3] * p.y + self.t.y,
        )
    }
    #[inline]
    pub fn apply_vec(&self, v: P2) -> P2 {
        P2::new(
            self.m[0] * v.x + self.m[1] * v.y,
            self.m[2] * v.x + self.m[3] * v.y,
        )
    }
    #[inline]
    pub fn apply_angle(&self, a: f64) -> f64 {
        if self.mirror {
            self.angle - a
        } else {
            self.angle + a
        }
    }
    /// Composition : (self ∘ o)(p) = self(o(p)).
    pub fn then_after(&self, o: &Xf) -> Xf {
        let a = &self.m;
        let b = &o.m;
        let m = [
            a[0] * b[0] + a[1] * b[2],
            a[0] * b[1] + a[1] * b[3],
            a[2] * b[0] + a[3] * b[2],
            a[2] * b[1] + a[3] * b[3],
        ];
        let t = self.apply(o.t);
        let mirror = self.mirror ^ o.mirror;
        let angle = if self.mirror {
            self.angle - o.angle
        } else {
            self.angle + o.angle
        };
        Xf {
            m,
            t,
            scale: self.scale * o.scale,
            angle,
            mirror,
        }
    }
    pub fn is_identity(&self) -> bool {
        *self == Xf::IDENTITY
    }
}

/// Segment de droite ou arc de cercle (sweep signé, |sweep| ≤ 2π).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Seg {
    Line {
        a: P2,
        b: P2,
    },
    Arc {
        c: P2,
        r: f64,
        theta0: f64,
        sweep: f64,
    },
}

impl Seg {
    pub fn line(a: P2, b: P2) -> Seg {
        Seg::Line { a, b }
    }
    pub fn arc(c: P2, r: f64, theta0: f64, sweep: f64) -> Seg {
        Seg::Arc {
            c,
            r,
            theta0,
            sweep,
        }
    }
    /// Arc de centre `c` allant de `p` à `q` (les deux points sont supposés
    /// sur le cercle) dans le sens donné (ccw = trigonométrique).
    pub fn arc_from_to(c: P2, p: P2, q: P2, ccw: bool) -> Seg {
        let r = 0.5 * (dist(c, p) + dist(c, q));
        let a0 = angle_of(p - c);
        let a1 = angle_of(q - c);
        let sw = ccw_delta(a0, a1);
        // points confondus : arc de longueur nulle (les cercles complets
        // se construisent avec `full_circle`)
        let sweep = if sw == 0.0 {
            0.0
        } else if ccw {
            sw
        } else {
            sw - TAU
        };
        Seg::Arc {
            c,
            r,
            theta0: a0,
            sweep,
        }
    }
    /// Arc de `a` à `b` d'ouverture signée `sweep` (droite si `sweep` ≈ 0).
    /// Centre : milieu de la corde + normale · (c/2)·(1 − b²)/(2b), b = tan(sweep/4).
    pub fn from_sweep(a: P2, b: P2, sweep: f64) -> Seg {
        let c = dist(a, b);
        if sweep.abs() < 1e-9 || c <= 0.0 {
            return Seg::line(a, b);
        }
        let bu = tan(sweep / 4.0);
        let u = (b - a) / c;
        let center = (a + b) * 0.5 + perp(u) * (0.5 * c * (1.0 - bu * bu) / (2.0 * bu));
        Seg::Arc {
            c: center,
            r: dist(center, a),
            theta0: angle_of(a - center),
            sweep,
        }
    }
    /// Ouverture signée (0 pour une droite).
    pub fn sweep(&self) -> f64 {
        match *self {
            Seg::Line { .. } => 0.0,
            Seg::Arc { sweep, .. } => sweep,
        }
    }
    /// Arc de `a` à `b` passant par `q` (droite si les points sont alignés).
    pub fn through(a: P2, q: P2, b: P2) -> Seg {
        match crate::constr::circle_3pts(a, q, b) {
            Some(c) if c.r < 1e4 * dist(a, b).max(1e-12) => {
                let (ta, tq, tb) = (angle_of(a - c.c), angle_of(q - c.c), angle_of(b - c.c));
                let ccw = ccw_delta(ta, tq) < ccw_delta(ta, tb);
                Seg::arc_from_to(c.c, a, b, ccw)
            }
            _ => Seg::line(a, b),
        }
    }
    /// Cercle complet commençant à l'angle `a0`.
    pub fn full_circle(c: P2, r: f64, a0: f64, ccw: bool) -> Seg {
        Seg::Arc {
            c,
            r,
            theta0: a0,
            sweep: if ccw { TAU } else { -TAU },
        }
    }

    #[inline]
    pub fn start(&self) -> P2 {
        match *self {
            Seg::Line { a, .. } => a,
            Seg::Arc { c, r, theta0, .. } => c + dir(theta0) * r,
        }
    }
    #[inline]
    pub fn end(&self) -> P2 {
        match *self {
            Seg::Line { b, .. } => b,
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => c + dir(theta0 + sweep) * r,
        }
    }
    pub fn is_arc(&self) -> bool {
        matches!(self, Seg::Arc { .. })
    }
    pub fn length(&self) -> f64 {
        match *self {
            Seg::Line { a, b } => dist(a, b),
            Seg::Arc { r, sweep, .. } => r * sweep.abs(),
        }
    }
    /// Point au paramètre t ∈ [0, 1].
    pub fn point_at(&self, t: f64) -> P2 {
        match *self {
            Seg::Line { a, b } => a + (b - a) * t,
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => c + dir(theta0 + sweep * t) * r,
        }
    }
    /// Tangente unitaire (sens de parcours) au paramètre t.
    pub fn tangent_at(&self, t: f64) -> P2 {
        match *self {
            Seg::Line { a, b } => unit(b - a),
            Seg::Arc { theta0, sweep, .. } => {
                let d = perp(dir(theta0 + sweep * t));
                if sweep >= 0.0 { d } else { -d }
            }
        }
    }
    pub fn tangent_start(&self) -> P2 {
        self.tangent_at(0.0)
    }
    pub fn tangent_end(&self) -> P2 {
        self.tangent_at(1.0)
    }
    pub fn reversed(&self) -> Seg {
        match *self {
            Seg::Line { a, b } => Seg::Line { a: b, b: a },
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => Seg::Arc {
                c,
                r,
                theta0: theta0 + sweep,
                sweep: -sweep,
            },
        }
    }
    /// Contribution à l'aire signée ∮ (x dy − y dx)/2.
    pub fn area_term(&self) -> f64 {
        match *self {
            Seg::Line { a, b } => 0.5 * (a.x * b.y - b.x * a.y),
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => {
                let t1 = theta0 + sweep;
                0.5 * r * (c.x * (sin(t1) - sin(theta0)) - c.y * (cos(t1) - cos(theta0)))
                    + 0.5 * r * r * sweep
            }
        }
    }
    /// L'angle `a` (absolu) est-il dans l'étendue angulaire de l'arc ?
    fn arc_contains_angle(theta0: f64, sweep: f64, a: f64) -> bool {
        if sweep.abs() >= TAU {
            return true;
        }
        if sweep >= 0.0 {
            ccw_delta(theta0, a) <= sweep
        } else {
            ccw_delta(a, theta0) <= -sweep
        }
    }
    pub fn bbox(&self) -> BBox {
        match *self {
            Seg::Line { a, b } => BBox::from_pts(a, b),
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => {
                let mut bb = BBox::from_pts(self.start(), self.end());
                for k in 0..4 {
                    let a = k as f64 * FRAC_PI_2;
                    if Self::arc_contains_angle(theta0, sweep, a) {
                        bb.add(c + dir(a) * r);
                    }
                }
                bb
            }
        }
    }
    /// Point le plus proche, paramètre associé et distance.
    pub fn closest(&self, p: P2) -> (P2, f64, f64) {
        match *self {
            Seg::Line { a, b } => {
                let d = b - a;
                let l2 = dot(d, d);
                let t = if l2 > 0.0 {
                    (dot(p - a, d) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let q = a + d * t;
                (q, t, dist(p, q))
            }
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => {
                let v = p - c;
                let a = angle_of(v);
                if norm(v) > 0.0 && Self::arc_contains_angle(theta0, sweep, a) {
                    let q = c + dir(a) * r;
                    let t = if sweep >= 0.0 {
                        ccw_delta(theta0, a) / sweep.max(1e-300)
                    } else {
                        ccw_delta(a, theta0) / (-sweep).max(1e-300)
                    };
                    (q, t.clamp(0.0, 1.0), dist(p, q))
                } else {
                    let s = self.start();
                    let e = self.end();
                    let (ds, de) = (dist(p, s), dist(p, e));
                    if ds <= de { (s, 0.0, ds) } else { (e, 1.0, de) }
                }
            }
        }
    }
    #[inline]
    pub fn dist(&self, p: P2) -> f64 {
        self.closest(p).2
    }
    pub fn transform(&self, x: &Xf) -> Seg {
        match *self {
            Seg::Line { a, b } => Seg::Line {
                a: x.apply(a),
                b: x.apply(b),
            },
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => Seg::Arc {
                c: x.apply(c),
                r: r * x.scale,
                theta0: x.apply_angle(theta0),
                sweep: if x.mirror { -sweep } else { sweep },
            },
        }
    }
    /// Découpe un arc en morceaux d'ouverture ≤ `max_sweep`.
    pub fn split_arc(&self, max_sweep: f64) -> Vec<Seg> {
        match *self {
            Seg::Line { .. } => vec![*self],
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => {
                let n = ((sweep.abs() / max_sweep) - 1e-12).ceil().max(1.0) as usize;
                let step = sweep / n as f64;
                (0..n)
                    .map(|i| Seg::Arc {
                        c,
                        r,
                        theta0: theta0 + step * i as f64,
                        sweep: step,
                    })
                    .collect()
            }
        }
    }
    /// Échantillonnage régulier (n ≥ 1 intervalles), extrémité finale exclue.
    pub fn sample(&self, n: usize) -> Vec<P2> {
        (0..n).map(|i| self.point_at(i as f64 / n as f64)).collect()
    }
    /// Sous-segment entre les paramètres t0 et t1.
    pub fn sub(&self, t0: f64, t1: f64) -> Seg {
        match *self {
            Seg::Line { .. } => Seg::Line {
                a: self.point_at(t0),
                b: self.point_at(t1),
            },
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => Seg::Arc {
                c,
                r,
                theta0: theta0 + sweep * t0,
                sweep: sweep * (t1 - t0),
            },
        }
    }
    pub fn is_finite(&self) -> bool {
        match *self {
            Seg::Line { a, b } => a.is_finite() && b.is_finite(),
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => c.is_finite() && r.is_finite() && theta0.is_finite() && sweep.is_finite(),
        }
    }
}

/// Jonction entre deux segments consécutifs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Joint {
    /// Continuité G1.
    Tangent,
    /// Angle vif.
    Corner,
    /// Rebroussement (angle intérieur nul).
    Cusp,
}

/// Boucle fermée de segments (continuité G0). `joints[i]` est la jonction
/// au début de `segs[i]` (entre `segs[i-1]` et `segs[i]`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Loop {
    pub segs: Vec<Seg>,
    #[serde(default, skip_serializing)]
    pub joints: Vec<Joint>,
}

/// Angle de rotation (signé) entre deux tangentes.
pub fn turn_angle(t_in: P2, t_out: P2) -> f64 {
    atan2(cross(t_in, t_out), dot(t_in, t_out))
}

impl Loop {
    pub fn new(segs: Vec<Seg>) -> Loop {
        let mut l = Loop {
            segs,
            joints: vec![],
        };
        l.compute_joints();
        l
    }
    pub fn compute_joints(&mut self) {
        let n = self.segs.len();
        self.joints = (0..n)
            .map(|i| {
                let prev = &self.segs[(i + n - 1) % n];
                let cur = &self.segs[i];
                let a = turn_angle(prev.tangent_end(), cur.tangent_start());
                if a.abs() < 1e-6 {
                    Joint::Tangent
                } else if PI - a.abs() < 1e-6 {
                    Joint::Cusp
                } else {
                    Joint::Corner
                }
            })
            .collect();
    }
    pub fn circle(c: P2, r: f64) -> Loop {
        Loop::new(vec![Seg::full_circle(c, r, 0.0, true)])
    }
    pub fn polygon(pts: &[P2]) -> Loop {
        let n = pts.len();
        Loop::new(
            (0..n)
                .map(|i| Seg::line(pts[i], pts[(i + 1) % n]))
                .collect(),
        )
    }
    /// Aire signée (positive pour une boucle trigonométrique).
    pub fn signed_area(&self) -> f64 {
        self.segs.iter().map(|s| s.area_term()).sum()
    }
    pub fn area(&self) -> f64 {
        self.signed_area().abs()
    }
    pub fn is_ccw(&self) -> bool {
        self.signed_area() > 0.0
    }
    pub fn reversed(&self) -> Loop {
        Loop::new(self.segs.iter().rev().map(|s| s.reversed()).collect())
    }
    pub fn oriented(&self, ccw: bool) -> Loop {
        if self.is_ccw() == ccw {
            self.clone()
        } else {
            self.reversed()
        }
    }
    pub fn length(&self) -> f64 {
        self.segs.iter().map(|s| s.length()).sum()
    }
    pub fn bbox(&self) -> BBox {
        self.segs
            .iter()
            .fold(BBox::EMPTY, |b, s| b.union(&s.bbox()))
    }
    pub fn dist(&self, p: P2) -> f64 {
        self.segs
            .iter()
            .map(|s| s.dist(p))
            .fold(f64::INFINITY, f64::min)
    }
    /// Nombre d'enroulement exact (arcs découpés en morceaux y-monotones).
    pub fn winding(&self, p: P2) -> i32 {
        // Chaque morceau se termine exactement au début du suivant : la
        // boucle est fermée sans fente numérique (dir(2π) ≠ dir(0)).
        let n = self.segs.len();
        let mut w = 0;
        for (i, s) in self.segs.iter().enumerate() {
            let next_start = self.segs[(i + 1) % n].start();
            match *s {
                Seg::Line { a, .. } => w += crossing(a, next_start, p, |_| None),
                Seg::Arc {
                    c,
                    r,
                    theta0,
                    sweep,
                } => {
                    let pieces = monotone_pieces(theta0, sweep);
                    let np = pieces.len();
                    let mut start = s.start();
                    for (k, (t0, t1)) in pieces.into_iter().enumerate() {
                        let b = if k + 1 == np {
                            next_start
                        } else {
                            c + dir(t1) * r
                        };
                        w += crossing(start, b, p, |y| {
                            // x sur l'arc à l'ordonnée y
                            let dy = y - c.y;
                            let h = (r * r - dy * dy).max(0.0).sqrt();
                            let mid = 0.5 * (t0 + t1);
                            Some(if cos(mid) >= 0.0 { c.x + h } else { c.x - h })
                        });
                        start = b;
                    }
                }
            }
        }
        w
    }
    pub fn contains(&self, p: P2) -> bool {
        self.winding(p) != 0
    }
    pub fn transform(&self, x: &Xf) -> Loop {
        let segs: Vec<Seg> = self.segs.iter().map(|s| s.transform(x)).collect();
        let l = Loop::new(segs);
        if x.mirror { l.reversed() } else { l }
    }
    /// Points échantillonnés le long de la boucle avec un pas ≈ `step`.
    pub fn sample(&self, step: f64) -> Vec<P2> {
        let mut out = Vec::new();
        for s in &self.segs {
            let n = ((s.length() / step).ceil() as usize).clamp(1, 4096);
            out.extend(s.sample(n));
        }
        out
    }
    /// Point au paramètre d'abscisse curviligne `u` (modulo la longueur).
    pub fn point_at_length(&self, u: f64) -> (P2, P2) {
        let total = self.length();
        let mut u = u.rem_euclid(total.max(1e-300));
        let last = self.segs.len().saturating_sub(1);
        for (i, s) in self.segs.iter().enumerate() {
            let l = s.length();
            if u <= l || i == last {
                let t = if l > 0.0 {
                    (u / l).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                return (s.point_at(t), s.tangent_at(t));
            }
            u -= l;
        }
        (self.segs[0].start(), self.segs[0].tangent_start())
    }
    pub fn is_finite(&self) -> bool {
        self.segs.iter().all(|s| s.is_finite())
    }
    /// Toutes les arêtes sont-elles des segments de droite ?
    pub fn is_polygon(&self) -> bool {
        self.segs.iter().all(|s| !s.is_arc())
    }
    /// Si la boucle est un cercle complet (un ou plusieurs arcs de même
    /// centre et même rayon), le renvoie.
    pub fn as_circle(&self, tol: f64) -> Option<Circle> {
        let mut first: Option<(P2, f64)> = None;
        let mut total = 0.0;
        for s in &self.segs {
            match *s {
                Seg::Arc { c, r, sweep, .. } => {
                    if let Some((c0, r0)) = first {
                        if dist(c, c0) > tol || (r - r0).abs() > tol {
                            return None;
                        }
                    } else {
                        first = Some((c, r));
                    }
                    total += sweep;
                }
                Seg::Line { a, b } => {
                    if dist(a, b) > tol {
                        return None;
                    }
                }
            }
        }
        let (c, r) = first?;
        if (total.abs() - TAU).abs() < 1e-6 {
            Some(Circle { c, r })
        } else {
            None
        }
    }
}

/// Découpe l'intervalle angulaire d'un arc aux angles π/2 et 3π/2.
fn monotone_pieces(theta0: f64, sweep: f64) -> Vec<(f64, f64)> {
    let mut cuts = vec![theta0];
    let s = sweep.signum();
    // angles critiques k·π + π/2 strictement compris dans l'arc
    let (lo, hi) = if sweep >= 0.0 {
        (theta0, theta0 + sweep)
    } else {
        (theta0 + sweep, theta0)
    };
    let kmin = ((lo - FRAC_PI_2) / PI).floor() as i64 + 1;
    let kmax = ((hi - FRAC_PI_2) / PI).ceil() as i64 - 1;
    let mut crit: Vec<f64> = (kmin..=kmax)
        .map(|k| k as f64 * PI + FRAC_PI_2)
        .filter(|&a| a > lo && a < hi)
        .collect();
    if s < 0.0 {
        crit.reverse();
    }
    cuts.extend(crit);
    cuts.push(theta0 + sweep);
    cuts.windows(2).map(|w| (w[0], w[1])).collect()
}

/// Contribution au nombre d'enroulement d'un morceau monotone en y allant de
/// a à b (règle demi-ouverte). `x_at` donne l'abscisse exacte pour un arc.
fn crossing(a: P2, b: P2, p: P2, x_at: impl Fn(f64) -> Option<f64>) -> i32 {
    if a.y <= p.y {
        if b.y > p.y {
            let x = x_at(p.y).unwrap_or_else(|| a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y));
            if x > p.x {
                return 1;
            }
        }
    } else if b.y <= p.y {
        let x = x_at(p.y).unwrap_or_else(|| a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y));
        if x > p.x {
            return -1;
        }
    }
    0
}

/// Région : boucle extérieure trigonométrique et trous en sens horaire.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub outer: Loop,
    #[serde(default)]
    pub holes: Vec<Loop>,
}

impl Region {
    /// Construit une région en normalisant les orientations.
    pub fn new(outer: Loop, holes: Vec<Loop>) -> Region {
        Region {
            outer: outer.oriented(true),
            holes: holes.into_iter().map(|h| h.oriented(false)).collect(),
        }
    }
    pub fn simple(outer: Loop) -> Region {
        Region::new(outer, vec![])
    }
    pub fn circle(c: P2, r: f64) -> Region {
        Region::simple(Loop::circle(c, r))
    }
    pub fn area(&self) -> f64 {
        self.outer.area() - self.holes.iter().map(|h| h.area()).sum::<f64>()
    }
    pub fn perimeter(&self) -> f64 {
        self.outer.length() + self.holes.iter().map(|h| h.length()).sum::<f64>()
    }
    pub fn bbox(&self) -> BBox {
        self.outer.bbox()
    }
    pub fn loops(&self) -> impl Iterator<Item = &Loop> {
        std::iter::once(&self.outer).chain(self.holes.iter())
    }
    pub fn segs(&self) -> impl Iterator<Item = &Seg> {
        self.loops().flat_map(|l| l.segs.iter())
    }
    pub fn contains(&self, p: P2) -> bool {
        self.outer.contains(p) && !self.holes.iter().any(|h| h.contains(p))
    }
    /// Distance au bord (non signée).
    pub fn boundary_dist(&self, p: P2) -> f64 {
        self.segs().map(|s| s.dist(p)).fold(f64::INFINITY, f64::min)
    }
    /// Distance signée : positive à l'intérieur.
    pub fn sdf(&self, p: P2) -> f64 {
        let d = self.boundary_dist(p);
        if self.contains(p) { d } else { -d }
    }
    pub fn transform(&self, x: &Xf) -> Region {
        Region::new(
            self.outer.transform(x),
            self.holes.iter().map(|h| h.transform(x)).collect(),
        )
    }
    pub fn is_finite(&self) -> bool {
        self.loops().all(|l| l.is_finite())
    }
    pub fn centroid(&self) -> P2 {
        // centroïde approché par échantillonnage du bord (pondéré par l'aire)
        let pts = self.outer.sample(self.outer.length() / 256.0);
        let n = pts.len();
        let mut a = 0.0;
        let mut c = P2::ZERO;
        for i in 0..n {
            let p = pts[i];
            let q = pts[(i + 1) % n];
            let cr = cross(p, q);
            a += cr;
            c += (p + q) * cr;
        }
        if a.abs() > 0.0 {
            c / (3.0 * a)
        } else {
            self.bbox().center()
        }
    }
}

/// Cercle, entité de premier ordre.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Circle {
    pub c: P2,
    pub r: f64,
}

impl Circle {
    pub fn new(c: P2, r: f64) -> Circle {
        Circle { c, r }
    }
    pub fn to_loop(&self) -> Loop {
        Loop::circle(self.c, self.r)
    }
    pub fn to_region(&self) -> Region {
        Region::circle(self.c, self.r)
    }
    pub fn transform(&self, x: &Xf) -> Circle {
        Circle {
            c: x.apply(self.c),
            r: self.r * x.scale,
        }
    }
}
