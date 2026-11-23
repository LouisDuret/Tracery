//! R09 — Redentage générique d'un jour (§ 5.6), terminal.

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use motifs::cusp::{cusp_construction, cusp_region};

pub struct Cusping;

impl Rule for Cusping {
    fn id(&self) -> &'static str {
        "R09"
    }
    fn name(&self) -> &'static str {
        "Redentage"
    }
    fn terminal(&self) -> bool {
        true
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if a.inscribed.r < 3.0 * ctx.min_opening_r() {
            return 0.0;
        }
        let base = ctx.style.cusping;
        // les lancettes et les cercles ont des règles dédiées
        if a.is(CellType::Lancet) || a.circle.is_some() {
            return 0.3 * base;
        }
        // une grande cellule de premier niveau doit être subdivisée, et les
        // cellules très allongées se redentent mal
        if ctx.depth <= 1 && a.inscribed.r > 6.0 * ctx.w {
            return 0.05 * base;
        }
        if a.elongation > 3.5 {
            return 0.3 * base;
        }
        base
    }
    fn sample(&self, _ctx: &Ctx, _a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        Params::new()
            .with("f", unif_in(rng, 0.22, 0.38))
            .with("kp", unif_in(rng, 1.05, 1.3))
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let kp = p.get("kp", 1.15);
        let mut rho = p.get("f", 0.3) * a.inscribed.r;
        // nombre de lobes ≈ périmètre·k′/(2ρ) ; rayon ≥ 1,2·w
        let per = a.region.outer.length();
        // redentage gothique : peu de grands lobes (≤ 6), jamais une frange
        rho = rho.max(per * kp / (2.0 * 6.0)).max(1.2 * ctx.w_next);
        if rho > 0.48 * a.inscribed.r {
            // cellule effilée (moitié de double mouchette, mouchette) : les
            // lobes se logent dans la tête, la queue redevient pierre
            if a.is(CellType::Tapered) && 1.2 * ctx.w_next < 0.42 * a.inscribed.r {
                rho = 0.42 * a.inscribed.r;
            } else {
                return Err(RuleError::Constraint("cellule trop étroite pour des lobes"));
            }
        }
        if rho < ctx.min_opening_r() {
            return Err(RuleError::Constraint("lobes trop petits"));
        }
        let c = cusp_region(&a.region, rho, kp, ctx.sym_axis(a))
            .ok_or(RuleError::Degenerate("offset intérieur vide"))?;
        if c.regions.is_empty() {
            return Err(RuleError::Degenerate("redentage vide"));
        }
        Ok(Expansion {
            children: vec![],
            openings: c.regions.clone(),
            construction: cusp_construction(&c),
            key_ratio: None,
        })
    }
}
