//! R21 — Tête polylobée de lancette (figure 3) : arc brisé redenté,
//! construit à la règle et au compas.
//!
//! Repère local : naissances à y = 0, axe x = 0, demi-portée a, rayon R.
//! L'arc droit a son centre en C = (a − R, 0).
//! - **Lobe de clef**, de rayon ρ, sur l'axe, tangent intérieurement aux deux
//!   arcs : |c₀ − C| = R − ρ, d'où y₀ = √((R − ρ)² − (R − a)²).
//! - **Lobes latéraux**, tangents à l'arc droit : cₖ = C + (R − ρ)·dir(θₖ).
//!   θₖ est choisi pour que |cₖ − cₖ₋₁| = g·ρ (bissection), puis tout est
//!   reporté à gauche par symétrie exacte.
//! - **Redent** entre deux lobes voisins : la pierre bornée par l'arc et par
//!   les deux lobes. Sa pointe est l'intersection des deux cercles la plus
//!   proche de l'arc.
//!
//! Le jour suit l'arc partout ailleurs : la pointe est conservée, et l'ogive
//! garde l'épaisseur des autres barreaux. Trois lobes (tête trilobée) ou cinq
//! (quintilobée). Terminal.

use super::*;
use crate::analysis::CellMeta;
use crate::genome::Params;
use geom::ConstructionOp;
use geom::clip::intersect;
use geom::constr::inter_circle_circle;

pub struct TrefoilHead;

/// Centres des lobes du côté droit (le premier est le lobe de clef) et
/// leurs points de contact avec l'arc, en repère local.
fn right_lobes(a: f64, r: f64, rho: f64, n_side: usize, gap: f64) -> Option<Vec<(P2, P2)>> {
    let c = P2::new(a - r, 0.0);
    let h2 = (r - rho) * (r - rho) - (r - a) * (r - a);
    if h2 <= 0.0 || r <= rho {
        return None;
    }
    let c0 = P2::new(0.0, h2.sqrt());
    let contact = |q: P2| c + unit(q - c) * r;
    let mut out = vec![(c0, contact(c0))];
    let mut th_prev = angle_of(c0 - c);
    for _ in 0..n_side {
        let prev = out.last()?.0;
        let at = |th: f64| c + dir(th) * (r - rho);
        // la distance au lobe précédent décroît quand θ remonte vers lui
        let target = gap * rho;
        if dist(at(0.0), prev) < target {
            return None; // plus de place au-dessus des naissances
        }
        let (mut lo, mut hi) = (0.0, th_prev);
        for _ in 0..60 {
            let m = 0.5 * (lo + hi);
            if dist(at(m), prev) > target {
                lo = m;
            } else {
                hi = m;
            }
        }
        let th = 0.5 * (lo + hi);
        let ck = at(th);
        out.push((ck, contact(ck)));
        th_prev = th;
    }
    Some(out)
}

impl Rule for TrefoilHead {
    fn id(&self) -> &'static str {
        "R21"
    }
    fn name(&self) -> &'static str {
        "Tête polylobée"
    }
    fn terminal(&self) -> bool {
        true
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        match a.meta {
            Some(CellMeta::Lancet { span, .. }) if span >= 4.0 * ctx.min_opening_r() => {
                (0.4 + ctx.style.cusping).min(1.2)
            }
            _ => 0.0,
        }
    }
    fn sample(&self, _ctx: &Ctx, _a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        Params::new()
            .with("n", if unif(rng) < 0.75 { 3.0 } else { 5.0 })
            .with("rho", unif_in(rng, 0.2, 0.3))
            .with("gap", unif_in(rng, 1.55, 1.8))
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let Some(CellMeta::Lancet {
            x,
            span,
            radius,
            y_spring,
            ..
        }) = a.meta
        else {
            return Err(RuleError::NotApplicable("pas une lancette"));
        };
        let n0 = if p.get_usize("n", 3) >= 5 { 5 } else { 3 };
        // paramètres tirés d'abord, puis recherche ordonnée (trilobé de
        // préférence) jusqu'à des redents bien proportionnés
        let mut tries = vec![(n0, p.get("rho", 0.25), p.get("gap", 1.65))];
        for n in [n0, 3] {
            for rf in [0.25, 0.22, 0.28, 0.32, 0.18] {
                for gf in [1.65, 1.8, 1.5, 1.9] {
                    tries.push((n, rf, gf));
                }
            }
        }
        let mut err = RuleError::NotApplicable("aucune tête polylobée");
        for (n, rf, gf) in tries {
            match head(ctx, a, x, y_spring, span, radius, n, rf * span, gf) {
                Ok(e) => return Ok(e),
                Err(e) => err = e,
            }
        }
        Err(err)
    }
}

#[allow(clippy::too_many_arguments)]
fn head(
    ctx: &Ctx,
    a: &Analysis,
    x: f64,
    y_spring: f64,
    span: f64,
    r: f64,
    n: usize,
    rho: f64,
    gap: f64,
) -> Result<Expansion, RuleError> {
    let half = span / 2.0;
    let lobes = right_lobes(half, r, rho, (n - 1) / 2, gap)
        .ok_or(RuleError::Constraint("lobes hors de l'arc"))?;
    let c = P2::new(half - r, 0.0);
    let to_doc = Xf::translate(P2::new(x, y_spring));
    let mirror = Xf::mirror_x(x);
    let min_cusp = 0.5 * ctx.w_next.max(ctx.w_min);
    let tol = Tol::new(4.0 * span);
    let mut stones: Vec<Region> = Vec::new();
    let mut construction = Vec::new();
    for k in 1..lobes.len() {
        let (c1, t1) = lobes[k - 1];
        let (c2, t2) = lobes[k];
        let xs = inter_circle_circle(&Circle::new(c1, rho), &Circle::new(c2, rho), &tol);
        if xs.len() != 2 {
            return Err(RuleError::Constraint("lobes disjoints"));
        }
        // pointe du redent : l'intersection la plus proche de l'arc
        let tip = if dist(xs[0], c) > dist(xs[1], c) {
            xs[0]
        } else {
            xs[1]
        };
        let depth = r - dist(tip, c);
        let base = dist(t1, t2);
        if depth < min_cusp || depth > 0.6 * base || depth > 0.5 * rho || depth < 0.12 * base {
            return Err(RuleError::Constraint("redent mal proportionné"));
        }
        // pierre entre l'arc et les deux lobes
        let o1 = unit(t1 - c) * rho;
        let o2 = unit(t2 - c) * rho;
        let q = Loop::polygon(&[t1 + o1, t1, tip, t2, t2 + o2]).oriented(true);
        if geom::clip::self_intersects(&q) {
            return Err(RuleError::Degenerate("coin de redent"));
        }
        let mut s = intersect(&[Region::circle(c, r)], &Region::simple(q));
        s = subtract(&s, &Region::circle(c1, rho));
        s = subtract(&s, &Region::circle(c2, rho));
        for st in s.into_iter().filter(|z| z.area() > 1e-9 * rho * rho) {
            let st = st.transform(&to_doc);
            stones.push(st.transform(&mirror));
            stones.push(st);
        }
        let tip_d = to_doc.apply(tip);
        construction.push(ConstructionEntity::point(tip_d, ConstructionOp::Cusp));
        construction.push(ConstructionEntity::point(
            mirror.apply(tip_d),
            ConstructionOp::Cusp,
        ));
    }
    let mut open = vec![a.region.clone()];
    for s in &stones {
        open = subtract(&open, s);
    }
    let open = drop_chips(open, ctx.min_opening_r());
    if open.len() != 1 {
        return Err(RuleError::Degenerate("jour morcelé"));
    }
    for (ck, _) in &lobes {
        let cd = to_doc.apply(*ck);
        for q in [cd, mirror.apply(cd)] {
            construction.push(ConstructionEntity::Circle {
                c: q,
                r: rho,
                op: ConstructionOp::Cusp,
            });
        }
    }
    Ok(Expansion {
        children: vec![],
        openings: open,
        construction,
        key_ratio: Some(rho / span),
    })
}
