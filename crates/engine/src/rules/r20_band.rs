//! R20 — Bande : frise de cercles répétés le long de l'axe principal, de
//! rayon égal au rayon inscrit (avec écart), espacés de w.

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use geom::sdf::free_value;
use geom::{ConstructionOp, Region};

pub struct Band;

impl Rule for Band {
    fn id(&self) -> &'static str {
        "R20"
    }
    fn name(&self) -> &'static str {
        "Frise"
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only || a.elongation < 3.0 || a.inscribed.r < ctx.edge_gap() + 2.0 * ctx.w {
            return 0.0;
        }
        a.confidence(CellType::Band)
            .max(0.4 * a.confidence(CellType::Polygon))
    }
    fn sample(&self, _ctx: &Ctx, _a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        Params::new().with("fill", unif_in(rng, 0.8, 1.0))
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let f = &a.region;
        let g = ctx.edge_gap();
        let w = ctx.w;
        let ins = inscribed_with_gap(f, g).ok_or(RuleError::Degenerate("pas de cercle"))?;
        let rho = ins.r * p.get("fill", 1.0);
        // axe : direction de la plus grande dimension de la boîte
        let bb = a.bbox;
        let horiz = bb.size().x >= bb.size().y;
        let (lo, hi) = if horiz {
            (bb.min.x, bb.max.x)
        } else {
            (bb.min.y, bb.max.y)
        };
        let avail = hi - lo - 2.0 * g;
        let n = ((avail + w) / (2.0 * rho + w)).floor() as usize;
        if n < 2 {
            return Err(RuleError::Constraint("bande trop courte"));
        }
        let pitch = (avail - 2.0 * rho) / (n as f64 - 1.0).max(1.0);
        let mut regions = Vec::new();
        let mut children = Vec::new();
        for i in 0..n {
            let t = lo + g + rho + pitch * i as f64;
            let c = if horiz {
                P2::new(t, ins.c.y)
            } else {
                P2::new(ins.c.x, t)
            };
            let r = free_value(f, &[], g, c).min(rho);
            if r < 1.5 * w {
                continue;
            }
            regions.push(Region::circle(c, r));
            children.push(ChildSpec::new(Region::circle(c, r), Some(CellType::Circle)));
        }
        if regions.len() < 2 {
            return Err(RuleError::Constraint("frise vide"));
        }
        let cons = regions
            .iter()
            .filter_map(|r| r.outer.as_circle(1e-9))
            .map(|c| ConstructionEntity::point(c.c, ConstructionOp::Inscribed))
            .collect();
        for r in carve(f, &regions, ctx.carve_gap(), ctx.min_opening_r()) {
            children.push(ChildSpec::new(r, None));
        }
        Ok(Expansion {
            children,
            openings: vec![],
            construction: cons,
            key_ratio: Some(rho / ins.r.max(1e-12)),
        })
    }
}
