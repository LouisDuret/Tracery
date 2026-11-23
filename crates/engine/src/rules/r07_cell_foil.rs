//! R07 / R08 — Triangle ou quadrilatère curviligne → trilobe ou quadrilobe
//! inscrit, lobes orientés vers les coins (terminal).

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use motifs::foils::Foil;

pub struct CellFoil {
    pub id: &'static str,
    pub corners: usize,
}

impl Rule for CellFoil {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        if self.corners == 3 {
            "Trilobe inscrit"
        } else {
            "Quadrilobe inscrit"
        }
    }
    fn terminal(&self) -> bool {
        true
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if a.inscribed.r < 2.2 * ctx.min_opening_r() {
            return 0.0;
        }
        if self.corners == 3 {
            a.confidence(CellType::Triangle)
        } else {
            a.confidence(CellType::Quad)
                .max(0.6 * a.confidence(CellType::Lens))
        }
    }
    fn sample(&self, ctx: &Ctx, _a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        Params::new().with(
            "k",
            (ctx.style.k + unif_in(rng, -0.05, 0.15)).clamp(1.0, 1.5),
        )
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let c = a.inscribed;
        let n = self.corners;
        // phase : vers le coin le plus aigu (le plus haut en cas d'égalité)
        let mut corners = a.corners.clone();
        corners.sort_by(|x, y| {
            x.interior
                .total_cmp(&y.interior)
                .then(y.p.y.total_cmp(&x.p.y))
                .then(x.p.x.total_cmp(&y.p.x))
        });
        let phase = match (ctx.sym_axis(a), corners.first()) {
            (Some(_), _) => {
                // symétrie verticale : lobe vers le haut ou vers le bas selon
                // la position du coin le plus aigu
                if corners.first().is_some_and(|k| k.p.y < c.c.y) {
                    -FRAC_PI_2
                } else {
                    FRAC_PI_2
                }
            }
            (None, Some(k)) => angle_of(k.p - c.c),
            _ => FRAC_PI_2,
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
            key_ratio: None,
        })
    }
}
