//! R15 — Cercles maximaux (§ 5.5, annexe A.2) et R03 — tête multi-cercles.
//!
//! Placement glouton : tant que le rayon disponible ≥ r_min, prendre un
//! maximum de libre(x) = min(d_∂(x) − g, minⱼ(|x − cⱼ| − rⱼ − w)) parmi les q
//! meilleurs (tirage softmax), puis accrocher le cercle par Apollonius sur
//! ses contraintes actives (tangences exactes à l'écart w). En symétrie, le
//! premier cercle est sur l'axe et les suivants vont par paires miroirs.
//! Les cellules interstitielles sont R ∖ ⋃ D(cⱼ, rⱼ + w).

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use geom::sdf::{FreeGrid, Obstacle, free_value, refine_max, snap_circle};
use geom::{ConstructionOp, Region};
use rand_chacha::rand_core::SeedableRng;

pub struct MaxCircles {
    pub id: &'static str,
    pub head: bool,
}

fn softmax_pick(rng: &mut ChaCha8Rng, vals: &[f64], t: f64) -> usize {
    if vals.is_empty() {
        return 0;
    }
    let m = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let w: Vec<f64> = vals.iter().map(|v| exp((v - m) / t.max(1e-9))).collect();
    weighted_index(rng, &w).unwrap_or(0)
}

/// Maximum de la fonction libre sur l'axe vertical x = ax.
fn axis_max(f: &Region, obs: &[Obstacle], g: f64, ax: f64) -> (P2, f64) {
    let bb = f.bbox();
    let mut best = (P2::new(ax, bb.center().y), f64::NEG_INFINITY);
    for k in 0..=96 {
        let y = bb.min.y + (bb.max.y - bb.min.y) * k as f64 / 96.0;
        let q = P2::new(ax, y);
        let v = free_value(f, obs, g, q);
        if v > best.1 {
            best = (q, v);
        }
    }
    // raffinement 1D par dichotomie de pas
    let mut step = (bb.max.y - bb.min.y) / 96.0;
    let (mut q, mut v) = best;
    while step > 1e-9 * bb.diag() {
        let mut moved = false;
        for s in [step, -step] {
            let t = P2::new(ax, q.y + s);
            let tv = free_value(f, obs, g, t);
            if tv > v {
                q = t;
                v = tv;
                moved = true;
                break;
            }
        }
        if !moved {
            step *= 0.5;
        }
    }
    (q, v)
}

impl Rule for MaxCircles {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        if self.head {
            "Tête multi-cercles"
        } else {
            "Cercles maximaux"
        }
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only || a.inscribed.r < ctx.edge_gap() + 2.0 * ctx.w {
            return 0.0;
        }
        let t = |c: CellType| a.confidence(c);
        if self.head {
            return t(CellType::Tympan).max(0.2 * t(CellType::Arch));
        }
        let v = t(CellType::Irregular)
            .max(0.8 * t(CellType::Annulus))
            .max(0.6 * t(CellType::Band))
            .max(0.5 * t(CellType::Tapered))
            .max(0.5 * t(CellType::Tympan))
            .max(0.5 * t(CellType::Polygon))
            .max(0.4 * t(CellType::Lunule))
            .max(0.4 * t(CellType::Quad))
            .max(0.3 * t(CellType::Arch))
            .max(0.3 * t(CellType::Lens))
            .max(0.3 * t(CellType::Sector))
            .max(0.2 * t(CellType::Triangle));
        if a.circle.is_some() { 0.1 } else { v.max(0.1) }
    }
    fn sample(&self, _ctx: &Ctx, _a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        let count = if self.head {
            2.0 + (unif(rng) * 2.0).floor()
        } else {
            2.0 + (unif(rng) * 9.0).floor()
        };
        Params::new()
            .with("count", count)
            .with("rmax", unif_in(rng, 0.55, 1.0))
            .with("s", (rng.next_u32() >> 8) as f64)
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let f = &a.region;
        let w = ctx.w;
        let g = ctx.edge_gap();
        let count = p.get_usize("count", 4).max(1);
        let r_min = 2.0 * w;
        let mut rng = ChaCha8Rng::seed_from_u64(p.get("s", 0.0) as u64);
        let axis = ctx.sym_axis(a);
        let mut obs: Vec<Obstacle> = Vec::new();
        let mut circles: Vec<(Circle, Option<usize>)> = Vec::new(); // (cercle, jumeau de)
        let r_first = inscribed_with_gap(f, g).map(|c| c.r).unwrap_or(0.0);
        let r_max = (r_first * p.get("rmax", 1.0)).max(r_min);
        let grid_n = 36;
        while circles.len() < count {
            let grid = FreeGrid::new(f, &obs, g, grid_n);
            let mut cands: Vec<(P2, f64, bool)> = grid
                .local_maxima(r_min)
                .into_iter()
                .take(3)
                .map(|(q, _)| {
                    let (q, v) = refine_max(f, &obs, g, q, grid.step);
                    (q, v, false)
                })
                .collect();
            if let Some(ax) = axis {
                cands.retain(|(q, v, _)| (q.x - ax).abs() >= v + 0.5 * w);
                let (q, v) = axis_max(f, &obs, g, ax);
                if v >= r_min {
                    cands.push((q, v, true));
                }
            }
            cands.retain(|c| c.1 >= r_min);
            if cands.is_empty() {
                break;
            }
            let vals: Vec<f64> = cands.iter().map(|c| c.1.min(r_max)).collect();
            let i = softmax_pick(&mut rng, &vals, 0.15 * r_first.max(1e-9));
            let (q, v, on_axis) = cands[i];
            // accrochage exact (si le cercle n'est pas plafonné par r_max)
            let c = if v <= r_max {
                let (s, _) = snap_circle(f, &obs, g, q, v);
                match axis {
                    Some(ax) if on_axis && (s.c.x - ax).abs() > 1e-7 * ctx.tol.l => {
                        Circle::new(q, v)
                    }
                    _ => s,
                }
            } else {
                Circle::new(q, r_max)
            };
            if c.r < r_min {
                break;
            }
            let idx = circles.len();
            circles.push((c, None));
            obs.push(Obstacle { c, gap: w });
            if let (Some(ax), false) = (axis, on_axis) {
                let m = Circle::new(P2::new(2.0 * ax - c.c.x, c.c.y), c.r);
                circles.push((m, Some(idx)));
                obs.push(Obstacle { c: m, gap: w });
            }
        }
        if circles.is_empty() {
            return Err(RuleError::Constraint("aucun cercle"));
        }
        let regions: Vec<Region> = circles
            .iter()
            .map(|(c, _)| Region::circle(c.c, c.r))
            .collect();
        let mut children: Vec<ChildSpec> = Vec::new();
        for (k, (c, tw)) in circles.iter().enumerate() {
            let mut ch = ChildSpec::new(regions[k].clone(), Some(CellType::Circle));
            if let (Some(i), Some(ax)) = (tw, axis) {
                ch.twin = Some((*i, Xf::mirror_x(ax)));
            }
            let _ = c;
            children.push(ch);
        }
        for r in carve(f, &regions, ctx.carve_gap(), ctx.min_opening_r()) {
            children.push(ChildSpec::new(r, None));
        }
        let mut cons = Vec::new();
        for (c, _) in &circles {
            cons.push(ConstructionEntity::point(c.c, ConstructionOp::Inscribed));
            cons.push(ConstructionEntity::circle(*c, ConstructionOp::Inscribed));
        }
        let ratio = circles[0].0.r / a.inscribed.r.max(1e-12);
        Ok(Expansion {
            children,
            openings: vec![],
            construction: cons,
            key_ratio: Some(ratio),
        })
    }
}
