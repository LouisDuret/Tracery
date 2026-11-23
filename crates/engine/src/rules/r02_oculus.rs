//! R02 — Oculus : cellule → cercle inscrit maximal (tangences exactes par
//! Apollonius sur les trois bords les plus contraignants) + écoinçons.

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use geom::{ConstructionOp, Region};

pub struct Oculus;

impl Rule for Oculus {
    fn id(&self) -> &'static str {
        "R02"
    }
    fn name(&self) -> &'static str {
        "Oculus"
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only || a.circle.is_some() || a.inscribed.r < ctx.edge_gap() + 2.0 * ctx.w {
            return 0.0;
        }
        let t = |c: CellType| a.confidence(c);
        let w = (t(CellType::Tympan) * 1.0)
            .max(t(CellType::Sector) * 0.9)
            .max(t(CellType::Triangle) * 0.8)
            .max(t(CellType::Quad) * 0.7)
            .max(t(CellType::Lens) * 0.6)
            .max(t(CellType::Irregular) * 0.5)
            .max(t(CellType::Polygon) * 0.5)
            .max(t(CellType::Lunule) * 0.3)
            .max(t(CellType::Arch) * 0.3)
            .max(t(CellType::Annulus) * 0.2)
            .max(t(CellType::Panel) * 0.2);
        w.max(0.1)
    }
    fn sample(&self, ctx: &Ctx, a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        let shift = if ctx.sym_axis(a).is_some() || matches!(ctx.sym, crate::style::SymMode::Strict)
        {
            unif_in(rng, -0.08, 0.08)
        } else {
            unif_in(rng, -0.15, 0.15)
        };
        Params::new()
            .with("shift", shift)
            .with("fill", unif_in(rng, 0.9, 1.0))
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let f = &a.region;
        let g = ctx.edge_gap();
        let mut c =
            inscribed_with_gap(f, g).ok_or(RuleError::Degenerate("pas de cercle inscrit"))?;
        if let Some(ax) = ctx.sym_axis(a)
            && (c.c.x - ax).abs() > 1e-9 * ctx.tol.l
        {
            // recherche sur l'axe
            let mut best = (f64::NEG_INFINITY, c.c);
            let bb = f.bbox();
            for k in 0..=64 {
                let y = bb.min.y + (bb.max.y - bb.min.y) * k as f64 / 64.0;
                let q = P2::new(ax, y);
                let v = f.sdf(q) - g;
                if v > best.0 {
                    best = (v, q);
                }
            }
            let (q, v) = geom::sdf::refine_max(f, &[], g, best.1, (bb.max.y - bb.min.y) / 64.0);
            let q = P2::new(ax, q.y);
            c = Circle::new(q, (f.sdf(q) - g).min(v));
        }
        let shift = p.get("shift", 0.0) * c.r;
        let center = c.c + P2::new(0.0, shift);
        let r = (f.sdf(center) - g) * p.get("fill", 1.0);
        if r < 1.5 * ctx.w {
            return Err(RuleError::Constraint("oculus trop petit"));
        }
        let oc = Circle::new(center, r);
        let ring = Region::circle(oc.c, oc.r);
        let mut children = vec![ChildSpec::new(ring.clone(), Some(CellType::Circle))];
        for r in carve(f, &[ring], ctx.carve_gap(), ctx.min_opening_r()) {
            children.push(ChildSpec::new(r, None));
        }
        Ok(Expansion {
            children,
            openings: vec![],
            construction: vec![
                ConstructionEntity::circle(oc, ConstructionOp::Apollonius),
                ConstructionEntity::point(oc.c, ConstructionOp::Apollonius),
            ],
            key_ratio: Some(r / a.inscribed.r.max(1e-12)),
        })
    }
}
