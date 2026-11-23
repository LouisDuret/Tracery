//! R14 — Panneaux (perpendiculaire) : meneaux verticaux prolongés jusqu'à
//! l'intrados et traverses horizontales ; chaque panneau reçoit ensuite une
//! tête redentée (R01 à une lancette, R21) ou un redentage.

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use geom::ConstructionOp;

pub struct Panels;

impl Rule for Panels {
    fn id(&self) -> &'static str {
        "R14"
    }
    fn name(&self) -> &'static str {
        "Panneaux"
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only || a.is(CellType::Panel) {
            return 0.0;
        }
        let width = a.bbox.size().x;
        if width < 2.0 * 3.5 * ctx.w + ctx.w {
            return 0.0;
        }
        if a.arch.is_some() {
            0.9
        } else if !has_flat_base(a) {
            0.0
        } else {
            0.6 * a
                .confidence(CellType::Polygon)
                .max(a.confidence(CellType::Quad))
                .max(a.confidence(CellType::Band))
        }
    }
    fn sample(&self, ctx: &Ctx, a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        let width = a.bbox.size().x;
        let nmax = (((width + ctx.w) / (4.0 * ctx.w)).floor() as usize).clamp(2, 6);
        let n = 2 + (unif(rng) * (nmax - 1) as f64) as usize;
        let tr = weighted_index(rng, &[0.35, 0.45, 0.2]).unwrap_or(1);
        Params::new()
            .with("n", n.min(nmax) as f64)
            .with("transoms", tr as f64)
            .with("t1", unif_in(rng, 0.45, 0.7))
            .with("t2", unif_in(rng, 0.2, 0.35))
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let bb = a.bbox;
        let n = p.get_usize("n", 3).max(2);
        let w = ctx.w;
        let (x0, x1) = (bb.min.x, bb.max.x);
        let (y0, y1) = (bb.min.y, bb.max.y);
        let ext = bb.diag();
        let mut bands = Vec::new();
        let mut cons = Vec::new();
        for i in 1..n {
            let x = x0 + (x1 - x0) * i as f64 / n as f64;
            bands.push(line_band(
                P2::new(x, y0 - ext),
                P2::new(x, y1 + ext),
                w,
                0.0,
            ));
            cons.push(ConstructionEntity::Line {
                a: P2::new(x, y0),
                b: P2::new(x, y1),
                op: ConstructionOp::Axis,
            });
        }
        let top = a.arch.map(|ai| ai.y_spring).unwrap_or(y1);
        let nt = p.get_usize("transoms", 1);
        for (k, key) in ["t1", "t2"].iter().enumerate().take(nt) {
            let _ = k;
            let y = y0 + (top - y0) * p.get(key, 0.5);
            bands.push(line_band(
                P2::new(x0 - ext, y),
                P2::new(x1 + ext, y),
                w,
                0.0,
            ));
        }
        let cells = cut_bands(&a.region, &bands, ctx.min_opening_r());
        if cells.len() < 2 {
            return Err(RuleError::Degenerate("découpe sans effet"));
        }
        let children = cells
            .into_iter()
            .map(|r| ChildSpec::new(r, Some(CellType::Panel)))
            .collect();
        Ok(Expansion {
            children,
            openings: vec![],
            construction: cons,
            key_ratio: Some(1.0 / n as f64),
        })
    }
}

/// La cellule repose-t-elle sur une base horizontale (≥ 50 % de la largeur) ?
fn has_flat_base(a: &Analysis) -> bool {
    let bb = a.bbox;
    let eps = 1e-6 * bb.diag();
    a.region.outer.segs.iter().any(|s| match *s {
        geom::Seg::Line { a: p, b: q } => {
            (p.y - bb.min.y).abs() < eps
                && (q.y - bb.min.y).abs() < eps
                && (p.x - q.x).abs() > 0.5 * bb.size().x
        }
        _ => false,
    })
}
