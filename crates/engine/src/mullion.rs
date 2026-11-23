//! Fenêtre à meneaux et croisées (ADR-30), d'après les croisées des logis
//! de la fin du gothique : la forme est l'arête extérieure du chambranle de
//! pierre ; à l'intérieur, une croix de pierre (meneau vertical et traverse
//! horizontale) partage la baie en jours.
//!
//! - **Croisée** simple (un meneau, une traverse : quatre jours) ou double
//!   (deux meneaux : six jours). La traverse est au-dessus du milieu : les
//!   jours hauts sont plus courts que les jours bas. Une baie basse et
//!   large n'a que des meneaux, une baie très haute deux traverses.
//! - **Chambranle** mouluré : listel et gorge le long de l'arête
//!   extérieure ; autour de chaque jour, une baguette puis une gorge, qui
//!   laissent un listel plat au milieu des meneaux et de la traverse.
//! - **Angles hauts** droits ou arrondis (quart de cercle) : la baie et ses
//!   moulures suivent l'arrondi.
//! - **Bases** prismatiques au pied des meneaux, où meurent les moulures.
//! - **Vitrerie** en losanges ou à petits carreaux (plombs), ou sans.
//! - **Appui** saillant sous la baie, **larmier** à retours au-dessus,
//!   terminé par deux culots.
//!
//! Le chambranle, la croix et les moulures sont des reliefs ; les jours
//! sont des ouvertures. Tout est construit à la règle et au compas
//! (segments et arcs à centre explicite) et entièrement déterminé par la
//! seed.

use crate::design::{Design, Opening, Stats, TreeNode};
use crate::door::{Out, rect, rect_r};
use crate::genome::{GENERATOR_VERSION, Genome, Params};
use crate::rules::{unif, unif_in, weighted_index};
use crate::score::ScoreBreakdown;
use crate::seed::derived;
use crate::style::CrossKind;
use crate::{CancelToken, GenError};
use geom::clip::{intersect_loop, offset_region, subtract, union};
use geom::math::*;
use geom::{BBox, ConstructionEntity, ConstructionOp, Loop, Region, Seg};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;
use std::collections::BTreeMap;

/// Rectangle aux angles hauts arrondis (rayon `r`, quarts de cercle),
/// parcouru dans le sens trigonométrique.
pub(crate) fn round_top_rect(x0: f64, y0: f64, x1: f64, y1: f64, r: f64) -> Loop {
    let r = r.min(0.5 * (x1 - x0)).min(y1 - y0);
    if r <= 1e-9 * (x1 - x0) {
        return rect(x0, y0, x1, y1);
    }
    let segs = vec![
        Seg::line(P2::new(x0, y0), P2::new(x1, y0)),
        Seg::line(P2::new(x1, y0), P2::new(x1, y1 - r)),
        Seg::arc(P2::new(x1 - r, y1 - r), r, 0.0, FRAC_PI_2),
        Seg::line(P2::new(x1 - r, y1), P2::new(x0 + r, y1)),
        Seg::arc(P2::new(x0 + r, y1 - r), r, FRAC_PI_2, FRAC_PI_2),
        Seg::line(P2::new(x0, y1 - r), P2::new(x0, y0)),
    ];
    Loop::new(segs)
}

/// Dilatation (`d > 0`) ou érosion (`d < 0`) : plus grande composante.
fn grow(r: &Region, d: f64) -> Option<Region> {
    offset_region(r, -d)
        .into_iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))
}

/// La forme est-elle un rectangle d'axes horizontal et vertical ?
fn is_axis_rect(shape: &Region) -> bool {
    let bb = shape.bbox();
    let a = bb.size().x * bb.size().y;
    shape.holes.is_empty() && a > 0.0 && (shape.area() - a).abs() < 1e-6 * a
}

/// Bande de plomb de demi-largeur `h` autour de la droite passant par `p`
/// de direction `t`, assez longue pour traverser la boîte `bb`.
fn lead_strip(p: P2, t: P2, h: f64, bb: &BBox) -> Loop {
    let l = 2.0 * bb.diag();
    let n = perp(t) * h;
    let (a, b) = (p - t * l, p + t * l);
    Loop::polygon(&[a - n, b - n, b + n, a + n])
}

/// Vitrerie d'un jour : plombs en losanges ou à carreaux, de pas `dx`
/// (ajusté à la largeur du jour) et de hauteur `dy`, centrés sur le jour.
fn glazing(light: &Region, diamonds: bool, dx: f64, dy: f64, h: f64) -> Vec<Region> {
    let bb = light.bbox();
    let (lw, lh) = (bb.size().x, bb.size().y);
    let n = round(lw / dx).max(1.0);
    let dx = lw / n;
    let c = bb.center();
    let mut out = Vec::new();
    let mut strip = |p: P2, t: P2| {
        // point de la droite le plus proche du centre : la bande traverse
        let p = p + t * dot(c - p, t);
        let s = lead_strip(p, t, h, &bb);
        out.extend(intersect_loop(std::slice::from_ref(light), &s));
    };
    if diamonds {
        // deux familles de droites de pente ±dy/dx passant par les points
        // (x₀ + k·dx, bas du jour) : losanges de largeur dx et de hauteur dy
        let t1 = unit(P2::new(dx, dy));
        let t2 = unit(P2::new(-dx, dy));
        let reach = (lh * dx / dy / dx).ceil() as i64 + 1;
        let x0 = bb.min.x;
        for k in -reach..=(n as i64 + reach) {
            let p = P2::new(x0 + k as f64 * dx, bb.min.y);
            strip(p, t1);
            strip(p, t2);
        }
    } else {
        for k in 1..n as i64 {
            strip(P2::new(bb.min.x + k as f64 * dx, c.y), P2::new(0.0, 1.0));
        }
        let m = round(lh / dy).max(1.0);
        let dy = lh / m;
        for j in 1..m as i64 {
            strip(P2::new(c.x, bb.min.y + j as f64 * dy), P2::new(1.0, 0.0));
        }
    }
    out
}

/// Base prismatique au pied d'un meneau (axe `x`, largeur `m`).
fn base(x: f64, y: f64, m: f64, hb: f64) -> Region {
    Region::simple(Loop::polygon(&[
        P2::new(x - 0.5 * m, y),
        P2::new(x + 0.5 * m, y),
        P2::new(x + 0.5 * m, y + 0.55 * hb),
        P2::new(x + 0.22 * m, y + hb),
        P2::new(x - 0.22 * m, y + hb),
        P2::new(x - 0.5 * m, y + 0.55 * hb),
    ]))
}

/// Génère une fenêtre à meneaux et croisées.
pub fn generate_mullion(
    shape: &Region,
    genome: &Genome,
    cancel: &CancelToken,
) -> Result<Design, GenError> {
    #[cfg(not(target_arch = "wasm32"))]
    let t0 = std::time::Instant::now();
    let shape = Region::new(shape.outer.clone(), shape.holes.clone());
    let bb = shape.bbox();
    let (x0, y0, x1, y1) = (bb.min.x, bb.min.y, bb.max.x, bb.max.y);
    let (w, h) = (x1 - x0, y1 - y0);
    let xm = 0.5 * (x0 + x1);
    let u = w.min(h);
    let style = &genome.style;
    let mut rng = ChaCha8Rng::seed_from_u64(derived(genome.seed, 0x3E2E));
    let mut out = Out {
        ornaments: Vec::new(),
        openings: Vec::new(),
        construction: Vec::new(),
        nodes: Vec::new(),
        min_area: 1e-7 * u * u,
    };
    let cancelled = || {
        if cancel.is_cancelled() {
            Err(GenError::Cancelled)
        } else {
            Ok(())
        }
    };

    // ---------------------------------------------------------- mesures
    // chambranle f, meneau m, traverse mt (fractions de la petite dimension)
    let f = u * unif_in(&mut rng, 0.085, 0.115);
    let m = f * unif_in(&mut rng, 0.62, 0.78);
    let mt = m * unif_in(&mut rng, 0.92, 1.05);
    let rect_shape = is_axis_rect(&shape);

    // baie (intérieur du chambranle), sans les arrondis
    let bay = if rect_shape {
        Some(rect_r(x0 + f, y0 + f, x1 - f, y1 - f))
    } else {
        grow(&shape, -f)
    };
    let Some(bay0) = bay.filter(|b| b.area() > 0.05 * shape.area()) else {
        return Err(GenError::InvalidShape(
            "forme trop étroite pour une fenêtre à meneaux".into(),
        ));
    };
    let ob = bay0.bbox();
    let (bw, bh) = (ob.size().x, ob.size().y);

    // croisée : nombre de meneaux et de traverses
    let n_m: usize = match style.cross {
        CrossKind::Simple => 1,
        CrossKind::Double => 2,
        CrossKind::Auto => {
            if bw > 0.95 * bh || (bw > 0.75 * bh && unif(&mut rng) < 0.5) {
                2
            } else {
                1
            }
        }
    };
    // un jour trop étroit : moins de meneaux
    let n_m = (0..=n_m)
        .rev()
        .find(|&n| (bw - n as f64 * m) / (n + 1) as f64 > 1.6 * m)
        .unwrap_or(0);
    let light_w = (bw - n_m as f64 * m) / (n_m + 1) as f64;
    let n_t: usize = if bh < 1.1 * light_w {
        0
    } else if bh > 3.2 * light_w && bh > 2.4 * bw {
        2
    } else {
        1
    };
    let y_tr: Vec<f64> = match n_t {
        0 => vec![],
        1 => vec![ob.min.y + bh * unif_in(&mut rng, 0.58, 0.66)],
        _ => vec![ob.min.y + 0.42 * bh, ob.min.y + 0.72 * bh],
    };
    let x_mul: Vec<f64> = (1..=n_m)
        .map(|i| ob.min.x + i as f64 * (light_w + m) - 0.5 * m)
        .collect();

    // angles hauts arrondis (baies rectangulaires seulement)
    let rounded = rect_shape && style.round_top.unwrap_or_else(|| unif(&mut rng) < 0.4);
    let r_top = if rounded {
        let top_h = y_tr.last().map_or(bh, |y| ob.max.y - y - 0.5 * mt);
        (light_w * unif_in(&mut rng, 0.3, 0.5)).min(0.6 * top_h)
    } else {
        0.0
    };
    let bay = if rounded {
        Region::simple(round_top_rect(
            ob.min.x, ob.min.y, ob.max.x, ob.max.y, r_top,
        ))
    } else {
        bay0
    };

    // jours : la baie moins la croix
    let mut lights = vec![bay.clone()];
    let big = bb.grow(u);
    for &x in &x_mul {
        lights = subtract(
            &lights,
            &rect_r(x - 0.5 * m, big.min.y, x + 0.5 * m, big.max.y),
        );
    }
    for &y in &y_tr {
        lights = subtract(
            &lights,
            &rect_r(big.min.x, y - 0.5 * mt, big.max.x, y + 0.5 * mt),
        );
    }
    lights.retain(|l| l.area() > 1e-4 * bay.area());
    // de haut en bas, de gauche à droite
    lights.sort_by(|a, b| {
        let (ca, cb) = (a.bbox().center(), b.bbox().center());
        cb.y.total_cmp(&ca.y).then(ca.x.total_cmp(&cb.x))
    });
    cancelled()?;

    // ------------------------------------------------------- chambranle
    let p_frame = out.node("Chambranle et croisée");
    let mut stone = vec![shape.clone()];
    for l in &lights {
        stone = subtract(&stone, l);
    }
    out.relief(&p_frame, 1, stone);

    // moulures : arête extérieure (listel, gorge) et pourtour des jours
    let p_mould = out.node("Moulures");
    if let (Some(a), Some(b)) = (grow(&shape, -0.14 * f), grow(&shape, -0.26 * f)) {
        out.relief(&p_mould, 2, subtract(std::slice::from_ref(&shape), &a));
        out.creux(&p_mould, 1, subtract(&[a], &b));
    }
    let (g1, g2) = (0.24 * m, 0.42 * m);
    for l in &lights {
        let (Some(a), Some(b)) = (grow(l, g1), grow(l, g2)) else {
            continue;
        };
        out.relief(&p_mould, 2, subtract(std::slice::from_ref(&a), l));
        out.creux(&p_mould, 1, subtract(&[b], &a));
    }
    cancelled()?;

    // bases au pied des meneaux
    if rect_shape && !x_mul.is_empty() && unif(&mut rng) < 0.7 {
        let p_base = out.node("Bases");
        let hb = m * unif_in(&mut rng, 1.0, 1.5);
        let regs: Vec<Region> = x_mul.iter().map(|&x| base(x, ob.min.y, m, hb)).collect();
        out.relief(&p_base, 2, regs);
    }

    // ------------------------------------------------------------ jours
    let p_lights = out.node("Jours");
    for l in &lights {
        out.openings.push(Opening {
            region: l.clone(),
            depth: 1,
            path: p_lights.clone(),
            rule: "Jour".into(),
        });
    }

    // vitrerie : losanges, petits carreaux ou verre uni
    let glass = weighted_index(&mut rng, &[0.55, 0.25, 0.2]).unwrap_or(0);
    if glass < 2 {
        let p_glass = out.node("Vitrerie");
        let dx = light_w / round(light_w / (u * unif_in(&mut rng, 0.075, 0.1))).max(2.0);
        let dy = if glass == 0 {
            dx * unif_in(&mut rng, 1.45, 1.75)
        } else {
            dx * unif_in(&mut rng, 1.0, 1.25)
        };
        let lead = 0.5 * (0.12 * m).max(0.009 * u);
        for l in &lights {
            let regs = glazing(l, glass == 0, dx, dy, lead);
            out.relief(&p_glass, 3, regs);
            cancelled()?;
        }
    }

    // ------------------------------------------------- appui et larmier
    if rect_shape && unif(&mut rng) < 0.8 {
        let p_sill = out.node("Appui");
        let o = f * unif_in(&mut rng, 0.35, 0.6);
        let hs = f * unif_in(&mut rng, 0.55, 0.8);
        out.relief(&p_sill, 2, [rect_r(x0 - o, y0 - hs, x1 + o, y0 + 0.3 * f)]);
        // goutte d'eau sous l'appui
        out.creux(
            &p_sill,
            1,
            [rect_r(
                x0 - 0.6 * o,
                y0 - 0.78 * hs,
                x1 + 0.6 * o,
                y0 - 0.7 * hs,
            )],
        );
    }
    if rect_shape && unif(&mut rng) < 0.55 {
        let p_hood = out.node("Larmier");
        let o = f * unif_in(&mut rng, 0.45, 0.7);
        let hl = f * unif_in(&mut rng, 0.45, 0.6);
        let gap = f * unif_in(&mut rng, 0.15, 0.3);
        let drop = h * unif_in(&mut rng, 0.07, 0.12);
        let yb = y1 + gap;
        let (xa, xb) = (x0 - o, x1 + o);
        let mut hood = vec![rect_r(xa, yb, xb, yb + hl)];
        for xs in [xa, xb - hl] {
            hood = union(&hood, &rect_r(xs, yb - drop, xs + hl, yb + 0.5 * hl));
        }
        out.relief(&p_hood, 2, hood);
        // gorge sous la tablette
        out.creux(
            &p_hood,
            1,
            [rect_r(
                xa + 0.25 * hl,
                yb + 0.12 * hl,
                xb - 0.25 * hl,
                yb + 0.3 * hl,
            )],
        );
        // culots : bossages ronds au bas des retours
        let regs: Vec<Region> = [xa + 0.5 * hl, xb - 0.5 * hl]
            .iter()
            .map(|&xc| Region::circle(P2::new(xc, yb - drop), 0.62 * hl))
            .collect();
        out.relief(&p_hood, 3, regs);
    }
    cancelled()?;

    // ------------------------------------------------------ construction
    let axis = |a: P2, b: P2| ConstructionEntity::Line {
        a,
        b,
        op: ConstructionOp::Axis,
    };
    out.construction
        .push(axis(P2::new(xm, y0 - 0.1 * h), P2::new(xm, y1 + 0.1 * h)));
    for &x in &x_mul {
        out.construction.push(axis(P2::new(x, y0), P2::new(x, y1)));
    }
    for &y in &y_tr {
        out.construction.push(axis(P2::new(x0, y), P2::new(x1, y)));
    }
    if rounded {
        for c in [
            P2::new(ob.min.x + r_top, ob.max.y - r_top),
            P2::new(ob.max.x - r_top, ob.max.y - r_top),
        ] {
            out.construction
                .push(ConstructionEntity::point(c, ConstructionOp::ArchCenter));
            out.construction.push(ConstructionEntity::Circle {
                c,
                r: r_top,
                op: ConstructionOp::ArchCenter,
            });
        }
    }

    // ------------------------------------------------------------ arbre
    let root_bb = out
        .ornaments
        .iter()
        .fold(shape.bbox(), |b, o| b.union(&o.region.bbox()));
    let mut tree = vec![TreeNode {
        path: "0".into(),
        depth: 0,
        rule: "Croisée".into(),
        params: Params::new()
            .with("meneaux", n_m as f64)
            .with("traverses", n_t as f64)
            .with("chambranle", f)
            .with("meneau", m),
        types: vec!["croisée".into()],
        area: shape.area(),
        inscribed: 0.0,
        local_score: 1.0,
        twin_of: None,
        bbox: root_bb,
        locked: false,
        ratio: Some(bh / bw.max(1e-9)),
        candidates: 1,
        region: Some(shape.clone()),
    }];
    for (p, name) in &out.nodes {
        let orn = out.ornaments.iter().filter(|o| &o.path == p);
        let ops = out.openings.iter().filter(|o| &o.path == p);
        let regs: Vec<&Region> = orn
            .map(|o| &o.region)
            .chain(ops.map(|o| &o.region))
            .collect();
        let nb = regs.iter().fold(BBox::EMPTY, |b, r| b.union(&r.bbox()));
        if nb.is_empty() {
            continue;
        }
        tree.push(TreeNode {
            path: p.clone(),
            depth: 1,
            rule: name.clone(),
            params: Params::new(),
            types: vec![],
            area: regs.iter().map(|r| r.area()).sum(),
            inscribed: 0.0,
            local_score: 1.0,
            twin_of: None,
            bbox: nb,
            locked: false,
            ratio: None,
            candidates: 1,
            region: None,
        });
    }

    let mut terms = BTreeMap::new();
    terms.insert("croisée".to_string(), 1.0);
    #[allow(unused_mut)]
    let mut stats = Stats {
        nodes: tree.len(),
        ..Default::default()
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        stats.millis = t0.elapsed().as_secs_f64() * 1000.0;
    }
    let mut d = Design {
        version: GENERATOR_VERSION.to_string(),
        shape,
        openings: out.openings,
        construction: out.construction,
        tree,
        score: ScoreBreakdown { total: 1.0, terms },
        stats,
        // unité du rendu en relief : demi-chambranle ; puis le meneau
        widths: vec![0.6 * f, m],
        warnings: Vec::new(),
        door: true,
        iron: false,
        ornaments: out.ornaments,
    };
    d.stats.primitives = d.primitive_count();
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::validate;
    use crate::{Family, StyleParams, WindowKind};

    fn genome(seed: u64, cross: CrossKind, round: Option<bool>) -> Genome {
        let mut st = StyleParams::for_family(Family::Flamboyant);
        st.window = WindowKind::Meneaux;
        st.cross = cross;
        st.round_top = round;
        Genome::new(seed, st)
    }

    fn window(w: f64, h: f64) -> Region {
        rect_r(-0.5 * w, 0.0, 0.5 * w, h)
    }

    #[test]
    fn croisees_simple_et_double() {
        let c = CancelToken::new();
        let r = window(1000.0, 1700.0);
        let d = generate_mullion(&r, &genome(1, CrossKind::Simple, Some(false)), &c).unwrap();
        assert!(d.door && !d.iron);
        assert_eq!(d.openings.len(), 4, "croisée simple : quatre jours");
        let d = generate_mullion(&r, &genome(1, CrossKind::Double, Some(false)), &c).unwrap();
        assert_eq!(d.openings.len(), 6, "croisée double : six jours");
        // jours hauts plus courts que les jours bas
        let hs: Vec<f64> = d
            .openings
            .iter()
            .map(|o| o.region.bbox().size().y)
            .collect();
        assert!(hs[0] < hs[5]);
        // symétrie : premier et dernier jour du haut de même largeur
        let ws: Vec<f64> = d
            .openings
            .iter()
            .map(|o| o.region.bbox().size().x)
            .collect();
        assert!((ws[0] - ws[2]).abs() < 1e-6 * 1000.0);
    }

    #[test]
    fn angles_arrondis() {
        let c = CancelToken::new();
        let r = window(900.0, 1600.0);
        let d = generate_mullion(&r, &genome(2, CrossKind::Simple, Some(true)), &c).unwrap();
        // les deux jours du haut portent chacun un quart de cercle
        let arcs = |o: &Opening| o.region.segs().filter(|s| s.is_arc()).count();
        assert_eq!(arcs(&d.openings[0]), 1);
        assert_eq!(arcs(&d.openings[1]), 1);
        assert_eq!(arcs(&d.openings[2]), 0);
    }

    #[test]
    fn invariants_et_determinisme() {
        let c = CancelToken::new();
        let shapes = [
            window(1000.0, 1700.0),
            window(1600.0, 1400.0),
            window(600.0, 2200.0),
            Region::simple(motifs::arch_window(motifs::ArchKind::equilateral(), 1000.0, 1200.0).0),
            Region::circle(P2::ZERO, 600.0),
        ];
        for (i, r) in shapes.iter().enumerate() {
            for seed in 0..6 {
                let g = genome(seed, CrossKind::Auto, None);
                let a = generate_mullion(r, &g, &c).unwrap();
                let b = generate_mullion(r, &g, &c).unwrap();
                assert_eq!(a.openings, b.openings, "forme {i}, seed {seed}");
                assert_eq!(
                    serde_json::to_string(&a.ornaments).unwrap(),
                    serde_json::to_string(&b.ornaments).unwrap()
                );
                assert!(!a.openings.is_empty());
                let v = validate(&a, 0.0, 0.0);
                assert!(v.is_empty(), "forme {i}, seed {seed} : {v:?}");
            }
        }
    }
}
