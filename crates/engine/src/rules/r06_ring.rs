//! R06 — Couronne de cercles : cercle → n cercles tangents (écart w) entre
//! eux et au bord, plus un cercle central tangent à tous (§ 4.5).

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use geom::{ConstructionOp, Region};
use motifs::foils::ring_of_circles;

pub struct Ring;

impl Rule for Ring {
    fn id(&self) -> &'static str {
        "R06"
    }
    fn name(&self) -> &'static str {
        "Couronne de cercles"
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only {
            return 0.0;
        }
        match a.circle {
            Some(c) if c.r >= 7.0 * ctx.w => 1.0,
            _ => 0.0,
        }
    }
    fn sample(&self, ctx: &Ctx, a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        let c = a.circle.unwrap_or(a.inscribed);
        let mut ok: Vec<usize> = (3..=12)
            .filter(|&n| ring_of_circles(c.c, c.r, n, ctx.w, 0.0).0[0].r >= 2.0 * ctx.w)
            .collect();
        if ok.is_empty() {
            ok.push(3);
        }
        let w: Vec<f64> = ok
            .iter()
            .map(|&n| if (4..=8).contains(&n) { 1.5 } else { 0.7 })
            .collect();
        let n = weighted_index(rng, &w).map(|i| ok[i]).unwrap_or(4);
        Params::new()
            .with("n", n as f64)
            .with("cusp_up", if unif(rng) < 0.5 { 1.0 } else { 0.0 })
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let circ = a.circle.ok_or(RuleError::NotApplicable("pas un cercle"))?;
        let n = p.get_usize("n", 6).clamp(3, 16);
        let phase = FRAC_PI_2
            + if p.get("cusp_up", 0.0) > 0.5 {
                PI / n as f64
            } else {
                0.0
            };
        let (circles, r0) = ring_of_circles(circ.c, circ.r, n, ctx.w, phase);
        if circles[0].r < 1.5 * ctx.w {
            return Err(RuleError::Constraint("cercles trop petits"));
        }
        let mut items: Vec<Region> = Vec::new();
        let mut children = Vec::new();
        for (i, ci) in circles.iter().enumerate() {
            let r = Region::circle(ci.c, ci.r);
            items.push(r.clone());
            let mut ch = ChildSpec::new(r, Some(CellType::Circle));
            if i > 0 {
                ch.twin = Some((0, Xf::rotate_about(circ.c, TAU * i as f64 / n as f64)));
            }
            children.push(ch);
        }
        if r0 >= 1.5 * ctx.w {
            let r = Region::circle(circ.c, r0);
            items.push(r.clone());
            children.push(ChildSpec::new(r, Some(CellType::Circle)));
        }
        for r in carve(&a.region, &items, ctx.carve_gap(), ctx.min_opening_r()) {
            children.push(ChildSpec::new(r, None));
        }
        let mut cons: Vec<ConstructionEntity> = circles
            .iter()
            .map(|c| ConstructionEntity::point(c.c, ConstructionOp::Rose))
            .collect();
        cons.push(ConstructionEntity::Circle {
            c: circ.c,
            r: dist(circles[0].c, circ.c),
            op: ConstructionOp::Rose,
        });
        Ok(Expansion {
            children,
            openings: vec![],
            construction: cons,
            key_ratio: Some(circles[0].r / circ.r),
        })
    }
}
