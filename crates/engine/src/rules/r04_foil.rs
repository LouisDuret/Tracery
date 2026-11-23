//! R04 — Polylobe : cercle → polylobe terminal (§ 4.5).

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use motifs::foils::Foil;

pub struct Polylobe;

impl Rule for Polylobe {
    fn id(&self) -> &'static str {
        "R04"
    }
    fn name(&self) -> &'static str {
        "Polylobe"
    }
    fn terminal(&self) -> bool {
        true
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if a.circle.is_none() && a.confidence(CellType::Circle) < 0.9 {
            return 0.0;
        }
        // lobes assez grands
        let r = a.inscribed.r;
        if r < 2.0 * ctx.min_opening_r() {
            return 0.0;
        }
        1.0
    }
    fn sample(&self, ctx: &Ctx, _a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        let foils = &ctx.style.foils;
        let w: Vec<f64> = foils
            .iter()
            .map(|n| match n {
                4 => 1.4,
                3 | 6 => 1.1,
                _ => 0.8,
            })
            .collect();
        let n = weighted_index(rng, &w).map(|i| foils[i]).unwrap_or(4) as f64;
        let k = (ctx.style.k + unif_in(rng, -0.08, 0.12)).clamp(1.0, 1.5);
        let cusp_up = if unif(rng) < 0.3 { 1.0 } else { 0.0 };
        Params::new()
            .with("n", n)
            .with("k", k)
            .with("cusp_up", cusp_up)
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let c = a.circle.unwrap_or(a.inscribed);
        let n = p.get_usize("n", 4).clamp(3, 12);
        let phase = FRAC_PI_2
            + if p.get("cusp_up", 0.0) > 0.5 {
                PI / n as f64
            } else {
                0.0
            };
        let foil = Foil {
            c: c.c,
            ri: c.r,
            n,
            k: p.get("k", 1.15),
            phase,
        };
        if foil.r() < ctx.min_opening_r() {
            return Err(RuleError::Constraint("lobes trop petits"));
        }
        Ok(Expansion {
            children: vec![],
            openings: vec![geom::Region::simple(foil.to_loop())],
            construction: foil.construction(),
            key_ratio: Some(foil.r() / c.r),
        })
    }
}
