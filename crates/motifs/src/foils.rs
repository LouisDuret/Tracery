//! Polylobes, redents et couronnes de cercles (§ 4.5).
//!
//! Polylobe à n lobes inscrit dans un cercle de rayon Rᵢ : centres des
//! lobes sur un cercle de rayon d, à la phase φ ; chaque lobe est tangent
//! intérieurement au cercle Rᵢ, et le chevauchement k ≥ 1 règle les redents :
//!   d = Rᵢ / (1 + k·sin(π/n)),   r = Rᵢ − d.
//! Pointe des redents : ρ_c = d·cos(π/n) + √(r² − d²·sin²(π/n)), aux angles
//! φ + (2j + 1)π/n.

use geom::math::*;
use geom::{Circle, ConstructionEntity, ConstructionOp, Loop, Seg};

/// Paramètres d'un polylobe.
#[derive(Clone, Copy, Debug)]
pub struct Foil {
    pub c: P2,
    pub ri: f64,
    pub n: usize,
    pub k: f64,
    pub phase: f64,
}

impl Foil {
    pub fn d(&self) -> f64 {
        self.ri / (1.0 + self.k * sin(PI / self.n as f64))
    }
    pub fn r(&self) -> f64 {
        self.ri - self.d()
    }
    /// Distance des pointes des redents au centre.
    pub fn cusp_radius(&self) -> f64 {
        let (d, r) = (self.d(), self.r());
        let sn = sin(PI / self.n as f64);
        d * cos(PI / self.n as f64) + (r * r - d * d * sn * sn).max(0.0).sqrt()
    }
    pub fn lobes(&self) -> Vec<Circle> {
        let (d, r) = (self.d(), self.r());
        (0..self.n)
            .map(|j| {
                Circle::new(
                    self.c + dir(self.phase + TAU * j as f64 / self.n as f64) * d,
                    r,
                )
            })
            .collect()
    }
    /// Contour du jour : union des disques des lobes (et du polygone de
    /// leurs centres), parcouru dans le sens trigonométrique.
    pub fn to_loop(&self) -> Loop {
        let n = self.n as f64;
        let rc = self.cusp_radius();
        let lobes = self.lobes();
        let segs: Vec<Seg> = lobes
            .iter()
            .enumerate()
            .map(|(j, l)| {
                let j = j as f64;
                let cb = self.c + dir(self.phase + (2.0 * j - 1.0) * PI / n) * rc;
                let ca = self.c + dir(self.phase + (2.0 * j + 1.0) * PI / n) * rc;
                Seg::arc_from_to(l.c, cb, ca, true)
            })
            .collect();
        Loop::new(segs)
    }
    pub fn construction(&self) -> Vec<ConstructionEntity> {
        let mut v = vec![ConstructionEntity::Circle {
            c: self.c,
            r: self.d(),
            op: ConstructionOp::FoilLobe,
        }];
        for l in self.lobes() {
            v.push(ConstructionEntity::point(l.c, ConstructionOp::FoilLobe));
        }
        v
    }
}

/// Couronne de n cercles tangents entre eux et intérieurement au cercle
/// (c, R), avec un écart `gap` entre cercles et avec le bord :
///   d + r = R − gap,  2·d·sin(π/n) = 2r + gap
/// ⇒ d = (2R − gap) / (2(1 + sin(π/n))).
/// Renvoie (cercles, rayon du cercle central tangent à tous avec l'écart).
pub fn ring_of_circles(c: P2, big_r: f64, n: usize, gap: f64, phase: f64) -> (Vec<Circle>, f64) {
    let sn = sin(PI / n as f64);
    let d = (2.0 * big_r - gap) / (2.0 * (1.0 + sn));
    let r = big_r - gap - d;
    let circles = (0..n)
        .map(|j| Circle::new(c + dir(phase + TAU * j as f64 / n as f64) * d, r))
        .collect();
    (circles, d - r - gap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foil_tangencies() {
        for n in 3..=8 {
            for k in [1.0, 1.15, 1.4] {
                let f = Foil {
                    c: P2::new(1.0, -2.0),
                    ri: 3.0,
                    n,
                    k,
                    phase: FRAC_PI_2,
                };
                let l = f.to_loop();
                assert!(l.is_ccw(), "n={n}");
                // chaque lobe est tangent intérieurement au cercle Rᵢ
                for lobe in f.lobes() {
                    assert!((dist(lobe.c, f.c) + lobe.r - f.ri).abs() < 1e-12);
                }
                // la boucle est fermée
                for (i, s) in l.segs.iter().enumerate() {
                    let nx = &l.segs[(i + 1) % l.segs.len()];
                    assert!(dist(s.end(), nx.start()) < 1e-9);
                }
                if k == 1.0 {
                    // lobes tangents entre eux : r = d·sin(π/n)
                    assert!((f.r() - f.d() * sin(PI / n as f64)).abs() < 1e-12);
                }
            }
        }
    }

    #[test]
    fn ring_tangency() {
        let (cs, r0) = ring_of_circles(P2::ZERO, 10.0, 6, 0.5, 0.0);
        for (i, a) in cs.iter().enumerate() {
            let b = &cs[(i + 1) % cs.len()];
            assert!((dist(a.c, b.c) - a.r - b.r - 0.5).abs() < 1e-12);
            assert!((10.0 - dist(a.c, P2::ZERO) - a.r - 0.5).abs() < 1e-12);
            assert!((dist(a.c, P2::ZERO) - a.r - r0 - 0.5).abs() < 1e-12);
        }
    }
}
