//! Distance signée exacte, cercles inscrits maximaux (§ 5.2, § 5.5).
//!
//! La fonction « libre » d'une région F avec des obstacles circulaires
//! (cⱼ, rⱼ) et un écart w est :
//!   libre(x) = min(d_∂F(x) − g_bord, minⱼ(|x − cⱼ| − rⱼ − w))
//! Le maximum de libre(x) donne le plus grand cercle plaçable en x.
//! Ce maximum, obtenu numériquement, est ensuite « accroché » par une
//! résolution d'Apollonius sur les trois contraintes actives, ce qui donne
//! des tangences exactes (construction au compas).

use crate::constr::{Obj, apollonius};
use crate::math::*;
use crate::seg::{Circle, Region, Seg};

/// Obstacle circulaire avec écart minimal.
#[derive(Clone, Copy, Debug)]
pub struct Obstacle {
    pub c: Circle,
    pub gap: f64,
}

/// Fonction libre (voir l'en-tête du module).
pub fn free_value(r: &Region, obstacles: &[Obstacle], edge_gap: f64, p: P2) -> f64 {
    let mut v = r.sdf(p) - edge_gap;
    for o in obstacles {
        v = v.min(dist(p, o.c.c) - o.c.r - o.gap);
    }
    v
}

/// Grille de valeurs libres sur la boîte englobante.
pub struct FreeGrid {
    pub n: usize,
    pub origin: P2,
    pub step: f64,
    pub vals: Vec<f64>,
}

impl FreeGrid {
    pub fn new(r: &Region, obstacles: &[Obstacle], edge_gap: f64, n: usize) -> FreeGrid {
        let bb = r.bbox();
        let size = bb.size();
        let step = size.x.max(size.y) / n as f64;
        let origin = bb.min + P2::splat(step * 0.5);
        let mut vals = vec![f64::NEG_INFINITY; n * n];
        for j in 0..n {
            for i in 0..n {
                let p = origin + P2::new(i as f64, j as f64) * step;
                if p.x > bb.max.x || p.y > bb.max.y {
                    continue;
                }
                vals[j * n + i] = free_value(r, obstacles, edge_gap, p);
            }
        }
        FreeGrid {
            n,
            origin,
            step,
            vals,
        }
    }
    pub fn pos(&self, i: usize, j: usize) -> P2 {
        self.origin + P2::new(i as f64, j as f64) * self.step
    }
    /// Maxima locaux (8-voisinage), triés par valeur décroissante puis par
    /// indice (ordre déterministe).
    pub fn local_maxima(&self, min_val: f64) -> Vec<(P2, f64)> {
        let n = self.n;
        let mut out = Vec::new();
        for j in 0..n {
            for i in 0..n {
                let v = self.vals[j * n + i];
                if v < min_val || !v.is_finite() {
                    continue;
                }
                let mut is_max = true;
                'nb: for dj in -1i64..=1 {
                    for di in -1i64..=1 {
                        if di == 0 && dj == 0 {
                            continue;
                        }
                        let (ii, jj) = (i as i64 + di, j as i64 + dj);
                        if ii < 0 || jj < 0 || ii >= n as i64 || jj >= n as i64 {
                            continue;
                        }
                        let w = self.vals[jj as usize * n + ii as usize];
                        // départage strict par indice pour les plateaux
                        let idx = (jj as usize * n + ii as usize) < j * n + i;
                        if w > v || (w == v && idx) {
                            is_max = false;
                            break 'nb;
                        }
                    }
                }
                if is_max {
                    out.push((self.pos(i, j), v));
                }
            }
        }
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        out
    }
}

/// Raffinement local d'un maximum par recherche de motif (déterministe).
pub fn refine_max(
    r: &Region,
    obstacles: &[Obstacle],
    edge_gap: f64,
    start: P2,
    step0: f64,
) -> (P2, f64) {
    let f = |p: P2| free_value(r, obstacles, edge_gap, p);
    let mut p = start;
    let mut v = f(p);
    let mut step = step0;
    let min_step = step0 * 1e-7;
    let dirs = [
        P2::new(1.0, 0.0),
        P2::new(-1.0, 0.0),
        P2::new(0.0, 1.0),
        P2::new(0.0, -1.0),
        P2::new(
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        P2::new(
            -std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        P2::new(
            std::f64::consts::FRAC_1_SQRT_2,
            -std::f64::consts::FRAC_1_SQRT_2,
        ),
        P2::new(
            -std::f64::consts::FRAC_1_SQRT_2,
            -std::f64::consts::FRAC_1_SQRT_2,
        ),
    ];
    let mut iters = 0;
    while step > min_step && iters < 400 {
        iters += 1;
        let mut improved = false;
        for d in dirs {
            let q = p + d * step;
            let w = f(q);
            if w > v {
                p = q;
                v = w;
                improved = true;
                break;
            }
        }
        if !improved {
            step *= 0.5;
        }
    }
    (p, v)
}

/// Cercle inscrit maximal approché (pôle d'inaccessibilité).
pub fn max_inscribed_approx(
    r: &Region,
    obstacles: &[Obstacle],
    edge_gap: f64,
    n: usize,
) -> Option<(P2, f64)> {
    let g = FreeGrid::new(r, obstacles, edge_gap, n);
    let maxima = g.local_maxima(0.0);
    let mut best: Option<(P2, f64)> = None;
    for &(p, _) in maxima.iter().take(4) {
        let (q, v) = refine_max(r, obstacles, edge_gap, p, g.step);
        if best.is_none_or(|b| v > b.1) {
            best = Some((q, v));
        }
    }
    best.filter(|b| b.1 > 0.0)
}

/// Contrainte active candidate pour l'accrochage.
#[derive(Clone, Copy, Debug)]
struct Active {
    obj: Obj,
    slack: f64,
    key: P2,
}

/// Construit les objets d'Apollonius à partir du bord de la région et des
/// obstacles, pour un cercle approché (c0, r0).
fn active_constraints(
    r: &Region,
    obstacles: &[Obstacle],
    edge_gap: f64,
    c0: P2,
    r0: f64,
) -> Vec<Active> {
    let mut out: Vec<Active> = Vec::new();
    for s in r.segs() {
        let (q, t, d) = s.closest(c0);
        let slack = d - r0 - edge_gap;
        let interior = t > 1e-9 && t < 1.0 - 1e-9;
        let obj = match *s {
            Seg::Line { a, b } if interior => {
                let mut n = unit(perp(b - a));
                if dot(n, c0 - a) < 0.0 {
                    n = -n;
                }
                Obj::Line {
                    n,
                    d: dot(n, a) + edge_gap,
                    s: 1.0,
                }
            }
            Seg::Arc { c, r: ra, .. } if interior => {
                if dist(c0, c) < ra {
                    Obj::Circle {
                        c,
                        r: ra - edge_gap,
                        s: -1.0,
                    }
                } else {
                    Obj::Circle {
                        c,
                        r: ra + edge_gap,
                        s: 1.0,
                    }
                }
            }
            _ => {
                if edge_gap > 0.0 {
                    Obj::Circle {
                        c: q,
                        r: edge_gap,
                        s: 1.0,
                    }
                } else {
                    Obj::Point(q)
                }
            }
        };
        out.push(Active { obj, slack, key: q });
    }
    for o in obstacles {
        let d = dist(c0, o.c.c);
        out.push(Active {
            obj: Obj::Circle {
                c: o.c.c,
                r: o.c.r + o.gap,
                s: 1.0,
            },
            slack: d - o.c.r - o.gap - r0,
            key: o.c.c + unit(c0 - o.c.c) * o.c.r,
        });
    }
    out.sort_by(|a, b| a.slack.total_cmp(&b.slack));
    // dédoublonnage des contacts confondus (sommets partagés)
    let mut uniq: Vec<Active> = Vec::new();
    for a in out {
        if uniq
            .iter()
            .any(|u| dist(u.key, a.key) < 1e-7 * (1.0 + r0) && obj_same(&u.obj, &a.obj))
        {
            continue;
        }
        uniq.push(a);
    }
    uniq
}

fn obj_same(a: &Obj, b: &Obj) -> bool {
    match (a, b) {
        (Obj::Circle { c: c1, r: r1, .. }, Obj::Circle { c: c2, r: r2, .. }) => {
            dist(*c1, *c2) < 1e-9 && (r1 - r2).abs() < 1e-9
        }
        (Obj::Point(p), Obj::Point(q)) => dist(*p, *q) < 1e-9,
        _ => false,
    }
}

/// Cercle inscrit maximal exact : maximum numérique puis accrochage par
/// Apollonius sur les trois contraintes les plus actives. Renvoie le cercle
/// (centre, rayon libre) et un indicateur d'exactitude.
pub fn max_inscribed(
    r: &Region,
    obstacles: &[Obstacle],
    edge_gap: f64,
    n: usize,
) -> Option<(Circle, bool)> {
    let (c0, r0) = max_inscribed_approx(r, obstacles, edge_gap, n)?;
    Some(snap_circle(r, obstacles, edge_gap, c0, r0))
}

/// Accroche un cercle approché sur les contraintes actives (tangences
/// exactes). En cas d'échec, renvoie le cercle approché.
pub fn snap_circle(
    r: &Region,
    obstacles: &[Obstacle],
    edge_gap: f64,
    c0: P2,
    r0: f64,
) -> (Circle, bool) {
    let act = active_constraints(r, obstacles, edge_gap, c0, r0);
    let scale = r.bbox().diag();
    let tol = 1e-9 * scale;
    if act.len() >= 3 {
        // essayer les combinaisons parmi les 5 plus actives
        let m = act.len().min(5);
        let mut best: Option<(Circle, f64)> = None;
        for i in 0..m {
            for j in (i + 1)..m {
                for k in (j + 1)..m {
                    let objs = [act[i].obj, act[j].obj, act[k].obj];
                    for sol in apollonius(&objs, 1e-12 * scale) {
                        if dist(sol.c, c0) > 0.25 * r0 + 1e-6 * scale
                            || (sol.r - r0).abs() > 0.25 * r0
                        {
                            continue;
                        }
                        // faisabilité
                        let fv = free_value(r, obstacles, edge_gap, sol.c);
                        if fv < sol.r - 1e3 * tol {
                            continue;
                        }
                        let score = (sol.r - r0).abs() + dist(sol.c, c0);
                        if best.is_none_or(|b| score < b.1) {
                            best = Some((sol, score));
                        }
                    }
                }
            }
        }
        if let Some((c, _)) = best {
            return (c, true);
        }
    }
    (Circle::new(c0, r0), false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seg::Loop;

    #[test]
    fn inscribed_in_square_is_exact() {
        let sq = Region::simple(Loop::polygon(&[
            P2::new(0.0, 0.0),
            P2::new(4.0, 0.0),
            P2::new(4.0, 4.0),
            P2::new(0.0, 4.0),
        ]));
        let (c, exact) = max_inscribed(&sq, &[], 0.0, 24).expect("cercle");
        assert!(exact);
        assert!((c.r - 2.0).abs() < 1e-9);
        assert!(dist(c.c, P2::new(2.0, 2.0)) < 1e-9);
    }

    #[test]
    fn inscribed_in_triangle() {
        let tri = Region::simple(Loop::polygon(&[
            P2::new(0.0, 0.0),
            P2::new(3.0, 0.0),
            P2::new(0.0, 4.0),
        ]));
        // rayon inscrit = aire / demi-périmètre = 6 / 6 = 1
        let (c, _) = max_inscribed(&tri, &[], 0.0, 32).expect("cercle");
        assert!((c.r - 1.0).abs() < 1e-9, "{c:?}");
    }
}
