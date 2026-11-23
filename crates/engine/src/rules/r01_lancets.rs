//! R01 — Lancettes : baie → n lancettes en arc brisé + tympan (§ 4.6).
//!
//! Les lancettes sont des colonnes de largeur sₗ = (s − 2g − (n − 1)·w)/n
//! (g : écart au bord), couronnées d'un arc brisé de brisure ρ′ ; leurs
//! naissances sont abaissées si nécessaire pour que l'arc reste dans la baie
//! avec la marge g. Le tympan est le résidu F ∖ dilate(lancettes, w).
//! « 4 = 2 × 2 » s'obtient par récursion (une lancette peut recevoir R01).

use super::*;
use crate::analysis::{CellMeta, CellType};
use crate::genome::Params;
use geom::{ConstructionOp, Region};
use motifs::{ArchKind, arch_window};

pub struct Lancets;

fn lancet_width(s: f64, n: usize, w: f64) -> f64 {
    let g = 0.5 * w;
    (s - 2.0 * g - (n as f64 - 1.0) * w) / n as f64
}

impl Rule for Lancets {
    fn id(&self) -> &'static str {
        "R01"
    }
    fn name(&self) -> &'static str {
        "Lancettes"
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only {
            return 0.0;
        }
        let Some(ai) = a.arch else { return 0.0 };
        let s = ai.x1 - ai.x0;
        // au moins deux lancettes de largeur ≥ 3w, ou une seule en panneau
        let min_n = if a.is(CellType::Panel) { 1 } else { 2 };
        if lancet_width(s, min_n, ctx.w) < 3.0 * ctx.w {
            return 0.0;
        }
        if a.is(CellType::Panel) {
            0.8
        } else if a.is(CellType::Lancet) {
            0.5
        } else {
            1.0
        }
    }
    fn sample(&self, ctx: &Ctx, a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        let ai = a.arch.expect("accepts garantit une baie");
        let s = ai.x1 - ai.x0;
        let mut ns: Vec<(usize, f64)> = if a.is(CellType::Panel) {
            vec![(1, 1.0)]
        } else {
            vec![(2, 0.6), (3, 0.25), (4, 0.15)]
        };
        ns.retain(|(n, _)| lancet_width(s, *n, ctx.w) >= 3.0 * ctx.w);
        let w: Vec<f64> = ns.iter().map(|x| x.1).collect();
        let n = weighted_index(rng, &w).map(|i| ns[i].0).unwrap_or(2);
        let (lo, hi) = ctx.def.rho_range;
        // arc brisé franc : en deçà de ρ = 0,9, l'arc paraît en plein cintre
        let rho = unif_in(rng, lo.max(0.9), hi.max(1.0));
        let drop = unif_in(rng, 0.0, 0.25);
        Params::new()
            .with("n", n as f64)
            .with("rho", rho)
            .with("drop", drop)
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let ai = a.arch.ok_or(RuleError::NotApplicable("pas de baie"))?;
        let f = &a.region;
        let n = p.get_usize("n", 2).max(1);
        let rho = p.get("rho", 1.0).max(0.9);
        let w = ctx.w;
        let g = 0.5 * w;
        let s = ai.x1 - ai.x0;
        let sl = lancet_width(s, n, w);
        if sl < 2.5 * w {
            return Err(RuleError::Constraint("lancettes trop étroites"));
        }
        let kind = ArchKind::Pointed { rho };
        let r_l = rho * sl;
        let y_b = ai.y_base + g;
        let y_hi = ai.y_spring - p.get("drop", 0.0) * sl;
        let y_lo = y_b + 0.4 * sl;
        if y_hi <= y_lo {
            return Err(RuleError::Constraint("baie trop basse"));
        }
        let make = |i: usize, y_l: f64| -> Region {
            let xc = ai.x0 + g + i as f64 * (sl + w) + sl / 2.0;
            let (l, _) = arch_window(kind, sl, y_l - y_b);
            Region::simple(l.transform(&Xf::translate(P2::new(xc, y_l))))
        };
        let fits = |y_l: f64| -> bool {
            // la première et la dernière lancette suffisent par symétrie de
            // construction ; on teste toutes pour les baies irrégulières
            (0..n).all(|i| fits_inside(f, &make(i, y_l).outer, 0.98 * g))
        };
        let y_l = if fits(y_hi) {
            y_hi
        } else {
            if !fits(y_lo) {
                return Err(RuleError::Constraint("aucune hauteur de naissance valide"));
            }
            let (mut lo, mut hi) = (y_lo, y_hi);
            for _ in 0..30 {
                let m = 0.5 * (lo + hi);
                if fits(m) {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            lo
        };
        let lancets: Vec<Region> = (0..n).map(|i| make(i, y_l)).collect();
        let mut children: Vec<ChildSpec> = Vec::new();
        let mut cons = Vec::new();
        for (i, l) in lancets.iter().enumerate() {
            let xc = ai.x0 + g + i as f64 * (sl + w) + sl / 2.0;
            let mut c = ChildSpec::new(l.clone(), Some(CellType::Lancet));
            c.meta = Some(CellMeta::Lancet {
                x: xc,
                span: sl,
                radius: r_l,
                y_spring: y_l,
                y_bottom: y_b,
            });
            if i > 0 {
                c.twin = Some((0, Xf::translate(P2::new(i as f64 * (sl + w), 0.0))));
            }
            children.push(c);
            cons.push(ConstructionEntity::point(
                P2::new(xc - sl / 2.0 + r_l, y_l),
                ConstructionOp::ArchCenter,
            ));
            cons.push(ConstructionEntity::point(
                P2::new(xc + sl / 2.0 - r_l, y_l),
                ConstructionOp::ArchCenter,
            ));
        }
        cons.push(ConstructionEntity::Line {
            a: P2::new(ai.x0, y_l),
            b: P2::new(ai.x1, y_l),
            op: ConstructionOp::Axis,
        });
        let rest = carve(f, &lancets, ctx.carve_gap(), ctx.min_opening_r());
        for (k, r) in rest.into_iter().enumerate() {
            let hint = if k == 0 && r.bbox().min.y >= y_l - 1e-9 * ctx.tol.l {
                Some(CellType::Tympan)
            } else {
                None
            };
            children.push(ChildSpec::new(r, hint));
        }
        Ok(Expansion {
            children,
            openings: vec![],
            construction: cons,
            key_ratio: Some(sl / s),
        })
    }
}
