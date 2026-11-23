//! Fonction d'évaluation (§ 5.7) : score local (choix des candidats) et score
//! global détaillé. Les termes coûteux sont calculés sur un raster CPU
//! déterministe.

use crate::analysis::Analysis;
use crate::design::{Opening, TreeNode};
use crate::rules::{Ctx, Expansion};
use crate::style::{Canon, StyleDef, StyleParams, SymMode};
use geom::constr::nearest_canon;
use geom::math::*;
use geom::sdf::max_inscribed;
use geom::{BBox, Region};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Score détaillé : total et termes normalisés dans [0, 1].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub total: f64,
    pub terms: BTreeMap<String, f64>,
}

fn band_score(x: f64, lo: f64, hi: f64, softness: f64) -> f64 {
    if x < lo {
        exp(-((lo - x) / softness).powi(2))
    } else if x > hi {
        exp(-((x - hi) / softness).powi(2))
    } else {
        1.0
    }
}

/// Appariement miroir des pièces (centroïdes et aires) par rapport à x = ax.
fn mirror_match(parts: &[&Region], ax: f64, tol: f64) -> f64 {
    if parts.is_empty() {
        return 1.0;
    }
    let info: Vec<(P2, f64)> = parts
        .iter()
        .map(|r| (r.bbox().center(), r.area()))
        .collect();
    let mut ok = 0;
    for (c, a) in &info {
        let m = P2::new(2.0 * ax - c.x, c.y);
        if info
            .iter()
            .any(|(d, b)| dist(*d, m) < tol && (a - b).abs() <= 0.03 * a.max(*b))
        {
            ok += 1;
        }
    }
    ok as f64 / info.len() as f64
}

/// Score local d'une expansion (choix parmi les K candidats).
pub fn local_score(ctx: &Ctx, a: &Analysis, rule_id: &str, e: &Expansion, prior: f64) -> f64 {
    let area = a.area.max(1e-300);
    let cov = e.covered_area() / area;
    let terminal = e.children.is_empty();
    let tau = ctx.style.density;
    let mut terms: Vec<(f64, f64)> = Vec::new();
    // couverture
    let s_cov = if terminal {
        exp(-((cov - 0.85) / 0.3).powi(2))
    } else {
        let c = (tau.sqrt() + 0.12).clamp(0.6, 0.92);
        exp(-((cov - c) / 0.22).powi(2))
    };
    terms.push((s_cov, 1.0));
    // structure : une grande cellule laissée en jour simple est un vide
    if terminal {
        let rel = a.inscribed.r / (4.0 * ctx.w);
        let pen = match rule_id {
            "R19" => 1.0,
            "R09" => 0.8,
            _ => 0.25,
        };
        let s = if rel > 1.0 && ctx.depth + 1 < ctx.max_depth {
            exp(-(rel - 1.0) * 1.5 * pen)
        } else {
            1.0
        };
        terms.push((s, 2.0));
    } else {
        // hiérarchie : rapport des rayons inscrits enfant / parent
        // moyenne pondérée par l'aire : les petits écoinçons comptent peu
        let mut acc = 0.0;
        let mut n = 0.0;
        for c in &e.children {
            let rr = max_inscribed(&c.region, &[], 0.0, 12)
                .map(|x| x.0.r)
                .unwrap_or(0.0);
            let wa = c.region.area();
            acc += wa * band_score(rr / a.inscribed.r.max(1e-300), 0.2, 0.65, 0.12);
            n += wa;
        }
        terms.push((if n > 0.0 { acc / n } else { 0.0 }, 0.7));
        let count = (e.children.len() + e.openings.len()) as f64;
        terms.push((exp(-(ln(count / 4.0)).abs() * 0.4), 0.4));
        terms.push((1.0, 2.0));
    }
    // préférence du style (poids de la règle × poids d'application, normalisé)
    terms.push((prior.clamp(0.0, 1.0), 1.0));
    // symétrie
    if let (Some(ax), true) = (a.sym_x, ctx.sym != SymMode::Free) {
        let parts: Vec<&Region> = e
            .children
            .iter()
            .map(|c| &c.region)
            .chain(e.openings.iter())
            .collect();
        let s = mirror_match(&parts, ax, 0.01 * a.bbox.diag());
        let wgt =
            ctx.def.score_weight("sym", 0.8) * if ctx.sym == SymMode::Strict { 2.0 } else { 1.0 };
        terms.push((s, wgt));
    }
    // proportions canoniques
    if let (Some(r), true) = (e.key_ratio, ctx.style.canon != Canon::Off) {
        let (_, d) = nearest_canon(r);
        terms.push((
            exp(-d / 0.15),
            if ctx.style.canon == Canon::Strict {
                1.0
            } else {
                0.4
            },
        ));
    }
    let (s, w) = terms
        .iter()
        .fold((0.0, 0.0), |acc, t| (acc.0 + t.0 * t.1, acc.1 + t.1));
    if w > 0.0 { s / w } else { 0.0 }
}

/// Raster binaire déterministe sur la boîte englobante.
pub struct Raster {
    pub nx: usize,
    pub ny: usize,
    pub origin: P2,
    pub step: f64,
    /// 0 = dehors, 1 = pierre, 2 = jour.
    pub cells: Vec<u8>,
}

impl Raster {
    pub fn new(shape: &Region, openings: &[Opening], n: usize) -> Raster {
        let bb = shape.bbox();
        let size = bb.size();
        let step = size.x.max(size.y) / n as f64;
        let nx = ((size.x / step).ceil() as usize).max(1);
        let ny = ((size.y / step).ceil() as usize).max(1);
        let origin = bb.min + P2::splat(step * 0.5);
        let obbs: Vec<BBox> = openings.iter().map(|o| o.region.bbox()).collect();
        let mut cells = vec![0u8; nx * ny];
        for j in 0..ny {
            for i in 0..nx {
                let p = origin + P2::new(i as f64, j as f64) * step;
                if !shape.contains(p) {
                    continue;
                }
                let mut v = 1u8;
                for (k, o) in openings.iter().enumerate() {
                    let b = &obbs[k];
                    if p.x >= b.min.x
                        && p.x <= b.max.x
                        && p.y >= b.min.y
                        && p.y <= b.max.y
                        && o.region.contains(p)
                    {
                        v = 2;
                        break;
                    }
                }
                cells[j * nx + i] = v;
            }
        }
        Raster {
            nx,
            ny,
            origin,
            step,
            cells,
        }
    }
    /// Transformée de distance euclidienne exacte (Felzenszwalb–Huttenlocher)
    /// des pixels de valeur `v` vers les autres ; distances en pixels.
    pub fn edt(&self, v: u8) -> Vec<f64> {
        edt(self.nx, self.ny, |i| self.cells[i] == v)
    }
}

/// Transformée de distance exacte (carrés des distances puis racine),
/// colonnes puis lignes, chacune en parallèle (résultat identique au calcul
/// séquentiel).
pub fn edt(nx: usize, ny: usize, inside: impl Fn(usize) -> bool + Sync) -> Vec<f64> {
    use rayon::prelude::*;
    const INF: f64 = 1e20;
    if nx == 0 || ny == 0 {
        return Vec::new();
    }
    // colonnes, rangées en mémoire colonne par colonne
    let mut cols = vec![0.0f64; nx * ny];
    cols.par_chunks_mut(ny).enumerate().for_each(|(i, col)| {
        let mut buf = vec![0.0; ny];
        for (j, b) in buf.iter_mut().enumerate() {
            *b = if inside(j * nx + i) { INF } else { 0.0 };
        }
        dt1d(&buf, col);
    });
    // lignes
    let mut f = vec![0.0f64; nx * ny];
    f.par_chunks_mut(nx).enumerate().for_each(|(j, row)| {
        let buf: Vec<f64> = (0..nx).map(|i| cols[i * ny + j]).collect();
        dt1d(&buf, row);
        for x in row.iter_mut() {
            *x = x.sqrt();
        }
    });
    f
}

fn dt1d(f: &[f64], d: &mut [f64]) {
    let n = f.len();
    let mut v = vec![0usize; n];
    let mut z = vec![0.0f64; n + 1];
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    for q in 1..n {
        // z[0] = −∞ garantit l'arrêt avant k = 0
        let s = loop {
            let p = v[k];
            let s =
                ((f[q] + (q * q) as f64) - (f[p] + (p * p) as f64)) / (2.0 * (q as f64 - p as f64));
            if s <= z[k] {
                k -= 1;
            } else {
                break s;
            }
        };
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, dq) in d.iter_mut().enumerate().take(n) {
        while z[k + 1] < q as f64 {
            k += 1;
        }
        let p = v[k];
        let dx = q as f64 - p as f64;
        *dq = dx * dx + f[p];
    }
}

/// Score global détaillé (coordonnées normalisées).
#[allow(clippy::too_many_arguments)]
pub fn global_score(
    shape: &Region,
    openings: &[Opening],
    tree: &[TreeNode],
    widths: &[f64],
    style: &StyleParams,
    def: &StyleDef,
    sym_x: Option<f64>,
) -> ScoreBreakdown {
    let mut terms: BTreeMap<String, f64> = BTreeMap::new();
    let area = shape.area().max(1e-300);
    let w0 = widths.first().copied().unwrap_or(1.0).max(1e-12);
    // densité
    let tau = openings.iter().map(|o| o.region.area()).sum::<f64>() / area;
    terms.insert("dens".into(), exp(-((tau - style.density) / 0.08).powi(2)));

    let ras = Raster::new(shape, openings, 160);
    let px = ras.step;
    // régularité des meneaux (crêtes de la distance dans la pierre)
    let d = ras.edt(1);
    let (nx, ny) = (ras.nx, ras.ny);
    let w_lo = widths.last().copied().unwrap_or(w0);
    let (mut ridge, mut good, mut stone, mut blob) = (0usize, 0usize, 0usize, 0usize);
    for j in 0..ny {
        for i in 0..nx {
            let k = j * nx + i;
            if ras.cells[k] != 1 {
                continue;
            }
            stone += 1;
            let dk = d[k] * px;
            if dk > 1.25 * w0 {
                blob += 1;
            }
            let mut is_ridge = d[k] >= 1.0;
            for (di, dj) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                let (ii, jj) = (i as i64 + di, j as i64 + dj);
                if ii >= 0
                    && jj >= 0
                    && (ii as usize) < nx
                    && (jj as usize) < ny
                    && d[jj as usize * nx + ii as usize] > d[k]
                {
                    is_ridge = false;
                }
            }
            if is_ridge {
                ridge += 1;
                let t = 2.0 * dk;
                if t >= 0.5 * w_lo && t <= 2.4 * w0 {
                    good += 1;
                }
            }
        }
    }
    let good_frac = if ridge > 0 {
        good as f64 / ridge as f64
    } else {
        0.0
    };
    let blob_frac = if stone > 0 {
        blob as f64 / stone as f64
    } else {
        0.0
    };
    terms.insert(
        "meneau".into(),
        good_frac * (1.0 - (3.0 * blob_frac).min(1.0)),
    );

    // grands jours non traités
    let mut big = 0.0;
    let mut tot = 0.0;
    for o in openings {
        let a = o.region.area();
        tot += a;
        let plain = matches!(o.rule.as_str(), "R19" | "fallback");
        if plain {
            let r = max_inscribed(&o.region, &[], 0.0, 10)
                .map(|x| x.0.r)
                .unwrap_or(0.0);
            if r > 4.5 * w0 {
                big += a;
            }
        }
    }
    terms.insert("vide".into(), if tot > 0.0 { 1.0 - big / tot } else { 0.0 });

    // quasi-tangences entre jours (pierre trop fine) : échantillonnage
    let w_min = widths.last().copied().unwrap_or(w0) * 0.5;
    let mut near = 0usize;
    let mut defects = 0usize;
    let obbs: Vec<BBox> = openings.iter().map(|o| o.region.bbox()).collect();
    for i in 0..openings.len() {
        for j in (i + 1)..openings.len() {
            if !obbs[i].overlaps(&obbs[j], 2.0 * w0) {
                continue;
            }
            near += 1;
            let ri = &openings[i].region;
            let rj = &openings[j].region;
            let step = ri.perimeter() / 64.0;
            let md = ri
                .outer
                .sample(step)
                .into_iter()
                .map(|p| rj.boundary_dist(p))
                .fold(f64::INFINITY, f64::min);
            if md < w_min {
                defects += 1;
            }
        }
    }
    terms.insert(
        "tan".into(),
        if near > 0 {
            1.0 - defects as f64 / near as f64
        } else {
            1.0
        },
    );

    // symétrie (IoU raster des jours avec leur image miroir)
    if let Some(ax) = sym_x {
        let mut inter = 0usize;
        let mut uni = 0usize;
        for j in 0..ny {
            for i in 0..nx {
                let p = ras.origin + P2::new(i as f64, j as f64) * px;
                let mx = 2.0 * ax - p.x;
                let mi = ((mx - ras.origin.x) / px).round();
                let a = ras.cells[j * nx + i] == 2;
                let b = if mi >= 0.0 && (mi as usize) < nx {
                    ras.cells[j * nx + mi as usize] == 2
                } else {
                    false
                };
                if a && b {
                    inter += 1;
                }
                if a || b {
                    uni += 1;
                }
            }
        }
        terms.insert(
            "sym".into(),
            if uni > 0 {
                inter as f64 / uni as f64
            } else {
                1.0
            },
        );
    }

    // équilibre : variance de la densité de pierre sur une grille 8 × 8
    let mut dens = Vec::new();
    for gj in 0..8 {
        for gi in 0..8 {
            let (i0, i1) = (gi * nx / 8, (gi + 1) * nx / 8);
            let (j0, j1) = (gj * ny / 8, (gj + 1) * ny / 8);
            let (mut ins, mut st) = (0usize, 0usize);
            for j in j0..j1 {
                for i in i0..i1 {
                    match ras.cells[j * nx + i] {
                        1 => {
                            ins += 1;
                            st += 1;
                        }
                        2 => ins += 1,
                        _ => {}
                    }
                }
            }
            let cell = (i1 - i0) * (j1 - j0);
            if cell > 0 && ins * 2 >= cell {
                dens.push(st as f64 / ins as f64);
            }
        }
    }
    let var = if dens.len() > 1 {
        let m = dens.iter().sum::<f64>() / dens.len() as f64;
        dens.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / dens.len() as f64
    } else {
        0.0
    };
    terms.insert("equil".into(), exp(-var / 0.03));

    // hiérarchie et cohérence (arbre)
    let mut hier = Vec::new();
    for n in tree {
        if let Some(parent) = tree.iter().find(|p| {
            n.path.len() > p.path.len()
                && n.path.starts_with(&p.path)
                && n.path[p.path.len()..].starts_with('/')
                && !n.path[p.path.len() + 1..].contains('/')
        }) && parent.inscribed > 0.0
            && parent.depth > 0
        {
            hier.push(band_score(n.inscribed / parent.inscribed, 0.2, 0.65, 0.15));
        }
    }
    terms.insert(
        "hier".into(),
        if hier.is_empty() {
            0.5
        } else {
            hier.iter().sum::<f64>() / hier.len() as f64
        },
    );
    let max_d = tree.iter().map(|n| n.depth).max().unwrap_or(0);
    let mut coh = Vec::new();
    for dpt in 1..=max_d {
        let mut rules: Vec<&str> = tree
            .iter()
            .filter(|n| n.depth == dpt && n.twin_of.is_none())
            .map(|n| n.rule.as_str())
            .collect();
        let cnt = rules.len();
        rules.sort();
        rules.dedup();
        if cnt > 1 {
            coh.push(1.0 - (rules.len() as f64 - 1.0) / (cnt as f64 - 1.0).max(1.0) * 0.8);
        }
    }
    terms.insert(
        "coher".into(),
        if coh.is_empty() {
            1.0
        } else {
            coh.iter().sum::<f64>() / coh.len() as f64
        },
    );
    // complexité
    let prims: usize = openings.iter().map(|o| o.region.segs().count()).sum();
    let target = 30.0 * style.depth as f64;
    terms.insert(
        "compl".into(),
        exp(-(ln((prims as f64).max(1.0) / target)).abs() * 0.7),
    );
    // proportions
    if style.canon != Canon::Off {
        let r: Vec<f64> = tree
            .iter()
            .filter_map(|n| n.ratio)
            .map(|x| exp(-nearest_canon(x).1 / 0.15))
            .collect();
        terms.insert(
            "prop".into(),
            if r.is_empty() {
                0.5
            } else {
                r.iter().sum::<f64>() / r.len() as f64
            },
        );
    }
    let defaults = [
        ("dens", 1.0),
        ("meneau", 1.5),
        ("vide", 1.0),
        ("tan", 1.0),
        ("sym", 0.8),
        ("hier", 0.7),
        ("prop", 0.5),
        ("equil", 0.6),
        ("compl", 0.4),
        ("coher", 0.4),
    ];
    let mut s = 0.0;
    let mut wsum = 0.0;
    for (k, d) in defaults {
        if let Some(v) = terms.get(k) {
            let w = def.score_weight(k, d);
            s += w * v;
            wsum += w;
        }
    }
    ScoreBreakdown {
        total: if wsum > 0.0 { s / wsum } else { 0.0 },
        terms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edt_line() {
        // 1 × 7 : pixels intérieurs au milieu
        let inside = [false, true, true, true, true, true, false];
        let d = edt(7, 1, |i| inside[i]);
        assert_eq!(d, vec![0.0, 1.0, 2.0, 3.0, 2.0, 1.0, 0.0]);
    }
}
