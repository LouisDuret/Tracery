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
//!   extérieure, baguette en saillie le long de la baie.
//! - **Croix en saillie** sur le chambranle, avec un filet plus saillant
//!   au milieu des meneaux et de la traverse ; les vitres et leurs plombs
//!   sont au fond de la baie.
//! - **Angles hauts** droits ou arrondis (quart de cercle) : la baie, ses
//!   moulures et l'arête extérieure du chambranle (arc concentrique)
//!   suivent l'arrondi.
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
use crate::door::{
    Bay, Out, accolade_half, band_between, crockets, fleuron, length_of, offset_half, pinnacle,
    point_on, rect, rect_r,
};
use crate::genome::{GENERATOR_VERSION, Genome, Params};
use crate::rules::{unif, unif_in, weighted_index};
use crate::score::ScoreBreakdown;
use crate::seed::derived;
use crate::style::CrossKind;
use crate::{CancelToken, GenError};
use geom::clip::{intersect_loop, offset_region, subtract, union};
use geom::math::*;
use geom::{BBox, ConstructionEntity, ConstructionOp, Loop, Region, Seg, Xf};
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

/// Bande horizontale symétrique : `left` est le profil de son bout gauche,
/// du haut vers le bas ; le bout droit en est le miroir par `m`. Parcourue
/// dans le sens trigonométrique.
fn band_loop(left: Vec<Seg>, m: &Xf) -> Loop {
    let (Some(first), Some(last)) = (left.first(), left.last()) else {
        return Loop::new(Vec::new());
    };
    let (lt, lb) = (first.start(), last.end());
    let right: Vec<Seg> = left
        .iter()
        .rev()
        .map(|s| s.transform(m).reversed())
        .collect();
    let mut segs = left.clone();
    segs.push(Seg::line(lb, m.apply(lb)));
    segs.extend(right);
    segs.push(Seg::line(m.apply(lt), lt));
    Loop::new(segs).oriented(true)
}

/// Nez ovale d'une tablette d'appui, du haut (`xs`, `yt`) au bas (`xs`,
/// `yb`), de saillie `a` : arc de tête (rayon trois quarts de la demi-hauteur)
/// et deux arcs de flanc qui lui sont tangents en `phi` et qui passent par
/// les arêtes de la tablette, qu'ils rejoignent en biais.
fn nose(xs: f64, yt: f64, yb: f64, a: f64, phi: f64) -> Vec<Seg> {
    let b = 0.5 * (yt - yb);
    let yc = 0.5 * (yt + yb);
    let (t, tb) = (P2::new(xs, yt), P2::new(xs, yb));
    let r2 = (0.75 * b).min(0.6 * a);
    let c2 = P2::new(xs - a + r2, yc);
    let u = P2::new(cos(phi), sin(phi));
    let p = c2 + u * r2;
    let d = p - t;
    let dt = dot(d, u);
    if dt <= 1e-9 * b {
        return vec![Seg::through(t, P2::new(xs - a, yc), tb)];
    }
    let r1 = dot(d, d) / (2.0 * dt);
    let c1 = p - u * r1;
    let flip = |q: P2| P2::new(q.x, 2.0 * yc - q.y);
    vec![
        Seg::arc_from_to(c1, t, p, true),
        Seg::arc_from_to(c2, p, flip(p), true),
        Seg::arc_from_to(flip(c1), flip(p), tb, true),
    ]
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
        (light_w * unif_in(&mut rng, 0.14, 0.24)).min(0.35 * top_h)
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
    // arête extérieure du chambranle : arrondie elle aussi, concentrique à
    // la baie (même centre, rayon augmenté de la largeur du chambranle)
    let shape = if rounded {
        Region::simple(round_top_rect(x0, y0, x1, y1, r_top + f))
    } else {
        shape
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
    // Le chambranle est le plan de fond (niveau 1). La croix et la baguette
    // qui borde la baie sont en saillie (niveau 2), le filet de la croix
    // plus encore (niveau 3) : meneaux et traverses avancent, les vitres
    // sont au fond de la baie.
    let p_frame = out.node("Chambranle");
    out.relief(&p_frame, 1, subtract(std::slice::from_ref(&shape), &bay));
    let p_mould = out.node("Moulures");
    // arête extérieure : listel et gorge
    if let (Some(a), Some(b)) = (grow(&shape, -0.14 * f), grow(&shape, -0.26 * f)) {
        out.relief(&p_mould, 2, subtract(std::slice::from_ref(&shape), &a));
        out.creux(&p_mould, 1, subtract(&[a], &b));
    }
    // baguette le long de la baie, au nu de la croix
    if let Some(a) = grow(&bay, 0.3 * m) {
        out.relief(&p_mould, 2, subtract(std::slice::from_ref(&a), &bay));
    }
    let p_cross = out.node("Croisée");
    let mut cross = vec![bay.clone()];
    for l in &lights {
        cross = subtract(&cross, l);
    }
    out.relief(&p_cross, 2, cross);
    // filet au milieu des meneaux et de la traverse
    let wf = 0.32 * m;
    let mut fillets = Vec::new();
    for &x in &x_mul {
        let r = rect(x - 0.5 * wf, big.min.y, x + 0.5 * wf, big.max.y);
        fillets.extend(intersect_loop(std::slice::from_ref(&bay), &r));
    }
    for &y in &y_tr {
        let r = rect(big.min.x, y - 0.5 * wf, big.max.x, y + 0.5 * wf);
        fillets.extend(intersect_loop(std::slice::from_ref(&bay), &r));
    }
    out.relief(&p_cross, 3, fillets);
    cancelled()?;

    // bases au pied des meneaux, devant le filet
    if rect_shape && !x_mul.is_empty() && unif(&mut rng) < 0.7 {
        let p_base = out.node("Bases");
        let hb = m * unif_in(&mut rng, 1.0, 1.5);
        let regs: Vec<Region> = x_mul.iter().map(|&x| base(x, ob.min.y, m, hb)).collect();
        out.relief(&p_base, 3, regs);
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

    // ---------------------------------------------------- couronnement
    // Accolade, larmier droit ou rien. L'accolade est favorisée sur les
    // baies étroites et évitée sur les larges ; une baie arrondie n'a pas
    // de larmier droit (ADR-30).
    let ratio = w / h.max(1e-9);
    let w_acc = if rect_shape {
        (1.4 - 1.35 * ratio).clamp(0.0, 0.85)
    } else {
        0.0
    };
    let w_hood = if rect_shape && !rounded {
        0.5 * (1.0 - w_acc)
    } else {
        0.0
    };
    let crown =
        weighted_index(&mut rng, &[w_acc, w_hood, (1.0 - w_acc - w_hood).max(0.0)]).unwrap_or(2);
    // sans couronnement (lucarne) : le tirage est fait quand même, pour
    // que la suite de la seed ne change pas
    let crown = if style.plain_top { 2 } else { crown };
    let b = f * unif_in(&mut rng, 0.45, 0.6); // largeur de la moulure
    let gap = f * unif_in(&mut rng, 0.15, 0.3); // jour entre chambranle et moulure
    let e = gap + 0.5 * b; // axe de la moulure, depuis l'arête
    let drop = h * unif_in(&mut rng, 0.06, 0.1);
    let pinnacles = crown == 0 && unif(&mut rng) < 0.55;
    let pw = w * unif_in(&mut rng, 0.1, 0.14);
    let x_pin = x0 - e - 0.5 * b - 0.4 * pw; // axe du pinacle gauche
    let m_x = Xf::mirror_x(xm);

    // ------------------------------------------------------------ appui
    // Profil vu de face, de haut en bas (ADR-30) : tablette à nez ovale
    // (deux arcs de flanc et un arc de tête) marquée de deux méridiens,
    // rebord sous la tablette, congé creux qui rentre jusqu'au nu de la
    // face, face, baguette et filet du bas. Chaque moulure est une bande
    // horizontale dont les bouts suivent le profil, en miroir sur l'axe.
    let mut y_floor = y0;
    if rect_shape && (pinnacles || unif(&mut rng) < 0.8) {
        let p_sill = out.node("Appui");
        let hh = f * unif_in(&mut rng, 1.25, 1.6); // hauteur totale
        let th = hh * unif_in(&mut rng, 0.45, 0.5); // tablette
        let a = th * unif_in(&mut rng, 0.65, 0.8); // saillie du nez
        let pt = hh * unif_in(&mut rng, 0.18, 0.24); // tablette au-delà de la face
        let phi = unif_in(&mut rng, 112.0, 118.0) * PI / 180.0;
        // nu de la face : sous les pinacles, le nez les déborde
        let op = if pinnacles {
            (x0 - (x_pin - 0.5 * pw) + 0.15 * f - pt - a).max(0.15 * f)
        } else {
            f * unif_in(&mut rng, 0.15, 0.35)
        };
        let xp = x0 - op;
        let xs = xp - pt; // naissance du nez
        let yt = y0;
        let y_tab = yt - th;
        let y_lip = y_tab - 0.03 * hh;
        let rc = 0.1 * hh;
        let y_cv = y_lip - 0.12 * hh;
        let yb = yt - hh;
        let (y_bt, y_bb) = (yb + 0.14 * hh, yb + 0.035 * hh);
        let pb = 0.045 * hh;
        let band = |left: Vec<Seg>| Region::simple(band_loop(left, &m_x));
        // tablette, puis deux méridiens qui disent l'arrondi du nez
        out.relief(&p_sill, 3, [band(nose(xs, yt, y_tab, a, phi))]);
        for k in [1.0, 2.0] {
            let nk = nose(xs + 0.2 * a * k, yt, y_tab, a * (1.0 - 0.36 * k), phi);
            out.relief(&p_sill, 3, [band(nk)]);
        }
        // rebord sous la tablette
        out.relief(
            &p_sill,
            2,
            [band(vec![Seg::line(
                P2::new(xs + 0.01 * hh, y_tab),
                P2::new(xs + 0.03 * hh, y_lip),
            )])],
        );
        // congé creux : quart de cercle de centre hors de la pierre
        let cc = P2::new(xp - rc, y_lip - rc);
        let cove = band(vec![
            Seg::arc_from_to(cc, P2::new(xp - rc, y_lip), P2::new(xp, y_lip - rc), false),
            Seg::line(P2::new(xp, y_lip - rc), P2::new(xp, y_cv)),
        ]);
        out.relief(&p_sill, 1, [cove.clone()]);
        out.creux(&p_sill, 1, [cove]);
        // face, baguette, filet
        out.relief(
            &p_sill,
            1,
            [band(vec![Seg::line(P2::new(xp, y_cv), P2::new(xp, y_bt))])],
        );
        out.relief(
            &p_sill,
            2,
            [band(vec![Seg::through(
                P2::new(xp, y_bt),
                P2::new(xp - pb, 0.5 * (y_bt + y_bb)),
                P2::new(xp, y_bb),
            )])],
        );
        out.relief(
            &p_sill,
            1,
            [band(vec![Seg::line(P2::new(xp, y_bb), P2::new(xp, yb))])],
        );
        // joints de pose sous les meneaux d'un long appui
        if w > 0.75 * h && !x_mul.is_empty() && unif(&mut rng) < 0.7 {
            let jw = 0.5 * (0.035 * f).max(0.003 * u);
            let joints: Vec<Region> = x_mul
                .iter()
                .map(|&x| rect_r(x - jw, yb, x + jw, yt))
                .collect();
            out.creux(&p_sill, 1, joints);
        }
        y_floor = yt;
    }

    // --------------------------------------------------------- accolade
    if crown == 0 {
        // naissance : sous l'arrondi des angles, ou au-dessus des angles
        // droits ; les retours descendent jusqu'aux culots ou aux pinacles
        let ys = if rounded { y1 - (r_top + f) } else { y1 + gap };
        let y_low = if pinnacles { y_floor } else { ys - drop };
        let bay = Bay {
            x0,
            x1,
            xm,
            y0: y_low,
            ys,
            ya: y1,
            s: w,
            head: None,
        };
        // l'axe de la moulure passe au large du chambranle
        let clear = |segs: &[Seg]| {
            let l = length_of(segs);
            (0..=48).all(|k| {
                let (p, _) = point_on(segs, l * k as f64 / 48.0);
                -shape.sdf(p) >= 0.5 * b + 0.4 * gap
            })
        };
        let mut acc = None;
        for _ in 0..40 {
            let y_t = y1 + e + w * unif_in(&mut rng, 0.22, 0.42);
            let tip = unif_in(&mut rng, 0.30, 0.42) * PI;
            if let Some((centre, y_t)) = accolade_half(&bay, e, 0.0, 1.0, y_t, tip)
                && clear(&centre)
                && let (Some(o), Some(i)) = (
                    offset_half(&centre, 0.5 * b, xm),
                    offset_half(&centre, -0.5 * b, xm),
                )
                && let Some(band) = band_between(&o, &i, &m_x)
            {
                acc = Some((centre, o, band, y_t));
                break;
            }
        }
        if let Some((centre, outer, band, y_t)) = acc {
            let p_acc = out.node("Accolade");
            out.relief(&p_acc, 2, [band]);
            let g = 0.2 * b;
            if let (Some(go), Some(gi)) = (
                offset_half(&centre, 0.5 * g, xm),
                offset_half(&centre, -0.5 * g, xm),
            ) && let Some(groove) = band_between(&go, &gi, &m_x)
            {
                out.creux(&p_acc, 1, [groove]);
            }
            for sg in &centre {
                if let Seg::Arc { c, r, .. } = *sg {
                    for q in [c, m_x.apply(c)] {
                        out.construction
                            .push(ConstructionEntity::point(q, ConstructionOp::ArchCenter));
                        out.construction.push(ConstructionEntity::Circle {
                            c: q,
                            r,
                            op: ConstructionOp::ArchCenter,
                        });
                    }
                }
            }
            // crochets en chou frisé sur l'extrados, de la pointe jusqu'au
            // milieu de l'arc convexe
            if unif(&mut rng) < 0.75 {
                let p_croc = out.node("Crochets");
                let jamb = if outer.len() >= 3 {
                    outer[0].length()
                } else {
                    0.0
                };
                let a1 = outer
                    .len()
                    .checked_sub(2)
                    .map_or(0.0, |i| outer[i].length());
                let dc = w * unif_in(&mut rng, 0.1, 0.14);
                let lc = b * unif_in(&mut rng, 1.6, 2.2);
                crockets(&mut out, &p_croc, &outer, &m_x, b, jamb + 0.5 * a1, dc, lc);
            }
            // fleuron : bouquet de choux frisés ou croix fleuronnée
            let p_fin = out.node("Fleuron");
            let tip_y = outer.last().map_or(y_t, |s| s.end().y);
            let fh = w * unif_in(&mut rng, 0.22, 0.34);
            fleuron(&mut out, &mut rng, &p_fin, xm, y_t, tip_y, b, fh);
            if pinnacles {
                // pinacles de part et d'autre : fût, gâble, flèche à
                // crochets et épi ; l'accolade naît contre eux
                let p_pin = out.node("Pinacles");
                let hb = pw * unif_in(&mut rng, 0.8, 1.2);
                let y_pt = ys + (y_t - ys) * unif_in(&mut rng, 0.2, 0.45);
                let left = pinnacle(&mut rng, x_pin, pw, y_floor, y_floor + hb, y_pt);
                for (kind, lv, r) in left {
                    let rr = r.transform(&m_x);
                    out.add(&p_pin, kind, lv, [r, rr]);
                }
            } else {
                // culots au bas des retours
                let p_cul = out.node("Culots");
                let c = P2::new(x0 - e, y_low);
                out.relief(
                    &p_cul,
                    3,
                    [
                        Region::circle(c, 0.62 * b),
                        Region::circle(m_x.apply(c), 0.62 * b),
                    ],
                );
            }
        }
    }

    // ---------------------------------------------------- larmier droit
    if crown == 1 {
        let p_hood = out.node("Larmier");
        let o = f * unif_in(&mut rng, 0.45, 0.7);
        let hl = b;
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
        // l'arête extérieure est arrondie aussi, concentrique à la baie
        let outer: Vec<Seg> = d.shape.segs().filter(|s| s.is_arc()).copied().collect();
        let inner: Vec<Seg> = d.openings[0]
            .region
            .segs()
            .filter(|s| s.is_arc())
            .copied()
            .collect();
        assert_eq!(outer.len(), 2);
        let (Seg::Arc { c: co, r: ro, .. }, Seg::Arc { c: ci, r: ri, .. }) = (outer[1], inner[0])
        else {
            panic!("arcs attendus");
        };
        assert!(dist(co, ci) < 1e-9 * 900.0, "centres confondus");
        assert!(ro > ri);
        // la forme reste dans le rectangle tracé
        let bb = d.shape.bbox();
        assert!(bb.min.x >= -450.0 - 1e-9 && bb.max.y <= 1600.0 + 1e-9);
    }

    #[test]
    fn accolades() {
        // accolade : fleuron sur l'axe, au-dessus de la baie ; fréquente sur
        // une baie étroite, rare sur une baie large
        let c = CancelToken::new();
        let count = |r: &Region, h: f64, round: Option<bool>| {
            let mut n = 0;
            for seed in 0..40 {
                let d = generate_mullion(r, &genome(seed, CrossKind::Auto, round), &c).unwrap();
                if !d.tree.iter().any(|t| t.rule == "Accolade") {
                    continue;
                }
                n += 1;
                let p_fin = d
                    .tree
                    .iter()
                    .find(|t| t.rule == "Fleuron")
                    .unwrap()
                    .path
                    .clone();
                let bb = d
                    .ornaments
                    .iter()
                    .filter(|o| o.path == p_fin)
                    .fold(BBox::EMPTY, |b, o| b.union(&o.region.bbox()));
                assert!(bb.center().x.abs() < 1e-6 * h, "fleuron sur l'axe");
                assert!(bb.min.y > h, "fleuron au-dessus de la baie");
            }
            n
        };
        let narrow = count(&window(800.0, 1500.0), 1500.0, None);
        let rounded = count(&window(800.0, 1500.0), 1500.0, Some(true));
        let wide = count(&window(1800.0, 1200.0), 1200.0, None);
        assert!(
            narrow >= 20 && rounded >= 20,
            "étroite : {narrow}, arrondie : {rounded}"
        );
        assert!(wide <= 6, "large : {wide}");
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
