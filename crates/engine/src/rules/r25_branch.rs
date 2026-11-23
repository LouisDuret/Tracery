//! R25 — Branche tangente : un barreau naît tangentiellement d'un barreau
//! existant (bord de la cellule) et rejoint tangentiellement un autre bord.
//! Il partage la cellule en deux mouchettes dont les queues effilées naissent
//! aux points de raccord (§ 4.7).
//!
//! C'est la croissance propre au réseau flamboyant : les grandes formes sont
//! tracées d'abord, puis de nouveaux barreaux s'en détachent en suivant
//! leurs tangentes. Le barreau est un biarc G1 dont les tangentes
//! d'extrémité sont celles du contour aux points de départ et d'arrivée ;
//! appliquée récursivement selon la complexité, la règle fait émerger des
//! mouchettes de plus en plus petites.

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use geom::ConstructionOp;
use geom::constr::biarc;

pub struct Branch;

/// Indice du segment du contour à l'abscisse curviligne `u`.
fn seg_index_at(l: &Loop, u: f64) -> usize {
    let total = l.length();
    let mut u = u.rem_euclid(total.max(1e-300));
    for (i, s) in l.segs.iter().enumerate() {
        let sl = s.length();
        if u <= sl {
            return i;
        }
        u -= sl;
    }
    l.segs.len().saturating_sub(1)
}

impl Rule for Branch {
    fn id(&self) -> &'static str {
        "R25"
    }
    fn name(&self) -> &'static str {
        "Branche tangente"
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only || !a.region.holes.is_empty() || a.circle.is_some() {
            return 0.0;
        }
        // deux mouchettes au jour lisible de part et d'autre du barreau
        // (et pas d'emboîtement en fouillis : la cellule doit rester ample)
        if a.inscribed.r < 3.0 * ctx.w + 4.0 * ctx.min_opening_r()
            || a.inscribed.r < 0.045 * ctx.root_area.sqrt()
        {
            return 0.0;
        }
        let t = |c: CellType| a.confidence(c);
        t(CellType::Tapered)
            .max(0.8 * t(CellType::Lunule))
            .max(0.6 * t(CellType::Triangle))
            .max(0.5 * t(CellType::Quad))
            .max(0.5 * t(CellType::Irregular))
            .max(0.4 * t(CellType::Lens))
    }
    fn sample(&self, _ctx: &Ctx, _a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        Params::new()
            .with("phase", unif(rng))
            .with("split", unif_in(rng, 0.3, 0.5))
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let f = &a.region;
        let l = &f.outer;
        let len = l.length();
        let m = 28usize;
        let phase = p.get("phase", 0.0);
        let split = p.get("split", 0.4);
        // épaisseurs presque uniformes (voir `generate`)
        let w = ctx.w;
        // couples (A, B) sur des côtés différents, rangés selon la part de
        // contour qu'ils isolent (proche de `split`) : ordre déterministe
        let mut pairs: Vec<(f64, f64, f64)> = Vec::new();
        for i in 0..m {
            for j in i + 1..m {
                let ua = len * (i as f64 + phase) / m as f64;
                let ub = len * (j as f64 + phase) / m as f64;
                if seg_index_at(l, ua) == seg_index_at(l, ub) {
                    continue;
                }
                let frac = (ub - ua) / len;
                let fr = frac.min(1.0 - frac);
                if fr < 0.18 {
                    continue;
                }
                pairs.push((ua, ub, (fr - split).abs()));
            }
        }
        pairs.sort_by(|x, y| x.2.total_cmp(&y.2));
        let margin = -1e-6 * ctx.tol.l;
        let mut err = RuleError::NotApplicable("aucune branche possible");
        for &(ua, ub, _) in pairs.iter().take(40) {
            let (pa, ta) = l.point_at_length(ua);
            let (pb, tb) = l.point_at_length(ub);
            let chord = pb - pa;
            if norm(chord) < 1.2 * a.inscribed.r {
                continue;
            }
            // départ et arrivée dans le fil du contour, orientés vers l'autre bout
            let ta = if dot(ta, chord) >= 0.0 {
                unit(ta)
            } else {
                -unit(ta)
            };
            let tb = if dot(tb, chord) >= 0.0 {
                unit(tb)
            } else {
                -unit(tb)
            };
            // l'axe de la branche est tangent à l'axe du barreau porteur, à
            // w/2 hors du jour : le bord de la bande affleure alors le bord
            // de la cellule (raccord G1, sans marche d'une demi-épaisseur)
            let (pa, pb) = (shift_out(f, pa, ta, 0.5 * w), shift_out(f, pb, tb, 0.5 * w));
            let segs = biarc(pa, ta, pb, tb);
            if segs.len() != 2 {
                err = RuleError::Degenerate("branche rectiligne");
                continue;
            }
            // dans la cellule (à w/2 près aux extrémités) ; au milieu, place
            // pour le barreau entier
            let ok = segs
                .iter()
                .all(|g| g.sample(40).iter().all(|q| f.sdf(*q) >= margin - 0.5 * w));
            let mid = segs[0].end();
            if !ok || f.sdf(mid) < 1.2 * w {
                err = RuleError::Constraint("branche hors cellule");
                continue;
            }
            let bands: Vec<Loop> = segs.iter().map(|g| seg_band(g, w)).collect();
            let pieces = cut_bands(f, &bands, ctx.min_opening_r());
            if pieces.len() != 2 {
                err = RuleError::Degenerate("partage ≠ 2 mouchettes");
                continue;
            }
            let (a0, a1) = (pieces[0].area(), pieces[1].area());
            if a0.min(a1) < 0.25 * a0.max(a1) {
                err = RuleError::Constraint("mouchettes déséquilibrées");
                continue;
            }
            let mut construction = vec![
                ConstructionEntity::point(pa, ConstructionOp::Tangent),
                ConstructionEntity::point(pb, ConstructionOp::Tangent),
            ];
            for g in &segs {
                if let Seg::Arc { c, r, .. } = *g {
                    construction.push(ConstructionEntity::point(c, ConstructionOp::Mouchette));
                    construction.push(ConstructionEntity::Circle {
                        c,
                        r,
                        op: ConstructionOp::Mouchette,
                    });
                }
            }
            return Ok(Expansion {
                children: pieces
                    .into_iter()
                    .map(|r| ChildSpec::new(r, Some(CellType::Tapered)))
                    .collect(),
                openings: vec![],
                construction,
                key_ratio: Some(a0.min(a1) / a0.max(a1)),
            });
        }
        Err(err)
    }
}

use geom::Seg;

/// Point `p` du bord de `f` (tangente `t`) reporté de `d` vers l'extérieur,
/// le long de la normale.
fn shift_out(f: &Region, p: P2, t: P2, d: f64) -> P2 {
    let n = perp(unit(t));
    let h = 0.25 * d.max(1e-12);
    // la normale intérieure est celle du côté où la distance signée croît
    let inward = if f.sdf(p + n * h) >= f.sdf(p - n * h) {
        n
    } else {
        -n
    };
    p - inward * d
}

/// Bande (épaisseur w) autour d'un arc ou d'un segment.
fn seg_band(s: &Seg, w: f64) -> Loop {
    match *s {
        Seg::Line { a, b } => line_band(a, b, w, 0.02 * w),
        Seg::Arc {
            c,
            r,
            theta0,
            sweep,
        } => arc_band(c, r, theta0, sweep, w, 0.02 * w),
    }
}
