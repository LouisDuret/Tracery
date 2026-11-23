//! R05 — Rose radiale : cercle → oculus central + n secteurs séparés par des
//! meneaux radiaux (§ 4.8). Chaque bord de secteur est une droite parallèle
//! au rayon à la distance w/2 : p(t) = c + t·u + (w/2)·n, avec
//! t = √(ρ² − w²/4) sur le cercle de rayon ρ.

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use geom::{ConstructionOp, Region, Seg};

pub struct Rose;

fn max_n(big_r: f64, w: f64) -> usize {
    ((TAU * big_r) / (5.0 * w)).floor() as usize
}

impl Rule for Rose {
    fn id(&self) -> &'static str {
        "R05"
    }
    fn name(&self) -> &'static str {
        "Rose radiale"
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only {
            return 0.0;
        }
        match ring_of(a) {
            Some((c, _)) if c.r >= 9.0 * ctx.w && max_n(c.r, ctx.w) >= 5 => {
                if a.circle.is_some() {
                    1.0
                } else {
                    1.5
                }
            }
            _ => 0.0,
        }
    }
    fn sample(&self, ctx: &Ctx, a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        let c = ring_of(a).map(|r| r.0).unwrap_or(a.inscribed);
        let nmax = max_n(c.r, ctx.w).clamp(5, 16);
        let n = 5 + (unif(rng) * (nmax - 4) as f64).floor() as usize;
        Params::new()
            .with("n", n.min(nmax) as f64)
            .with("r0", unif_in(rng, 0.18, 0.38))
            .with("spoke_up", if unif(rng) < 0.5 { 1.0 } else { 0.0 })
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let (circ, hole) = ring_of(a).ok_or(RuleError::NotApplicable("pas un cercle"))?;
        let (c, big_r) = (circ.c, circ.r);
        let w = ctx.w;
        let n = p.get_usize("n", 8).clamp(3, 48);
        // couronne : le trou tient lieu d'oculus central
        let rc = hole.unwrap_or(p.get("r0", 0.25) * big_r);
        let r_in = if hole.is_some() { rc } else { rc + w };
        let phase = FRAC_PI_2
            + if p.get("spoke_up", 1.0) > 0.5 {
                0.0
            } else {
                PI / n as f64
            };
        if r_in <= w {
            return Err(RuleError::Constraint("oculus central trop petit"));
        }
        let da = TAU / n as f64;
        let half_in = asin((w / 2.0) / r_in);
        if da <= 2.0 * half_in + 2.5 * w / r_in {
            return Err(RuleError::Constraint("secteurs trop étroits"));
        }
        let t_in = (r_in * r_in - w * w / 4.0).sqrt();
        let t_out = (big_r * big_r - w * w / 4.0).sqrt();
        let sector = |i: usize| -> Region {
            let a0 = phase + da * i as f64;
            let a1 = a0 + da;
            let (u0, u1) = (dir(a0), dir(a1));
            let p0 = c + u0 * t_in + perp(u0) * (w / 2.0);
            let q0 = c + u0 * t_out + perp(u0) * (w / 2.0);
            let q1 = c + u1 * t_out - perp(u1) * (w / 2.0);
            let p1 = c + u1 * t_in - perp(u1) * (w / 2.0);
            Region::simple(geom::Loop::new(vec![
                Seg::line(p0, q0),
                Seg::arc_from_to(c, q0, q1, true),
                Seg::line(q1, p1),
                Seg::arc_from_to(c, p1, p0, false),
            ]))
        };
        let mut children = Vec::new();
        if hole.is_none() {
            children.push(ChildSpec::new(
                Region::circle(c, rc),
                Some(CellType::Circle),
            ));
        }
        let first = children.len();
        for i in 0..n {
            let mut ch = ChildSpec::new(sector(i), Some(CellType::Sector));
            if i > 0 {
                ch.twin = Some((first, Xf::rotate_about(c, da * i as f64)));
            }
            children.push(ch);
        }
        let mut cons = vec![
            ConstructionEntity::circle(Circle::new(c, rc), ConstructionOp::Rose),
            ConstructionEntity::point(c, ConstructionOp::Rose),
        ];
        for i in 0..n {
            let u = dir(phase + da * i as f64);
            cons.push(ConstructionEntity::Line {
                a: c + u * rc,
                b: c + u * big_r,
                op: ConstructionOp::Axis,
            });
        }
        Ok(Expansion {
            children,
            openings: vec![],
            construction: cons,
            key_ratio: Some(rc / big_r),
        })
    }
}

/// Cercle extérieur et rayon du trou concentrique éventuel (couronne).
fn ring_of(a: &Analysis) -> Option<(Circle, Option<f64>)> {
    if let Some(c) = a.circle {
        return Some((c, None));
    }
    if a.region.holes.len() == 1 {
        let tol = 1e-6 * a.bbox.diag();
        let o = a.region.outer.as_circle(tol)?;
        let h = a.region.holes[0].as_circle(tol)?;
        if dist(o.c, h.c) < tol && h.r < o.r {
            return Some((o, Some(h.r)));
        }
    }
    None
}
