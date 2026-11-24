//! Façade gothique (ADR-32), d'après les logis de la fin du gothique
//! (château des ducs de Bretagne, palais Jacques-Cœur, hôtels de Meaux et
//! de Rouen) : la forme, un rectangle, est le mur de la façade.
//!
//! - **Plan** (`plan`) : étages (rez-de-chaussée plus haut), travées
//!   régulières, fenêtres à meneaux de même taille et de même génome à
//!   chaque niveau, portes en arc brisé au rez-de-chaussée, tours
//!   hexagonales ou octogonales posées sur une travée, qui montent d'un
//!   étage au-dessus de la façade (accès aux toits). L'application crée
//!   les fenêtres et les portes du plan comme des formes ordinaires,
//!   modifiables une à une.
//! - **Décor du mur** (`generate_facade`) : soubassement, cordons
//!   (corniches entre les étages) sous les appuis, corniche à modillons et
//!   arcature aveugle, balustrade à quadrilobes ; tours à pans, bandeaux,
//!   corniche à consoles, flèche couverte d'assises et croix d'épi, ou
//!   parapet crénelé.
//!
//! Les fenêtres et les portes ne font pas partie de ce remplage : elles
//! sont générées chacune par son propre générateur. Les marges entre les
//! niveaux tiennent compte de l'encombrement mesuré de leur décor
//! (accolades, fleurons, appuis). Tout est construit à la règle et au
//! compas et entièrement déterminé par la seed.

use crate::design::{Design, OrnKind, Stats, TreeNode};
use crate::door::{Out, chou, cross_finial, rect_r};
use crate::genome::{GENERATOR_VERSION, Genome, Params};
use crate::rules::{unif, unif_in, weighted_index};
use crate::score::ScoreBreakdown;
use crate::seed::derived;
use crate::style::{CrossKind, Family, StyleParams, WindowKind};
use crate::{CancelToken, GenError};
use geom::clip::{intersect_loop, subtract, union};
use geom::math::*;
use geom::{BBox, ConstructionEntity, ConstructionOp, Loop, Region, Seg, Xf};
use motifs::{ArchKind, arch_window};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;
use std::collections::BTreeMap;

/// Encombrement du décor d'une croisée au-dessus de son arête haute
/// (accolade, fleuron), en largeurs de la croisée (mesuré : 0,93).
pub const WIN_UP: f64 = 0.95;
/// … sous son arête basse (appui), en largeurs (mesuré : 0,18).
pub const WIN_DOWN: f64 = 0.19;
/// … de chaque côté, en largeurs (mesuré : 0,22).
pub const WIN_SIDE: f64 = 0.23;
/// Décor d'une porte au-dessus de sa clé, en portées (mesuré : 2,06).
pub const DOOR_UP: f64 = 2.1;
/// … de chaque côté, en portées (mesuré : 0,34).
pub const DOOR_SIDE: f64 = 0.35;

/// Ouverture du plan : fenêtre (rectangle) ou porte (arc brisé), dans une
/// boîte (centre, largeur, hauteur, arc compris).
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedOpening {
    pub door: bool,
    pub arch: Option<ArchKind>,
    pub center: P2,
    pub w: f64,
    pub h: f64,
    pub genome: Genome,
    /// Niveau (0 : rez-de-chaussée ; le dernier niveau des tours est
    /// l'étage qui dépasse la façade).
    pub floor: usize,
    /// Tour qui la porte.
    pub tower: Option<usize>,
    /// Croisée d'une lucarne.
    pub dormer: bool,
    /// Baie d'apparat au-dessus d'un portail, d'un autre style que les
    /// fenêtres du niveau (remplage en arc brisé ou croisée haute).
    pub special: bool,
    /// Encombrement de son décor.
    pub reach: Reach,
}

impl PlannedOpening {
    /// Contour de la baie.
    pub fn region(&self) -> Region {
        match self.arch {
            Some(k) => {
                let ha = motifs::arch_curve(k, self.w).height;
                let jamb = (self.h - ha).max(0.0);
                let (l, _) = arch_window(k, self.w, jamb);
                let y_spring = self.center.y - 0.5 * self.h + jamb;
                Region::simple(l.transform(&Xf::translate(P2::new(self.center.x, y_spring))))
            }
            None => rect_r(
                self.center.x - 0.5 * self.w,
                self.center.y - 0.5 * self.h,
                self.center.x + 0.5 * self.w,
                self.center.y + 0.5 * self.h,
            ),
        }
    }
    /// Boîte du décor (mesuré) autour de la baie.
    pub fn decor_box(&self) -> BBox {
        let (x, y) = (self.center.x, self.center.y);
        let (hw, hh) = (0.5 * self.w, 0.5 * self.h);
        let (r, s) = (self.reach, self.w);
        BBox::from_pts(
            P2::new(x - hw - r.side * s, y - hh - r.down * s),
            P2::new(x + hw + r.side * s, y + hh + r.up * s),
        )
    }
}

/// Couronnement d'une tour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TowerTop {
    /// Flèche à pans couverte d'assises, croix d'épi.
    Spire,
    /// Parapet crénelé (terrasse).
    Crenels,
}

/// Tour à pans posée devant une travée.
#[derive(Clone, Debug, PartialEq)]
pub struct TowerPlan {
    pub xc: f64,
    /// Nombre de pans (6 ou 8) ; un pan fait face.
    pub sides: usize,
    /// Rayon du cercle circonscrit.
    pub r: f64,
    /// Largeur vue.
    pub width: f64,
    pub y0: f64,
    /// Arase des murs (sous la corniche de la tour).
    pub y_top: f64,
    /// Hauteur de la corniche (consoles, bandeau, larmier).
    pub cornice: f64,
    pub top: TowerTop,
    /// Hauteur de la flèche ou du parapet.
    pub top_h: f64,
    /// Bandeaux (ordonnée de leur axe) et leur hauteur.
    pub bands: Vec<f64>,
    pub band_h: f64,
    /// Échauguette : hauteur du cul-de-lampe sous le fût (0 : tour posée
    /// au sol).
    pub corbel: f64,
    /// Niveaux de relief ajoutés (échauguette accrochée à une tour, qui
    /// passe devant elle).
    pub lift: u8,
}

impl TowerPlan {
    /// Abscisses des arêtes verticales vues, de gauche à droite (pans dont
    /// la normale regarde l'observateur ; un pan fait face).
    pub fn edges(&self) -> Vec<f64> {
        tower_edges(self.sides, self.r)
            .into_iter()
            .map(|x| self.xc + x)
            .collect()
    }
    /// Largeur du pan de face.
    pub fn front(&self) -> f64 {
        2.0 * self.r * sin(PI / self.sides as f64)
    }
}

/// Arêtes vues (abscisses relatives à l'axe) d'un prisme à `n` pans de
/// rayon `r`, un pan face à l'observateur.
fn tower_edges(n: usize, r: f64) -> Vec<f64> {
    let n = n.max(3);
    let step = 2.0 * PI / n as f64;
    let mut xs: Vec<f64> = Vec::new();
    for k in 0..n {
        let a = FRAC_PI_2 + 0.5 * step + k as f64 * step;
        let b = a + step;
        // normale du pan : au milieu des deux sommets
        if sin(a + 0.5 * step) > 1e-6 {
            xs.push(r * cos(a));
            xs.push(r * cos(b));
        }
    }
    xs.sort_by(|a, b| a.total_cmp(b));
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-9 * r);
    xs
}

/// Toit du logis à deux versants, vu du long pan : un rectangle d'ardoise
/// de l'égout au faîtage, entre deux pignons de pierre vus de chant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roof {
    /// Égout (haut de la corniche).
    pub y0: f64,
    /// Hauteur jusqu'au faîtage.
    pub h: f64,
    /// Épaisseur des pignons.
    pub gable: f64,
}

/// Cheminée : souche carrée à tablette, fûts ronds groupés à couronne
/// évasée ornée de rosaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chimney {
    pub xc: f64,
    /// Pied de la souche (sur le toit ou le pignon).
    pub y0: f64,
    /// Haut de la souche (sous la tablette).
    pub base_top: f64,
    /// Largeur de la souche.
    pub w: f64,
    /// Nombre de fûts vus.
    pub shafts: usize,
    /// Haut des fûts (sous la couronne).
    pub shaft_top: f64,
}

/// Lucarne de pierre posée sur la corniche (ou derrière la balustrade),
/// dans le toit : front entre deux jambages, gâble mouluré, pinacles de
/// part et d'autre ; sa croisée est une ouverture du plan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dormer {
    pub xc: f64,
    /// Largeur du front (jambages compris).
    pub w: f64,
    /// Pied du front (haut de la corniche).
    pub y0: f64,
    /// Naissance du gâble.
    pub y_gable: f64,
    /// Hauteur du gâble.
    pub gable_h: f64,
    /// Largeur des pinacles.
    pub pin_w: f64,
}

impl Dormer {
    /// Front et gâble (le toit s'y arrête).
    pub fn silhouette(&self) -> Region {
        let (l, r) = (self.xc - 0.5 * self.w, self.xc + 0.5 * self.w);
        Region::simple(Loop::polygon(&[
            P2::new(l, self.y0),
            P2::new(r, self.y0),
            P2::new(r, self.y_gable),
            P2::new(self.xc, self.y_gable + self.gable_h),
            P2::new(l, self.y_gable),
        ]))
    }
}

/// Plan de la façade.
#[derive(Clone, Debug, PartialEq)]
pub struct FacadePlan {
    pub x0: f64,
    pub x1: f64,
    pub y0: f64,
    /// Haut de la façade (balustrade comprise).
    pub y1: f64,
    /// Lignes des planchers : `floors[k]` bas du niveau k ; le dernier est
    /// le bas de la corniche.
    pub floors: Vec<f64>,
    /// Axes des travées.
    pub columns: Vec<f64>,
    pub bay: f64,
    /// Hauteur d'un étage courant.
    pub storey: f64,
    /// Hauteur du soubassement (0 : sans).
    pub plinth: f64,
    /// Cordons : (bas, haut).
    pub cordons: Vec<(f64, f64)>,
    /// Corniche : (bas, haut).
    pub cornice: (f64, f64),
    pub balustrade: Option<(f64, f64)>,
    /// Toit à deux versants posé sur la corniche (derrière la balustrade).
    pub roof: Roof,
    pub dormers: Vec<Dormer>,
    pub chimneys: Vec<Chimney>,
    pub towers: Vec<TowerPlan>,
    /// Échauguettes aux angles, en encorbellement.
    pub bartizans: Vec<TowerPlan>,
    pub openings: Vec<PlannedOpening>,
    /// Unité du rendu en relief (demi-chambranle d'une croisée courante).
    pub unit: f64,
}

/// Fenêtre d'un niveau : allège, hauteur, rapport largeur / hauteur et
/// largeur maximale.
#[derive(Clone, Copy, Debug)]
struct Win {
    sill: f64,
    h: f64,
    w: f64,
}

/// Réduit une fenêtre pour que son décor (`up` largeurs au-dessus) tienne
/// sous `room` (depuis son arête basse).
fn fit_window(win: &mut Win, room: f64, up: f64) {
    if win.h + up * win.w <= room {
        return;
    }
    let ratio = win.w / win.h.max(1e-9);
    let h = (room / (1.0 + up * ratio)).max(0.0);
    win.h = h;
    win.w = ratio * h;
}

/// Encombrement du décor d'une ouverture, en largeurs de la baie (en
/// portées pour une porte) : au-dessus de son arête haute (sa clé), sous
/// son arête basse, de chaque côté.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reach {
    pub up: f64,
    pub down: f64,
    pub side: f64,
}

impl Reach {
    /// Pire cas mesuré d'une croisée.
    pub const WINDOW: Reach = Reach {
        up: WIN_UP,
        down: WIN_DOWN,
        side: WIN_SIDE,
    };
    /// Pire cas mesuré d'une porte.
    pub const DOOR: Reach = Reach {
        up: DOOR_UP,
        down: 0.0,
        side: DOOR_SIDE,
    };
}

/// Décor réel d'une ouverture, mesuré en la générant : les générateurs de
/// croisées et de portes sont invariants par homothétie, la mesure vaut
/// pour toute baie de mêmes proportions et de même génome.
fn measure(region: &Region, g: &Genome, fallback: Reach) -> Reach {
    let bb = region.bbox();
    let s = bb.size().x.max(1e-9);
    match crate::generate(region, g, &CancelToken::new()) {
        Ok(d) => {
            let b = d.bbox();
            let m = 0.02;
            Reach {
                up: ((b.max.y - bb.max.y) / s).max(0.0) + m,
                down: ((bb.min.y - b.min.y) / s).max(0.0) + m,
                side: ((b.max.x - bb.max.x).max(bb.min.x - b.min.x) / s).max(0.0) + m,
            }
        }
        Err(_) => fallback,
    }
}

/// Baie en arc brisé : portée `s`, hauteur totale `h`, seuil en (xc, y0).
fn arch_region(kind: ArchKind, xc: f64, y0: f64, s: f64, h: f64) -> Region {
    let ha = motifs::arch_curve(kind, s).height;
    let jamb = (h - ha).max(0.0);
    let (l, _) = arch_window(kind, s, jamb);
    Region::simple(l.transform(&Xf::translate(P2::new(xc, y0 + jamb))))
}

/// Demi-largeur de prisme (sommet le plus écarté) pour un rayon unité.
fn prism_half(sides: usize) -> f64 {
    tower_edges(sides, 1.0)
        .iter()
        .fold(0.0f64, |m, x| m.max(x.abs()))
}

/// Les boîtes se chevauchent-elles ?
fn overlap(a: &BBox, b: &BBox) -> bool {
    a.min.x < b.max.x - 1e-9
        && b.min.x < a.max.x - 1e-9
        && a.min.y < b.max.y - 1e-9
        && b.min.y < a.max.y - 1e-9
}

impl TowerPlan {
    /// Boîte de la silhouette (cul-de-lampe, corniche, flèche comprises).
    pub fn bbox(&self) -> BBox {
        let flare = if self.corbel > 0.0 { 0.07 } else { 0.22 };
        let hw = 0.5 * self.width + flare * self.width;
        let lo = if self.corbel > 0.0 {
            self.y0 - self.corbel - 0.1 * self.width
        } else {
            self.y0
        };
        BBox::from_pts(
            P2::new(self.xc - hw, lo),
            P2::new(self.xc + hw, self.y_top + self.cornice + 1.2 * self.top_h),
        )
    }
}

/// Plan de la façade inscrite dans `bb` (déterministe : boîte et génome).
pub fn plan(bb: &BBox, genome: &Genome) -> FacadePlan {
    let st = &genome.style;
    let mut rng = ChaCha8Rng::seed_from_u64(derived(genome.seed, 0xFACA));
    let (x0, y0, x1, y1) = (bb.min.x, bb.min.y, bb.max.x, bb.max.y);
    let (w, hh) = ((x1 - x0).max(1e-9), (y1 - y0).max(1e-9));

    // -------------------------------------------------------- niveaux
    let n = if st.floors > 0 {
        st.floors.min(8) as usize
    } else {
        let base = unif_in(&mut rng, 2.2, 3.6) * (1.6 * hh / w).sqrt().clamp(0.5, 2.0);
        (round(base) as usize).clamp(1, 6)
    };
    let g0 = unif_in(&mut rng, 1.12, 1.3); // rez-de-chaussée plus haut
    let c_r = unif_in(&mut rng, 0.2, 0.27); // corniche
    let balus = unif(&mut rng) < 0.45;
    let b_r = if balus {
        unif_in(&mut rng, 0.3, 0.38)
    } else {
        0.0
    };
    let hu = hh / (g0 + (n - 1) as f64 + c_r + b_r);
    let h0 = g0 * hu;
    let mut floors = vec![y0];
    for k in 0..n {
        let hk = if k == 0 { h0 } else { hu };
        floors.push(floors[k] + hk);
    }
    let yc0 = floors[n];
    let yc1 = yc0 + c_r * hu;
    let balustrade = balus.then_some((yc1, y1));
    // toit à deux versants entre deux pignons de pierre, haut et raide
    // (exhaussé de la balustrade : les lucarnes naissent au-dessus)
    let roof_h = hu * unif_in(&mut rng, 1.1, 1.45) + (y1 - yc1);
    let roof = Roof {
        y0: yc1,
        h: roof_h,
        gable: hu * unif_in(&mut rng, 0.1, 0.15),
    };
    let height = |k: usize| floors[k + 1] - floors[k];

    // -------------------------------------------------------- travées
    // Rythme irrégulier : travées de largeurs inégales (symétriques une
    // fois sur deux), trumeaux d'angle ; assez de mur aux angles qui
    // portent une échauguette (rares).
    let bay_t = hu * unif_in(&mut rng, 0.85, 1.15);
    let bart_w = hu * unif_in(&mut rng, 0.3, 0.4);
    let side = unif(&mut rng) < 0.5;
    let (mut bart_l, mut bart_r) = match weighted_index(&mut rng, &[0.78, 0.1, 0.12]) {
        Some(1) => (true, true),
        Some(2) => (side, !side),
        _ => (false, false),
    };
    if n < 2 {
        // un seul niveau : le cul-de-lampe tomberait sur les portes
        bart_l = false;
        bart_r = false;
    }
    let sym = unif(&mut rng) < 0.5;
    let pier_l = bay_t * unif_in(&mut rng, 0.05, 0.3);
    let pier_r = if sym {
        pier_l
    } else {
        bay_t * unif_in(&mut rng, 0.05, 0.3)
    };
    let nb = if st.bays > 0 {
        st.bays.min(40) as usize
    } else {
        (round((w - pier_l - pier_r) / bay_t) as usize).max(1)
    };
    let mut ratios: Vec<f64> = (0..nb).map(|_| unif_in(&mut rng, 0.72, 1.3)).collect();
    if sym {
        for i in 0..nb / 2 {
            ratios[nb - 1 - i] = ratios[i];
        }
    }
    let layout = |m_l: f64, m_r: f64| -> Vec<f64> {
        let avail = (w - m_l - m_r).max(1e-9);
        let sum: f64 = ratios.iter().sum();
        ratios.iter().map(|r| r / sum * avail).collect()
    };
    let (mut m_l, mut m_r) = (pier_l, pier_r);
    for _ in 0..4 {
        let bays = layout(m_l, m_r);
        let bmin = bays.iter().fold(f64::INFINITY, |m, b| m.min(*b));
        // décor d'une croisée haute ou d'une lucarne : 0,46 travée au plus
        let need = |b_end: f64| 0.66 * bart_w + 0.46 * bmin + 0.03 * hu - 0.5 * b_end;
        if bart_l {
            m_l = m_l.max(need(bays[0]));
        }
        if bart_r {
            m_r = m_r.max(need(bays[nb - 1]));
        }
        if sym {
            let m = m_l.max(m_r);
            (m_l, m_r) = (m, m);
        }
    }
    if m_l + m_r > 0.5 * w {
        (bart_l, bart_r) = (false, false);
        (m_l, m_r) = (pier_l, pier_r);
    }
    let bays = layout(m_l, m_r);
    let bw = bays.iter().fold(f64::INFINITY, |m, b| m.min(*b));
    let mut columns: Vec<f64> = Vec::with_capacity(nb);
    let mut xa = x0 + m_l;
    for b in &bays {
        columns.push(xa + 0.5 * b);
        xa += b;
    }

    let plinth = if unif(&mut rng) < 0.7 {
        hu * unif_in(&mut rng, 0.07, 0.11)
    } else {
        0.0
    };
    // cordons : partout, un sur deux en moyenne, ou aucun
    let q = match st.cordons {
        Some(true) => 1.0,
        Some(false) => 0.0,
        None => [0.0, 0.6, 1.0][weighted_index(&mut rng, &[0.2, 0.25, 0.55]).unwrap_or(2)],
    };
    let has_cordon: Vec<bool> = (0..n).map(|k| unif(&mut rng) < q && k >= 1).collect();
    let cordon_h = hu * unif_in(&mut rng, 0.05, 0.07);
    let gap = 0.03 * hu;

    // ----------------------------------------------------------- tours
    let max_t = if nb >= 5 {
        2
    } else if nb >= 3 {
        1
    } else {
        0
    };
    let want_t = match st.towers {
        Some(t) => t as usize,
        None => weighted_index(&mut rng, &[0.15, 0.6, 0.25]).unwrap_or(1),
    };
    // pas de tour sur une travée d'angle qui porte une échauguette
    let first_ok = usize::from(bart_l || bart_r);
    let nt = if nb >= 3 && 2 * first_ok + 3 > nb {
        want_t.min(max_t).min(1)
    } else {
        want_t.min(max_t)
    };
    let mut tower_cols: Vec<usize> = Vec::new();
    if nt == 1 {
        // aux extrémités ou sur une travée intérieure
        let mut i = if unif(&mut rng) < 0.5 {
            if unif(&mut rng) < 0.5 { 0 } else { nb - 1 }
        } else {
            1 + (unif(&mut rng) * (nb - 2) as f64) as usize
        };
        if (i == 0 && bart_l) || (i == nb - 1 && bart_r) {
            i = if i == 0 { 1 } else { nb - 2 };
        }
        tower_cols.push(i.min(nb - 1));
    } else if nt == 2 {
        // paire symétrique, séparée d'au moins une travée
        let lo = first_ok;
        let k = ((nb - 3) / 2 + 1).saturating_sub(lo).max(1);
        let i = lo + ((unif(&mut rng) * k as f64) as usize).min(k - 1);
        tower_cols.extend([i, nb - 1 - i]);
    }
    let free: Vec<usize> = (0..nb).filter(|i| !tower_cols.contains(i)).collect();

    // ---------------------------------------------------------- portes
    // (pas sur une travée d'angle sous une échauguette)
    let door_ok: Vec<usize> = free
        .iter()
        .copied()
        .filter(|&i| !((i == 0 && bart_l) || (i == nb - 1 && bart_r)))
        .collect();
    let door_pool = if door_ok.is_empty() { &free } else { &door_ok };
    let mut door_cols: Vec<usize> = Vec::new();
    if !door_pool.is_empty() {
        let mid = nb / 2;
        let first = if nb % 2 == 1 && door_pool.contains(&mid) {
            mid
        } else {
            door_pool[((unif(&mut rng) * door_pool.len() as f64) as usize).min(door_pool.len() - 1)]
        };
        door_cols.push(first);
        let twin = nb - 1 - first;
        if door_pool.len() >= 4
            && twin != first
            && first.abs_diff(twin) >= 2
            && door_pool.contains(&twin)
            && unif(&mut rng) < 0.35
        {
            door_cols.push(twin);
        }
    }

    // --------------------------------------------------------- génomes
    let family = st.family;
    let level_genome = |rng: &mut ChaCha8Rng, k: usize| {
        let cross = [CrossKind::Auto, CrossKind::Simple, CrossKind::Double]
            [weighted_index(rng, &[0.5, 0.25, 0.25]).unwrap_or(0)];
        let round_top = Some(unif(rng) < 0.3);
        let ws = StyleParams {
            window: WindowKind::Meneaux,
            cross,
            round_top,
            ..StyleParams::for_family(family)
        };
        Genome::new(derived(genome.seed, 0x100 + k as u64) % 1_000_000_000, ws)
    };
    let genomes: Vec<Genome> = (0..=n).map(|k| level_genome(&mut rng, k)).collect();
    let door_genome = Genome::new(
        derived(genome.seed, 0xD0) % 1_000_000_000,
        StyleParams {
            door: true,
            ..StyleParams::for_family(family)
        },
    );
    let door_kind = ArchKind::Pointed {
        rho: if unif(&mut rng) < 0.6 { 1.0 } else { 1.15 },
    };
    // baies d'apparat au-dessus des portails, d'un autre style que les
    // fenêtres du niveau : remplage flamboyant en arc brisé, ou croisée
    // haute et étroite d'un génome à part
    let sp_tracery = unif(&mut rng) < 0.5;
    let sp_kind = ArchKind::Pointed {
        rho: unif_in(&mut rng, 1.0, 1.25),
    };
    let sp_ratio = unif_in(&mut rng, 0.42, 0.56); // largeur / hauteur
    let sp_h = unif_in(&mut rng, 0.95, 1.12); // hauteur / croisée du niveau
    let sp_cross = if unif(&mut rng) < 0.5 {
        CrossKind::Simple
    } else {
        CrossKind::Double
    };
    let sp_genome = Genome::new(
        derived(genome.seed, 0x5BEC) % 1_000_000_000,
        if sp_tracery {
            StyleParams {
                depth: 3,
                ..StyleParams::for_family(Family::Flamboyant)
            }
        } else {
            StyleParams {
                window: WindowKind::Meneaux,
                cross: sp_cross,
                round_top: Some(true),
                ..StyleParams::for_family(family)
            }
        },
    );
    // un remplage n'a pas de décor hors de sa forme
    let sp_reach = if sp_tracery {
        Reach {
            up: 0.03,
            down: 0.03,
            side: 0.03,
        }
    } else {
        measure(
            &rect_r(0.0, 0.0, 1000.0 * sp_ratio, 1000.0),
            &sp_genome,
            Reach::WINDOW,
        )
    };

    // ---------------------------------------------- fenêtres, du haut
    // Taille visée, proportions et allège de chaque niveau ; le décor de
    // sa croisée (accolade, larmier, appui) est mesuré sur son génome : un
    // niveau sans accolade a des baies plus grandes.
    let mut wins: Vec<Win> = Vec::with_capacity(n);
    let mut reach: Vec<Reach> = Vec::with_capacity(n);
    for (k, gk) in genomes.iter().enumerate().take(n) {
        let hk = height(k);
        let (h, sill, ratio) = if k == 0 {
            (
                hk * unif_in(&mut rng, 0.36, 0.46),
                hk * unif_in(&mut rng, 0.3, 0.38),
                unif_in(&mut rng, 0.65, 0.9),
            )
        } else {
            (
                hk * unif_in(&mut rng, 0.6, 0.7),
                hk * unif_in(&mut rng, 0.2, 0.25),
                unif_in(&mut rng, 0.55, 0.75),
            )
        };
        let r = measure(&rect_r(0.0, 0.0, 1000.0 * ratio, 1000.0), gk, Reach::WINDOW);
        let mut win = Win {
            sill,
            h,
            w: ratio * h,
        };
        // dans la plus étroite des travées, décor compris
        let cap = 0.46 * bw / (0.5 + r.side);
        if win.w > cap {
            let f = cap / win.w;
            win.w *= f;
            win.h *= f;
        }
        wins.push(win);
        reach.push(r);
    }
    let mut cordons_at: Vec<Option<(f64, f64)>> = vec![None; n];
    // limite haute du décor des ouvertures du niveau k
    let mut lims = vec![yc0 - gap; n];
    for k in (0..n).rev() {
        let lim = if k + 1 == n {
            yc0 - gap
        } else {
            match cordons_at[k + 1] {
                Some((b, _)) => b - gap,
                None => floors[k + 1] + wins[k + 1].sill - reach[k + 1].down * wins[k + 1].w - gap,
            }
        };
        lims[k] = lim;
        let room = lim - floors[k] - wins[k].sill;
        fit_window(&mut wins[k], room, reach[k].up);
        if has_cordon[k] {
            let top = floors[k] + wins[k].sill - reach[k].down * wins[k].w - 0.01 * hu;
            cordons_at[k] = Some((top - cordon_h, top));
        }
    }
    let cordons: Vec<(f64, f64)> = cordons_at.iter().flatten().copied().collect();

    // ---------------------------------------------------------- portes
    // Portails du logis, tous semblables et imposants : leur décor peut
    // monter jusque sous les fenêtres du deuxième niveau (la croisée du
    // premier, au-dessus de la porte, est alors omise).
    let ha1 = motifs::arch_curve(door_kind, 1.0).height;
    let mut ds = h0 * unif_in(&mut rng, 0.42, 0.52);
    let mut dh = (h0 * unif_in(&mut rng, 0.7, 0.82)).max((ha1 + 0.6) * ds);
    let door_reach = if door_cols.is_empty() {
        Reach::DOOR
    } else {
        measure(
            &arch_region(door_kind, 0.0, 0.0, 1000.0, 1000.0 * dh / ds),
            &door_genome,
            Reach::DOOR,
        )
    };
    for &c in &door_cols {
        for j in [c.wrapping_sub(1), c + 1] {
            if j < nb {
                let d = (columns[c] - columns[j]).abs();
                let max_s = (d - 0.46 * bw - 0.02 * hu) / (0.5 + door_reach.side);
                if ds > max_s {
                    let f = (max_s / ds).max(0.0);
                    ds *= f;
                    dh *= f;
                }
            }
        }
    }
    let takes = n >= 2 && y0 + dh + door_reach.up * ds > lims[0];
    let door_lim = if takes { lims[1] } else { lims[0] };
    if y0 + dh + door_reach.up * ds > door_lim {
        let f = (door_lim - y0) / (dh + door_reach.up * ds);
        ds *= f;
        dh *= f;
    }

    // ------------------------------------------------------ ouvertures
    let mut openings: Vec<PlannedOpening> = Vec::new();
    for k in 0..n {
        let win = wins[k];
        for (i, &xc) in columns.iter().enumerate() {
            if tower_cols.contains(&i) {
                continue;
            }
            if door_cols.contains(&i) {
                if k == 0 {
                    openings.push(PlannedOpening {
                        door: true,
                        arch: Some(door_kind),
                        center: P2::new(xc, y0 + 0.5 * dh),
                        w: ds,
                        h: dh,
                        genome: door_genome.clone(),
                        floor: 0,
                        tower: None,
                        dormer: false,
                        special: false,
                        reach: door_reach,
                    });
                    continue;
                }
                if k == 1 && takes {
                    continue;
                }
                // au-dessus du portail : une baie d'apparat, le bas de son
                // décor au niveau de celui des croisées (au-dessus du cordon)
                let foot = floors[k] + win.sill - reach[k].down * win.w;
                let mut sw = Win {
                    sill: 0.0,
                    h: sp_h * win.h,
                    w: sp_ratio * sp_h * win.h,
                };
                let cap = 0.46 * bw / (0.5 + sp_reach.side);
                if sw.w > cap {
                    let f = cap / sw.w;
                    sw.w *= f;
                    sw.h *= f;
                }
                for _ in 0..3 {
                    let bot = foot + sp_reach.down * sw.w;
                    fit_window(&mut sw, lims[k] - bot, sp_reach.up);
                }
                let bot = foot + sp_reach.down * sw.w;
                if sw.h > 0.1 * hu && bot + sw.h + sp_reach.up * sw.w <= lims[k] + 1e-9 {
                    openings.push(PlannedOpening {
                        door: false,
                        arch: sp_tracery.then_some(sp_kind),
                        center: P2::new(xc, bot + 0.5 * sw.h),
                        w: sw.w,
                        h: sw.h,
                        genome: sp_genome.clone(),
                        floor: k,
                        tower: None,
                        dormer: false,
                        special: true,
                        reach: sp_reach,
                    });
                }
                continue;
            }
            if win.w < 0.05 * hu || win.h < 0.05 * hu {
                continue;
            }
            openings.push(PlannedOpening {
                door: false,
                arch: None,
                center: P2::new(xc, floors[k] + win.sill + 0.5 * win.h),
                w: win.w,
                h: win.h,
                genome: genomes[k].clone(),
                floor: k,
                tower: None,
                dormer: false,
                special: false,
                reach: reach[k],
            });
        }
    }
    // demi-encombrement du décor des ouvertures de chaque travée
    let col_half: Vec<f64> = columns
        .iter()
        .map(|&x| {
            openings
                .iter()
                .filter(|o| (o.center.x - x).abs() < 1e-9)
                .fold(0.0f64, |m, o| m.max((0.5 + o.reach.side) * o.w))
        })
        .collect();

    // ---------------------------------------------------- tours (suite)
    let mut towers: Vec<TowerPlan> = Vec::new();
    if !tower_cols.is_empty() {
        let mut width = (w - m_l - m_r) / nb as f64 * unif_in(&mut rng, 0.6, 0.78);
        // place laissée par le décor des ouvertures voisines (coyau de la
        // flèche compris)
        for &c in &tower_cols {
            for j in [c.wrapping_sub(1), c + 1] {
                if j < nb {
                    let d = (columns[c] - columns[j]).abs();
                    width = width.min((d - col_half[j] - 0.03 * hu) / 0.74);
                }
            }
        }
        let width = width.max(0.2 * hu);
        let sides = if unif(&mut rng) < 0.5 { 6 } else { 8 };
        let r = 0.5 * width / prism_half(sides);
        // un étage au-dessus de la corniche : les murs s'arrêtent vers le
        // faîtage du toit, auquel la tour donne accès
        let y_top = (yc0 + hu * unif_in(&mut rng, 0.92, 1.02)).min(yc1 + 0.95 * roof_h);
        let cornice = hu * unif_in(&mut rng, 0.18, 0.24);
        let top = if unif(&mut rng) < 0.85 {
            TowerTop::Spire
        } else {
            TowerTop::Crenels
        };
        // flèche haute et aiguë, évasée à la base
        let top_h = match top {
            TowerTop::Spire => width * unif_in(&mut rng, 2.0, 2.7),
            TowerTop::Crenels => hu * unif_in(&mut rng, 0.28, 0.36),
        };
        let band_h = 0.05 * hu;
        let tw_f = unif_in(&mut rng, 0.48, 0.6); // largeur des fenêtres / pan
        let t_ratio = unif_in(&mut rng, 1.5, 1.9);
        // allège de chaque niveau : les fenêtres montent avec la vis
        let stagger: Vec<f64> = (0..n).map(|_| unif_in(&mut rng, 0.18, 0.42)).collect();
        let with_door = unif(&mut rng) < 0.55;
        let d_f = unif_in(&mut rng, 0.55, 0.68);
        for (t, &c) in tower_cols.iter().enumerate() {
            let tp = TowerPlan {
                xc: columns[c],
                sides,
                r,
                width,
                y0,
                y_top,
                cornice,
                top,
                top_h,
                bands: floors[1..].to_vec(),
                band_h,
                corbel: 0.0,
                lift: 0,
            };
            let front = tp.front();
            // un niveau de plus que la façade
            for k in 0..=n {
                let (bot, lim, hk) = if k < n {
                    (
                        floors[k] + stagger[k] * height(k) + WIN_DOWN * front * tw_f,
                        floors[k + 1] - 0.5 * band_h - gap,
                        height(k),
                    )
                } else {
                    // étage haut : au-dessus des toits, sous les consoles
                    let tw = front * tw_f;
                    let bot = yc0 + 0.5 * band_h + WIN_DOWN * tw + 0.2 * hu;
                    (bot, y_top - 0.4 * cornice - gap, hu)
                };
                if k == 0 && with_door {
                    let mut s = front * d_f;
                    let mut th = (h0 * unif_in(&mut rng, 0.5, 0.6)).max((ha1 + 0.5) * s);
                    let need = th + DOOR_UP * s;
                    if need > lim - y0 {
                        let f = (lim - y0) / need;
                        s *= f;
                        th *= f;
                    }
                    openings.push(PlannedOpening {
                        door: true,
                        arch: Some(door_kind),
                        center: P2::new(tp.xc, y0 + 0.5 * th),
                        w: s,
                        h: th,
                        genome: door_genome.clone(),
                        floor: 0,
                        tower: Some(t),
                        dormer: false,
                        special: false,
                        reach: Reach::DOOR,
                    });
                    continue;
                }
                let tw = front * tw_f;
                let mut win = Win {
                    sill: 0.0,
                    h: (tw * t_ratio).min(0.42 * hk),
                    w: tw,
                };
                win.w = win.w.min(win.h / 1.2);
                fit_window(&mut win, lim - bot, WIN_UP);
                if win.w < 0.04 * hu || win.h < 0.06 * hu {
                    continue;
                }
                openings.push(PlannedOpening {
                    door: false,
                    arch: None,
                    center: P2::new(tp.xc, bot + 0.5 * win.h),
                    w: win.w,
                    h: win.h,
                    genome: genomes[k.min(n)].clone(),
                    floor: k,
                    tower: Some(t),
                    dormer: false,
                    special: false,
                    reach: Reach::WINDOW,
                });
            }
            towers.push(tp);
        }
    }

    // ----------------------------------------------------- échauguettes
    // aux angles (rares), en encorbellement : cul-de-lampe dans le dernier
    // étage, fût à pans qui traverse la corniche, poivrière ou créneaux
    let mut bartizans: Vec<TowerPlan> = Vec::new();
    if bart_l || bart_r {
        let sides = if unif(&mut rng) < 0.5 { 6 } else { 8 };
        let y_body = yc0 - hu * unif_in(&mut rng, 0.3, 0.42);
        let corbel = hu * unif_in(&mut rng, 0.32, 0.42);
        let y_top = (yc1 + hu * unif_in(&mut rng, 0.35, 0.5)).max(y1 + 0.2 * hu);
        let top = if unif(&mut rng) < 0.8 {
            TowerTop::Spire
        } else {
            TowerTop::Crenels
        };
        let top_h = match top {
            TowerTop::Spire => bart_w * unif_in(&mut rng, 1.4, 1.9),
            TowerTop::Crenels => hu * unif_in(&mut rng, 0.2, 0.26),
        };
        for (on, xc) in [(bart_l, x0 + 0.08 * bart_w), (bart_r, x1 - 0.08 * bart_w)] {
            if on {
                bartizans.push(TowerPlan {
                    xc,
                    sides,
                    r: 0.5 * bart_w / prism_half(sides),
                    width: bart_w,
                    y0: y_body,
                    y_top,
                    cornice: 0.14 * hu,
                    top,
                    top_h,
                    bands: Vec::new(),
                    band_h: 0.0,
                    corbel,
                    lift: 0,
                });
            }
        }
    }
    // échauguettes accrochées aux tours (deux tours sur cinq), sous leur
    // corniche, d'un côté où le décor de la travée voisine laisse la place
    for (ti, t) in towers.iter().enumerate() {
        if unif(&mut rng) >= 0.4 {
            continue;
        }
        let c = tower_cols[ti];
        let pref = unif(&mut rng) < 0.5;
        let y_tb = t.y_top - hu * unif_in(&mut rng, 0.66, 0.8);
        let y_b = (yc0 - hu * unif_in(&mut rng, 0.05, 0.2)).min(y_tb - 0.35 * hu);
        let corbel = hu * unif_in(&mut rng, 0.28, 0.36);
        let k_h = unif_in(&mut rng, 1.3, 1.6);
        for right in [pref, !pref] {
            let sg = if right { 1.0 } else { -1.0 };
            let j = if right { c + 1 } else { c.wrapping_sub(1) };
            let room = if j < nb {
                (columns[j] - t.xc).abs() - col_half[j] - 0.03 * hu
            } else {
                f64::INFINITY
            };
            let bwb = (0.42 * t.width).min((room - 0.5 * t.width) / 0.78);
            if bwb < 0.18 * hu {
                continue;
            }
            bartizans.push(TowerPlan {
                xc: t.xc + sg * (0.5 * t.width + 0.15 * bwb),
                sides: t.sides,
                r: 0.5 * bwb / prism_half(t.sides),
                width: bwb,
                y0: y_b,
                y_top: y_tb,
                cornice: 0.12 * hu,
                top: TowerTop::Spire,
                top_h: bwb * k_h,
                bands: Vec::new(),
                band_h: 0.0,
                corbel,
                lift: 1,
            });
            break;
        }
    }
    let bart_boxes: Vec<BBox> = bartizans.iter().map(|b| b.bbox()).collect();

    // -------------------------------------------------------- lucarnes
    // sur chaque travée sans tour, ou une sur deux, entre les pignons ;
    // leurs croisées ont toutes le même génome et la même taille
    let ridge = roof.y0 + roof.h;
    let mut dormers: Vec<Dormer> = Vec::new();
    let mut dormer_boxes: Vec<BBox> = Vec::new();
    {
        let every = nb < 4 || unif(&mut rng) < 0.65;
        let parity = (unif(&mut rng) < 0.5) as usize;
        let top = wins[n - 1];
        // croisée sans couronnement : le gâble en tient lieu, la baie
        // prend la largeur de celles du dernier étage
        let mut dw = (top.w * unif_in(&mut rng, 0.88, 1.0)).min(0.4 * bw);
        let ratio = unif_in(&mut rng, 1.25, 1.55);
        let g_r = unif_in(&mut rng, 0.6, 0.85);
        let mut lg = genomes[n].clone();
        lg.style.plain_top = true;
        let dr = measure(
            &rect_r(0.0, 0.0, 1000.0, 1000.0 * ratio),
            &lg,
            Reach {
                up: 0.04,
                ..Reach::WINDOW
            },
        );
        let base = y1;
        let room = ridge - base;
        let fixed = 0.05 * hu + 0.03 * hu;
        let k_front = 1.0 + 2.0 * dr.side + 0.16;
        let per = |dw: f64| fixed + dw * (dr.down + ratio + dr.up) + g_r * k_front * dw;
        if per(dw) > room {
            dw *= (room - fixed).max(0.0) / (per(dw) - fixed);
        }
        if dw > 0.1 * hu {
            let wd = k_front * dw;
            let pin_w = 0.16 * wd;
            let sill = base + 0.05 * hu + dr.down * dw;
            let dh = ratio * dw;
            let y_gable = sill + dh + dr.up * dw + 0.03 * hu;
            let (gl, gr) = (x0 + roof.gable + 0.02 * hu, x1 - roof.gable - 0.02 * hu);
            for (i, &xc) in columns.iter().enumerate() {
                if tower_cols.contains(&i) || (!every && i % 2 != parity) {
                    continue;
                }
                // entre les pignons, loin des échauguettes
                let half = 0.5 * wd + pin_w;
                let dbox = BBox::from_pts(
                    P2::new(xc - half, yc1),
                    P2::new(xc + half, y_gable + g_r * wd + 0.45 * wd),
                );
                if xc - half < gl || xc + half > gr || bart_boxes.iter().any(|b| overlap(b, &dbox))
                {
                    continue;
                }
                dormers.push(Dormer {
                    xc,
                    w: wd,
                    y0: yc1,
                    y_gable,
                    gable_h: g_r * wd,
                    pin_w,
                });
                dormer_boxes.push(dbox);
                openings.push(PlannedOpening {
                    door: false,
                    arch: None,
                    center: P2::new(xc, sill + 0.5 * dh),
                    w: dw,
                    h: dh,
                    genome: lg.clone(),
                    floor: n,
                    tower: None,
                    dormer: true,
                    special: false,
                    reach: dr,
                });
            }
        }
    }

    // ------------------------------------------------------- cheminées
    // Souche carrée à tablette, fûts ronds groupés à couronne évasée ;
    // sur les pignons et sur le toit, là où ni lucarne ni tour ne gêne.
    let mut chimneys: Vec<Chimney> = Vec::new();
    {
        let cw = hu * unif_in(&mut rng, 0.3, 0.38);
        let shafts = if unif(&mut rng) < 0.55 { 2 } else { 3 };
        let shaft_h = hu * unif_in(&mut rng, 0.5, 0.7);
        let on_gables = unif(&mut rng) < 0.45;
        let n_roof = weighted_index(&mut rng, &[0.4, 0.45, 0.15]).unwrap_or(1);
        let mut cands: Vec<(f64, f64)> = Vec::new(); // (abscisse, pied)
        if on_gables {
            // à l'aplomb du mur : la tablette (0,58 souche) s'arrête au nu du pignon
            for x in [x0 + 0.58 * cw, x1 - 0.58 * cw] {
                cands.push((x, ridge - 0.25 * hu));
            }
        }
        // sur le toit : au droit des trumeaux, puis des travées libres
        let mut spots: Vec<f64> = columns.windows(2).map(|c| 0.5 * (c[0] + c[1])).collect();
        spots.extend(columns.iter().copied());
        let mut order: Vec<usize> = (0..spots.len()).collect();
        for i in (1..order.len()).rev() {
            let j = ((unif(&mut rng) * (i + 1) as f64) as usize).min(i);
            order.swap(i, j);
        }
        let foot = ridge - roof.h * unif_in(&mut rng, 0.3, 0.45);
        for &i in order.iter().take(n_roof) {
            cands.push((spots[i], foot));
        }
        let base_top = ridge + 0.18 * hu;
        let mut boxes: Vec<BBox> = Vec::new();
        for (x, y_foot) in cands {
            let bx = BBox::from_pts(
                P2::new(x - 0.62 * cw, y_foot),
                P2::new(x + 0.62 * cw, base_top + shaft_h + 0.2 * hu),
            );
            let clear = !dormer_boxes.iter().any(|b| overlap(b, &bx))
                && !bart_boxes.iter().any(|b| overlap(b, &bx))
                && !towers.iter().any(|t| overlap(&t.bbox(), &bx))
                && !boxes.iter().any(|b| overlap(b, &bx));
            if clear {
                boxes.push(bx);
                chimneys.push(Chimney {
                    xc: x,
                    y0: y_foot,
                    base_top,
                    w: cw,
                    shafts,
                    shaft_top: base_top + shaft_h,
                });
            }
        }
    }

    // unité du rendu : demi-chambranle d'une croisée courante
    let typ = wins[if n > 1 { 1 } else { 0 }];
    let unit = 0.6 * 0.1 * typ.w.min(typ.h).max(0.02 * hu);
    FacadePlan {
        x0,
        x1,
        y0,
        y1,
        floors,
        columns,
        bay: (w - m_l - m_r) / nb as f64,
        storey: hu,
        plinth,
        cordons,
        cornice: (yc0, yc1),
        balustrade,
        roof,
        dormers,
        chimneys,
        towers,
        bartizans,
        openings,
        unit,
    }
}

/// Découpe des régions en pans : bandes verticales entre les abscisses
/// `xs` (croissantes).
fn facets(regs: &[Region], xs: &[f64], y_lo: f64, y_hi: f64) -> Vec<Region> {
    let mut v = Vec::new();
    for p in xs.windows(2) {
        let strip = Loop::polygon(&[
            P2::new(p[0], y_lo),
            P2::new(p[1], y_lo),
            P2::new(p[1], y_hi),
            P2::new(p[0], y_hi),
        ]);
        v.extend(intersect_loop(regs, &strip));
    }
    v
}

/// Arêtes d'une tour élargie de `o` (bandeaux, corniche) : homothétie des
/// sommets autour de l'axe.
fn scaled_edges(t: &TowerPlan, o: f64) -> Vec<f64> {
    let k = (0.5 * t.width + o) / (0.5 * t.width);
    t.edges().iter().map(|x| t.xc + (x - t.xc) * k).collect()
}

/// Bande horizontale à pans d'une tour : de `ya` à `yb`, débordant de `o`.
fn tower_band(t: &TowerPlan, o: f64, ya: f64, yb: f64) -> Vec<Region> {
    let e = scaled_edges(t, o);
    let (Some(&l), Some(&r)) = (e.first(), e.last()) else {
        return Vec::new();
    };
    facets(&[rect_r(l, ya, r, yb)], &e, ya - 1.0, yb + 1.0)
}

/// Rangée de consoles (modillons) à trois ressauts, sous `y_top`, de
/// hauteur `h`, centrées sur `xs`, de largeur `cw` en haut.
fn corbels(xs: &[f64], y_top: f64, h: f64, cw: f64) -> Vec<Region> {
    let mut v = Vec::new();
    for &x in xs {
        let tiers = [(0.6, 0.0, 0.34), (0.8, 0.34, 0.66), (1.0, 0.66, 1.0)];
        let mut c: Vec<Region> = Vec::new();
        for (wf, a, b) in tiers {
            let r = rect_r(
                x - 0.5 * wf * cw,
                y_top - h + a * h,
                x + 0.5 * wf * cw,
                y_top - h + b * h,
            );
            c = if c.is_empty() { vec![r] } else { union(&c, &r) };
        }
        // culot arrondi sous le premier ressaut
        c = union(&c, &Region::circle(P2::new(x, y_top - h), 0.3 * cw));
        v.extend(c);
    }
    v
}

/// Pans de la flèche (tour : coyau évasé à la base ; échauguette : cône
/// droit), pieds des arêtiers et pointe.
fn spire_faces(t: &TowerPlan) -> (Vec<Loop>, Vec<P2>, P2) {
    let yr = t.y_top + t.cornice;
    let apex = P2::new(t.xc, yr + t.top_h);
    if t.corbel > 0.0 {
        let ee = scaled_edges(t, 0.07 * t.width);
        let faces = ee
            .windows(2)
            .map(|q| Loop::polygon(&[P2::new(q[0], yr), P2::new(q[1], yr), apex]))
            .collect();
        let hips = ee[1..ee.len() - 1]
            .iter()
            .map(|&x| P2::new(x, yr))
            .collect();
        return (faces, hips, apex);
    }
    let eave = scaled_edges(t, 0.22 * t.width);
    let brk = scaled_edges(t, 0.06 * t.width);
    let hf = 0.16 * t.top_h;
    let yb = yr + hf;
    // coyau : courbe concave de l'égout à la cassure, sous sa corde
    let mid = |e: P2, b: P2| P2::new(e.x + 0.72 * (b.x - e.x), yr + 0.38 * hf);
    let faces = eave
        .windows(2)
        .zip(brk.windows(2))
        .map(|(e, b)| {
            let (e0, e1) = (P2::new(e[0], yr), P2::new(e[1], yr));
            let (b0, b1) = (P2::new(b[0], yb), P2::new(b[1], yb));
            Loop::new(vec![
                Seg::line(e0, e1),
                Seg::through(e1, mid(e1, b1), b1),
                Seg::line(b1, apex),
                Seg::line(apex, b0),
                Seg::through(b0, mid(e0, b0), e0),
            ])
            .oriented(true)
        })
        .collect();
    let hips = brk[1..brk.len() - 1]
        .iter()
        .map(|&x| P2::new(x, yb))
        .collect();
    (faces, hips, apex)
}

/// Silhouette d'une tour ou d'une échauguette : les moulures du mur, le
/// toit et les pignons s'y arrêtent.
fn silhouette(t: &TowerPlan, ground: f64) -> Vec<Region> {
    let (l, r) = (t.xc - 0.5 * t.width, t.xc + 0.5 * t.width);
    let o = 0.09 * t.width;
    let eps = 1e-6 * t.width;
    let yr = t.y_top + t.cornice;
    let mut v = if t.corbel > 0.0 {
        // fût et cul-de-lampe (trapèze jusqu'au culot)
        let yb = t.y0 - t.corbel;
        let q = 0.12 * t.width;
        union(
            &[rect_r(l, t.y0 - eps, r, t.y_top)],
            &Region::simple(Loop::polygon(&[
                P2::new(t.xc - q, yb),
                P2::new(t.xc + q, yb),
                P2::new(r, t.y0),
                P2::new(l, t.y0),
            ])),
        )
    } else {
        vec![rect_r(l, ground, r, t.y_top)]
    };
    v = union(&v, &rect_r(l - o, t.y_top - eps, r + o, yr));
    match t.top {
        TowerTop::Spire => {
            let (faces, _, _) = spire_faces(t);
            for f in faces {
                let f = Region::simple(f.transform(&Xf::translate(P2::new(0.0, -eps))));
                v = union(&v, &f);
            }
            v
        }
        TowerTop::Crenels => union(&v, &rect_r(l - o, yr - eps, r + o, yr + t.top_h)),
    }
}

/// Cheminée : souche à assises et tablette, fûts ronds groupés à assises,
/// couronnes évasées (congé, bandeau à rosace, listel), qui se rejoignent.
fn chimney_parts(c: &Chimney, hu: f64) -> Vec<(OrnKind, u8, Region)> {
    let mut v: Vec<(OrnKind, u8, Region)> = Vec::new();
    let (xc, w) = (c.xc, c.w);
    let lw = 0.004 * hu;
    v.push((
        OrnKind::Relief,
        2,
        rect_r(xc - 0.5 * w, c.y0, xc + 0.5 * w, c.base_top),
    ));
    let mut y = c.base_top - 0.14 * hu;
    while y > c.y0 + 0.05 * hu {
        v.push((
            OrnKind::Creux,
            1,
            rect_r(xc - 0.5 * w, y - lw, xc + 0.5 * w, y + lw),
        ));
        y -= 0.14 * hu;
    }
    let tab = c.base_top + 0.06 * hu;
    v.push((
        OrnKind::Relief,
        3,
        rect_r(
            xc - 0.53 * w,
            c.base_top - 0.025 * hu,
            xc + 0.53 * w,
            c.base_top,
        ),
    ));
    v.push((
        OrnKind::Relief,
        3,
        rect_r(xc - 0.58 * w, c.base_top, xc + 0.58 * w, tab),
    ));
    let span = 0.88 * w;
    let sw = span / c.shafts as f64;
    for i in 0..c.shafts {
        let x = xc - 0.5 * span + sw * (i as f64 + 0.5);
        v.push((
            OrnKind::Relief,
            2,
            rect_r(x - 0.46 * sw, tab, x + 0.46 * sw, c.shaft_top),
        ));
        let mut y = tab + 0.16 * hu;
        while y < c.shaft_top - 0.05 * hu {
            v.push((
                OrnKind::Creux,
                1,
                rect_r(x - 0.46 * sw, y - lw, x + 0.46 * sw, y + lw),
            ));
            y += 0.16 * hu;
        }
        // couronne : congé évasé, bandeau à rosace, listel
        let (yt, yc, yl) = (
            c.shaft_top,
            c.shaft_top + 0.07 * hu,
            c.shaft_top + 0.15 * hu,
        );
        v.push((
            OrnKind::Relief,
            3,
            Region::simple(Loop::polygon(&[
                P2::new(x - 0.46 * sw, yt),
                P2::new(x + 0.46 * sw, yt),
                P2::new(x + 0.72 * sw, yc),
                P2::new(x - 0.72 * sw, yc),
            ])),
        ));
        v.push((
            OrnKind::Relief,
            3,
            rect_r(x - 0.72 * sw, yc, x + 0.72 * sw, yl),
        ));
        v.push((
            OrnKind::Creux,
            2,
            quatrefoil(
                P2::new(x, 0.5 * (yc + yl)),
                (0.3 * (yl - yc)).min(0.22 * sw),
            ),
        ));
        v.push((
            OrnKind::Relief,
            3,
            rect_r(x - 0.66 * sw, yl, x + 0.66 * sw, yl + 0.03 * hu),
        ));
    }
    v
}

/// Quadrilobe de demi-encombrement `half`, centré en `c`.
fn quatrefoil(c: P2, half: f64) -> Region {
    let unit = motifs::shapes::foil_outline(P2::ZERO, 1.0, 4, 1.15);
    let bb = unit.bbox();
    let k = half / (0.5 * bb.size().x.max(bb.size().y)).max(1e-9);
    Region::simple(unit.transform(&Xf::scale(k)).transform(&Xf::translate(c)))
}

/// Façade gothique : décor du mur (les ouvertures sont des formes à part).
pub fn generate_facade(
    shape: &Region,
    genome: &Genome,
    cancel: &CancelToken,
) -> Result<Design, GenError> {
    #[cfg(not(target_arch = "wasm32"))]
    let t0 = std::time::Instant::now();
    let shape = Region::new(shape.outer.clone(), shape.holes.clone());
    let bb = shape.bbox();
    let p = plan(&bb, genome);
    let (x0, x1, y0) = (p.x0, p.x1, p.y0);
    let w = x1 - x0;
    let hu = p.storey;
    let (yc0, yc1) = p.cornice;
    let mut rng = ChaCha8Rng::seed_from_u64(derived(genome.seed, 0xFACB));
    let mut out = Out {
        ornaments: Vec::new(),
        openings: Vec::new(),
        construction: Vec::new(),
        nodes: Vec::new(),
        min_area: 1e-9 * w * (p.y1 - y0),
    };
    let cancelled = || {
        if cancel.is_cancelled() {
            Err(GenError::Cancelled)
        } else {
            Ok(())
        }
    };
    let big = 10.0 * (w + p.y1 - y0);
    // les tours passent devant le mur, ses moulures s'y arrêtent
    let tower_cuts: Vec<Vec<Region>> = p
        .towers
        .iter()
        .chain(&p.bartizans)
        .map(|t| silhouette(t, y0 - big))
        .collect();
    let cut = |regs: Vec<Region>| -> Vec<Region> {
        let mut v = regs;
        for c in &tower_cuts {
            for r in c {
                v = subtract(&v, r);
            }
        }
        v
    };

    // ------------------------------------------------------------- mur
    let p_wall = out.node("Mur");
    out.relief(&p_wall, 0, [rect_r(x0, y0, x1, yc1)]);

    // ---------------------------------------------------- soubassement
    if p.plinth > 0.0 {
        let p_pl = out.node("Soubassement");
        let ph = p.plinth;
        let mut regs = vec![rect_r(x0, y0, x1, y0 + 0.8 * ph)];
        let mut top = vec![rect_r(x0, y0 + 0.8 * ph, x1, y0 + ph)];
        // interrompu au droit des portes
        for o in p.openings.iter().filter(|o| o.door && o.tower.is_none()) {
            let b = o.decor_box();
            let r = rect_r(b.min.x, y0 - 1.0, b.max.x, y0 + 2.0 * ph);
            regs = subtract(&regs, &r);
            top = subtract(&top, &r);
        }
        out.relief(&p_pl, 1, cut(regs));
        out.relief(&p_pl, 2, cut(top));
    }

    // --------------------------------------------------------- cordons
    if !p.cordons.is_empty() {
        let p_co = out.node("Cordons");
        // interrompus au droit des portails qui montent jusqu'à eux
        let doors: Vec<Region> = p
            .openings
            .iter()
            .filter(|o| o.door && o.tower.is_none())
            .map(|o| {
                let b = o.decor_box();
                rect_r(b.min.x, b.min.y, b.max.x, b.max.y + 0.02 * hu)
            })
            .collect();
        let gaps = |v: Vec<Region>| doors.iter().fold(v, |a, d| subtract(&a, d));
        for &(b, t) in &p.cordons {
            let h = t - b;
            out.relief(&p_co, 2, cut(gaps(vec![rect_r(x0, b, x1, t)])));
            // gorge sous le larmier
            out.creux(
                &p_co,
                1,
                cut(gaps(vec![rect_r(x0, b + 0.22 * h, x1, b + 0.4 * h)])),
            );
        }
    }
    cancelled()?;

    // -------------------------------------------------------- corniche
    let ch = yc1 - yc0;
    {
        let p_frieze = out.node("Frise");
        out.relief(&p_frieze, 1, cut(vec![rect_r(x0, yc0, x1, yc0 + 0.5 * ch)]));
        // modillons réguliers ; arcature aveugle entre eux
        let sp_t = ch * unif_in(&mut rng, 0.6, 0.8);
        let nc = (round(w / sp_t) as usize).max(2);
        let sp = w / nc as f64;
        let cw = 0.36 * sp;
        let xs: Vec<f64> = (0..nc).map(|i| x0 + sp * (i as f64 + 0.5)).collect();
        let arcs = unif(&mut rng) < 0.75;
        if arcs {
            let p_arc = out.node("Arcature");
            let k = ArchKind::Pointed {
                rho: if unif(&mut rng) < 0.5 { 1.0 } else { 1.2 },
            };
            let s = 0.62 * (sp - cw);
            let ha = motifs::arch_curve(k, s).height;
            let (ya, yb) = (yc0 + 0.08 * ch, yc0 + 0.42 * ch);
            let (s, jamb) = if ha > yb - ya {
                let f = (yb - ya) / ha * 0.9;
                (s * f, 0.0)
            } else {
                (s, yb - ya - ha)
            };
            let ha = motifs::arch_curve(k, s).height;
            let mut regs = Vec::new();
            for i in 0..nc.saturating_sub(1) {
                let xm = 0.5 * (xs[i] + xs[i + 1]);
                let (l, _) = arch_window(k, s, jamb);
                regs.push(Region::simple(
                    l.transform(&Xf::translate(P2::new(xm, yb - ha))),
                ));
            }
            out.creux(&p_arc, 2, cut(regs));
        }
        let p_mod = out.node("Modillons");
        out.relief(&p_mod, 2, cut(corbels(&xs, yc0 + 0.55 * ch, 0.47 * ch, cw)));
        let p_cor = out.node("Corniche");
        let (o1, o2) = (0.12 * ch, 0.2 * ch);
        out.relief(
            &p_cor,
            2,
            cut(vec![rect_r(
                x0 - o1,
                yc0 + 0.55 * ch,
                x1 + o1,
                yc0 + 0.82 * ch,
            )]),
        );
        out.creux(
            &p_cor,
            1,
            cut(vec![rect_r(
                x0 - o1,
                yc0 + 0.62 * ch,
                x1 + o1,
                yc0 + 0.7 * ch,
            )]),
        );
        out.relief(
            &p_cor,
            3,
            cut(vec![rect_r(x0 - o2, yc0 + 0.82 * ch, x1 + o2, yc1)]),
        );
    }
    cancelled()?;

    // ------------------------------------------------------------- toit
    // deux versants entre deux pignons de pierre vus de chant
    let ridge = p.roof.y0 + p.roof.h;
    let chimney_rects: Vec<Region> = p
        .chimneys
        .iter()
        .map(|c| {
            rect_r(
                c.xc - 0.62 * c.w,
                c.y0,
                c.xc + 0.62 * c.w,
                c.shaft_top + 0.2 * hu,
            )
        })
        .collect();
    {
        let rf = p.roof;
        let g = rf.gable;
        let p_roof = out.node("Toit");
        let (ya, yb) = (rf.y0, ridge);
        let (la, ra) = (x0 + g, x1 - g);
        let face = Loop::polygon(&[
            P2::new(la, ya),
            P2::new(ra, ya),
            P2::new(ra, yb),
            P2::new(la, yb),
        ]);
        let mut roof_regs = vec![Region::simple(face.clone())];
        for d in &p.dormers {
            roof_regs = subtract(&roof_regs, &d.silhouette());
        }
        out.relief(&p_roof, 1, cut(roof_regs));
        // assises d'ardoise
        let nrow = (round(rf.h / (0.07 * hu)) as usize).clamp(6, 30);
        let lw = 0.004 * hu;
        let mut rows = Vec::new();
        for i in 1..nrow {
            let y = ya + rf.h * i as f64 / nrow as f64;
            let strip = rect_r(la - 1.0, y - lw, ra + 1.0, y + lw);
            rows.extend(intersect_loop(std::slice::from_ref(&strip), &face));
        }
        for d in &p.dormers {
            rows = subtract(&rows, &d.silhouette());
        }
        for c in &chimney_rects {
            rows = subtract(&rows, c);
        }
        out.creux(&p_roof, 1, cut(rows));
        // crête de faîtage
        let p_cr = out.node("Faîtage");
        let hc = 0.05 * hu;
        out.relief(&p_cr, 2, cut(vec![rect_r(la, yb, ra, yb + hc)]));
        // pignons : chaperon vu de chant, crochets sur l'arête, épi au
        // sommet (sauf sous une cheminée)
        let p_pg = out.node("Pignons");
        let coping = 0.08 * hu;
        for (xl, sg) in [(x0, -1.0), (x1 - g, 1.0)] {
            out.relief(&p_pg, 2, cut(vec![rect_r(xl, yc1, xl + g, yb + coping)]));
            let x_out = if sg < 0.0 { xl } else { xl + g };
            let mut y = yc1 + 0.35 * hu;
            let mut leaves = Vec::new();
            while y < yb - 0.1 * hu {
                leaves.extend(chou(
                    P2::new(x_out, y),
                    P2::new(sg, 0.0),
                    P2::new(0.0, 1.0),
                    0.1 * hu,
                ));
                y += 0.24 * hu;
            }
            out.relief(&p_pg, 3, cut(leaves));
            let xm = xl + 0.5 * g;
            if !p.chimneys.iter().any(|c| (c.xc - xm).abs() < c.w) {
                for (kind, lv, reg) in cross_finial(P2::new(xm, yb + coping), 0.3 * hu) {
                    out.add(&p_pg, kind, lv, cut(vec![reg]));
                }
            }
        }
    }
    cancelled()?;

    // -------------------------------------------------------- cheminées
    if !p.chimneys.is_empty() {
        let p_ch = out.node("Cheminées");
        for c in &p.chimneys {
            for (kind, lv, reg) in chimney_parts(c, hu) {
                out.add(&p_ch, kind, lv, cut(vec![reg]));
            }
        }
    }
    cancelled()?;

    // -------------------------------------------------------- lucarnes
    if !p.dormers.is_empty() {
        let p_d = out.node("Lucarnes");
        let p_g = out.node("Lucarnes : gâbles");
        let p_p = out.node("Lucarnes : pinacles");
        for d in &p.dormers {
            let (l, r) = (d.xc - 0.5 * d.w, d.xc + 0.5 * d.w);
            let m = Xf::mirror_x(d.xc);
            // front et jambages
            out.relief(&p_d, 1, [d.silhouette()]);
            let jw = 0.08 * d.w;
            out.relief(
                &p_d,
                2,
                [
                    rect_r(l, p.y1, l + jw, d.y_gable),
                    rect_r(r - jw, p.y1, r, d.y_gable),
                ],
            );
            // gâble : rampants moulurés débordants, trilobe aveugle, épi
            let apex = P2::new(d.xc, d.y_gable + d.gable_h);
            let t = 0.09 * d.w;
            let o = 0.06 * d.w;
            let k = d.gable_h / (0.5 * d.w);
            let lift = P2::new(0.0, t * (1.0 + k * k).sqrt());
            let rampant = |side: f64| {
                let a = P2::new(d.xc + side * (0.5 * d.w + o), d.y_gable - o * k);
                Region::simple(Loop::polygon(&[a, apex, apex + lift, a + lift]).oriented(true))
            };
            out.relief(&p_g, 2, [rampant(-1.0), rampant(1.0)]);
            out.relief(
                &p_g,
                2,
                [rect_r(l - o, d.y_gable - 0.6 * t, r + o, d.y_gable)],
            );
            let c = P2::new(d.xc, d.y_gable + 0.36 * d.gable_h);
            let u = motifs::shapes::foil_outline(P2::ZERO, 1.0, 3, 1.15);
            let ub = u.bbox();
            let kk = 0.2 * d.gable_h / (0.5 * ub.size().x.max(ub.size().y)).max(1e-9);
            let tre = Region::simple(u.transform(&Xf::scale(kk)).transform(&Xf::translate(c)));
            out.creux(&p_g, 2, [tre]);
            for (kind, lv, reg) in cross_finial(apex + lift, 0.3 * d.w) {
                out.add(&p_g, kind, lv, [reg]);
            }
            // pinacles de part et d'autre
            let x_pin = l - 0.5 * d.pin_w;
            let hb = d.pin_w * unif_in(&mut rng, 0.8, 1.2);
            let y_pt = d.y_gable + 0.15 * d.gable_h;
            let left = crate::door::pinnacle(&mut rng, x_pin, d.pin_w, p.y1, p.y1 + hb, y_pt);
            for (kind, lv, reg) in left {
                let rr = reg.transform(&m);
                out.add(&p_p, kind, lv, [reg, rr]);
            }
        }
    }
    cancelled()?;

    // ------------------------------------------------------ balustrade
    if let Some((ba, bb_)) = p.balustrade {
        let p_bal = out.node("Balustrade");
        let bh = bb_ - ba;
        let rail = 0.16 * bh;
        let (pa, pb) = (ba + rail, bb_ - rail);
        let ph = pb - pa;
        let ncell = (round(w / ph) as usize).max(1);
        let cwid = w / ncell as f64;
        let mut panel = vec![rect_r(x0, pa, x1, pb)];
        for i in 0..ncell {
            let c = P2::new(x0 + cwid * (i as f64 + 0.5), 0.5 * (pa + pb));
            panel = subtract(&panel, &quatrefoil(c, 0.4 * cwid.min(ph)));
        }
        out.relief(&p_bal, 2, cut(vec![rect_r(x0, ba, x1, pa)]));
        out.relief(&p_bal, 2, cut(panel));
        // dés tous les quatre jours, main courante
        let step = if ncell >= 8 { 4 } else { 2 };
        let mut posts = Vec::new();
        for i in (0..=ncell).step_by(step) {
            let x = x0 + cwid * i as f64;
            let hw = 0.09 * cwid;
            posts.push(rect_r((x - hw).max(x0), pa, (x + hw).min(x1), pb));
        }
        out.relief(&p_bal, 3, cut(posts));
        out.relief(
            &p_bal,
            3,
            cut(vec![rect_r(x0 - 0.3 * rail, pb, x1 + 0.3 * rail, bb_)]),
        );
    }
    cancelled()?;

    // ------------------------------------------------------------ tours
    let nt = p.towers.len();
    for (ti, t) in p.towers.iter().chain(&p.bartizans).enumerate() {
        let label = if ti < nt {
            format!("Tour {}", ti + 1)
        } else {
            format!("Échauguette {}", ti - nt + 1)
        };
        let p_t = out.node(&label);
        let e = t.edges();
        let (l, r) = (t.xc - 0.5 * t.width, t.xc + 0.5 * t.width);
        // niveaux de relief (une échauguette accrochée passe devant sa tour)
        let lv = |k: u8| (k + t.lift).min(3);
        // les bandeaux et les arêtes d'une tour s'arrêtent à ses échauguettes
        let shield: Vec<Region> = if t.corbel > 0.0 {
            Vec::new()
        } else {
            p.bartizans
                .iter()
                .filter(|b| b.lift > 0 && (b.xc - t.xc).abs() < t.width)
                .flat_map(|b| silhouette(b, y0 - big))
                .collect()
        };
        let minus = |v: Vec<Region>| shield.iter().fold(v, |a, s| subtract(&a, s));
        // pans
        out.relief(
            &p_t,
            lv(1),
            facets(
                &[rect_r(l, t.y0, r, t.y_top)],
                &e,
                t.y0 - 1.0,
                t.y_top + 1.0,
            ),
        );
        // arêtes, hors du décor des ouvertures de la tour
        let lw = 0.006 * t.width;
        let mut ar: Vec<Region> = e[1..e.len().saturating_sub(1)]
            .iter()
            .map(|&x| rect_r(x - lw, t.y0, x + lw, t.y_top))
            .collect();
        for o in p.openings.iter().filter(|o| o.tower == Some(ti)) {
            let b = o.decor_box();
            ar = subtract(&ar, &rect_r(b.min.x, b.min.y, b.max.x, b.max.y));
        }
        out.creux(&p_t, 1, minus(ar));
        if t.corbel > 0.0 {
            // cul-de-lampe : cône à pans qui se resserre jusqu'au culot,
            // cerclé de trois moulures ; archère sur le pan de face
            let p_cl = out.node(&format!("{label} : cul-de-lampe"));
            let yb = t.y0 - t.corbel;
            let k_bot = 0.22;
            let top_e = t.edges();
            let bot_e: Vec<f64> = top_e.iter().map(|x| t.xc + (x - t.xc) * k_bot).collect();
            let cone: Vec<Region> = top_e
                .windows(2)
                .zip(bot_e.windows(2))
                .map(|(q, b)| {
                    Region::simple(Loop::polygon(&[
                        P2::new(b[0], yb),
                        P2::new(b[1], yb),
                        P2::new(q[1], t.y0),
                        P2::new(q[0], t.y0),
                    ]))
                })
                .collect();
            out.relief(&p_cl, lv(1), cone);
            let hm = 0.09 * t.corbel;
            for f in [0.0, 0.38, 0.72] {
                let y = t.y0 - f * t.corbel;
                // largeur du cône à cette hauteur, plus le débord de la moulure
                let kw = 1.0 - (1.0 - k_bot) * f;
                let o = (kw - 1.0) * 0.5 * t.width + 0.05 * t.width;
                out.relief(&p_cl, lv(2), tower_band(t, o, y - hm, y));
            }
            out.relief(
                &p_cl,
                lv(3),
                [Region::circle(
                    P2::new(t.xc, t.y0 - t.corbel),
                    0.09 * t.width,
                )],
            );
            let hs = 0.32 * (t.y_top - t.y0);
            let ym = 0.5 * (t.y0 + t.y_top);
            out.creux(
                &p_t,
                2,
                [rect_r(
                    t.xc - 0.035 * t.width,
                    ym - 0.5 * hs,
                    t.xc + 0.035 * t.width,
                    ym + 0.5 * hs,
                )],
            );
        }
        // base et bandeaux
        let p_b = out.node(&format!("{label} : bandeaux"));
        if t.corbel <= 0.0 {
            let base_h = p.plinth.max(0.07 * hu) * 1.1;
            out.relief(
                &p_b,
                2,
                minus(tower_band(t, 0.05 * t.width, t.y0, t.y0 + base_h)),
            );
        }
        for &yb in &t.bands {
            out.relief(
                &p_b,
                2,
                minus(tower_band(
                    t,
                    0.035 * t.width,
                    yb - 0.5 * t.band_h,
                    yb + 0.5 * t.band_h,
                )),
            );
        }
        // corniche : consoles, bandeau, larmier
        let p_c = out.node(&format!("{label} : corniche"));
        let tc = t.cornice;
        let nk = match (t.corbel > 0.0, t.sides == 6) {
            (true, true) => 3,
            (true, false) => 4,
            (false, true) => 5,
            (false, false) => 7,
        };
        let xs: Vec<f64> = (0..nk)
            .map(|i| l + t.width * (i as f64 + 0.5) / nk as f64)
            .collect();
        out.relief(
            &p_c,
            lv(2),
            corbels(&xs, t.y_top, 0.4 * tc, 0.42 * t.width / nk as f64),
        );
        out.relief(
            &p_c,
            lv(2),
            tower_band(t, 0.06 * t.width, t.y_top, t.y_top + 0.6 * tc),
        );
        out.relief(
            &p_c,
            3,
            tower_band(t, 0.09 * t.width, t.y_top + 0.6 * tc, t.y_top + tc),
        );
        let yr = t.y_top + tc;
        match t.top {
            TowerTop::Spire => {
                let p_r = out.node(&format!("{label} : flèche"));
                let (faces, hips, apex) = spire_faces(t);
                out.relief(&p_r, lv(2), faces.iter().cloned().map(Region::simple));
                // assises d'ardoise et arêtiers
                let nrow = (round(t.top_h / (0.09 * t.width)) as usize).clamp(6, 24);
                let lw2 = 0.004 * t.width;
                let mut rows = Vec::new();
                for i in 1..nrow {
                    let y = yr + t.top_h * i as f64 / nrow as f64;
                    let strip = rect_r(l - t.width, y - lw2, r + t.width, y + lw2);
                    for f in &faces {
                        rows.extend(intersect_loop(std::slice::from_ref(&strip), f));
                    }
                }
                for &a in &hips {
                    let n = perp(unit(apex - a)) * (1.5 * lw2);
                    rows.push(Region::simple(
                        Loop::polygon(&[a - n, a + n, apex + n, apex - n]).oriented(true),
                    ));
                }
                out.creux(&p_r, 1, rows);
                let p_f = out.node(&format!("{label} : épi"));
                for (kind, lv, reg) in cross_finial(apex, 0.3 * t.width) {
                    out.add(&p_f, kind, lv, [reg]);
                }
            }
            TowerTop::Crenels => {
                let p_r = out.node(&format!("{label} : créneaux"));
                let ee = scaled_edges(t, 0.06 * t.width);
                let (pl, pr) = (ee[0], ee[ee.len() - 1]);
                let hp = t.top_h;
                let nm = if t.sides == 6 { 4 } else { 5 };
                let mw = (pr - pl) / (2 * nm - 1) as f64;
                let mut par = vec![rect_r(pl, yr, pr, yr + 0.55 * hp)];
                for i in 0..nm {
                    let xa = pl + 2.0 * mw * i as f64;
                    par = union(&par, &rect_r(xa, yr + 0.5 * hp, xa + mw, yr + hp));
                }
                out.relief(&p_r, lv(2), facets(&par, &ee, yr - 1.0, yr + 2.0 * hp));
            }
        }
        cancelled()?;
    }

    // ---------------------------------------------------- construction
    let axis = |a: P2, b: P2| ConstructionEntity::Line {
        a,
        b,
        op: ConstructionOp::Axis,
    };
    for &x in &p.columns {
        out.construction.push(axis(P2::new(x, y0), P2::new(x, yc1)));
    }
    for &y in &p.floors {
        out.construction.push(axis(P2::new(x0, y), P2::new(x1, y)));
    }

    // ------------------------------------------------------------ arbre
    let root_bb = out
        .ornaments
        .iter()
        .fold(shape.bbox(), |b, o| b.union(&o.region.bbox()));
    let mut tree = vec![TreeNode {
        path: "0".into(),
        depth: 0,
        rule: "Façade".into(),
        params: Params::new()
            .with("étages", (p.floors.len() - 1) as f64)
            .with("travées", p.columns.len() as f64)
            .with("tours", p.towers.len() as f64)
            .with("étage courant", hu),
        types: vec!["façade".into()],
        area: shape.area(),
        inscribed: 0.0,
        local_score: 1.0,
        twin_of: None,
        bbox: root_bb,
        locked: false,
        ratio: Some((p.y1 - y0) / w.max(1e-9)),
        candidates: 1,
        region: Some(shape.clone()),
    }];
    for (path, name) in &out.nodes {
        let regs: Vec<&Region> = out
            .ornaments
            .iter()
            .filter(|o| &o.path == path)
            .map(|o| &o.region)
            .collect();
        let nb = regs.iter().fold(BBox::EMPTY, |b, r| b.union(&r.bbox()));
        if nb.is_empty() {
            continue;
        }
        tree.push(TreeNode {
            path: path.clone(),
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
    terms.insert("façade".to_string(), 1.0);
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
        widths: vec![p.unit, p.unit],
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
    use crate::hidden::door_drawing;
    use crate::validate::validate;

    fn facade(seed: u64) -> Genome {
        Genome::new(
            seed,
            StyleParams {
                facade: true,
                ..StyleParams::for_family(crate::Family::Flamboyant)
            },
        )
    }

    fn bb(w: f64, h: f64) -> BBox {
        BBox::from_pts(P2::new(-0.5 * w, 0.0), P2::new(0.5 * w, h))
    }

    #[test]
    fn aretes_des_tours() {
        let h = tower_edges(6, 1.0);
        assert_eq!(h.len(), 4);
        assert!((h[0] + 1.0).abs() < 1e-9 && (h[1] + 0.5).abs() < 1e-9);
        let o = tower_edges(8, 1.0);
        assert_eq!(o.len(), 4);
        assert!((o[3] - cos(PI / 8.0)).abs() < 1e-9);
        assert!((o[2] - sin(PI / 8.0)).abs() < 1e-9);
    }

    /// Les ouvertures (et leur décor) ne se chevauchent pas, restent dans
    /// la façade ou sa tour, et ne touchent ni cordons ni corniche ; les
    /// fenêtres d'un même niveau de la façade ont même génome et même
    /// taille.
    #[test]
    fn plans_coherents() {
        let mut towers = 0;
        let mut dormers = 0;
        let mut bartizans = 0;
        let mut corners = 0;
        let mut chimneys = 0;
        let mut specials = 0;
        let (mut doors, mut door_w, mut wins, mut win_h) = (0, 0.0, 0, 0.0);
        let mut irregular = 0;
        for (w, h) in [
            (24000.0, 14000.0),
            (16000.0, 13000.0),
            (9000.0, 12000.0),
            (30000.0, 9000.0),
        ] {
            for seed in 0..40 {
                let p = plan(&bb(w, h), &facade(seed));
                towers += p.towers.len();
                assert!(
                    p.openings.iter().any(|o| o.door),
                    "{w}×{h} seed {seed} : pas de porte"
                );
                let boxes: Vec<BBox> = p.openings.iter().map(|o| o.decor_box()).collect();
                for (i, a) in boxes.iter().enumerate() {
                    for b in &boxes[i + 1..] {
                        let ov = a.min.x < b.max.x - 1e-6
                            && b.min.x < a.max.x - 1e-6
                            && a.min.y < b.max.y - 1e-6
                            && b.min.y < a.max.y - 1e-6;
                        assert!(!ov, "{w}×{h} seed {seed} : décors chevauchés {a:?} {b:?}");
                    }
                    for &(cb, ct) in &p.cordons {
                        // les portails interrompent les cordons
                        if p.openings[i].tower.is_none() && !p.openings[i].door {
                            assert!(
                                a.max.y <= cb + 1e-6 || a.min.y >= ct - 1e-6,
                                "{w}×{h} seed {seed} : cordon traversé"
                            );
                        }
                    }
                    if p.openings[i].dormer {
                        assert!(a.min.y >= p.cornice.1 - 1e-6, "lucarne sous la corniche");
                    } else if p.openings[i].tower.is_none() {
                        assert!(a.max.y <= p.cornice.0 + 1e-6, "corniche touchée");
                    } else {
                        let t = &p.towers[p.openings[i].tower.unwrap()];
                        assert!(a.max.y <= t.y_top + 1e-6, "corniche de tour touchée");
                    }
                }
                // même niveau : même génome, même taille
                let plain = |o: &&PlannedOpening| !o.door && !o.special && o.tower.is_none();
                for o in p.openings.iter().filter(plain) {
                    for q in p
                        .openings
                        .iter()
                        .filter(plain)
                        .filter(|q| q.floor == o.floor)
                    {
                        assert_eq!(o.genome, q.genome);
                        assert!((o.w - q.w).abs() < 1e-9 && (o.h - q.h).abs() < 1e-9);
                        assert!((o.center.y - q.center.y).abs() < 1e-9);
                    }
                }
                // lucarnes : la croisée et son décor tiennent sous le gâble
                for o in p.openings.iter().filter(|o| o.dormer) {
                    let d = p
                        .dormers
                        .iter()
                        .find(|d| (d.xc - o.center.x).abs() < 1e-9)
                        .expect("lucarne de la croisée");
                    let b = o.decor_box();
                    assert!(b.max.y <= d.y_gable + 1e-6);
                    assert!(b.min.x >= d.xc - 0.5 * d.w - 1e-6);
                    assert!(b.max.x <= d.xc + 0.5 * d.w + 1e-6);
                    assert!(b.min.y >= p.y1 - 1e-6);
                }
                dormers += p.dormers.len();
                // échauguettes : rien ne passe sous leur silhouette
                for t in &p.bartizans {
                    let s = BBox::from_pts(
                        P2::new(t.xc - 0.6 * t.width, t.y0 - t.corbel),
                        P2::new(t.xc + 0.6 * t.width, t.y_top + t.cornice + t.top_h),
                    );
                    for b in &boxes {
                        let ov = s.min.x < b.max.x - 1e-6
                            && b.min.x < s.max.x - 1e-6
                            && s.min.y < b.max.y - 1e-6
                            && b.min.y < s.max.y - 1e-6;
                        assert!(!ov, "{w}×{h} seed {seed} : échauguette sur une ouverture");
                    }
                }
                bartizans += p.bartizans.len();
                corners += p.bartizans.iter().filter(|b| b.lift == 0).count();
                chimneys += p.chimneys.len();
                specials += p.openings.iter().filter(|o| o.special).count();
                for o in &p.openings {
                    if o.door && o.tower.is_none() {
                        doors += 1;
                        door_w += o.w / p.storey;
                    } else if !o.door && !o.special && o.tower.is_none() && !o.dormer && o.floor > 0
                    {
                        wins += 1;
                        win_h += o.h / p.storey;
                    }
                }
                // lucarnes et cheminées : loin des échauguettes et des tours
                for t in p.bartizans.iter().chain(&p.towers) {
                    let tb = t.bbox();
                    for d in &p.dormers {
                        let db = BBox::from_pts(
                            P2::new(d.xc - 0.5 * d.w - d.pin_w, d.y0),
                            P2::new(d.xc + 0.5 * d.w + d.pin_w, d.y_gable + d.gable_h),
                        );
                        if t.corbel > 0.0 {
                            assert!(
                                !overlap(&tb, &db),
                                "{w}×{h} seed {seed} : lucarne sous une échauguette"
                            );
                        }
                    }
                    for c in &p.chimneys {
                        // dans l'emprise du bâtiment
                        assert!(
                            c.xc - 0.58 * c.w >= p.x0 - 1e-6 && c.xc + 0.58 * c.w <= p.x1 + 1e-6
                        );
                        let cb = BBox::from_pts(
                            P2::new(c.xc - 0.6 * c.w, c.y0),
                            P2::new(c.xc + 0.6 * c.w, c.shaft_top),
                        );
                        assert!(
                            !overlap(&tb, &cb),
                            "{w}×{h} seed {seed} : cheminée dans une tour"
                        );
                    }
                }
                // travées irrégulières
                if p.columns.len() >= 3 {
                    let g: Vec<f64> = p.columns.windows(2).map(|c| c[1] - c[0]).collect();
                    let spread = g.iter().fold(0.0f64, |m, x| m.max(*x))
                        - g.iter().fold(f64::INFINITY, |m, x| m.min(*x));
                    if spread > 0.05 * p.bay {
                        irregular += 1;
                    }
                }
                // tours : un étage de plus que la façade
                for t in &p.towers {
                    assert!(t.y_top >= p.cornice.0 + 0.9 * p.storey);
                    assert!(t.y_top > p.y1);
                    // les tours ne dépassent guère le faîtage
                    assert!(t.y_top <= p.roof.y0 + p.roof.h + 1e-6);
                    assert!(t.sides == 6 || t.sides == 8);
                }
            }
        }
        assert!(towers > 20, "trop peu de tours : {towers}");
        assert!(dormers > 100, "trop peu de lucarnes : {dormers}");
        assert!(bartizans > 25, "trop peu d'échauguettes : {bartizans}");
        assert!(corners < 75, "trop d'échauguettes d'angle : {corners}");
        assert!(chimneys > 60, "trop peu de cheminées : {chimneys}");
        assert!(specials > 100, "trop peu de baies d'apparat : {specials}");
        let door_w = door_w / doors.max(1) as f64;
        assert!(door_w > 0.38, "portails trop étroits : {door_w}");
        let win_h = win_h / wins.max(1) as f64;
        assert!(win_h > 0.48, "fenêtres trop petites : {win_h}");
        assert!(irregular > 80, "travées trop régulières : {irregular}");
    }

    #[test]
    fn reglages() {
        let mut g = facade(3);
        g.style.floors = 4;
        g.style.bays = 7;
        g.style.towers = Some(0);
        g.style.cordons = Some(true);
        let p = plan(&bb(20000.0, 16000.0), &g);
        assert_eq!(p.floors.len(), 5);
        assert_eq!(p.columns.len(), 7);
        // les tours prennent des travées : sept colonnes en tout
        for (floors, bays) in [(2u8, 3u8), (5, 9), (1, 1), (6, 12)] {
            let mut g = facade(11);
            g.style.floors = floors;
            g.style.bays = bays;
            g.style.towers = Some(2);
            let p = plan(&bb(20000.0, 16000.0), &g);
            assert_eq!(p.floors.len(), floors as usize + 1);
            assert_eq!(p.columns.len(), bays as usize);
            for (i, &x) in p.columns.iter().enumerate() {
                let tower = p.towers.iter().any(|t| (t.xc - x).abs() < 1e-9);
                let wins = p
                    .openings
                    .iter()
                    .any(|o| o.tower.is_none() && (o.center.x - x).abs() < 1e-9);
                assert!(tower || wins, "travée {i} vide ({floors} × {bays})");
            }
        }
        assert!(p.towers.is_empty());
        assert_eq!(p.cordons.len(), 3);
        g.style.towers = Some(2);
        g.style.cordons = Some(false);
        let p = plan(&bb(20000.0, 16000.0), &g);
        assert_eq!(p.towers.len(), 2);
        assert!(p.cordons.is_empty());
        // tours sur deux travées distinctes
        let (a, b) = (&p.towers[0], &p.towers[1]);
        assert!(a.xc < b.xc);
    }

    #[test]
    fn decor_valide_et_deterministe() {
        let shape = Region::simple(motifs::shapes::rectangle(
            P2::new(0.0, 7000.0),
            22000.0,
            14000.0,
        ));
        for seed in 0..12 {
            let g = facade(seed);
            let d = crate::generate(&shape, &g, &CancelToken::new()).unwrap();
            assert!(d.door);
            assert!(d.ornaments.len() > 20);
            let v = validate(&d, 0.0, 0.0);
            assert!(v.is_empty(), "seed {seed}: {v:?}");
            let e = crate::generate(&shape, &g, &CancelToken::new()).unwrap();
            assert_eq!(
                serde_json::to_string(&d.ornaments).unwrap(),
                serde_json::to_string(&e.ornaments).unwrap()
            );
            let dr = door_drawing(&d);
            assert!(!dr.lines.is_empty());
        }
    }
}
