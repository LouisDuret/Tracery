//! R23 — Redents de mouchette : quelques grands lobes tangents au contour,
//! logés dans la tête de la cellule, séparés par des redents pointus ; la
//! queue reste ouverte (§ 4.8).
//!
//! Chaque lobe est un cercle de rayon ρ tangent intérieurement au contour :
//! son centre est sur la parallèle intérieure à distance ρ (le lobe épouse la
//! grande forme), et les lobes se suivent le long de cette parallèle. Deux lobes voisins se
//! coupent ; la pierre comprise entre le contour et leurs arcs forme le
//! redent, dont la pointe est leur intersection la plus proche du bord.
//! Le jour est la cellule privée des redents : son contour conserve les
//! grandes courbes de la forme englobante.

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use geom::ConstructionOp;
use geom::clip::intersect;
use geom::constr::inter_circle_circle;

pub struct Redents;

/// Gradient (unitaire) de la distance signée au bord, vers l'intérieur.
fn grad(f: &Region, p: P2, e: f64) -> P2 {
    let gx = f.sdf(p + P2::new(e, 0.0)) - f.sdf(p - P2::new(e, 0.0));
    let gy = f.sdf(p + P2::new(0.0, e)) - f.sdf(p - P2::new(0.0, e));
    unit(P2::new(gx, gy))
}

impl Rule for Redents {
    fn id(&self) -> &'static str {
        "R23"
    }
    fn name(&self) -> &'static str {
        "Redents de mouchette"
    }
    fn terminal(&self) -> bool {
        true
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if !a.region.holes.is_empty() || a.circle.is_some() {
            return 0.0;
        }
        // lobes et redents lisibles
        if a.inscribed.r < 4.0 * ctx.min_opening_r() + 2.0 * ctx.w_next {
            return 0.0;
        }
        let t = |c: CellType| a.confidence(c);
        t(CellType::Tapered)
            .max(0.8 * t(CellType::Lens))
            .max(0.6 * t(CellType::Lunule))
            .max(0.5 * t(CellType::Triangle))
            .max(0.4 * t(CellType::Sector))
            .max(0.3 * t(CellType::Irregular))
    }
    fn sample(&self, _ctx: &Ctx, _a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        Params::new()
            .with("n", if unif(rng) < 0.6 { 3.0 } else { 2.0 })
            .with("rho", unif_in(rng, 0.38, 0.55))
            .with("gap", unif_in(rng, 1.5, 1.8))
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let h = a.inscribed.c;
        let rin = a.inscribed.r;
        // la queue : pointe la plus aiguë ; les lobes se logent du côté opposé
        let tail = a
            .corners
            .iter()
            .min_by(|x, y| x.interior.total_cmp(&y.interior))
            .map(|c| c.p)
            .unwrap_or_else(|| h - (a.centroid - h));
        let n0 = p.get_usize("n", 3).clamp(2, 4);
        // les paramètres tirés d'abord, puis une recherche ordonnée de la
        // configuration qui donne des redents bien proportionnés
        let mut tries = vec![(n0, p.get("rho", 0.45), p.get("gap", 1.65))];
        for n in [n0, 5 - n0] {
            for rf in [0.35, 0.45, 0.55] {
                for gf in [1.5, 1.65, 1.8, 1.9] {
                    tries.push((n, rf, gf));
                }
            }
        }
        let mut err = RuleError::NotApplicable("aucun redent");
        for (n, rf, gf) in tries {
            match build(ctx, a, tail, n, rf * rin, gf) {
                Ok(e) => return Ok(e),
                Err(e) => err = e,
            }
        }
        Err(err)
    }
}

/// Redents de `n` lobes de rayon ρ. Les centres des lobes sont posés à la
/// suite sur la parallèle intérieure au contour à distance ρ (chaque lobe
/// est donc tangent au contour), espacés de `gap`·ρ le long de cette
/// parallèle et centrés sur le point le plus éloigné de la queue. Le jour garde le contour de la
/// cellule partout ailleurs : seuls les redents (pierre entre deux lobes
/// voisins) lui sont retirés.
fn build(
    ctx: &Ctx,
    a: &Analysis,
    tail: P2,
    n: usize,
    rho: f64,
    gap: f64,
) -> Result<Expansion, RuleError> {
    let f = &a.region;
    let rin = a.inscribed.r;
    {
        let far = a.bbox.diag();
        let e = 1e-5 * far;
        let g = offset_region(f, rho)
            .into_iter()
            .max_by(|x, y| x.area().total_cmp(&y.area()))
            .ok_or(RuleError::Degenerate("parallèle intérieure vide"))?;
        let gl = &g.outer;
        let len = gl.length();
        let span = (n - 1) as f64 * gap * rho;
        if span > 0.6 * len {
            return Err(RuleError::Constraint("tête trop petite pour les lobes"));
        }
        let m = 320;
        // point de tête : le plus éloigné de la queue
        let key = |u: f64| -dist(gl.point_at_length(u).0, tail);
        let u_head = (0..m)
            .map(|i| len * i as f64 / m as f64)
            .min_by(|x, y| key(*x).total_cmp(&key(*y)))
            .unwrap_or(0.0);
        let mut lobes: Vec<(P2, P2, P2)> = Vec::new(); // centre, normale, contact
        for i in 0..n {
            let u = u_head + (i as f64 - 0.5 * (n - 1) as f64) * gap * rho;
            let c = gl.point_at_length(u).0;
            let nrm = grad(f, c, e);
            lobes.push((c, nrm, c - nrm * rho));
        }
        let min_cusp = 0.5 * ctx.w_next.max(ctx.w_min);
        let tol = Tol::new(far);
        let mut stones: Vec<Region> = Vec::new();
        let mut construction = Vec::new();
        for k in 0..n - 1 {
            let (c1, n1, t1) = lobes[k];
            let (c2, n2, t2) = lobes[k + 1];
            let xs = inter_circle_circle(&Circle::new(c1, rho), &Circle::new(c2, rho), &tol);
            if xs.len() != 2 {
                return Err(RuleError::Constraint("lobes disjoints"));
            }
            let x = if f.sdf(xs[0]) < f.sdf(xs[1]) {
                xs[0]
            } else {
                xs[1]
            };
            // pointe assez saillante pour être lisible en pierre
            if f.sdf(x) < min_cusp {
                return Err(RuleError::Constraint("redent trop court"));
            }
            // un redent est une pointe de pierre trapue, pas une épine :
            // base large et saillie mesurée
            let base = dist(t1, t2);
            let depth = f.sdf(x);
            // (une pointe longue se lit comme une branche qui ne mène nulle part)
            if base < 3.0 * min_cusp
                || depth > 0.6 * base
                || depth > 0.5 * rho
                || depth < 0.12 * base
            {
                return Err(RuleError::Constraint("redent mal proportionné"));
            }
            let d = rho;
            let q = Loop::polygon(&[t1 - n1 * d, t1, x, t2, t2 - n2 * d]).oriented(true);
            if geom::clip::self_intersects(&q) {
                return Err(RuleError::Degenerate("coin de redent"));
            }
            let mut s = intersect(std::slice::from_ref(f), &Region::simple(q));
            s = subtract(&s, &Region::circle(c1, rho));
            s = subtract(&s, &Region::circle(c2, rho));
            let s: Vec<Region> = s.into_iter().filter(|r| r.area() > 1e-9 * a.area).collect();
            // compacité : une épine (lobes presque parallèles) est refusée
            let tri = 0.5 * cross(t2 - t1, x - t1).abs();
            let sa: f64 = s.iter().map(|r| r.area()).sum();
            if tri <= 0.0 || sa < 0.4 * tri {
                return Err(RuleError::Constraint("redent en épine"));
            }
            stones.extend(s);
            construction.push(ConstructionEntity::point(x, ConstructionOp::Cusp));
        }
        let mut open = vec![f.clone()];
        for s in &stones {
            open = subtract(&open, s);
        }
        let open = drop_chips(open, ctx.min_opening_r());
        if open.is_empty() {
            return Err(RuleError::Degenerate("jour vide"));
        }
        for (c, _, _) in &lobes {
            construction.push(ConstructionEntity::Circle {
                c: *c,
                r: rho,
                op: ConstructionOp::Cusp,
            });
        }
        Ok(Expansion {
            children: vec![],
            openings: open,
            construction,
            key_ratio: Some(rho / rin),
        })
    }
}
