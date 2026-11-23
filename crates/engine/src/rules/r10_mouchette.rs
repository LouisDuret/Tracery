//! R10 — Mouchette, R11 — Soufflet / dague : jour effilé ajusté à une
//! cellule (§ 4.7).
//!
//! Tête : cercle inscrit maximal (h, ρ) réduit d'un facteur f. Queue : coin
//! le plus aigu T (légèrement rentré). Chaque côté est un arc passant par T
//! et tangent au cercle de tête :
//! - intérieurement (côté convexe) : |c − T| = R, |c − h| = R − ρ ;
//! - extérieurement (côté concave, contre-courbe) : |c − T| = R, |c − h| = R + ρ.
//!
//! Parmi des rayons R tirés d'une liste, on garde pour chaque côté l'arc le
//! plus bombé qui reste dans la cellule. R11 impose le même choix des deux
//! côtés (soufflet symétrique), R10 les choisit indépendamment (mouchette).

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use geom::constr::inter_circle_circle;
use geom::{ConstructionOp, Region, Seg};

pub struct Mouchette {
    pub id: &'static str,
    pub symmetric: bool,
}

#[derive(Clone, Copy, Debug)]
struct Side {
    seg: Seg,
    center: P2,
    a: P2,
    bulge: f64,
    key: (bool, usize),
}

/// Arcs candidats d'un côté. `right` : côté droit de l'axe T→h.
fn side_candidates(f: &Region, t: P2, h: P2, rho: f64, right: bool, margin: f64) -> Vec<Side> {
    let dd = dist(t, h);
    let u = unit(h - t);
    let tol = Tol::new(dd * 8.0);
    let mut out = Vec::new();
    let factors = [0.75, 1.0, 1.4, 2.0, 3.0, 5.0, 9.0];
    for internal in [true, false] {
        for (fi, &k) in factors.iter().enumerate() {
            let r = k * dd;
            let rh = if internal { r - rho } else { r + rho };
            if rh <= 0.0 {
                continue;
            }
            let cs = inter_circle_circle(&Circle::new(t, r), &Circle::new(h, rh), &tol);
            if cs.len() != 2 {
                continue;
            }
            // côté droit convexe : centre à gauche de l'axe ; concave : à droite
            let want_left = internal == right;
            let c = if (cross(u, cs[0] - t) > 0.0) == want_left {
                cs[0]
            } else {
                cs[1]
            };
            let a = c + unit(h - c) * r;
            // sens : intérieur → trigonométrique pour le côté droit
            let ccw = internal;
            let seg = if right {
                Seg::arc_from_to(c, t, a, ccw)
            } else {
                Seg::arc_from_to(c, a, t, ccw)
            };
            if seg.length() > 3.5 * dd {
                continue; // grand tour : configuration inversée
            }
            let pts = seg.sample(48);
            if pts.iter().skip(1).any(|p| f.sdf(*p) < margin) {
                continue;
            }
            // bombement : écart maximal à l'axe, du bon côté
            let sgn = if right { -1.0 } else { 1.0 };
            let bulge = pts
                .iter()
                .map(|p| sgn * cross(u, *p - t))
                .fold(f64::NEG_INFINITY, f64::max);
            out.push(Side {
                seg,
                center: c,
                a,
                bulge,
                key: (internal, fi),
            });
        }
    }
    out
}

impl Rule for Mouchette {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        if self.symmetric {
            "Soufflet"
        } else {
            "Mouchette"
        }
    }
    fn terminal(&self) -> bool {
        true
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if a.inscribed.r < 2.5 * ctx.min_opening_r()
            || !a.corners.iter().any(|c| c.interior < 0.6 * PI)
        {
            return 0.0;
        }
        if ctx.depth <= 1 && a.inscribed.r > 6.0 * ctx.w {
            return 0.0;
        }
        let t = |c: CellType| a.confidence(c);
        let base = t(CellType::Tapered)
            .max(t(CellType::Lunule))
            .max(0.5 * t(CellType::Triangle))
            .max(0.4 * t(CellType::Irregular))
            .max(0.3 * t(CellType::Quad));
        if self.symmetric {
            base.max(t(CellType::Lens))
        } else {
            base
        }
    }
    fn sample(&self, _ctx: &Ctx, a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        let sharp: Vec<usize> = (0..a.corners.len())
            .filter(|&i| a.corners[i].interior < 0.6 * PI)
            .collect();
        let pick = if sharp.is_empty() {
            0
        } else {
            sharp[(unif(rng) * sharp.len() as f64) as usize % sharp.len()]
        };
        Params::new()
            .with("corner", pick as f64)
            .with("head", unif_in(rng, 0.78, 0.95))
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let f = &a.region;
        let ci = p
            .get_usize("corner", 0)
            .min(a.corners.len().saturating_sub(1));
        let corner = a
            .corners
            .get(ci)
            .ok_or(RuleError::NotApplicable("pas de coin"))?;
        let head = a.inscribed;
        let rho = head.r * p.get("head", 0.9);
        let h = head.c;
        // queue rentrée vers la tête
        let t = corner.p + unit(h - corner.p) * (0.12 * rho);
        if dist(t, h) < 1.5 * rho {
            return Err(RuleError::Degenerate("queue trop courte"));
        }
        let margin = 1e-7 * ctx.tol.l;
        let right = side_candidates(f, t, h, rho, true, margin);
        let left = side_candidates(f, t, h, rho, false, margin);
        let (rs, ls) = if self.symmetric {
            let mut best: Option<(Side, Side, f64)> = None;
            for r in &right {
                if let Some(l) = left.iter().find(|l| l.key == r.key) {
                    let v = r.bulge.min(l.bulge);
                    if best.as_ref().is_none_or(|b| v > b.2) {
                        best = Some((*r, *l, v));
                    }
                }
            }
            let b = best.ok_or(RuleError::Constraint("aucun soufflet ne tient"))?;
            (b.0, b.1)
        } else {
            let pick = |v: &[Side]| v.iter().copied().max_by(|x, y| x.bulge.total_cmp(&y.bulge));
            (
                pick(&right).ok_or(RuleError::Constraint("côté droit"))?,
                pick(&left).ok_or(RuleError::Constraint("côté gauche"))?,
            )
        };
        let head_arc = Seg::arc_from_to(h, rs.a, ls.a, true);
        // la tête doit passer du côté opposé à la queue
        let far = h + unit(h - t) * rho;
        if head_arc.dist(far) > 1e-6 * rho {
            return Err(RuleError::Degenerate("tête inversée"));
        }
        let l = geom::Loop::new(vec![rs.seg, head_arc, ls.seg]);
        if !l.is_ccw() || geom::clip::self_intersects(&l) {
            return Err(RuleError::Degenerate("contour invalide"));
        }
        if !fits_inside(f, &l, -1e-6 * ctx.tol.l) {
            return Err(RuleError::Constraint("déborde de la cellule"));
        }
        let op = if self.symmetric {
            ConstructionOp::Soufflet
        } else {
            ConstructionOp::Mouchette
        };
        Ok(Expansion {
            children: vec![],
            openings: vec![Region::simple(l)],
            construction: vec![
                ConstructionEntity::Circle { c: h, r: rho, op },
                ConstructionEntity::point(h, op),
                ConstructionEntity::point(rs.center, op),
                ConstructionEntity::point(ls.center, op),
                ConstructionEntity::point(t, op),
            ],
            key_ratio: Some(rho / dist(t, h)),
        })
    }
}
