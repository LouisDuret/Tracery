//! Ferronnerie (ADR-26) : la forme sélectionnée (rectangle, arc) devient une
//! porte de bois à un ou deux vantaux, selon ses proportions, ornée de
//! ferrures gothiques :
//!
//! - vantaux de planches verticales (joints en creux), battement au milieu
//!   d'une porte à deux vantaux ;
//! - pentures : bandes de fer horizontales partant des gonds, terminées en
//!   fourche à deux volutes ou en fleur de lis, d'où naissent des crosses
//!   (volutes) au-dessus et au-dessous ;
//! - ou bien une tige verticale à rinceaux (paires de crosses feuillues)
//!   entre deux pentures ;
//! - fleurons dans l'œil des volutes, clous le long des bandes, anneau sur
//!   platine en rosace et entrée de serrure.
//!
//! Tout est construit au compas : une volute est un grand arc suivi de
//! quarts de cercle dont le rayon décroît d'un rapport constant, chaque
//! centre étant pris sur la normale commune au point de raccord (G1).

use crate::design::{Design, OrnKind, Ornament, Stats, TreeNode};
use crate::door::{Bay, Out, rect_r, shield};
use crate::genome::{GENERATOR_VERSION, Genome, Params};
use crate::rules::{arc_band, line_band, unif, unif_in};
use crate::score::ScoreBreakdown;
use crate::seed::derived;
use crate::{CancelToken, GenError};
use geom::clip::{offset_region, union, union_loop};
use geom::math::*;
use geom::{BBox, ConstructionEntity, ConstructionOp, Loop, Region, Seg, Xf};
use motifs::shapes::foil_outline;
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{Rng, SeedableRng};
use std::collections::BTreeMap;

/// Volute au compas : depuis `p`, tangente `t`, un grand arc de rayon `r0`
/// et d'ouverture `sweep0`, puis `nq` quarts de cercle de rayon décroissant
/// (rapport `q`). `turn` > 0 : enroulement à gauche du sens de marche.
fn volute(p: P2, t: P2, r0: f64, sweep0: f64, nq: usize, q: f64, turn: f64) -> Vec<Seg> {
    let t = unit(t);
    let s = if turn >= 0.0 { 1.0 } else { -1.0 };
    let n = perp(t) * s;
    let mut c = p + n * r0;
    let mut r = r0;
    let mut theta = angle(p - c);
    let mut segs = Vec::new();
    for k in 0..=nq {
        let sw = if k == 0 { sweep0 } else { FRAC_PI_2 } * s;
        let sg = Seg::arc(c, r, theta, sw);
        let pe = sg.end();
        segs.push(sg);
        theta += sw;
        let r2 = r * q;
        c = pe + (c - pe) * (r2 / r);
        r = r2;
    }
    segs
}

fn angle(v: P2) -> f64 {
    v.y.atan2(v.x)
}

/// Bande de fer autour d'un tracé, d'épaisseur décroissante (`w0`, puis
/// `taper` à chaque élément, sans descendre sous `w_min`).
fn band(path: &[Seg], w0: f64, taper: f64, w_min: f64) -> Vec<Region> {
    let mut res: Vec<Region> = Vec::new();
    let mut w = w0;
    for sg in path {
        let l = match *sg {
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => arc_band(c, r, theta0, sweep, w.min(1.6 * r), 0.15 * w),
            Seg::Line { a, b } => line_band(a, b, w, 0.15 * w),
        };
        res = if res.is_empty() {
            vec![Region::simple(l)]
        } else {
            union_loop(&res, &l)
        };
        w = (w * taper).max(w_min);
    }
    res
}

/// Feuille en navette (deux arcs) de `base` à `tip`.
fn navette(base: P2, tip: P2, bulge: f64) -> Region {
    let l = Loop::new(vec![
        Seg::from_sweep(base, tip, bulge),
        Seg::from_sweep(tip, base, bulge),
    ]);
    Region::simple(l.oriented(true))
}

/// Rosace (fleuron à `n` lobes) de rayon extérieur ≈ `r`.
fn rosette(c: P2, r: f64, n: usize) -> Region {
    Region::simple(foil_outline(c, 0.62 * r, n, 1.15).oriented(true))
}

/// Disque.
fn disc(c: P2, r: f64) -> Region {
    Region::circle(c, r)
}

/// Encombrement d'un élément déjà posé (cercle).
#[derive(Clone, Copy)]
struct Disk {
    c: P2,
    r: f64,
}

/// Vantail : bord des gonds `a`, bord libre `b` (abscisses), région
/// (intérieur, marge comprise) et sens (+1 si les gonds sont à gauche).
struct Leaf {
    a: f64,
    b: f64,
    dir: f64,
    /// Vantail entier, et vantail rétréci de la marge.
    reg: Region,
    inner: Region,
    /// Motif reporté : isométries du repère local vers le vantail (chaque
    /// copie doit tenir) et cadre local (x0, y0, x1, y1).
    views: Vec<Xf>,
    frame: Option<(f64, f64, f64, f64)>,
}

impl Leaf {
    fn width(&self) -> f64 {
        (self.b - self.a).abs()
    }
    /// Le disque tient-il dans le vantail, pour chacune de ses copies ?
    fn holds(&self, d: Disk) -> bool {
        if let Some((x0, y0, x1, y1)) = self.frame
            && (d.c.x - d.r < x0 || d.c.x + d.r > x1 || d.c.y - d.r < y0 || d.c.y + d.r > y1)
        {
            return false;
        }
        if self.views.is_empty() {
            return self.inner.sdf(d.c) >= d.r;
        }
        self.views
            .iter()
            .all(|x| self.inner.sdf(x.apply(d.c)) >= d.r)
    }
}

/// Ornements d'un vantail : (nature, niveau, nœud, région).
type Orns = Vec<(OrnKind, u8, &'static str, Region)>;

/// Fleur de lis au bout d'une bande (point `e`, direction `d`, largeur `w`).
fn fleur_de_lis(e: P2, d: P2, w: f64) -> (Vec<Seg>, Vec<Region>) {
    let d = unit(d);
    let n = perp(d);
    let mut paths = Vec::new();
    for s in [1.0, -1.0] {
        // pétales latéraux : volutes qui s'écartent puis se recourbent
        let t = unit(d + n * (1.4 * s));
        paths.extend(volute(e, t, 1.1 * w, 0.95 * PI, 1, 0.6, -s));
    }
    let petal = navette(e - d * (0.3 * w), e + d * (3.2 * w), 1.0);
    let collar = Region::simple(line_band(
        e - d * (0.4 * w),
        e + d * (0.2 * w),
        2.2 * w,
        0.0,
    ));
    (paths, vec![petal, collar])
}

/// Forge d'un vantail : bandes de fer (fusionnées), ornements posés,
/// encombrements et crosses déjà placées (pour y greffer des rameaux).
struct Forge<'a> {
    rng: ChaCha8Rng,
    leaf: &'a Leaf,
    taken: Vec<Disk>,
    iron: Vec<Region>,
    orns: Orns,
    /// Crosses posées : premier arc, indice de l'encombrement, largeur.
    crooks: Vec<(Seg, usize, f64)>,
    cons: Vec<ConstructionEntity>,
    /// Largeur minimale d'un rameau (fin de la récursion).
    w_floor: f64,
}

impl Forge<'_> {
    fn add_path(&mut self, path: &[Seg], w0: f64, taper: f64) {
        for r in band(path, w0, taper, 0.45 * w0) {
            self.iron = if self.iron.is_empty() {
                vec![r]
            } else {
                union(&self.iron, &r)
            };
        }
    }
    fn orn(&mut self, kind: OrnKind, lv: u8, node: &'static str, r: Region) {
        self.orns.push((kind, lv, node, r));
    }
    /// Place libre pour le disque `d` (en ignorant l'encombrement `skip`).
    fn free(&self, d: Disk, skip: Option<usize>) -> bool {
        self.leaf.holds(d)
            && self
                .taken
                .iter()
                .enumerate()
                .all(|(i, o)| Some(i) == skip || dist(o.c, d.c) >= 0.97 * (o.r + d.r))
    }
    /// Pose une crosse (voir [`crook`]) ; renvoie son indice.
    #[allow(clippy::too_many_arguments)]
    fn crook(
        &mut self,
        p: P2,
        t: P2,
        side: P2,
        r_max: f64,
        w: f64,
        skip: Option<usize>,
        eye: bool,
    ) -> Option<usize> {
        let t = unit(t);
        let turn = if dot(perp(t), side) > 0.0 { 1.0 } else { -1.0 };
        // le départ mord à peine dans la pièce mère : un bord exactement
        // tangent au sien ferait échouer l'union, qui perdrait la crosse
        let p = p - perp(t) * (turn * 0.12 * w);
        let sweep0 = unif_in(&mut self.rng, 1.05, 1.35) * PI;
        let q = unif_in(&mut self.rng, 0.58, 0.66);
        let nq0 = 2 + (unif(&mut self.rng) < 0.5) as usize;
        let flower = unif(&mut self.rng);
        let mut r = r_max;
        for _ in 0..14 {
            if r < 1.6 * w {
                return None;
            }
            let c = p + perp(t) * (turn * r);
            let d = Disk { c, r: r + 0.5 * w };
            if !self.free(d, skip) {
                r *= 0.87;
                continue;
            }
            // fer fin : l'enroulement se resserre d'un quart de plus tant
            // que le dernier arc reste nettement plus large que le fer
            let mut nq = nq0;
            while nq < 4 && r * q.powi(nq as i32 + 1) > 1.6 * w {
                nq += 1;
            }
            let path = volute(p, t, r, sweep0, nq, q, turn);
            // largeur constante : pas de ressaut d'un arc à l'autre
            self.add_path(&path, w, 1.0);
            let last = *path.last().unwrap_or(&path[0]);
            let end = last.end();
            let eye_c = match last {
                Seg::Arc { c, .. } => c,
                _ => c,
            };
            if eye && r > 3.0 * w && nq <= 3 {
                if flower < 0.55 {
                    // rosace dans l'œil de la volute
                    let rr = (0.4 * r).max(1.2 * w);
                    let cc = c + (end - c) * 0.3;
                    self.orn(OrnKind::Relief, 2, "Fleurons", rosette(cc, rr, 5));
                    self.orn(OrnKind::Relief, 3, "Clous", disc(cc, 0.22 * rr));
                } else {
                    // feuille en navette dans le creux de la crosse
                    let base = eye_c + (end - eye_c) * 0.2;
                    let tip = c + (c - p) * 0.4;
                    if dist(base, tip) > 2.0 * w {
                        self.orn(OrnKind::Relief, 2, "Fleurons", navette(base, tip, 0.9));
                    }
                    self.orn(OrnKind::Relief, 3, "Clous", disc(end, 0.5 * w));
                }
            } else {
                // bouton au bout de la volute
                self.orn(OrnKind::Relief, 3, "Clous", disc(end, 0.7 * w));
            }
            self.cons
                .push(ConstructionEntity::point(c, ConstructionOp::ArchCenter));
            self.taken.push(d);
            self.crooks.push((path[0], self.taken.len() - 1, w));
            return Some(self.crooks.len() - 1);
        }
        None
    }
    /// Rameaux greffés sur le grand arc des crosses, récursivement : chaque
    /// crosse posée depuis l'indice `from` porte, à des places fixes de son
    /// grand arc, des contre-volutes (en S) tournées vers l'extérieur, qui en
    /// portent à leur tour, sur `depth` générations au plus. Le rayon et la
    /// largeur décroissent d'une génération à l'autre ; la récursion s'arrête
    /// d'elle-même quand la place ou la largeur manque.
    fn grow(&mut self, from: usize, depth: usize) {
        // (fraction du grand arc, rapport du rayon) : le rameau du milieu est
        // le plus grand, les deux autres l'encadrent
        const SPROUTS: [(f64, f64); 3] = [(0.24, 0.36), (0.55, 0.5), (0.86, 0.34)];
        let mut from = from;
        for level in 0..depth {
            let to = self.crooks.len();
            if from == to {
                break;
            }
            for k in from..to {
                let (arc, di, w) = self.crooks[k];
                let Seg::Arc { c, r, .. } = arc else { continue };
                let ws = (0.8 * w).max(self.w_floor);
                for (f, kr) in SPROUTS {
                    let p = arc.point_at(f);
                    let t = arc.tangent_at(f);
                    let out = unit(p - c);
                    // départ tangent : le bord intérieur du rameau épouse
                    // le bord extérieur de la crosse mère (même tangente)
                    let p0 = c + out * (r + 0.5 * w - 0.5 * ws);
                    let eye = level == 0 && kr > 0.4;
                    if self.crook(p0, t, out, kr * r, ws, Some(di), eye).is_some() {
                        continue;
                    }
                    // pas de place pour une volute : une feuille
                    let len = kr * r;
                    let dirl = unit(out * 1.0 + t * 0.6);
                    let tip = p0 + dirl * len;
                    let d = Disk {
                        c: p0 + dirl * (0.5 * len),
                        r: 0.5 * len,
                    };
                    if len > 4.0 * ws && self.free(d, Some(di)) {
                        self.orn(OrnKind::Relief, 2, "Fleurons", navette(p0, tip, 0.95));
                        self.taken.push(d);
                    }
                }
            }
            from = to;
        }
    }
}

/// Symétrie par rapport à la droite horizontale y = y0.
fn mirror_h(y0: f64) -> Xf {
    Xf {
        m: [1.0, 0.0, 0.0, -1.0],
        t: P2::new(0.0, 2.0 * y0),
        scale: 1.0,
        angle: 0.0,
        mirror: true,
    }
}

/// Motif forgé dans un repère local, puis reporté par des isométries.
struct Motif {
    iron: Vec<Region>,
    orns: Orns,
    disks: Vec<Disk>,
    cons: Vec<P2>,
}

impl Motif {
    /// Forge `build` dans le repère local : chaque élément doit tenir dans
    /// le cadre `frame` et, reporté par chacune des isométries `views`,
    /// dans le vantail (encombrements déjà pris : `taken`, en local).
    fn forge(
        rng: &mut ChaCha8Rng,
        leaf: &Leaf,
        views: Vec<Xf>,
        frame: (f64, f64, f64, f64),
        taken: Vec<Disk>,
        w_floor: f64,
        build: impl FnOnce(&mut Forge),
    ) -> Motif {
        let local = Leaf {
            a: leaf.a,
            b: leaf.b,
            dir: leaf.dir,
            reg: leaf.reg.clone(),
            inner: leaf.inner.clone(),
            views,
            frame: Some(frame),
        };
        let n0 = taken.len();
        let mut f = Forge {
            rng: ChaCha8Rng::seed_from_u64(rng.next_u64()),
            leaf: &local,
            taken,
            iron: Vec::new(),
            orns: Vec::new(),
            crooks: Vec::new(),
            cons: Vec::new(),
            w_floor,
        };
        build(&mut f);
        let cons = f
            .cons
            .iter()
            .filter_map(|c| match c {
                ConstructionEntity::Point { p, .. } => Some(*p),
                _ => None,
            })
            .collect();
        Motif {
            iron: f.iron,
            orns: f.orns,
            disks: f.taken[n0..].to_vec(),
            cons,
        }
    }
    /// Copies du motif par les isométries `xs`.
    fn place(
        &self,
        xs: &[Xf],
        iron: &mut Vec<Region>,
        orns: &mut Orns,
        disks: &mut Vec<Disk>,
        cons: &mut Vec<ConstructionEntity>,
    ) {
        for x in xs {
            iron.extend(self.iron.iter().map(|r| r.transform(x)));
            for (k, lv, n, r) in &self.orns {
                orns.push((*k, *lv, *n, r.transform(x)));
            }
            disks.extend(self.disks.iter().map(|d| Disk {
                c: x.apply(d.c),
                r: d.r,
            }));
            cons.extend(
                self.cons
                    .iter()
                    .map(|c| ConstructionEntity::point(x.apply(*c), ConstructionOp::ArchCenter)),
            );
        }
    }
}

/// Ferrures d'un vantail (gonds du côté `leaf.a`). Chaque penture porte le
/// même motif, symétrique par rapport à son axe : il est forgé une fois
/// pour la moitié haute, dans la place commune à toutes les pentures, puis
/// reporté en miroir et sur chaque penture.
#[allow(clippy::too_many_arguments)]
fn leaf_ironwork(
    mut rng: ChaCha8Rng,
    leaf: &Leaf,
    straps_y: &[f64],
    t: f64,
    hm: f64,
    pattern: usize,
    mut taken: Vec<Disk>,
    head_stem: bool,
    cons: &mut Vec<ConstructionEntity>,
) -> Orns {
    let lw = leaf.width();
    let d = leaf.dir;
    let mut iron: Vec<Region> = Vec::new();
    let mut orns: Orns = Vec::new();
    let fork = unif(&mut rng) < 0.6;
    // bord des gonds commun à toutes les pentures (piédroit courbe)
    let inside = |x: f64, y: f64| {
        [-0.75, 0.0, 0.75]
            .iter()
            .all(|k| leaf.reg.sdf(P2::new(x, y + k * t)) > 1e-6 * lw)
    };
    let mut xh = leaf.a;
    while !straps_y.iter().all(|&y| inside(xh, y)) && (xh - leaf.a).abs() < 0.5 * lw {
        xh += d * 0.01 * lw;
    }
    let straps: Vec<f64> = straps_y
        .iter()
        .copied()
        .filter(|&y| inside(xh, y))
        .collect();
    if straps.is_empty() {
        return orns;
    }
    // une fleur de lis demande de la place avant le bord libre
    let room_end = if fork { lw - 1.0 * t } else { lw - 4.5 * t };
    let len = (lw * unif_in(&mut rng, 0.74, 0.88)).min(room_end) - (xh - leaf.a).abs();
    let x_end = xh + d * len;
    // demi-hauteur disponible : la moitié de l'écart entre pentures
    let mut sorted = straps.clone();
    sorted.sort_by(f64::total_cmp);
    let half = sorted
        .windows(2)
        .map(|w| 0.5 * (w[1] - w[0]))
        .fold(hm + 0.55 * t, f64::min)
        - 0.25 * t;
    // repère de la penture : moitié haute ; chaque copie (en haut, en
    // miroir en bas, sur chaque penture) doit tenir dans le vantail
    let frame = (
        leaf.a.min(leaf.b) - lw,
        -0.55 * t,
        leaf.a.max(leaf.b) + lw,
        half,
    );
    let mut views: Vec<Xf> = Vec::new();
    for &y in &straps {
        views.push(Xf::translate(P2::new(0.0, y)));
        views.push(Xf {
            t: P2::new(0.0, y),
            ..mirror_h(0.0)
        });
    }
    // encombrements déjà pris (anneau, serrure), vus depuis chaque penture
    let mut local_taken: Vec<Disk> = Vec::new();
    for &y in &straps {
        for o in &taken {
            let c = P2::new(o.c.x, o.c.y - y);
            if c.y.abs() < half + o.r {
                local_taken.push(Disk { c, r: o.r });
                local_taken.push(Disk {
                    c: P2::new(c.x, -c.y),
                    r: o.r,
                });
            }
        }
    }
    if fork {
        // l'autre volute de la fourche est le miroir de celle-ci
    } else {
        local_taken.push(Disk {
            c: P2::new(x_end + d * 1.4 * t, 0.0),
            r: 2.2 * t,
        });
    }
    // tige verticale au milieu du vantail (rinceaux entre pentures, ou tige
    // à fleur de lis de la tête) : sa place est réservée dans le motif des
    // pentures, qu'aucune crosse ne la traverse
    let xs = leaf.a + d * 0.5 * lw;
    let ws = 0.36 * t;
    if head_stem || (pattern == 1 && straps.len() >= 2) {
        let mut y = 0.0;
        while y < half + ws {
            local_taken.push(Disk {
                c: P2::new(xs, y),
                r: 0.5 * ws + 0.2 * t,
            });
            y += 0.5 * ws;
        }
    }
    // fer des volutes nettement plus fin que la bande
    let w_main = 0.24 * t;
    let w_fill = 0.16 * t;
    let w_floor = 0.09 * t;
    let back = unif(&mut rng) < 0.65;
    let motif = Some({
        Motif::forge(&mut rng, leaf, views, frame, local_taken, w_floor, |f| {
            if fork {
                let r = (half / 2.9).min(0.11 * lw);
                f.crook(
                    P2::new(x_end, 0.0),
                    P2::new(d, 0.0),
                    P2::new(0.0, 1.0),
                    r,
                    0.3 * t,
                    None,
                    true,
                );
            }
            if pattern == 0 {
                // grandes crosses régulièrement espacées le long de la
                // bande, assez écartées pour que des rameaux s'y greffent
                let r_max = (half / 2.9).min(0.12 * lw) * unif_in(&mut f.rng, 0.85, 1.0);
                let span = (x_end - xh).abs();
                let nb = (span * 0.9 / (3.0 * r_max)).floor().clamp(1.0, 6.0) as usize;
                for k in 0..nb {
                    let fr = (k as f64 + 0.6) / (nb as f64 + 0.2);
                    let x = xh + d * (span * fr * 0.95);
                    let tdir = if back { -d } else { d };
                    // départ tangent : le bord intérieur de la crosse épouse
                    // le bord de la bande
                    f.crook(
                        P2::new(x, 0.5 * (t - w_main)),
                        P2::new(tdir, 0.0),
                        P2::new(0.0, 1.0),
                        r_max,
                        w_main,
                        None,
                        true,
                    );
                }
            }
            // volutes greffées tangentiellement sur les grandes, puis sur
            // celles-ci, etc.
            f.grow(0, 4);
            // remplissage : petites crosses dans la place restante, qui
            // portent elles aussi leurs rameaux
            let fill_from = f.crooks.len();
            let span = (x_end - xh).abs();
            for k in 0..14 {
                let x = xh + d * (span * (k as f64 + 0.5) / 14.0);
                let tdir = if k % 2 == 0 { d } else { -d };
                f.crook(
                    P2::new(x, 0.5 * (t - w_fill)),
                    P2::new(tdir, 0.0),
                    P2::new(0.0, 1.0),
                    (half / 2.15).min(0.055 * lw),
                    w_fill,
                    None,
                    true,
                );
            }
            f.grow(fill_from, 3);
        })
    });
    // report du motif sur chaque penture, en haut et en miroir en bas
    let mut disks: Vec<Disk> = Vec::new();
    for &y in &straps {
        let up = Xf::translate(P2::new(0.0, y));
        let down = Xf {
            t: P2::new(0.0, y),
            ..mirror_h(0.0)
        };
        let mut pieces: Vec<Region> = Vec::new();
        let mut local_orns: Orns = Vec::new();
        if let Some(m) = &motif {
            m.place(&[up, down], &mut pieces, &mut local_orns, &mut disks, cons);
        }
        // la bande et son motif ne font qu'une pièce de fer
        let mut strap: Vec<Region> = band(
            &[Seg::line(P2::new(xh + d * 0.2 * t, y), P2::new(x_end, y))],
            t,
            1.0,
            t,
        );
        for p in pieces {
            strap = union(&strap, &p);
        }
        if !fork {
            let (paths, regs) = fleur_de_lis(P2::new(x_end, y), P2::new(d, 0.0), 0.8 * t);
            for r in band(&paths, 0.4 * t, 1.0, 0.2 * t) {
                strap = union(&strap, &r);
            }
            for r in regs {
                local_orns.push((OrnKind::Relief, 2, "Fleurons", r));
            }
        }
        iron.extend(strap);
        orns.extend(local_orns);
        // gond (charnière) contre le bord et clous le long de la bande
        orns.push((
            OrnKind::Relief,
            3,
            "Pentures",
            rect_r(xh, y - 0.75 * t, xh + d * 0.9 * t, y + 0.75 * t),
        ));
        let n = (len / (1.7 * t)).floor() as usize;
        for k in 1..n {
            let x = xh + d * (k as f64 * 1.7 * t);
            orns.push((OrnKind::Relief, 3, "Clous", disc(P2::new(x, y), 0.2 * t)));
        }
        disks.push(Disk {
            c: P2::new(x_end, y),
            r: 1.2 * t,
        });
    }
    taken.extend(disks);

    // tige à rinceaux entre la première et la dernière penture,
    // symétrique de part et d'autre (miroir vertical) et de haut en bas
    // (miroir horizontal au milieu)
    let (y_lo, y_hi) = (sorted[0], sorted[sorted.len() - 1]);
    let stem = |iron: &mut Vec<Region>,
                orns: &mut Orns,
                taken: &mut Vec<Disk>,
                cons: &mut Vec<ConstructionEntity>,
                rng: &mut ChaCha8Rng,
                (ya, yb): (f64, f64),
                sym_h: bool| {
        iron.extend(band(
            &[Seg::line(P2::new(xs, ya), P2::new(xs, yb))],
            ws,
            1.0,
            ws,
        ));
        let mid = 0.5 * (ya + yb);
        let y_from = if sym_h { mid } else { ya };
        // côté droit de la tige seulement : l'autre côté est son miroir
        // le cadre inclut la tige : les crosses en partent tangentiellement
        let frame = (xs - 0.5 * ws, y_from - 0.4 * t, xs + 2.0 * lw, yb);
        let mut seen = taken.clone();
        let mx = Xf::mirror_x(xs);
        seen.extend(taken.iter().map(|o| Disk {
            c: mx.apply(o.c),
            r: o.r,
        }));
        if sym_h {
            let mh = mirror_h(mid);
            let more: Vec<Disk> = seen
                .iter()
                .map(|o| Disk {
                    c: mh.apply(o.c),
                    r: o.r,
                })
                .collect();
            seen.extend(more);
        }
        let r_max = (0.12 * lw).min(0.5 * lw - ws);
        let mut views = vec![Xf::IDENTITY, mx];
        if sym_h {
            views.push(mirror_h(mid));
        }
        let m = Motif::forge(rng, leaf, views, frame, seen, w_floor, |f| {
            let mut y = if sym_h {
                mid + 0.5 * r_max
            } else {
                ya + 1.5 * t
            };
            while y < yb - 2.0 * t {
                // départ tangent au bord de la tige
                f.crook(
                    P2::new(xs + 0.5 * (ws - w_main), y),
                    P2::new(0.0, 1.0),
                    P2::new(1.0, 0.0),
                    r_max,
                    w_main,
                    None,
                    true,
                );
                y += 1.05 * r_max;
            }
            f.grow(0, 4);
        });
        let mut xsl = vec![Xf::IDENTITY, mx];
        if sym_h {
            xsl.push(mirror_h(mid));
            // miroir double : symétrie centrale autour de (xs, mid)
            xsl.push(Xf {
                m: [-1.0, 0.0, 0.0, -1.0],
                t: P2::new(2.0 * xs, 2.0 * mid),
                scale: 1.0,
                angle: PI,
                mirror: false,
            });
        }
        let mut pieces = Vec::new();
        let mut disks = Vec::new();
        m.place(&xsl, &mut pieces, orns, &mut disks, cons);
        iron.extend(pieces);
        taken.extend(disks);
    };
    if pattern == 1 && straps.len() >= 2 {
        stem(
            &mut iron,
            &mut orns,
            &mut taken,
            cons,
            &mut rng,
            (y_lo, y_hi),
            true,
        );
    }
    // tige à fleur de lis dans la tête en arc
    if head_stem {
        let mut h = 0.0;
        while leaf.inner.sdf(P2::new(xs, y_hi + h + 0.05 * lw)) > 2.5 * t {
            h += 0.05 * lw;
        }
        if h > 0.3 * lw.min(8.0 * t) {
            let top = y_hi + h - 2.0 * t;
            let (paths, regs) = fleur_de_lis(P2::new(xs, top), P2::new(0.0, 1.0), 0.6 * t);
            iron.extend(band(&paths, 0.3 * t, 1.0, 0.16 * t));
            for r in regs {
                orns.push((OrnKind::Relief, 2, "Fleurons", r));
            }
            taken.push(Disk {
                c: P2::new(xs, top + 1.2 * t),
                r: 2.2 * t,
            });
            stem(
                &mut iron,
                &mut orns,
                &mut taken,
                cons,
                &mut rng,
                (y_hi, top),
                false,
            );
        }
    }
    // une seule pièce par recouvrement : fusion des fers
    let mut merged: Vec<Region> = Vec::new();
    for r in iron {
        merged = if merged.is_empty() {
            vec![r]
        } else {
            union(&merged, &r)
        };
    }
    let mut res: Orns = merged
        .into_iter()
        .map(|r| (OrnKind::Relief, 2, "Pentures", r))
        .collect();
    res.extend(orns);
    res
}

/// Génère une porte de bois à ferrures.
pub fn generate_iron(
    shape: &Region,
    genome: &Genome,
    cancel: &CancelToken,
) -> Result<Design, GenError> {
    #[cfg(not(target_arch = "wasm32"))]
    let t0 = std::time::Instant::now();
    let shape = Region::new(shape.outer.clone(), shape.holes.clone());
    let bay = Bay::read(&shape);
    let (x0, x1, xm, y0, ys) = (bay.x0, bay.x1, bay.xm, bay.y0, bay.ys);
    let w_all = bay.s;
    let h_all = bay.ya - y0;
    let mut rng = ChaCha8Rng::seed_from_u64(derived(genome.seed, 0xFE22));
    let mut out = Out {
        ornaments: Vec::new(),
        openings: Vec::new(),
        construction: Vec::new(),
        nodes: Vec::new(),
        min_area: 1e-7 * w_all * w_all,
    };
    let cancelled = || {
        if cancel.is_cancelled() {
            Err(GenError::Cancelled)
        } else {
            Ok(())
        }
    };

    // ------------------------------------------------------------ vantaux
    // deux vantaux pour une porte large, un seul sinon
    let ratio = w_all / h_all.max(1e-9);
    let double = ratio > 0.55;
    let lw = if double { 0.5 * w_all } else { w_all };
    let t = lw * unif_in(&mut rng, 0.06, 0.075); // largeur des pentures
    let p_wood = out.node("Vantaux");
    let p_planks = out.node("Planches");
    out.relief(&p_wood, 1, [shape.clone()]);
    let g = 0.012 * lw; // joint entre planches
    let n_pl = (lw / (w_all * unif_in(&mut rng, 0.11, 0.16)).max(1e-9))
        .round()
        .max(3.0) as usize;
    let mut joints: Vec<f64> = Vec::new();
    let leaves_x: Vec<(f64, f64)> = if double {
        vec![(x0, xm), (x1, xm)]
    } else if unif(&mut rng) < 0.5 {
        vec![(x0, x1)]
    } else {
        vec![(x1, x0)]
    };
    for &(a, b) in &leaves_x {
        for k in 1..n_pl {
            joints.push(a + (b - a) * k as f64 / n_pl as f64);
        }
    }
    for x in &joints {
        out.creux(
            &p_planks,
            1,
            geom::clip::intersect(
                &[rect_r(x - 0.5 * g, y0 - lw, x + 0.5 * g, bay.ya + lw)],
                &shape,
            ),
        );
    }
    if double {
        // battement : joint large au milieu
        out.creux(
            &p_planks,
            2,
            geom::clip::intersect(
                &[rect_r(xm - 1.2 * g, y0 - lw, xm + 1.2 * g, bay.ya + lw)],
                &shape,
            ),
        );
    }
    cancelled()?;

    // ------------------------------------------------------------ pentures
    // les pentures portent toutes le même motif, symétrique : leur place
    // dépend de la demi-hauteur `hm` de ce motif
    let h_body = ys - y0;
    let margin = 0.9 * t;
    let mut hm = lw * unif_in(&mut rng, 0.2, 0.25);
    let y_bot = |hm: f64| y0 + margin + hm + 0.3 * t;
    let y_top = |hm: f64| {
        if bay.head.is_some() {
            ys - h_body * 0.04
        } else {
            bay.ya - margin - hm - 0.3 * t
        }
    };
    // trop peu de hauteur : motif plus bas
    while y_top(hm) - y_bot(hm) < 2.2 * hm + t && hm > 0.05 * lw {
        hm *= 0.9;
    }
    let (yb, yt) = (y_bot(hm), y_top(hm));
    let mut straps_y = vec![yb];
    if h_body / lw > 2.4 && yt - yb > 4.6 * hm + 2.0 * t {
        straps_y.push(0.5 * (yb + yt));
    }
    straps_y.push(yt);
    let pattern = if unif(&mut rng) < 0.6 { 0 } else { 1 };
    let p_iron = out.node("Pentures");
    let p_fl = out.node("Fleurons");
    let p_nails = out.node("Clous");
    let p_ring = out.node("Anneau");
    let p_lock = out.node("Serrure");
    let path_of = |k: &str| match k {
        "Pentures" => p_iron.clone(),
        "Fleurons" => p_fl.clone(),
        "Clous" => p_nails.clone(),
        "Anneau" => p_ring.clone(),
        _ => p_lock.clone(),
    };
    // l'anneau d'abord : il réserve sa place sur le premier vantail
    let gaps: Vec<(f64, f64)> = {
        let mut ys_sorted = straps_y.clone();
        ys_sorted.sort_by(f64::total_cmp);
        ys_sorted.windows(2).map(|w| (w[0], w[1])).collect()
    };
    let (ga, gb) = gaps
        .iter()
        .copied()
        .max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)))
        .unwrap_or((y0, ys));
    let y_ring = 0.5 * (ga + gb);
    // un seul vantail est forgé ; le second d'une porte double en est le
    // miroir exact (anneau compris, la serrure reste sur le premier)
    let (a, b) = leaves_x[0];
    let dir = if b > a { 1.0 } else { -1.0 };
    // bande du vantail, débordant de la forme côté gonds (et des deux
    // côtés pour un vantail unique) : pas de bord confondu avec elle
    let (lo, hi) = (a.min(b), a.max(b));
    let ext = |x: f64| {
        if (x - xm).abs() < 1e-9 * w_all {
            x
        } else {
            x + (x - xm).signum() * lw
        }
    };
    let strip = rect_r(ext(lo), y0 - lw, ext(hi), bay.ya + lw);
    let leaf_reg = geom::clip::intersect(&[strip], &shape)
        .into_iter()
        .max_by(|p, q| p.area().total_cmp(&q.area()));
    let mut work: Orns = Vec::new();
    let mut cons: Vec<ConstructionEntity> = Vec::new();
    if let Some(leaf_reg) = leaf_reg {
        let inner = offset_region(&leaf_reg, margin)
            .into_iter()
            .max_by(|p, q| p.area().total_cmp(&q.area()))
            .unwrap_or_else(|| leaf_reg.clone());
        let leaf = Leaf {
            a,
            b,
            dir,
            reg: leaf_reg.clone(),
            inner,
            views: Vec::new(),
            frame: None,
        };
        let mut taken: Vec<Disk> = Vec::new();
        // anneau sur platine et entrée de serrure, près du bord libre
        let rho = (0.075 * lw).max(1.2 * t);
        let c = P2::new(b - dir * (0.2 * lw).max(1.6 * rho), y_ring);
        let rr = 1.05 * rho;
        let rc = c - P2::new(0.0, rr - 0.25 * rho);
        work.push((OrnKind::Relief, 2, "Anneau", rosette(c, rho, 6)));
        for r in geom::clip::subtract(&[disc(rc, rr + 0.12 * rho)], &disc(rc, rr - 0.12 * rho)) {
            work.push((OrnKind::Relief, 3, "Anneau", r));
        }
        work.push((OrnKind::Relief, 3, "Anneau", disc(c, 0.28 * rho)));
        taken.push(Disk {
            c: rc,
            r: rr + 0.5 * rho,
        });
        taken.push(Disk { c, r: 1.1 * rho });
        let kc = c - P2::new(dir * 2.0 * rho, 0.6 * rho);
        let kc = if leaf.holds(Disk {
            c: kc,
            r: 0.8 * rho,
        }) {
            kc
        } else {
            c - P2::new(0.0, 3.0 * rho)
        };
        work.push((
            OrnKind::Relief,
            2,
            "Serrure",
            shield(kc, 1.1 * rho, 1.5 * rho),
        ));
        work.push((
            OrnKind::Creux,
            2,
            "Serrure",
            disc(kc + P2::new(0.0, 0.2 * rho), 0.16 * rho),
        ));
        work.push((
            OrnKind::Creux,
            2,
            "Serrure",
            rect_r(
                kc.x - 0.07 * rho,
                kc.y - 0.35 * rho,
                kc.x + 0.07 * rho,
                kc.y + 0.2 * rho,
            ),
        ));
        taken.push(Disk {
            c: kc,
            r: 0.95 * rho,
        });
        let leaf_rng = ChaCha8Rng::seed_from_u64(derived(genome.seed, 0x1E4F));
        work.extend(leaf_ironwork(
            leaf_rng,
            &leaf,
            &straps_y,
            t,
            hm,
            pattern,
            taken,
            bay.head.is_some(),
            &mut cons,
        ));
    }
    let mirror = Xf::mirror_x(xm);
    for (kind, lv, node, r) in &work {
        out.add(&path_of(node), *kind, *lv, [r.clone()]);
        if double && *node != "Serrure" {
            out.add(&path_of(node), *kind, *lv, [r.transform(&mirror)]);
        }
    }
    for c in &cons {
        out.construction.push(*c);
        if double {
            out.construction.push(c.transform(&mirror));
        }
    }
    cancelled()?;

    // garde-fou : rien ne dépasse de la forme (formes quelconques)
    let eps = 1e-6 * shape.bbox().diag();
    let all = std::mem::take(&mut out.ornaments);
    for o in all {
        let step = o.region.perimeter() / 64.0;
        if o.region
            .outer
            .sample(step)
            .iter()
            .all(|p| shape.sdf(*p) >= -eps)
            && o.region.holes.is_empty()
        {
            out.ornaments.push(o);
        } else {
            let regs = geom::clip::intersect(std::slice::from_ref(&o.region), &shape);
            out.add(&o.path, o.kind, o.level, regs);
        }
    }
    // les joints des planches passent sous le fer
    let irons: Vec<(Region, BBox)> = out
        .ornaments
        .iter()
        .filter(|o| o.kind == OrnKind::Relief && o.level >= 2)
        .map(|o| (o.region.clone(), o.region.bbox()))
        .collect();
    let all = std::mem::take(&mut out.ornaments);
    for o in all {
        if o.path != p_planks {
            out.ornaments.push(o);
            continue;
        }
        let bb = o.region.bbox();
        let mut regs = vec![o.region.clone()];
        for (ir, ib) in &irons {
            if ib.overlaps(&bb, 0.0) {
                regs = geom::clip::subtract(&regs, ir);
            }
        }
        out.add(&o.path, o.kind, o.level, regs);
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
        rule: "Ferronnerie".into(),
        params: Params::new()
            .with("vantaux", if double { 2.0 } else { 1.0 })
            .with("penture", t)
            .with("motif", pattern as f64),
        types: vec!["ferronnerie".into()],
        area: shape.area(),
        inscribed: 0.0,
        local_score: 1.0,
        twin_of: None,
        bbox: root_bb,
        locked: false,
        ratio: Some(ratio),
        candidates: 1,
        region: Some(shape.clone()),
    }];
    for (p, name) in &out.nodes {
        let mine: Vec<&Ornament> = out.ornaments.iter().filter(|o| &o.path == p).collect();
        let bb = mine
            .iter()
            .fold(BBox::EMPTY, |bb, o| bb.union(&o.region.bbox()));
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
    terms.insert("ferronnerie".to_string(), 1.0);
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
        openings: Vec::new(),
        construction: out.construction,
        tree,
        score: ScoreBreakdown { total: 1.0, terms },
        stats,
        widths: vec![t, 0.6 * t],
        warnings: Vec::new(),
        door: true,
        iron: true,
        ornaments: out.ornaments,
    };
    d.stats.primitives = d.primitive_count();
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Family, StyleParams};
    use motifs::{ArchKind, arch_window};

    fn iron_genome(seed: u64) -> Genome {
        let mut st = StyleParams::for_family(Family::Flamboyant);
        st.iron = true;
        Genome::new(seed, st)
    }

    #[test]
    fn iron_inside_shape_and_deterministic() {
        let (l, _) = arch_window(ArchKind::Pointed { rho: 1.0 }, 1000.0, 1500.0);
        let arch = Region::simple(l);
        let rect = Region::simple(crate::door::rect(0.0, 0.0, 1800.0, 2200.0));
        for shape in [arch, rect] {
            for seed in 0..6 {
                let d = crate::generate(&shape, &iron_genome(seed), &CancelToken::new()).unwrap();
                assert!(d.iron && d.door);
                assert!(d.ornaments.iter().any(|o| o.path.ends_with("/2")));
                let eps = 1e-6 * shape.bbox().diag();
                for o in &d.ornaments {
                    for p in o.region.outer.sample(o.region.perimeter() / 40.0) {
                        assert!(shape.sdf(p) > -eps, "{} hors de la forme", o.path);
                    }
                }
                let d2 = crate::generate(&shape, &iron_genome(seed), &CancelToken::new()).unwrap();
                assert_eq!(d.ornaments, d2.ornaments);
            }
        }
    }

    /// Vantaux d'une porte double identiques (en miroir), deux pentures au
    /// moins sur chacun.
    #[test]
    fn iron_symmetries() {
        let rect = Region::simple(crate::door::rect(0.0, 0.0, 1800.0, 2200.0));
        let near = |a: &BBox, b: &BBox| {
            (a.min.x - b.min.x).abs() < 1e-3
                && (a.max.x - b.max.x).abs() < 1e-3
                && (a.min.y - b.min.y).abs() < 1e-3
                && (a.max.y - b.max.y).abs() < 1e-3
        };
        for seed in 0..4 {
            let d = crate::generate(&rect, &iron_genome(seed), &CancelToken::new()).unwrap();
            let lock = d
                .tree
                .iter()
                .find(|n| n.rule == "Serrure")
                .map(|n| n.path.clone())
                .unwrap_or_default();
            let mirror = Xf::mirror_x(900.0);
            for o in d.ornaments.iter().filter(|o| o.path != lock) {
                let mb = o.region.transform(&mirror).bbox();
                assert!(
                    d.ornaments
                        .iter()
                        .any(|q| q.path == o.path && near(&q.region.bbox(), &mb)),
                    "seed {seed} : {} sans miroir",
                    o.path
                );
            }
            // au moins deux pentures (gonds hauts et étroits)
            let gonds = d
                .ornaments
                .iter()
                .filter(|o| o.level == 3 && o.region.bbox().size().x < o.region.bbox().size().y)
                .count();
            assert!(gonds >= 4, "seed {seed} : {gonds} gonds");
        }
    }
}
