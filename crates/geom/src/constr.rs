//! Bibliothèque de constructions élémentaires (§ 4.3).
//!
//! Chaque construction est une fonction pure ; les tangences sont détectées
//! avec ε_tan passé en paramètre (via [`Tol`]).

use crate::math::*;
use crate::seg::{Circle, Seg};
use serde::{Deserialize, Serialize};

/// Droite n·p = d avec n unitaire.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub n: P2,
    pub d: f64,
}

impl Line {
    /// Droite passant par a et b ; n est la normale à gauche de a→b.
    pub fn through(a: P2, b: P2) -> Line {
        let n = unit(perp(b - a));
        Line { n, d: dot(n, a) }
    }
    pub fn signed_dist(&self, p: P2) -> f64 {
        dot(self.n, p) - self.d
    }
    pub fn dir(&self) -> P2 {
        P2::new(self.n.y, -self.n.x)
    }
    pub fn point(&self) -> P2 {
        self.n * self.d
    }
    /// Droite parallèle décalée de `o` du côté de la normale.
    pub fn offset(&self, o: f64) -> Line {
        Line {
            n: self.n,
            d: self.d + o,
        }
    }
}

/// Intersection droite–droite.
pub fn inter_line_line(l1: &Line, l2: &Line) -> Option<P2> {
    let det = cross(l1.n, l2.n);
    if det.abs() < 1e-14 {
        return None;
    }
    Some(P2::new(
        (l1.d * l2.n.y - l2.d * l1.n.y) / det,
        (l1.n.x * l2.d - l2.n.x * l1.d) / det,
    ))
}

/// Intersection droite–cercle : 0, 1 (tangence) ou 2 points.
pub fn inter_line_circle(l: &Line, c: &Circle, tol: &Tol) -> Vec<P2> {
    let dd = l.signed_dist(c.c);
    let foot = c.c - l.n * dd;
    let h2 = c.r * c.r - dd * dd;
    if (dd.abs() - c.r).abs() <= tol.tan {
        return vec![foot];
    }
    if h2 < 0.0 {
        return vec![];
    }
    let h = h2.sqrt();
    let t = l.dir();
    vec![foot - t * h, foot + t * h]
}

/// Intersection cercle–cercle : 0, 1 (tangence) ou 2 points. Le premier
/// point est à gauche de c1→c2.
pub fn inter_circle_circle(c1: &Circle, c2: &Circle, tol: &Tol) -> Vec<P2> {
    let d = dist(c1.c, c2.c);
    if d < tol.geom {
        return vec![];
    }
    let u = (c2.c - c1.c) / d;
    if (d - (c1.r + c2.r)).abs() <= tol.tan || (d - (c1.r - c2.r).abs()).abs() <= tol.tan {
        let s = if d > (c1.r - c2.r).abs() + tol.tan || c1.r >= c2.r {
            1.0
        } else {
            -1.0
        };
        return vec![c1.c + u * (c1.r * s)];
    }
    if d > c1.r + c2.r || d < (c1.r - c2.r).abs() {
        return vec![];
    }
    let a = (d * d + c1.r * c1.r - c2.r * c2.r) / (2.0 * d);
    let h = (c1.r * c1.r - a * a).max(0.0).sqrt();
    let m = c1.c + u * a;
    vec![m + perp(u) * h, m - perp(u) * h]
}

/// Tangentes d'un point à un cercle : points de contact.
pub fn tangents_point_circle(p: P2, c: &Circle) -> Vec<P2> {
    let d = dist(p, c.c);
    if d <= c.r {
        return vec![];
    }
    let a = acos(c.r / d);
    let base = angle_of(p - c.c);
    vec![c.c + dir(base + a) * c.r, c.c + dir(base - a) * c.r]
}

/// Tangentes communes à deux cercles (jusqu'à 4) : paires de points de
/// contact (sur c1, sur c2).
pub fn common_tangents(c1: &Circle, c2: &Circle) -> Vec<(P2, P2)> {
    let mut out = Vec::new();
    let d = dist(c1.c, c2.c);
    if d < 1e-300 {
        return out;
    }
    let base = angle_of(c2.c - c1.c);
    // extérieures : r1 − r2 ; intérieures : r1 + r2
    for (r2s, inner) in [(c2.r, false), (-c2.r, true)] {
        let cosv = (c1.r - r2s) / d;
        if cosv.abs() > 1.0 {
            continue;
        }
        let a = acos(cosv);
        for s in [1.0, -1.0] {
            let n = dir(base + s * a);
            let p1 = c1.c + n * c1.r;
            let p2 = c2.c + n * r2s;
            let _ = inner;
            out.push((p1, p2));
        }
    }
    out
}

/// Cercle passant par trois points (intersection des médiatrices).
pub fn circle_3pts(a: P2, b: P2, c: P2) -> Option<Circle> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-14 {
        return None;
    }
    let a2 = dot(a, a);
    let b2 = dot(b, b);
    let c2 = dot(c, c);
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let o = P2::new(ux, uy);
    Some(Circle::new(o, dist(o, a)))
}

/// Objet d'une condition de tangence pour le problème d'Apollonius.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Obj {
    /// |x − c| = r_c + σ·r : σ = +1 tangence extérieure, σ = −1 intérieure.
    Circle { c: P2, r: f64, s: f64 },
    /// n·x − d = σ·r : centre du côté σ de la droite, à distance r.
    Line { n: P2, d: f64, s: f64 },
    /// Point : cercle de rayon nul.
    Point(P2),
}

impl Obj {
    /// Résidu de la condition pour le cercle (x, r).
    pub fn residual(&self, x: P2, r: f64) -> f64 {
        match *self {
            Obj::Circle { c, r: rc, s } => dist(x, c) - (rc + s * r).abs(),
            Obj::Line { n, d, s } => dot(n, x) - d - s * r,
            Obj::Point(p) => dist(x, p) - r,
        }
    }
    /// Coefficients (a, b, e, f, quad) de l'équation
    /// quad·(x²+y²−r²) + a·x + b·y + e·r + f = 0.
    fn coefs(&self) -> (f64, f64, f64, f64, bool) {
        match *self {
            Obj::Circle { c, r, s } => (
                -2.0 * c.x,
                -2.0 * c.y,
                -2.0 * s * r,
                c.x * c.x + c.y * c.y - r * r,
                true,
            ),
            Obj::Point(p) => (-2.0 * p.x, -2.0 * p.y, 0.0, p.x * p.x + p.y * p.y, true),
            Obj::Line { n, d, s } => (n.x, n.y, -s, -d, false),
        }
    }
}

/// Problème d'Apollonius général (§ 4.3) : cercles (x, y, r), r > eps,
/// satisfaisant les trois conditions de tangence.
///
/// On soustrait la première équation quadratique des autres pour obtenir un
/// système linéaire 2×3, résolu en exprimant deux inconnues comme fonctions
/// affines de la troisième ; le report dans l'équation quadratique donne une
/// équation du second degré.
pub fn apollonius(objs: &[Obj; 3], eps: f64) -> Vec<Circle> {
    let eqs: Vec<(f64, f64, f64, f64, bool)> = objs.iter().map(|o| o.coefs()).collect();
    let quads: Vec<usize> = (0..3).filter(|&i| eqs[i].4).collect();
    let mut lins: Vec<[f64; 4]> = Vec::new();
    for e in eqs.iter().filter(|e| !e.4) {
        lins.push([e.0, e.1, e.2, e.3]);
    }
    let mut sols: Vec<Circle> = Vec::new();
    if quads.is_empty() {
        // 3 équations linéaires en (x, y, r)
        let m = [lins[0], lins[1], lins[2]];
        if let Some((x, y, r)) = solve3(&m)
            && r > eps
        {
            sols.push(Circle::new(P2::new(x, y), r));
        }
        return sols;
    }
    let q0 = eqs[quads[0]];
    for &qi in &quads[1..] {
        let e = eqs[qi];
        lins.push([e.0 - q0.0, e.1 - q0.1, e.2 - q0.2, e.3 - q0.3]);
    }
    debug_assert_eq!(lins.len(), 2);
    let (l1, l2) = (lins[0], lins[1]);
    // Choix de la paire d'inconnues la mieux conditionnée.
    // Inconnues : 0 = x, 1 = y, 2 = r.
    let pairs = [(0usize, 1usize, 2usize), (0, 2, 1), (1, 2, 0)];
    let mut best = (0.0, pairs[0]);
    for &p in &pairs {
        let det = l1[p.0] * l2[p.1] - l1[p.1] * l2[p.0];
        if det.abs() > best.0 {
            best = (det.abs(), p);
        }
    }
    if best.0 < 1e-14 {
        return sols;
    }
    let (i, j, k) = best.1;
    let det = l1[i] * l2[j] - l1[j] * l2[i];
    // l·[u_i, u_j] = −(l[k]·t + l[3])
    // u_i = (−(l1k t + l1f) l2j + (l2k t + l2f) l1j) / det
    let ui1 = (-l1[k] * l2[j] + l2[k] * l1[j]) / det;
    let ui0 = (-l1[3] * l2[j] + l2[3] * l1[j]) / det;
    let uj1 = (-l2[k] * l1[i] + l1[k] * l2[i]) / det;
    let uj0 = (-l2[3] * l1[i] + l1[3] * l2[i]) / det;
    // vecteur (x, y, r) = c0 + c1·t
    let mut c0 = [0.0; 3];
    let mut c1 = [0.0; 3];
    c0[i] = ui0;
    c1[i] = ui1;
    c0[j] = uj0;
    c1[j] = uj1;
    c0[k] = 0.0;
    c1[k] = 1.0;
    // q0 : x²+y²−r² + a x + b y + e r + f = 0
    let (a, b, e, f) = (q0.0, q0.1, q0.2, q0.3);
    let qa = c1[0] * c1[0] + c1[1] * c1[1] - c1[2] * c1[2];
    let qb =
        2.0 * (c0[0] * c1[0] + c0[1] * c1[1] - c0[2] * c1[2]) + a * c1[0] + b * c1[1] + e * c1[2];
    let qc = c0[0] * c0[0] + c0[1] * c0[1] - c0[2] * c0[2] + a * c0[0] + b * c0[1] + e * c0[2] + f;
    for t in solve_quadratic(qa, qb, qc) {
        let x = c0[0] + c1[0] * t;
        let y = c0[1] + c1[1] * t;
        let r = c0[2] + c1[2] * t;
        if r > eps && x.is_finite() && y.is_finite() {
            sols.push(Circle::new(P2::new(x, y), r));
        }
    }
    sols
}

/// Racines réelles de a t² + b t + c = 0 (forme numériquement stable).
pub fn solve_quadratic(a: f64, b: f64, c: f64) -> Vec<f64> {
    let scale = a.abs().max(b.abs()).max(c.abs());
    if scale == 0.0 {
        return vec![];
    }
    if a.abs() <= 1e-13 * scale {
        if b.abs() <= 1e-300 {
            return vec![];
        }
        return vec![-c / b];
    }
    let disc = b * b - 4.0 * a * c;
    if disc < -1e-12 * (b * b).max(1e-300) {
        return vec![];
    }
    let sq = disc.max(0.0).sqrt();
    let q = -0.5 * (b + if b >= 0.0 { sq } else { -sq });
    let mut r = Vec::with_capacity(2);
    if q != 0.0 {
        r.push(q / a);
        r.push(c / q);
    } else {
        r.push(0.0);
    }
    r
}

/// Résolution d'un système 3×3 (lignes [a, b, c, f] : a x + b y + c z + f = 0).
fn solve3(m: &[[f64; 4]; 3]) -> Option<(f64, f64, f64)> {
    let det3 = |a: [f64; 3], b: [f64; 3], c: [f64; 3]| {
        a[0] * (b[1] * c[2] - b[2] * c[1]) - a[1] * (b[0] * c[2] - b[2] * c[0])
            + a[2] * (b[0] * c[1] - b[1] * c[0])
    };
    let rows: Vec<[f64; 3]> = m.iter().map(|r| [r[0], r[1], r[2]]).collect();
    let rhs: Vec<f64> = m.iter().map(|r| -r[3]).collect();
    let d = det3(rows[0], rows[1], rows[2]);
    if d.abs() < 1e-14 {
        return None;
    }
    let col = |k: usize| {
        let mut r = rows.clone();
        for i in 0..3 {
            r[i][k] = rhs[i];
        }
        det3(r[0], r[1], r[2]) / d
    };
    Some((col(0), col(1), col(2)))
}

/// Cercles de rayon donné tangents à deux cercles (intersection de cercles
/// décalés). `s1`, `s2` : +1 tangence extérieure, −1 intérieure.
pub fn circles_tangent_two_circles(
    c1: &Circle,
    s1: f64,
    c2: &Circle,
    s2: f64,
    r: f64,
    tol: &Tol,
) -> Vec<Circle> {
    let a = Circle::new(c1.c, (c1.r + s1 * r).abs());
    let b = Circle::new(c2.c, (c2.r + s2 * r).abs());
    inter_circle_circle(&a, &b, tol)
        .into_iter()
        .map(|p| Circle::new(p, r))
        .collect()
}

/// Point de raccord de deux cercles tangents.
pub fn tangent_point(c1: &Circle, c2: &Circle) -> P2 {
    let d = dist(c1.c, c2.c);
    let u = unit(c2.c - c1.c);
    let external = (d - (c1.r + c2.r)).abs() < (d - (c1.r - c2.r).abs()).abs();
    if external || c1.r >= c2.r {
        c1.c + u * c1.r
    } else {
        c1.c - u * c1.r
    }
}

/// Arc partant de `p` avec la tangente unitaire `t` et aboutissant en `q`.
/// Renvoie une droite si les points sont alignés avec la tangente.
pub fn arc_from_tangent(p: P2, t: P2, q: P2) -> Seg {
    let n = perp(t);
    let v = q - p;
    let den = 2.0 * dot(n, v);
    if den.abs() < 1e-12 * dot(v, v).max(1e-300) {
        return Seg::line(p, q);
    }
    let r = dot(v, v) / den; // signé : > 0 → centre à gauche (arc trigonométrique)
    let c = p + n * r;
    Seg::arc_from_to(c, p, q, r > 0.0)
}

/// Biarc G1 (méthode à paramètres égaux, Bolton / Meek–Walton) entre
/// (p0, t0) et (p1, t1), tangentes unitaires.
pub fn biarc(p0: P2, t0: P2, p1: P2, t1: P2) -> Vec<Seg> {
    let v = p1 - p0;
    let t = t0 + t1;
    let denom = 2.0 * (1.0 - dot(t0, t1));
    let vt = dot(v, t);
    let d = if denom.abs() < 1e-12 {
        // tangentes parallèles
        let vt0 = dot(v, t0);
        if vt0.abs() < 1e-12 {
            return vec![Seg::line(p0, p1)];
        }
        dot(v, v) / (4.0 * vt0)
    } else {
        (-vt + (vt * vt + denom * dot(v, v)).sqrt()) / denom
    };
    let j = (p0 + t0 * d + p1 - t1 * d) * 0.5;
    let a1 = arc_from_tangent(p0, t0, j);
    let tj = a1.tangent_end();
    let a2 = arc_from_tangent(j, tj, p1);
    vec![a1, a2]
}

/// Ensemble canonique des rapports (§ 5.11).
pub fn canon_set() -> [f64; 11] {
    let phi = (1.0 + 5f64.sqrt()) / 2.0;
    [
        0.5,
        1.0 / 3.0,
        2.0 / 3.0,
        0.25,
        0.75,
        1.0 / 2f64.sqrt(),
        2f64.sqrt() - 1.0,
        3f64.sqrt() / 2.0,
        1.0 / 3f64.sqrt(),
        1.0 / phi,
        1.0 / (phi * phi),
    ]
}

/// Plus proche rapport canonique et distance logarithmique.
pub fn nearest_canon(x: f64) -> (f64, f64) {
    let mut best = (1.0, f64::INFINITY);
    for c in canon_set() {
        let d = (ln(x.max(1e-12)) - ln(c)).abs();
        if d < best.1 {
            best = (c, d);
        }
    }
    best
}

/// Division canonique d'un segment : extrême et moyenne raison.
pub fn golden_point(a: P2, b: P2) -> P2 {
    let phi = (1.0 + 5f64.sqrt()) / 2.0;
    a + (b - a) / phi
}

/// Cercle tangent à deux droites sécantes, de rayon donné, dans le secteur
/// indiqué par les côtés (congé).
pub fn fillet_lines(l1: &Line, s1: f64, l2: &Line, s2: f64, r: f64) -> Option<Circle> {
    let a = l1.offset(s1 * r);
    let b = l2.offset(s2 * r);
    inter_line_line(&a, &b).map(|c| Circle::new(c, r))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(objs: &[Obj; 3], sol: &Circle) {
        for o in objs {
            assert!(o.residual(sol.c, sol.r).abs() < 1e-9, "{o:?} {sol:?}");
        }
    }

    #[test]
    fn apollonius_three_circles() {
        let objs = [
            Obj::Circle {
                c: P2::new(0.0, 0.0),
                r: 1.0,
                s: 1.0,
            },
            Obj::Circle {
                c: P2::new(4.0, 0.0),
                r: 1.0,
                s: 1.0,
            },
            Obj::Circle {
                c: P2::new(2.0, 3.0),
                r: 0.5,
                s: 1.0,
            },
        ];
        let sols = apollonius(&objs, 1e-9);
        assert!(!sols.is_empty());
        for s in &sols {
            check(&objs, s);
        }
    }

    #[test]
    fn apollonius_lines_and_circle() {
        // carré unité : cercle tangent aux côtés gauche, bas, et au cercle
        let objs = [
            Obj::Line {
                n: P2::new(1.0, 0.0),
                d: 0.0,
                s: 1.0,
            },
            Obj::Line {
                n: P2::new(0.0, 1.0),
                d: 0.0,
                s: 1.0,
            },
            Obj::Circle {
                c: P2::new(1.0, 1.0),
                r: 0.5,
                s: 1.0,
            },
        ];
        let sols = apollonius(&objs, 1e-9);
        assert!(!sols.is_empty());
        for s in &sols {
            check(&objs, s);
        }
    }

    #[test]
    fn apollonius_three_lines() {
        let objs = [
            Obj::Line {
                n: P2::new(0.0, 1.0),
                d: 0.0,
                s: 1.0,
            },
            Obj::Line {
                n: P2::new(-1.0, 0.0),
                d: -1.0,
                s: 1.0,
            },
            Obj::Line {
                n: P2::new(1.0, 0.0),
                d: 0.0,
                s: 1.0,
            },
        ];
        let sols = apollonius(&objs, 1e-9);
        assert_eq!(sols.len(), 1);
        assert!((sols[0].r - 0.5).abs() < 1e-12);
    }

    /// Baie tiers-point canonique (§ 4.6) : oculus tangent intérieurement aux
    /// arcs principaux et extérieurement aux sous-arcs → r = s/4, y₀ = s√5/4.
    #[test]
    fn canonical_two_light_window() {
        let s = 2.0;
        let (r_main, r_sub) = (s, s / 2.0);
        let objs = [
            // arc principal gauche : centre à la naissance droite
            Obj::Circle {
                c: P2::new(s / 2.0, 0.0),
                r: r_main,
                s: -1.0,
            },
            Obj::Circle {
                c: P2::new(-s / 2.0, 0.0),
                r: r_main,
                s: -1.0,
            },
            // sous-arc : centres en (±s/2, 0), rayon s/2
            Obj::Circle {
                c: P2::new(s / 2.0, 0.0),
                r: r_sub,
                s: 1.0,
            },
        ];
        let sols = apollonius(&objs, 1e-9);
        let sol = sols
            .iter()
            .find(|c| (c.c.x).abs() < 1e-9 && c.c.y > 0.0)
            .expect("solution symétrique");
        assert!((sol.r - s / 4.0).abs() < 1e-12);
        assert!((sol.c.y - s * 5f64.sqrt() / 4.0).abs() < 1e-12);
    }

    #[test]
    fn circle_intersections() {
        let tol = Tol::new(10.0);
        let a = Circle::new(P2::ZERO, 1.0);
        let b = Circle::new(P2::new(1.0, 0.0), 1.0);
        let p = inter_circle_circle(&a, &b, &tol);
        assert_eq!(p.len(), 2);
        for q in p {
            assert!((dist(q, a.c) - 1.0).abs() < 1e-12);
            assert!((dist(q, b.c) - 1.0).abs() < 1e-12);
        }
        let c = Circle::new(P2::new(2.0, 0.0), 1.0);
        assert_eq!(inter_circle_circle(&a, &c, &tol).len(), 1);
    }

    #[test]
    fn biarc_is_g1() {
        let segs = biarc(
            P2::ZERO,
            P2::new(1.0, 0.0),
            P2::new(3.0, 1.0),
            unit(P2::new(1.0, 1.0)),
        );
        assert_eq!(segs.len(), 2);
        assert!(dist(segs[0].end(), segs[1].start()) < 1e-12);
        assert!(dist(segs[0].tangent_end(), segs[1].tangent_start()) < 1e-9);
        assert!(dist(segs[1].end(), P2::new(3.0, 1.0)) < 1e-9);
        assert!(dist(segs[1].tangent_end(), unit(P2::new(1.0, 1.0))) < 1e-9);
    }
}
