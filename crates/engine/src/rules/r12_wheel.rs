//! R12 — Roue de mouchettes : cercle → oculus central + n rayons courbes
//! (arcs partant radialement de l'anneau central) délimitant n cellules
//! effilées congruentes, tournées de 2π/n (§ 4.8). Les cellules reçoivent
//! ensuite des mouchettes (R10) ou un redentage.

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use geom::constr::arc_from_tangent;
use geom::{ConstructionOp, Region, Seg};

pub struct Wheel;

impl Rule for Wheel {
    fn id(&self) -> &'static str {
        "R12"
    }
    fn name(&self) -> &'static str {
        "Roue de mouchettes"
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only {
            return 0.0;
        }
        match a.circle {
            Some(c) if c.r >= 10.0 * ctx.w => 1.0,
            _ => 0.0,
        }
    }
    fn sample(&self, _ctx: &Ctx, _a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        let n = [3.0, 4.0, 5.0, 6.0][(unif(rng) * 4.0) as usize % 4];
        Params::new()
            .with("n", n)
            .with("sense", if unif(rng) < 0.5 { 1.0 } else { -1.0 })
            .with("rc", unif_in(rng, 0.2, 0.32))
            .with("twist", unif_in(rng, 0.7, 1.15))
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let circ = a.circle.ok_or(RuleError::NotApplicable("pas un cercle"))?;
        let (c, big_r) = (circ.c, circ.r);
        let w = ctx.w;
        let n = p.get_usize("n", 4).clamp(3, 8);
        let sense = if p.get("sense", 1.0) >= 0.0 {
            1.0
        } else {
            -1.0
        };
        let rc = p.get("rc", 0.25) * big_r;
        let twist = p.get("twist", 1.0) * TAU / n as f64;
        let central = Region::circle(c, rc);
        let ring = carve(
            &a.region,
            std::slice::from_ref(&central),
            ctx.carve_gap(),
            0.0,
        );
        let ring = ring
            .into_iter()
            .next()
            .ok_or(RuleError::Degenerate("anneau vide"))?;
        let mut bands = Vec::new();
        let mut cons = vec![ConstructionEntity::circle(
            Circle::new(c, rc),
            ConstructionOp::Rose,
        )];
        for j in 0..n {
            let phi = FRAC_PI_2 + TAU * j as f64 / n as f64;
            let p0 = c + dir(phi) * (rc + 0.55 * w);
            let q0 = c + dir(phi + sense * twist) * (big_r + w);
            match arc_from_tangent(p0, dir(phi), q0) {
                Seg::Arc {
                    c: cs,
                    r,
                    theta0,
                    sweep,
                } => {
                    bands.push(arc_band(cs, r, theta0, sweep, w, 1.5 * w));
                    cons.push(ConstructionEntity::point(cs, ConstructionOp::Mouchette));
                }
                Seg::Line { a, b } => bands.push(line_band(a, b, w, 1.5 * w)),
            }
        }
        let cells = cut_bands(&ring, &bands, ctx.min_opening_r());
        if cells.len() != n {
            return Err(RuleError::Degenerate("nombre de cellules inattendu"));
        }
        // tri angulaire à partir du premier rayon
        let mut cells: Vec<(f64, Region)> = cells
            .into_iter()
            .map(|r| (ccw_delta(FRAC_PI_2, angle_of(r.centroid() - c)), r))
            .collect();
        cells.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut children = vec![ChildSpec::new(central, Some(CellType::Circle))];
        for (i, (_, r)) in cells.into_iter().enumerate() {
            let mut ch = ChildSpec::new(r, Some(CellType::Tapered));
            if i > 0 {
                ch.twin = Some((1, Xf::rotate_about(c, TAU * i as f64 / n as f64)));
            }
            children.push(ch);
        }
        Ok(Expansion {
            children,
            openings: vec![],
            construction: cons,
            key_ratio: Some(rc / big_r),
        })
    }
}
