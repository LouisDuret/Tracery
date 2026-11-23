//! Porte gothique (ADR-23) : la forme sélectionnée (arc brisé, plein
//! cintre…) devient la baie d'une porte ; le décor est posé **à
//! l'extérieur** de la forme, surtout dans sa partie haute, comme sur les
//! portails flamboyants :
//!
//! - archivolte à moulures concentriques, gorges, bases et chapiteaux ;
//! - accolade : la moulure extérieure de l'archivolte quitte l'arc à
//!   l'angle φ et se retourne en contre-courbe (arc G1 tangent, construit
//!   au compas) jusqu'à une pointe sur l'axe ;
//! - crochets en feuille de chou frisé le long de l'accolade, fleuron
//!   (bouquet ou croix fleuronnée) à sa pointe ;
//! - pinacles latéraux : fût lisse ou à gorge, gâble ouvert sur une
//!   lancette, flèche pyramidale à crochets (lisse ou à tuiles), épi de
//!   faîtage en croix de feuilles ;
//! - encadrement : bandeau, corniche et panneau de lancettes aveugles
//!   (barres verticales, têtes brisées ou trilobées) ;
//! - dans la baie : linteau à coussinets et tympan (remplage aveugle, écu
//!   ou trilobe), baie pleine, ou tête trilobée redentée.
//!
//! Tout est construit à la règle et au compas (segments et arcs à centre
//! explicite) et entièrement déterminé par la seed.

use crate::design::{Design, Opening, OrnKind, Ornament, Stats, TreeNode};
use crate::genome::{GENERATOR_VERSION, Genome, Params};
use crate::rules::{unif, unif_in, weighted_index};
use crate::score::ScoreBreakdown;
use crate::seed::derived;
use crate::{CancelToken, GenError};
use geom::clip::{intersect_loop, offset_region, subtract, union, union_loop};
use geom::constr::biarc;
use geom::math::*;
use geom::{BBox, ConstructionEntity, ConstructionOp, Loop, Region, Seg, Xf};
use motifs::shapes::{foil_outline, teardrop};
use motifs::{ArchKind, arch_window};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;
use std::collections::BTreeMap;

/// Géométrie de la baie, lue sur la forme.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bay {
    pub(crate) x0: f64,
    pub(crate) x1: f64,
    /// Axe.
    pub(crate) xm: f64,
    /// Seuil.
    pub(crate) y0: f64,
    /// Ligne des naissances.
    pub(crate) ys: f64,
    /// Clé (sommet de l'intrados).
    pub(crate) ya: f64,
    /// Portée.
    pub(crate) s: f64,
    /// Arc de tête gauche (centre, rayon), issu du piédroit gauche.
    pub(crate) head: Option<(P2, f64)>,
}

impl Bay {
    pub(crate) fn read(shape: &Region) -> Bay {
        let bb = shape.bbox();
        let (x0, x1, y0, ya) = (bb.min.x, bb.max.x, bb.min.y, bb.max.y);
        let s = (x1 - x0).max(1e-9);
        let eps = 1e-6 * bb.diag();
        let mut best: Option<(f64, P2, f64)> = None;
        for sg in &shape.outer.segs {
            if let Seg::Arc { c, r, .. } = *sg {
                for (p, t) in [
                    (sg.start(), sg.tangent_start()),
                    (sg.end(), sg.tangent_end()),
                ] {
                    // naissance : sur le piédroit gauche, tangente verticale
                    if (p.x - x0).abs() < 10.0 * eps
                        && unit(t).x.abs() < 0.05
                        && p.y < ya - 1e-3 * s
                        && best.is_none_or(|b| p.y > b.0)
                    {
                        best = Some((p.y, c, r));
                    }
                }
            }
        }
        let (ys, head) = match best {
            Some((y, c, r)) => (y, Some((c, r))),
            None => (y0 + 0.62 * (ya - y0), None),
        };
        Bay {
            x0,
            x1,
            xm: 0.5 * (x0 + x1),
            y0,
            ys,
            ya,
            s,
            head,
        }
    }
    /// Rayon de l'arc brisé (ou plein cintre) de tête, s'il est régulier.
    fn pointed_radius(&self) -> Option<f64> {
        let (c, r) = self.head?;
        let ok = (c.y - self.ys).abs() < 1e-6 * self.s
            && (c.x - self.x0 - r).abs() < 1e-6 * self.s
            && r >= 0.5 * self.s - 1e-9 * self.s;
        ok.then_some(r)
    }
}

/// Rectangle (boucle trigonométrique).
pub(crate) fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop {
    let (xa, xb) = (x0.min(x1), x0.max(x1));
    let (ya, yb) = (y0.min(y1), y0.max(y1));
    Loop::polygon(&[
        P2::new(xa, ya),
        P2::new(xb, ya),
        P2::new(xb, yb),
        P2::new(xa, yb),
    ])
}

pub(crate) fn rect_r(x0: f64, y0: f64, x1: f64, y1: f64) -> Region {
    Region::simple(rect(x0, y0, x1, y1))
}

/// Dilatation (plus grande composante).
fn dilate(r: &Region, d: f64) -> Option<Region> {
    offset_region(r, -d)
        .into_iter()
        .max_by(|a, b| a.area().total_cmp(&b.area()))
}

/// Décalage d'un segment de `d` vers la gauche du sens de parcours.
fn offset_seg(s: &Seg, d: f64) -> Seg {
    match *s {
        Seg::Line { a, b } => {
            let n = perp(unit(b - a)) * d;
            Seg::line(a + n, b + n)
        }
        Seg::Arc {
            c,
            r,
            theta0,
            sweep,
        } => {
            let rr = if sweep > 0.0 { r - d } else { r + d };
            Seg::arc(c, rr, theta0, sweep)
        }
    }
}

/// Coupe le segment à sa première rencontre avec l'axe x = `xm`.
fn trim_to_axis(s: &Seg, xm: f64) -> Option<Seg> {
    match *s {
        Seg::Line { a, b } => {
            let dx = b.x - a.x;
            if dx.abs() < 1e-12 {
                return None;
            }
            let t = (xm - a.x) / dx;
            (t > 1e-9).then(|| Seg::line(a, a + (b - a) * t))
        }
        Seg::Arc {
            c,
            r,
            theta0,
            sweep,
        } => {
            let dx = xm - c.x;
            if dx.abs() > r || r <= 0.0 {
                return None;
            }
            let a = acos((dx / r).clamp(-1.0, 1.0));
            let mut best: Option<f64> = None;
            for ang in [a, -a] {
                let sw = if sweep > 0.0 {
                    ccw_delta(theta0, ang)
                } else {
                    -ccw_delta(ang, theta0)
                };
                if sw.abs() < 1e-9 || sw.abs() > 1.2 * PI {
                    continue;
                }
                if best.is_none_or(|b: f64| sw.abs() < b.abs()) {
                    best = Some(sw);
                }
            }
            best.map(|sw| Seg::arc(c, r, theta0, sw))
        }
    }
}

/// Bord décalé de `d` d'un demi-tracé gauche (du seuil à l'axe), coupé sur
/// l'axe.
fn offset_half(half: &[Seg], d: f64, xm: f64) -> Option<Vec<Seg>> {
    let mut v: Vec<Seg> = half.iter().map(|s| offset_seg(s, d)).collect();
    let last = v.pop()?;
    if let Seg::Arc { r, .. } = last
        && r <= 0.0
    {
        return None;
    }
    v.push(trim_to_axis(&last, xm)?);
    Some(v)
}

/// Région comprise entre deux demi-tracés gauches (bord extérieur `outer`
/// et intérieur `inner`) et leurs miroirs, fermée au seuil.
fn band_between(outer: &[Seg], inner: &[Seg], m: &Xf) -> Option<Region> {
    let mut segs: Vec<Seg> = outer.to_vec();
    segs.extend(outer.iter().rev().map(|s| s.transform(m).reversed()));
    let (bo, bi) = (outer.first()?.start(), inner.first()?.start());
    let (bo_r, bi_r) = (m.apply(bo), m.apply(bi));
    segs.push(Seg::line(bo_r, bi_r));
    segs.extend(inner.iter().map(|s| s.transform(m)));
    segs.extend(inner.iter().rev().map(|s| s.reversed()));
    segs.push(Seg::line(bi, bo));
    let l = Loop::new(segs).oriented(true);
    (!geom::clip::self_intersects(&l)).then(|| Region::simple(l))
}

/// Région enclose par un demi-tracé et son miroir, fermée au seuil.
fn inside_of(half: &[Seg], m: &Xf) -> Option<Region> {
    let mut segs: Vec<Seg> = half.to_vec();
    segs.extend(half.iter().rev().map(|s| s.transform(m).reversed()));
    let (a, b) = (half.first()?.start(), m.apply(half.first()?.start()));
    segs.push(Seg::line(b, a));
    let l = Loop::new(segs).oriented(true);
    (!geom::clip::self_intersects(&l)).then(|| Region::simple(l))
}

/// Point et tangente à l'abscisse curviligne `u` d'une suite de segments.
fn point_on(segs: &[Seg], u: f64) -> (P2, P2) {
    let mut u = u.max(0.0);
    for (i, s) in segs.iter().enumerate() {
        let l = s.length();
        if u <= l || i + 1 == segs.len() {
            let t = if l > 0.0 {
                (u / l).clamp(0.0, 1.0)
            } else {
                0.0
            };
            return (s.point_at(t), unit(s.tangent_at(t)));
        }
        u -= l;
    }
    (P2::ZERO, P2::new(0.0, 1.0))
}

fn length_of(segs: &[Seg]) -> f64 {
    segs.iter().map(|s| s.length()).sum()
}

/// Feuille de chou frisé : grappe de lobes qui pousse selon `n` depuis
/// `base` et s'enroule vers `t`.
pub(crate) fn chou(base: P2, n: P2, t: P2, l: f64) -> Vec<Region> {
    let n = unit(n);
    let mut t = t - n * dot(t, n);
    t = if norm(t) < 1e-9 { perp(n) } else { unit(t) };
    const LOBES: [(f64, f64, f64); 6] = [
        (0.16, 0.0, 0.19),
        (0.44, 0.08, 0.24),
        (0.70, 0.30, 0.19),
        (0.50, -0.22, 0.15),
        (0.84, 0.54, 0.13),
        (0.30, 0.24, 0.14),
    ];
    let mut res: Vec<Region> = Vec::new();
    for (k, (u, v, r)) in LOBES.iter().enumerate() {
        let c = base + n * (u * l) + t * (v * l);
        let lp = Loop::circle(c, r * l);
        res = if k == 0 {
            vec![Region::simple(lp)]
        } else {
            union_loop(&res, &lp)
        };
    }
    res
}

/// Bourgeon terminal (larme pointant vers le haut).
fn bud(base: P2, r: f64, h: f64) -> Vec<Region> {
    let c = base + P2::new(0.0, r);
    teardrop(c, r, base + P2::new(0.0, h.max(2.2 * r)))
        .map(|l| vec![Region::simple(l.oriented(true))])
        .unwrap_or_else(|| vec![Region::circle(c, r)])
}

/// Écu (blason) : bord supérieur droit, flancs verticaux, pointe en bas.
pub(crate) fn shield(c: P2, w: f64, h: f64) -> Region {
    let (a, top, mid, bot) = (w / 2.0, c.y + h / 2.0, c.y + 0.05 * h, c.y - h / 2.0);
    let l = Loop::new(vec![
        Seg::line(P2::new(-a + c.x, top), P2::new(-a + c.x, mid)),
        Seg::from_sweep(P2::new(-a + c.x, mid), P2::new(c.x, bot), 0.9),
        Seg::from_sweep(P2::new(c.x, bot), P2::new(a + c.x, mid), 0.9),
        Seg::line(P2::new(a + c.x, mid), P2::new(a + c.x, top)),
        Seg::line(P2::new(a + c.x, top), P2::new(-a + c.x, top)),
    ]);
    Region::simple(l.oriented(true))
}

/// Remplage aveugle : les jours d'une génération ordinaire dans `region`
/// deviennent des creux. Un remplage d'un seul tenant (jour unique) ne se
/// lit pas : on essaie quelques seeds dérivées, sinon on renonce.
fn blind_tracery(region: &Region, genome: &Genome, salt: u64, cancel: &CancelToken) -> Vec<Region> {
    let mut g = genome.clone();
    g.style.door = false;
    g.style.depth = genome.style.depth.clamp(2, 3);
    g.style.candidates = genome.style.candidates.clamp(1, 4);
    g.style.frame = true;
    g.style.w0 = 0.02;
    g.overrides.clear();
    for k in 0..4u64 {
        g.seed = derived(genome.seed, salt.wrapping_add(k * 0x9E37));
        if let Ok(d) = crate::generate(region, &g, cancel)
            && d.openings.len() >= 3
        {
            return d.openings.into_iter().map(|o| o.region).collect();
        }
    }
    Vec::new()
}

/// Accumulateur des ornements, jours et nœuds.
pub(crate) struct Out {
    pub(crate) ornaments: Vec<Ornament>,
    pub(crate) openings: Vec<Opening>,
    pub(crate) construction: Vec<ConstructionEntity>,
    pub(crate) nodes: Vec<(String, String)>,
    pub(crate) min_area: f64,
}

impl Out {
    pub(crate) fn node(&mut self, name: &str) -> String {
        let p = format!("0/{}", self.nodes.len());
        self.nodes.push((p.clone(), name.to_string()));
        p
    }
    pub(crate) fn add(
        &mut self,
        path: &str,
        kind: OrnKind,
        level: u8,
        regs: impl IntoIterator<Item = Region>,
    ) {
        for r in regs {
            if r.area() > self.min_area && r.is_finite() {
                self.ornaments.push(Ornament {
                    region: r,
                    kind,
                    level,
                    path: path.to_string(),
                });
            }
        }
    }
    pub(crate) fn relief(&mut self, path: &str, level: u8, regs: impl IntoIterator<Item = Region>) {
        self.add(path, OrnKind::Relief, level, regs);
    }
    pub(crate) fn creux(&mut self, path: &str, level: u8, regs: impl IntoIterator<Item = Region>) {
        self.add(path, OrnKind::Creux, level, regs);
    }
}

/// Demi-accolade gauche (axe de la moulure), du seuil à la pointe sur
/// l'axe : piédroit, arc concentrique à l'arc de tête jusqu'à φ, puis
/// contre-courbe tangente en φ (son centre est sur le rayon prolongé), de
/// rayon `k2` fois le rayon minimal qui atteint l'axe, jusqu'à la pointe T.
/// La contre-courbe tourne en sens trigonométrique : φ est pris près de la
/// clé, là où la moulure est presque horizontale, pour que la pointe monte. Sans arc de tête
/// lisible : biarc depuis la naissance jusqu'à T (hauteur `y_t`, tangente
/// d'arrivée `tip_angle`). Renvoie le tracé et l'ordonnée de T.
fn accolade_half(
    bay: &Bay,
    e: f64,
    f: f64,
    k2: f64,
    y_t: f64,
    tip_angle: f64,
) -> Option<(Vec<Seg>, f64)> {
    let ps = P2::new(bay.x0 - e, bay.ys);
    let mut segs = Vec::new();
    if bay.ys > bay.y0 + 1e-9 * bay.s {
        segs.push(Seg::line(P2::new(bay.x0 - e, bay.y0), ps));
    }
    match bay.head {
        Some((c, r)) => {
            let rr = r + e;
            let th_s = angle_of(ps - c);
            let th_a = angle_of(P2::new(bay.xm, bay.ya) - c);
            let span = ccw_delta(th_a, th_s);
            if !(0.05..PI).contains(&span) {
                return None;
            }
            let phi = th_s - f * span;
            let a1 = Seg::arc(c, rr, th_s, -f * span);
            // rayon minimal : le cercle touche l'axe (c₂.x + r₂ = xm)
            let cp = cos(phi);
            let r2_min = (bay.xm - c.x - cp * rr) / (1.0 + cp).max(1e-6);
            if r2_min <= 0.0 {
                return None;
            }
            let r2 = k2 * r2_min;
            let c2 = c + dir(phi) * (rr + r2);
            let a2 = trim_to_axis(&Seg::arc(c2, r2, phi + PI, PI), bay.xm)?;
            // contre-courbe : ouverture raisonnable, pointe au-dessus de
            // l'archivolte
            let tip = a2.end();
            if a2.sweep() <= 0.05 || a2.sweep() > 0.75 * PI || tip.y < bay.ya + e + 0.12 * bay.s {
                return None;
            }
            segs.push(a1);
            segs.push(a2);
            Some((segs, tip.y))
        }
        None => {
            let t = P2::new(bay.xm, y_t);
            let bi = biarc(ps, P2::new(0.0, 1.0), t, dir(tip_angle));
            if bi.len() != 2 || bi[0].sweep() >= 0.0 || bi[1].sweep() <= 0.0 {
                return None;
            }
            segs.extend(bi);
            Some((segs, y_t))
        }
    }
}

/// Épi de faîtage en croix de feuilles : tige, nœud, deux feuilles
/// latérales et bourgeon terminal, posé sur la pointe `apex`, de hauteur `h`.
fn cross_finial(apex: P2, h: f64) -> Vec<(OrnKind, u8, Region)> {
    let ws = 0.11 * h;
    let x = apex.x;
    let mut v = vec![
        (
            OrnKind::Relief,
            3,
            rect_r(
                x - ws / 2.0,
                apex.y - 0.25 * h,
                x + ws / 2.0,
                apex.y + 0.8 * h,
            ),
        ),
        (
            OrnKind::Relief,
            3,
            rect_r(x - 1.7 * ws, apex.y, x + 1.7 * ws, apex.y + 0.12 * h),
        ),
    ];
    for sx in [-1.0, 1.0] {
        let base = P2::new(x + sx * 0.4 * ws, apex.y + 0.42 * h);
        for r in chou(base, P2::new(sx, 0.25), P2::new(0.0, 1.0), 0.42 * h) {
            v.push((OrnKind::Relief, 3, r));
        }
    }
    for r in bud(P2::new(x, apex.y + 0.66 * h), 0.13 * h, 0.34 * h) {
        v.push((OrnKind::Relief, 3, r));
    }
    v
}

/// Chevron (Λ) d'un gâble d'axe `xc`, de base `y_b` et de demi-largeur `hw`,
/// de pointe `apex_y` : bande entre les parallèles intérieures aux rampants
/// à `d0` et `d1`, coupée horizontalement en `y_cut` (pas de base).
fn chevron(xc: f64, y_b: f64, hw: f64, apex_y: f64, d0: f64, d1: f64, y_cut: f64) -> Loop {
    let b = P2::new(xc - hw, y_b);
    let t = unit(P2::new(xc, apex_y) - b);
    let n = P2::new(t.y, -t.x); // normale intérieure du rampant gauche
    let at_y = |d: f64| b + n * d + t * ((y_cut - b.y - n.y * d) / t.y);
    let at_x = |d: f64| b + n * d + t * ((xc - b.x - n.x * d) / t.x);
    let mirror = |p: P2| P2::new(2.0 * xc - p.x, p.y);
    let (l0, l1) = (at_y(d0), at_y(d1));
    Loop::polygon(&[l0, l1, at_x(d1), mirror(l1), mirror(l0), at_x(d0)])
}

/// Pinacle (vue de face), d'axe `xc` et de fût de largeur `pw`, du dessus de
/// sa base `y_base` (sol `y_floor`) à la naissance de ses gâbles `y_pt` :
/// - fût lisse ou à gorge ;
/// - gâble droit (rampants rectilignes, moulure en V) sur une petite
///   lancette, parfois trilobée, dont la tête monte sous la pointe ;
/// - flèche pyramidale à côtés droits, haute et effilée, lisse (arête
///   médiane) ou couverte de tuiles en écailles, avec des crochets en
///   chou frisé régulièrement espacés sur ses arêtes ;
/// - épi de faîtage en croix de feuilles.
fn pinnacle(
    rng: &mut ChaCha8Rng,
    xc: f64,
    pw: f64,
    y_floor: f64,
    y_base: f64,
    y_pt: f64,
) -> Vec<(OrnKind, u8, Region)> {
    let mut v: Vec<(OrnKind, u8, Region)> = Vec::new();
    let x_out = xc - 0.5 * pw;
    let x_in = xc + 0.5 * pw;
    let mp = Xf::mirror_x(xc);

    // gâble : rampants droits, légèrement débordants, sans base (ouvert)
    let tg = 0.14 * pw; // épaisseur de la moulure
    let wg = pw * unif_in(rng, 1.04, 1.16);
    let ln = pw * unif_in(rng, 0.42, 0.55);
    let rho = if unif(rng) < 0.5 { 1.0 } else { 1.2 };
    let head = ln * (rho - 0.25f64).sqrt();
    let hg = (pw * unif_in(rng, 1.25, 1.8)).max(1.6 * head + 3.0 * tg);
    let apex_g = P2::new(xc, y_pt + hg);
    let tri = Region::simple(Loop::polygon(&[
        P2::new(xc - wg / 2.0, y_pt),
        P2::new(xc + wg / 2.0, y_pt),
        apex_g,
    ]));

    // lancette, parfois trilobée : sa tête monte aussi haut que possible
    // sous la pointe du gâble sans toucher la moulure (ni sa gorge), son
    // corps descend dans le haut du fût
    let y_bot = y_pt - pw * unif_in(rng, 0.5, 1.0);
    let (ln, y_ls) = fit_lancet(xc, y_pt, wg / 2.0, hg, tg + 0.13 * pw, ln, rho);
    let lb = (y_ls - y_bot).max(0.4 * pw); // corps sous la naissance
    let trefoil = unif(rng) < 0.5;
    let x = Xf::translate(P2::new(xc, y_ls));
    let mut lancet: Vec<Region> = Vec::new();
    if trefoil
        && let Some((r, _)) = motifs::cusp::trefoil_lancet(ln, rho * ln, -lb, 0.25, 0.12 * ln)
    {
        lancet = r.iter().map(|q| q.transform(&x)).collect();
    }
    if lancet.is_empty() {
        let (l, _) = arch_window(ArchKind::Pointed { rho }, ln, lb);
        lancet = vec![Region::simple(l.transform(&x))];
    }

    // flèche : pyramide à côtés droits, qui naît derrière le gâble
    let hw = 0.5 * pw * unif_in(rng, 0.85, 0.98);
    let hs = pw * unif_in(rng, 3.5, 6.0);
    let y_s0 = y_pt;
    let apex = P2::new(xc, y_s0 + hs);
    let spire = Loop::polygon(&[P2::new(xc - hw, y_s0), P2::new(xc + hw, y_s0), apex]);

    // épi de faîtage : sa tige prolonge la flèche
    let mut finial = cross_finial(apex - P2::new(0.0, 0.35 * pw), pw * unif_in(rng, 1.1, 1.5));
    let stem = finial.remove(0).2;

    // silhouette d'un seul tenant (fût, face du gâble, flèche, tige de
    // l'épi) : ni la base du gâble ni celle de la flèche ne se dessinent,
    // et la flèche se fond dans la tige
    let body = union(&[rect_r(x_out, y_floor, x_in, y_pt + 0.02 * pw)], &tri);
    let spire_low = Loop::polygon(&[
        P2::new(xc - hw, y_s0 - 0.05 * pw),
        P2::new(xc + hw, y_s0 - 0.05 * pw),
        apex,
    ]);
    let body = union_loop(&body, &spire_low);
    for r in union(&body, &stem) {
        v.push((OrnKind::Relief, 2, r));
    }
    // rampants en saillie : chevron ouvert, coupé à la naissance ; gorge
    // le long de leur bord intérieur (moulure en V)
    v.push((
        OrnKind::Relief,
        3,
        Region::simple(chevron(xc, y_pt, wg / 2.0, apex_g.y, 0.0, tg, y_pt)),
    ));
    v.push((
        OrnKind::Creux,
        1,
        Region::simple(chevron(
            xc,
            y_pt,
            wg / 2.0,
            apex_g.y,
            tg,
            tg + 0.06 * pw,
            y_pt + 0.5 * tg,
        )),
    ));
    for r in lancet {
        v.push((OrnKind::Creux, 2, r));
    }
    // fleuron du gâble et crochet sur chaque rampant
    for r in bud(apex_g - P2::new(0.0, 0.05 * pw), 0.09 * pw, 0.32 * pw) {
        v.push((OrnKind::Relief, 3, r));
    }
    if unif(rng) < 0.6 {
        let bl = P2::new(xc - wg / 2.0, y_pt);
        let t = unit(apex_g - bl);
        let n = perp(t);
        let p = bl + (apex_g - bl) * 0.55;
        for r in chou(p - n * (0.04 * pw), n + t * 0.6, t, 0.32 * pw) {
            v.push((OrnKind::Relief, 3, r.transform(&mp)));
            v.push((OrnKind::Relief, 3, r));
        }
    }

    // fût : lisse ou creusé d'une gorge, sous la lancette
    let top = y_ls - lb - 0.3 * pw;
    if unif(rng) < 0.5 && top > y_base + pw {
        let gw = 0.16 * pw;
        v.push((
            OrnKind::Creux,
            1,
            rect_r(xc - gw / 2.0, y_base + 0.3 * pw, xc + gw / 2.0, top),
        ));
    }

    // flèche : tuiles ou arête médiane
    if unif(rng) < 0.4 {
        // tuiles en écailles : rangs horizontaux, joints décalés
        let gw = 0.035 * pw;
        let row = pw * unif_in(rng, 0.28, 0.36);
        let col = 0.3 * pw;
        let clip = offset_region(&Region::simple(spire.clone()), 0.06 * pw);
        let mut k = 1;
        let mut y = y_s0 + hg + 0.5 * row;
        while y < apex.y - 1.2 * row {
            let mut grooves = vec![rect_r(xc - pw, y - gw / 2.0, xc + pw, y + gw / 2.0)];
            let off = if k % 2 == 0 { 0.5 * col } else { 0.0 };
            let mut xj = xc - pw + off;
            while xj < xc + pw {
                grooves.push(rect_r(xj - gw / 2.0, y, xj + gw / 2.0, y + row));
                xj += col;
            }
            for gr in grooves {
                for c in &clip {
                    for r in intersect_loop(std::slice::from_ref(&gr), &c.outer) {
                        v.push((OrnKind::Creux, 1, r));
                    }
                }
            }
            y += row;
            k += 1;
        }
    } else {
        // arête médiane de la pyramide
        let gw = 0.03 * pw;
        v.push((
            OrnKind::Creux,
            1,
            rect_r(xc - gw / 2.0, y_s0 + hg, xc + gw / 2.0, apex.y - 0.6 * pw),
        ));
    }
    // crochets par paires, régulièrement espacés sur les arêtes
    let bl = P2::new(xc - hw, y_s0);
    let t = unit(apex - bl);
    let n = perp(t);
    let len = dist(bl, apex);
    let step = pw * unif_in(rng, 0.65, 0.95);
    let lc = 0.5 * pw;
    let mut u = (hg + 0.25 * pw) / t.y.max(0.2);
    while u < len - 1.3 * pw {
        let k = 1.0 - 0.35 * u / len;
        let p = bl + t * u;
        for r in chou(p - n * (0.05 * pw), n + t * 0.55, t, k * lc) {
            v.push((OrnKind::Relief, 3, r.transform(&mp)));
            v.push((OrnKind::Relief, 3, r));
        }
        u += step;
    }
    // épi de faîtage : nœud, feuilles et bourgeon devant la tige
    v.extend(finial);
    v
}

/// Lancette sous un gâble d'axe `xc`, de naissance `y_pt`, de demi-largeur
/// `hw` et de hauteur `hg` : renvoie la portée (réduite au besoin) et la
/// naissance la plus haute pour lesquelles la tête reste à `dm` au moins des
/// rampants.
fn fit_lancet(xc: f64, y_pt: f64, hw: f64, hg: f64, dm: f64, ln: f64, rho: f64) -> (f64, f64) {
    // gâble prolongé sous sa naissance : seuls les rampants comptent
    let ext = hg;
    let k = hw / hg;
    let gab = Region::simple(Loop::polygon(&[
        P2::new(xc - hw - k * ext, y_pt - ext),
        P2::new(xc + hw + k * ext, y_pt - ext),
        P2::new(xc, y_pt + hg),
    ]));
    let mut ln = ln;
    let mut best = (ln, y_pt);
    for _ in 0..5 {
        let head = ln * (rho - 0.25f64).sqrt();
        let (l, _) = arch_window(ArchKind::Pointed { rho }, ln, 0.01 * ln);
        let pts: Vec<P2> = l.segs.iter().flat_map(|s| s.sample(24)).collect();
        let fits = |y: f64| {
            pts.iter()
                .map(|p| P2::new(xc + p.x, y + p.y))
                .filter(|p| p.y >= y_pt)
                .all(|p| gab.sdf(p) >= dm)
        };
        // la tête entièrement sous la naissance tient toujours
        let (mut lo, mut hi) = (y_pt - head - 1e-3 * ln, y_pt + hg - head);
        if fits(hi) {
            lo = hi;
        } else {
            for _ in 0..30 {
                let m = 0.5 * (lo + hi);
                if fits(m) { lo = m } else { hi = m }
            }
        }
        best = (ln, lo);
        // tête assez haute dans le gâble : on garde cette portée
        if lo + head >= y_pt + 0.45 * hg {
            break;
        }
        ln *= 0.85;
    }
    best
}

/// Génère une porte décorée.
pub fn generate_door(
    shape: &Region,
    genome: &Genome,
    cancel: &CancelToken,
) -> Result<Design, GenError> {
    #[cfg(not(target_arch = "wasm32"))]
    let t0 = std::time::Instant::now();
    let shape = Region::new(shape.outer.clone(), shape.holes.clone());
    let bay = Bay::read(&shape);
    let s = bay.s;
    let (x0, x1, xm, y0, ys, ya) = (bay.x0, bay.x1, bay.xm, bay.y0, bay.ys, bay.ya);
    let m = Xf::mirror_x(xm);
    let mut rng = ChaCha8Rng::seed_from_u64(derived(genome.seed, 0xD00E));
    let mut out = Out {
        ornaments: Vec::new(),
        openings: Vec::new(),
        construction: Vec::new(),
        nodes: Vec::new(),
        min_area: 1e-7 * s * s,
    };
    let cancelled = || {
        if cancel.is_cancelled() {
            Err(GenError::Cancelled)
        } else {
            Ok(())
        }
    };

    // ------------------------------------------------------------ mesures
    let a = s * unif_in(&mut rng, 0.10, 0.15); // archivolte
    let b = s * unif_in(&mut rng, 0.05, 0.075); // moulure d'accolade
    let e = a + 0.5 * b; // axe de l'accolade, depuis l'intrados
    let g = 0.2 * a / 2.5; // gorge
    let above = rect(x0 - 50.0 * s, y0, x1 + 50.0 * s, ya + 50.0 * s);

    // baie prolongée sous le seuil (anneaux de moulure coupés net au sol)
    let ext = {
        let top = y0 + (0.3 * (ys - y0)).max(0.05 * s);
        let r = rect_r(x0 + 1e-4 * s, y0 - 4.0 * a, x1 - 1e-4 * s, top);
        union(std::slice::from_ref(&shape), &r)
            .into_iter()
            .max_by(|p, q| p.area().total_cmp(&q.area()))
            .unwrap_or_else(|| shape.clone())
    };
    let ring = |d1: f64, d2: f64| -> Vec<Region> {
        let Some(o) = dilate(&ext, d2) else {
            return Vec::new();
        };
        let res = if d1 <= 0.0 {
            subtract(&[o], &ext)
        } else {
            match dilate(&ext, d1) {
                Some(i) => subtract(&[o], &i),
                None => vec![o],
            }
        };
        intersect_loop(&res, &above)
    };

    // ------------------------------------------------------------ accolade
    let mut acc: Option<(Vec<Seg>, f64)> = None;
    // d'abord concentrique à l'arc de tête, sinon biarc depuis la naissance
    let bare = Bay { head: None, ..bay };
    for i in 0..40 {
        let bay = if i < 24 { bay } else { bare };
        let f = unif_in(&mut rng, 0.72, 0.95);
        let k2 = unif_in(&mut rng, 1.15, 2.6);
        let y_t = ya + a + s * unif_in(&mut rng, 0.25, 0.55);
        let tip = unif_in(&mut rng, 0.30, 0.42) * PI;
        if let Some((h, yt)) = accolade_half(&bay, e, f, k2, y_t, tip)
            && offset_half(&h, 0.5 * b, xm).is_some()
            && offset_half(&h, -0.5 * b, xm).is_some()
        {
            acc = Some((h, yt));
            break;
        }
    }
    let Some((centre, y_t)) = acc else {
        return Err(GenError::InvalidShape(
            "porte : accolade impossible sur cette forme".into(),
        ));
    };
    let outer = offset_half(&centre, 0.5 * b, xm).unwrap_or_default();
    let inner = offset_half(&centre, -0.5 * b, xm).unwrap_or_default();
    let tip_out = outer.last().map(|s| s.end()).unwrap_or(P2::new(xm, y_t));
    let u_out = inside_of(&outer, &m);
    let u_in = inside_of(&inner, &m);
    for sg in &centre {
        if let Seg::Arc { c, r, .. } = *sg {
            out.construction
                .push(ConstructionEntity::point(c, ConstructionOp::ArchCenter));
            out.construction.push(ConstructionEntity::point(
                m.apply(c),
                ConstructionOp::ArchCenter,
            ));
            out.construction.push(ConstructionEntity::Circle {
                c,
                r,
                op: ConstructionOp::Tangent,
            });
        }
    }
    if centre.len() >= 2 {
        let j = centre[centre.len() - 2].end();
        out.construction
            .push(ConstructionEntity::point(j, ConstructionOp::Tangent));
        out.construction.push(ConstructionEntity::point(
            m.apply(j),
            ConstructionOp::Tangent,
        ));
    }
    out.construction.push(ConstructionEntity::point(
        P2::new(xm, y_t),
        ConstructionOp::Tangent,
    ));

    // ------------------------------------------------------------ composition
    let has_frame = unif(&mut rng) < 0.65;
    let has_pinnacles = has_frame || unif(&mut rng) < 0.6;
    let fh = s * unif_in(&mut rng, 0.30, 0.55); // fleuron
    let ch = s * unif_in(&mut rng, 0.05, 0.08); // corniche
    let y_c = y_t + fh * unif_in(&mut rng, 0.8, 1.05);
    // pinacles enchâssés dans les piédroits : le fût mord sur la moulure
    // extérieure (accolade), qui passe derrière lui
    let ov = b * unif_in(&mut rng, 0.6, 1.2);
    let pw = s * unif_in(&mut rng, 0.12, 0.16);
    let x_in = x0 - e - 0.5 * b + ov; // bord intérieur du pinacle gauche
    let x_out = x_in - pw;
    let x_side = if has_pinnacles {
        x_out
    } else {
        x0 - e - 0.5 * b
    };

    // ------------------------------------------------------------ baie
    let p_bay = out.node("Baie");
    let p_arch = out.node("Archivolte");
    let p_acc = out.node("Accolade");
    let p_croc = out.node("Crochets");
    let p_fin = out.node("Fleuron");
    let p_tym = out.node("Tympan");
    let p_spa = out.node("Écoinçon");
    // plan de l'embrasure : la forme est de la pierre, la baie s'y découpe
    out.relief(&p_bay, 1, [shape.clone()]);
    let variant = weighted_index(&mut rng, &[0.3, 0.5, 0.2]).unwrap_or(1);
    let mut opening: Option<Region> = None;
    let mut tympan: Option<Region> = None;
    if variant == 2
        && let Some(r) = bay.pointed_radius()
    {
        let sh = unif_in(&mut rng, 0.2, 0.28);
        let dl = s * unif_in(&mut rng, 0.08, 0.14);
        if let Some((regs, cons)) = motifs::cusp::trefoil_lancet(s, r, -(ys - y0), sh, dl) {
            let x = Xf::translate(P2::new(xm, ys));
            let regs: Vec<Region> = regs.iter().map(|q| q.transform(&x)).collect();
            if regs.len() == 1 {
                opening = Some(regs[0].clone());
                out.construction
                    .extend(cons.iter().map(|c| c.transform(&x)));
            }
        }
    }
    if opening.is_none() && variant >= 1 && bay.head.is_some() && ys - y0 > 0.4 * s {
        // linteau à coussinets
        let y_l = (ys - s * unif_in(&mut rng, 0.0, 0.12)).max(y0 + 0.5 * (ys - y0));
        let lh = s * unif_in(&mut rng, 0.06, 0.09);
        let rc = if unif(&mut rng) < 0.7 {
            s * unif_in(&mut rng, 0.05, 0.09)
        } else {
            0.0
        };
        let l = if rc > 0.0 {
            Loop::new(vec![
                Seg::line(P2::new(x0, y0), P2::new(x1, y0)),
                Seg::line(P2::new(x1, y0), P2::new(x1, y_l - rc)),
                Seg::arc(P2::new(x1, y_l), rc, -FRAC_PI_2, -FRAC_PI_2),
                Seg::line(P2::new(x1 - rc, y_l), P2::new(x0 + rc, y_l)),
                Seg::arc(P2::new(x0, y_l), rc, 0.0, -FRAC_PI_2),
                Seg::line(P2::new(x0, y_l - rc), P2::new(x0, y0)),
            ])
        } else {
            rect(x0, y0, x1, y_l)
        };
        // la baie reste dans la forme, quelle qu'elle soit
        opening = intersect_loop(std::slice::from_ref(&shape), &l.oriented(true))
            .into_iter()
            .max_by(|p, q| p.area().total_cmp(&q.area()));
        let lintel = intersect_loop(
            std::slice::from_ref(&shape),
            &rect(x0 - s, y_l, x1 + s, y_l + lh),
        );
        out.relief(&p_tym, 2, lintel);
        if rc > 0.0 {
            for (cx, a0) in [(x1, PI), (x0, -FRAC_PI_2)] {
                let c = P2::new(cx, y_l);
                let arc = Seg::arc(c, rc, a0, FRAC_PI_2);
                let cor = Loop::new(vec![
                    arc,
                    Seg::line(arc.end(), c),
                    Seg::line(c, arc.start()),
                ]);
                out.relief(&p_tym, 2, [Region::simple(cor.oriented(true))]);
            }
        }
        tympan = intersect_loop(
            std::slice::from_ref(&shape),
            &rect(x0 - s, y_l + lh, x1 + s, ya + s),
        )
        .into_iter()
        .max_by(|p, q| p.area().total_cmp(&q.area()));
    }
    // la baie est toujours découpée dans la forme (redents, linteau)
    let opening = opening
        .and_then(|o| {
            intersect_loop(&[o], &shape.outer)
                .into_iter()
                .max_by(|p, q| p.area().total_cmp(&q.area()))
        })
        .unwrap_or_else(|| shape.clone());
    out.openings.push(Opening {
        region: opening,
        depth: 1,
        path: p_bay.clone(),
        rule: "PORTE".into(),
    });
    cancelled()?;

    // tympan : remplage aveugle, écu ou trilobe
    if let Some(ty) = &tympan {
        match weighted_index(&mut rng, &[0.45, 0.3, 0.25]).unwrap_or(0) {
            0 => {
                let v = blind_tracery(ty, genome, 0x7E4, cancel);
                if v.is_empty() {
                    if let Some((c, _)) = geom::sdf::max_inscribed(ty, &[], 0.0, 20) {
                        let lo = foil_outline(c.c, 0.62 * c.r, 3, 1.15);
                        out.creux(&p_tym, 2, [Region::simple(lo.oriented(true))]);
                    }
                } else {
                    out.creux(&p_tym, 2, v);
                }
            }
            1 => {
                if let Some((c, _)) = geom::sdf::max_inscribed(ty, &[], 0.0, 20) {
                    let w = 1.1 * c.r;
                    out.relief(&p_tym, 3, [shield(c.c, w, 1.3 * w)]);
                    let l = 0.9 * c.r;
                    for sx in [-1.0, 1.0] {
                        let base = c.c + P2::new(sx * 0.6 * w, -0.1 * w);
                        out.relief(
                            &p_tym,
                            3,
                            chou(base, P2::new(sx, 0.5), P2::new(0.0, 1.0), l),
                        );
                    }
                }
            }
            _ => {
                if let Some((c, _)) = geom::sdf::max_inscribed(ty, &[], 0.0, 20) {
                    let lo = foil_outline(c.c, 0.62 * c.r, 3, 1.15);
                    out.creux(&p_tym, 2, [Region::simple(lo.oriented(true))]);
                }
            }
        }
    }
    cancelled()?;

    // ------------------------------------------------------------ archivolte
    let n_m = if unif(&mut rng) < 0.5 { 2 } else { 3 };
    for k in 0..n_m {
        let (d1, d2) = (a * k as f64 / n_m as f64, a * (k + 1) as f64 / n_m as f64);
        let v = ring(d1, d2);
        out.relief(&p_arch, 2, v);
    }
    for k in 1..=n_m {
        let d = a * k as f64 / n_m as f64;
        let v = ring(d - 0.5 * g, d + 0.5 * g);
        out.creux(&p_arch, 1, v);
    }
    // bases et chapiteaux
    let hb = s * unif_in(&mut rng, 0.10, 0.18);
    for sx in [-1.0, 1.0] {
        let tr = |q: Region| if sx > 0.0 { q.transform(&m) } else { q };
        let xa = x_side - 0.15 * pw;
        out.relief(&p_arch, 3, [tr(rect_r(xa, y0, x0, y0 + 0.62 * hb))]);
        out.relief(&p_arch, 3, [tr(rect_r(xa + 0.03 * s, y0, x0, y0 + hb))]);
    }
    if bay.head.is_some() && ys - y0 > 0.3 * s && unif(&mut rng) < 0.65 {
        let hc = s * 0.04;
        for sx in [-1.0, 1.0] {
            let q = rect_r(x0 - e - 0.5 * b - 0.012 * s, ys - hc, x0, ys);
            out.relief(&p_arch, 3, [if sx > 0.0 { q.transform(&m) } else { q }]);
        }
    }
    cancelled()?;

    // ------------------------------------------------------------ accolade
    if let Some(band) = band_between(&outer, &inner, &m) {
        out.relief(&p_acc, 2, [band]);
    }
    if let (Some(go), Some(gi)) = (
        offset_half(&centre, 0.5 * g, xm),
        offset_half(&centre, -0.5 * g, xm),
    ) && let Some(groove) = band_between(&go, &gi, &m)
    {
        out.creux(&p_acc, 1, [groove]);
    }

    // écoinçon : entre l'archivolte et l'accolade
    let spandrel: Vec<Region> = match (&u_in, dilate(&ext, a + 1e-3 * s)) {
        (Some(ui), Some(dx)) => subtract(std::slice::from_ref(ui), &dx)
            .into_iter()
            .filter(|r| r.area() > 2e-3 * s * s)
            .collect(),
        _ => Vec::new(),
    };
    if let Some(sp) = spandrel.iter().max_by(|p, q| p.area().total_cmp(&q.area())) {
        match weighted_index(&mut rng, &[0.35, 0.35, 0.3]).unwrap_or(0) {
            0 => {
                let v = blind_tracery(sp, genome, 0x5BA, cancel);
                if v.is_empty() {
                    if let Some((c, _)) = geom::sdf::max_inscribed(sp, &[], 0.0, 20) {
                        let lo = foil_outline(c.c, 0.6 * c.r, 4, 1.15);
                        out.creux(&p_spa, 2, [Region::simple(lo.oriented(true))]);
                    }
                } else {
                    out.creux(&p_spa, 2, v);
                }
            }
            1 => {
                if let Some((c, _)) = geom::sdf::max_inscribed(sp, &[], 0.0, 20) {
                    let n = if unif(&mut rng) < 0.5 { 3 } else { 4 };
                    let lo = foil_outline(c.c, 0.6 * c.r, n, 1.15);
                    out.creux(&p_spa, 2, [Region::simple(lo.oriented(true))]);
                }
            }
            _ => {
                if let Some((c, _)) = geom::sdf::max_inscribed(sp, &[], 0.0, 20) {
                    let l = 1.5 * c.r;
                    out.relief(
                        &p_spa,
                        3,
                        chou(
                            c.c - P2::new(0.0, 0.45 * l),
                            P2::new(0.0, 1.0),
                            P2::new(1.0, 0.0),
                            l,
                        ),
                    );
                }
            }
        }
    }
    cancelled()?;

    // ------------------------------------------------------------ crochets
    let lo_len = length_of(&outer);
    let a1_len = outer
        .len()
        .checked_sub(2)
        .map(|i| outer[i].length())
        .unwrap_or(0.0);
    let jamb_len = if outer.len() >= 3 {
        outer[0].length()
    } else {
        0.0
    };
    let u_stop = jamb_len + 0.6 * a1_len;
    let dc = s * unif_in(&mut rng, 0.13, 0.19);
    let lc = b * unif_in(&mut rng, 1.8, 2.4);
    let mut k = 0;
    loop {
        let u = lo_len - (k as f64 + 0.7) * dc;
        if u < u_stop || k > 40 {
            break;
        }
        let (p, t) = point_on(&outer, u);
        let n_out = perp(t);
        let base = p - n_out * (0.3 * b);
        let leaf = chou(base, n_out + t * 0.35, t, lc);
        let mirrored: Vec<Region> = leaf.iter().map(|q| q.transform(&m)).collect();
        out.relief(&p_croc, 3, leaf);
        out.relief(&p_croc, 3, mirrored);
        k += 1;
    }

    // ------------------------------------------------------------ fleuron
    let sw = 0.7 * b;
    let top = y_t + fh;
    out.relief(
        &p_fin,
        3,
        [rect_r(
            xm - sw / 2.0,
            y_t - 0.5 * b,
            xm + sw / 2.0,
            top - 0.25 * fh,
        )],
    );
    out.relief(
        &p_fin,
        3,
        [rect_r(
            xm - 0.9 * b,
            tip_out.y - 0.1 * b,
            xm + 0.9 * b,
            tip_out.y + 0.45 * b,
        )],
    );
    let lf = b * unif_in(&mut rng, 1.6, 2.2);
    if unif(&mut rng) < 0.5 {
        // bouquet : deux étages de feuilles et un bourgeon
        for (yf, kf) in [(0.30, 1.0), (0.58, 0.75)] {
            for sx in [-1.0, 1.0] {
                let base = P2::new(xm + sx * 0.3 * sw, y_t + yf * fh);
                out.relief(
                    &p_fin,
                    3,
                    chou(base, P2::new(sx, 0.7), P2::new(0.0, 1.0), kf * lf),
                );
            }
        }
    } else {
        // croix fleuronnée
        let yy = y_t + 0.62 * fh;
        let arm = 0.3 * fh;
        out.relief(
            &p_fin,
            3,
            [rect_r(xm - arm, yy - sw / 2.0, xm + arm, yy + sw / 2.0)],
        );
        for sx in [-1.0, 1.0] {
            let base = P2::new(xm + sx * arm, yy);
            out.relief(
                &p_fin,
                3,
                chou(base, P2::new(sx, 0.3), P2::new(0.0, 1.0), 0.75 * lf),
            );
            let base = P2::new(xm + sx * 0.3 * sw, y_t + 0.26 * fh);
            out.relief(
                &p_fin,
                3,
                chou(base, P2::new(sx, 0.6), P2::new(0.0, 1.0), 0.8 * lf),
            );
        }
    }
    let rb = 0.42 * lf;
    out.relief(&p_fin, 3, bud(P2::new(xm, top - 2.6 * rb), rb, 2.6 * rb));
    out.construction.push(ConstructionEntity::Line {
        a: P2::new(xm, y0),
        b: P2::new(xm, top),
        op: ConstructionOp::Axis,
    });
    cancelled()?;

    // zone réservée devant le panneau : accolade, fleuron, crochets
    let keep_out: Vec<Region> = {
        let mut v: Vec<Region> = u_out
            .iter()
            .filter_map(|u| dilate(u, lc + 0.02 * s))
            .collect();
        v.push(rect_r(xm - 1.2 * lf, y_t, xm + 1.2 * lf, top + 0.05 * s));
        v
    };

    // ------------------------------------------------------------ encadrement
    if has_frame {
        let p_fr = out.node("Encadrement");
        let p_lan = out.node("Lancettes aveugles");
        let x_r = m.apply(P2::new(x_out, 0.0)).x;
        out.relief(&p_fr, 1, [rect_r(x_out, y0, x_r, y_c + ch)]);
        let cm = 0.03 * s;
        // la corniche s'arrête contre les fûts des pinacles, qui passent devant
        let (xc0, xc1) = if has_pinnacles {
            (x_in, m.apply(P2::new(x_in, 0.0)).x)
        } else {
            (x_out - cm, x_r + cm)
        };
        out.relief(&p_fr, 2, [rect_r(xc0, y_c, xc1, y_c + ch)]);
        out.creux(
            &p_fr,
            1,
            [rect_r(
                xc0,
                y_c + 0.3 * ch - 0.5 * g,
                xc1,
                y_c + 0.3 * ch + 0.5 * g,
            )],
        );
        // bandeau, interrompu par l'accolade
        let y_b = ys + unif_in(&mut rng, 0.55, 0.85) * (y_t - ys);
        let bh = s * unif_in(&mut rng, 0.035, 0.055);
        let xi0 = x_in;
        let xi1 = m.apply(P2::new(x_in, 0.0)).x;
        let mut band = vec![rect_r(xi0, y_b, xi1, y_b + bh)];
        if let Some(u) = u_out.as_ref().and_then(|u| dilate(u, 0.6 * g)) {
            band = subtract(&band, &u);
        }
        out.relief(&p_fr, 2, band);
        // panneau de lancettes aveugles
        let (yp0, yp1) = (y_b + bh + 0.02 * s, y_c - 0.02 * s);
        let wp = xi1 - xi0;
        if unif(&mut rng) < 0.8 && yp1 - yp0 > 0.25 * s {
            let n = (round(wp / (s * unif_in(&mut rng, 0.13, 0.2))) as usize).max(3);
            let bar = s * unif_in(&mut rng, 0.025, 0.04);
            let ls = (wp - (n + 1) as f64 * bar) / n as f64;
            let rho = if unif(&mut rng) < 0.5 { 1.0 } else { 1.25 };
            let hh = ls * (rho - 0.25f64).sqrt();
            let trefoil = unif(&mut rng) < 0.5;
            if ls > 0.03 * s && yp1 - yp0 > hh + 0.3 * ls {
                let body = yp1 - yp0 - hh;
                for i in 0..n {
                    let xc = xi0 + bar + 0.5 * ls + i as f64 * (ls + bar);
                    let x = Xf::translate(P2::new(xc, yp1 - hh));
                    let mut regs: Vec<Region> = Vec::new();
                    if trefoil
                        && let Some((r, _)) =
                            motifs::cusp::trefoil_lancet(ls, rho * ls, -body, 0.25, 0.12 * ls)
                    {
                        regs = r.iter().map(|q| q.transform(&x)).collect();
                    }
                    if regs.is_empty() {
                        let (l, _) = arch_window(ArchKind::Pointed { rho }, ls, body);
                        regs = vec![Region::simple(l.transform(&x))];
                    }
                    for ko in &keep_out {
                        regs = subtract(&regs, ko);
                    }
                    out.creux(&p_lan, 2, regs);
                }
            }
        }
    }
    cancelled()?;

    // ------------------------------------------------------------ pinacles
    if has_pinnacles {
        let p_pin = out.node("Pinacles");
        // naissance des gâbles (haut du fût)
        let y_pt = if has_frame {
            y_c + ch + s * unif_in(&mut rng, 0.0, 0.10)
        } else {
            ys + unif_in(&mut rng, 0.6, 1.0) * (y_t - ys)
        };
        let xc = 0.5 * (x_in + x_out);
        let left = pinnacle(&mut rng, xc, pw, y0, y0 + hb, y_pt);
        // le fût passe devant ce qu'il recouvre (moulures, accolade,
        // crochets, encadrement) : on le retranche, sauf des socles, sur
        // lesquels il repose
        let cutters: Vec<Region> = left
            .iter()
            .filter(|(k, lv, _)| *k == OrnKind::Relief && *lv == 2)
            .flat_map(|(_, _, r)| [r.clone(), r.transform(&m)])
            .collect();
        let old = std::mem::take(&mut out.ornaments);
        for o in old {
            let bb = o.region.bbox();
            let plinth = o.path == p_arch && o.level == 3 && bb.max.y <= y0 + hb + 1e-9 * s;
            if plinth || !cutters.iter().any(|c| c.bbox().overlaps(&bb, 0.0)) {
                out.ornaments.push(o);
                continue;
            }
            let a0 = o.region.area();
            let mut regs = vec![o.region.clone()];
            for c in &cutters {
                regs = subtract(&regs, c);
            }
            // une sculpture presque entièrement cachée disparaît
            let keep: Vec<Region> = regs
                .into_iter()
                .filter(|r| o.level < 3 || r.area() > 0.5 * a0)
                .collect();
            out.add(&o.path, o.kind, o.level, keep);
        }
        for (kind, lv, r) in left {
            let rr = r.transform(&m);
            out.add(&p_pin, kind, lv, [r, rr]);
        }
    }
    cancelled()?;

    // ------------------------------------------------------------ arbre
    let root_bb = out
        .ornaments
        .iter()
        .fold(shape.bbox(), |bb, o| bb.union(&o.region.bbox()));
    let mut tree = vec![TreeNode {
        path: "0".into(),
        depth: 0,
        rule: "Porte".into(),
        params: Params::new()
            .with("archivolte", a)
            .with("accolade", b)
            .with("pointe", y_t),
        types: vec!["porte".into()],
        area: shape.area(),
        inscribed: 0.0,
        local_score: 1.0,
        twin_of: None,
        bbox: root_bb,
        locked: false,
        ratio: Some((y_t - ys) / s),
        candidates: 1,
        region: Some(shape.clone()),
    }];
    for (p, name) in &out.nodes {
        let mine: Vec<&Ornament> = out.ornaments.iter().filter(|o| &o.path == p).collect();
        let bb = mine
            .iter()
            .fold(BBox::EMPTY, |bb, o| bb.union(&o.region.bbox()));
        let bb = if *p == p_bay { shape.bbox() } else { bb };
        if bb.is_empty() {
            continue;
        }
        tree.push(TreeNode {
            path: p.clone(),
            depth: 1,
            rule: name.clone(),
            params: Params::new(),
            types: vec![],
            area: mine.iter().map(|o| o.region.area()).sum(),
            inscribed: 0.0,
            local_score: 1.0,
            twin_of: None,
            bbox: bb,
            locked: false,
            ratio: None,
            candidates: 1,
            region: None,
        });
    }

    let mut terms = BTreeMap::new();
    terms.insert("porte".to_string(), 1.0);
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
        widths: vec![a, b],
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
    use crate::{Family, StyleParams};

    fn door_genome(seed: u64) -> Genome {
        let mut st = StyleParams::for_family(Family::Flamboyant);
        st.door = true;
        Genome::new(seed, st)
    }

    fn pointed(rho: f64) -> Region {
        let (l, _) = arch_window(ArchKind::Pointed { rho }, 1000.0, 1400.0);
        Region::simple(l)
    }

    #[test]
    fn door_is_decorated_outside() {
        for (i, shape) in [pointed(1.0), pointed(0.75), pointed(1.3)]
            .iter()
            .enumerate()
        {
            for seed in 0..12 {
                let d = generate_door(shape, &door_genome(seed), &CancelToken::new())
                    .unwrap_or_else(|e| panic!("forme {i}, seed {seed} : {e}"));
                assert!(d.door);
                assert_eq!(d.openings.len(), 1);
                assert!(crate::validate::validate(&d, 0.0, 0.0).is_empty());
                // le décor déborde largement de la forme, surtout en haut
                let bb = d.bbox();
                assert!(bb.max.y > shape.bbox().max.y + 200.0, "seed {seed}");
                assert!(d.ornaments.iter().any(|o| o.kind == OrnKind::Creux));
            }
        }
    }

    #[test]
    fn door_deterministic() {
        let a = generate_door(&pointed(1.0), &door_genome(5), &CancelToken::new()).unwrap();
        let b = generate_door(&pointed(1.0), &door_genome(5), &CancelToken::new()).unwrap();
        assert_eq!(
            serde_json::to_string(&a.ornaments).unwrap(),
            serde_json::to_string(&b.ornaments).unwrap()
        );
    }

    #[test]
    fn door_on_round_and_free_shapes() {
        let (l, _) = arch_window(ArchKind::Round, 900.0, 1200.0);
        for shape in [Region::simple(l), Region::circle(P2::ZERO, 500.0)] {
            for seed in 0..6 {
                let _ = generate_door(&shape, &door_genome(seed), &CancelToken::new());
            }
        }
    }
}
