//! R17 — Découpe symétrique : meneau sur l'axe de symétrie et deux moitiés
//! miroirs (la seconde est la copie exacte de la première).

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use geom::ConstructionOp;

pub struct SymSplit;

impl Rule for SymSplit {
    fn id(&self) -> &'static str {
        "R17"
    }
    fn name(&self) -> &'static str {
        "Découpe symétrique"
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only || a.sym_x.is_none() || a.circle.is_some() {
            return 0.0;
        }
        if a.bbox.size().x < 2.0 * (2.0 * ctx.w) + ctx.w {
            return 0.0;
        }
        let t = |c: CellType| a.confidence(c);
        t(CellType::Lens)
            .max(0.6 * t(CellType::Irregular))
            .max(0.5 * t(CellType::Quad))
            .max(0.4 * t(CellType::Polygon))
            .max(0.4 * t(CellType::Lunule))
            .max(0.3 * t(CellType::Tympan))
            .max(0.25 * t(CellType::Arch))
            .max(0.2)
    }
    fn sample(&self, _ctx: &Ctx, _a: &Analysis, _rng: &mut ChaCha8Rng) -> Params {
        Params::new()
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, _p: &Params) -> Result<Expansion, RuleError> {
        let ax = a.sym_x.ok_or(RuleError::NotApplicable("pas d'axe"))?;
        let bb = a.bbox;
        let ext = bb.diag();
        let band = line_band(
            P2::new(ax, bb.min.y - ext),
            P2::new(ax, bb.max.y + ext),
            ctx.w,
            0.0,
        );
        let cells = cut_bands(&a.region, &[band], ctx.min_opening_r());
        let (left, right): (Vec<_>, Vec<_>) =
            cells.into_iter().partition(|r| r.bbox().center().x < ax);
        if left.is_empty() || left.len() != right.len() {
            return Err(RuleError::Degenerate("moitiés non appariées"));
        }
        let m = Xf::mirror_x(ax);
        let mut children: Vec<ChildSpec> = left
            .iter()
            .map(|r| ChildSpec::new(r.clone(), None))
            .collect();
        let nl = children.len();
        for (i, l) in left.iter().enumerate() {
            let mut ch = ChildSpec::new(l.transform(&m), None);
            ch.twin = Some((i, m));
            children.push(ch);
        }
        debug_assert_eq!(children.len(), 2 * nl);
        Ok(Expansion {
            children,
            openings: vec![],
            construction: vec![ConstructionEntity::Line {
                a: P2::new(ax, bb.min.y),
                b: P2::new(ax, bb.max.y),
                op: ConstructionOp::Axis,
            }],
            key_ratio: None,
        })
    }
}
