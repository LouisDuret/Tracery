//! R16 — Découpe médiane : région allongée → coupes perpendiculaires à
//! l'axe principal (axe d'inertie du contour), en sous-régions plus régulières.

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;

pub struct MedialCut;

/// Axe principal d'inertie (points échantillonnés du bord).
fn principal_axis(a: &Analysis) -> (P2, P2, f64, f64) {
    let pts = a.region.outer.sample(a.region.outer.length() / 256.0);
    let n = pts.len().max(1) as f64;
    let m = pts.iter().fold(P2::ZERO, |s, p| s + *p) / n;
    let (mut cxx, mut cyy, mut cxy) = (0.0, 0.0, 0.0);
    for p in &pts {
        let d = *p - m;
        cxx += d.x * d.x;
        cyy += d.y * d.y;
        cxy += d.x * d.y;
    }
    let th = 0.5 * atan2(2.0 * cxy, cxx - cyy);
    let u = dir(th);
    let (mut tmin, mut tmax) = (f64::INFINITY, f64::NEG_INFINITY);
    for p in &pts {
        let t = dot(*p - m, u);
        tmin = tmin.min(t);
        tmax = tmax.max(t);
    }
    (m, u, tmin, tmax)
}

impl Rule for MedialCut {
    fn id(&self) -> &'static str {
        "R16"
    }
    fn name(&self) -> &'static str {
        "Découpe médiane"
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only || a.elongation < 2.8 {
            return 0.0;
        }
        let t = |c: CellType| a.confidence(c);
        t(CellType::Band)
            .max(0.6 * t(CellType::Tapered))
            .max(0.6 * t(CellType::Irregular))
            .max(0.4 * t(CellType::Polygon))
            .max(0.3)
    }
    fn sample(&self, _ctx: &Ctx, a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        let kmax = ((a.elongation / 2.0).floor() as usize).clamp(1, 4);
        let k = 1 + (unif(rng) * kmax as f64) as usize;
        Params::new()
            .with("k", k.min(kmax) as f64)
            .with("jitter", unif_in(rng, -0.25, 0.25))
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let (m, u, tmin, tmax) = principal_axis(a);
        let k = p.get_usize("k", 1).max(1);
        let len = tmax - tmin;
        let step = len / (k + 1) as f64;
        let ext = a.bbox.diag();
        let mut bands = Vec::new();
        for i in 1..=k {
            let t = tmin + step * (i as f64 + p.get("jitter", 0.0) * 0.5);
            let c = m + u * t;
            let n = perp(u);
            bands.push(line_band(c - n * ext, c + n * ext, ctx.w, 0.0));
        }
        let cells = cut_bands(&a.region, &bands, ctx.min_opening_r());
        if cells.len() < 2 {
            return Err(RuleError::Degenerate("découpe sans effet"));
        }
        Ok(Expansion {
            children: cells.into_iter().map(|r| ChildSpec::new(r, None)).collect(),
            openings: vec![],
            construction: vec![],
            key_ratio: None,
        })
    }
}
