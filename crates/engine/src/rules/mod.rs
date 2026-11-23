//! Grammaire de formes : trait `Rule`, contexte, expansion et registre
//! (§ 5.3, § 5.4, § 6.3). Une règle = un module isolé.

use crate::analysis::{Analysis, CellMeta, CellType};
use crate::design::Opening;
use crate::genome::Params;
use crate::style::{StyleDef, StyleParams, SymMode};
use geom::clip::{offset_region, subtract, subtract_loop};
use geom::math::*;
use geom::sdf::max_inscribed;
use geom::{Circle, ConstructionEntity, Loop, Region, Tol, Xf};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::Rng;

mod r01_lancets;
mod r02_oculus;
mod r04_foil;
mod r05_rose;
mod r06_ring;
mod r07_cell_foil;
mod r09_cusping;
mod r10_mouchette;
mod r12_wheel;
mod r14_panels;
mod r15_circles;
mod r16_medial;
mod r17_split;
mod r19_terminal;
mod r20_band;
mod r21_trefoil;
mod r22_double_mouchette;
mod r23_redents;
mod r25_branch;

/// Erreur d'application d'une règle (déclenche une alternative, INV-4).
#[derive(Debug, Clone, thiserror::Error)]
pub enum RuleError {
    #[error("règle non applicable : {0}")]
    NotApplicable(&'static str),
    #[error("contrainte violée : {0}")]
    Constraint(&'static str),
    #[error("géométrie dégénérée : {0}")]
    Degenerate(&'static str),
}

/// Contexte d'un nœud.
pub struct Ctx<'a> {
    pub style: &'a StyleParams,
    pub def: &'a StyleDef,
    pub tol: Tol,
    pub depth: u8,
    pub max_depth: u8,
    /// Largeur des meneaux créés par ce nœud (ordre depth + 1).
    pub w: f64,
    /// Largeur des meneaux d'ordre depth + 2 (petits-enfants).
    pub w_next: f64,
    /// Largeur minimale absolue (unités normalisées).
    pub w_min: f64,
    /// Aire de la forme racine.
    pub root_area: f64,
    /// La symétrie est-elle imposée à ce nœud ?
    pub sym: SymMode,
    pub terminal_only: bool,
}

impl Ctx<'_> {
    /// Écart entre un nouvel élément et le bord de la cellule.
    pub fn edge_gap(&self) -> f64 {
        0.6 * self.w
    }
    /// Dilatation utilisée pour séparer les résidus (évite les tangences
    /// exactes dans les booléens).
    pub fn carve_gap(&self) -> f64 {
        1.1 * self.w
    }
    /// Rayon minimal d'un jour utile.
    pub fn min_opening_r(&self) -> f64 {
        (0.45 * self.w).max(0.5 * self.w_min)
    }
    pub fn sym_axis(&self, a: &Analysis) -> Option<f64> {
        match self.sym {
            SymMode::Free => None,
            _ => a.sym_x,
        }
    }
}

/// Cellule enfant.
#[derive(Clone, Debug)]
pub struct ChildSpec {
    pub region: Region,
    pub hint: Option<CellType>,
    pub meta: Option<CellMeta>,
    /// Ce sous-arbre est la copie, par l'isométrie donnée, de celui de
    /// l'enfant `of` (jumelage).
    pub twin: Option<(usize, Xf)>,
}

impl ChildSpec {
    pub fn new(region: Region, hint: Option<CellType>) -> Self {
        ChildSpec {
            region,
            hint,
            meta: None,
            twin: None,
        }
    }
}

/// Résultat de l'application d'une règle.
#[derive(Clone, Debug, Default)]
pub struct Expansion {
    pub children: Vec<ChildSpec>,
    pub openings: Vec<Region>,
    pub construction: Vec<ConstructionEntity>,
    /// Rapport clé pour le terme de proportion (§ 5.11), s'il existe.
    pub key_ratio: Option<f64>,
}

impl Expansion {
    pub fn covered_area(&self) -> f64 {
        self.children.iter().map(|c| c.region.area()).sum::<f64>()
            + self.openings.iter().map(|o| o.area()).sum::<f64>()
    }
}

/// Règle de la grammaire.
pub trait Rule: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    /// Règle terminale (ne crée pas de cellules enfants) ?
    fn terminal(&self) -> bool {
        false
    }
    /// Poids d'application ≥ 0 (0 : non applicable).
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64;
    /// Tirage des paramètres selon les lois a priori.
    fn sample(&self, ctx: &Ctx, a: &Analysis, rng: &mut ChaCha8Rng) -> Params;
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError>;
}

/// Registre explicite, construit dans un ordre fixe (déterminisme).
pub fn registry() -> &'static [Box<dyn Rule>] {
    static REG: std::sync::OnceLock<Vec<Box<dyn Rule>>> = std::sync::OnceLock::new();
    REG.get_or_init(|| {
        vec![
            Box::new(r01_lancets::Lancets),
            Box::new(r02_oculus::Oculus),
            Box::new(r15_circles::MaxCircles {
                id: "R03",
                head: true,
            }),
            Box::new(r04_foil::Polylobe),
            Box::new(r05_rose::Rose),
            Box::new(r06_ring::Ring),
            Box::new(r07_cell_foil::CellFoil {
                id: "R07",
                corners: 3,
            }),
            Box::new(r07_cell_foil::CellFoil {
                id: "R08",
                corners: 4,
            }),
            Box::new(r09_cusping::Cusping),
            Box::new(r10_mouchette::Mouchette {
                id: "R10",
                symmetric: false,
            }),
            Box::new(r10_mouchette::Mouchette {
                id: "R11",
                symmetric: true,
            }),
            Box::new(r12_wheel::Wheel),
            Box::new(r14_panels::Panels),
            Box::new(r15_circles::MaxCircles {
                id: "R15",
                head: false,
            }),
            Box::new(r16_medial::MedialCut),
            Box::new(r17_split::SymSplit),
            Box::new(r19_terminal::Terminal),
            Box::new(r20_band::Band),
            Box::new(r21_trefoil::TrefoilHead),
            Box::new(r22_double_mouchette::DoubleMouchette),
            Box::new(r22_double_mouchette::SouffletHead),
            Box::new(r23_redents::Redents),
            Box::new(r25_branch::Branch),
        ]
    })
}

pub fn rule_by_id(id: &str) -> Option<&'static dyn Rule> {
    registry().iter().find(|r| r.id() == id).map(|b| b.as_ref())
}

// ---------------------------------------------------------------- outils

/// Tirage uniforme dans [0, 1).
pub fn unif(rng: &mut ChaCha8Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

pub fn unif_in(rng: &mut ChaCha8Rng, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * unif(rng)
}

/// Choix pondéré (poids ≥ 0) ; None si tous nuls.
pub fn weighted_index(rng: &mut ChaCha8Rng, w: &[f64]) -> Option<usize> {
    let tot: f64 = w.iter().filter(|x| x.is_finite() && **x > 0.0).sum();
    if tot <= 0.0 {
        return None;
    }
    let mut t = unif(rng) * tot;
    for (i, &x) in w.iter().enumerate() {
        if x > 0.0 && x.is_finite() {
            if t < x {
                return Some(i);
            }
            t -= x;
        }
    }
    w.iter().rposition(|x| *x > 0.0)
}

/// Dilatation d'un jour (exacte pour un cercle).
pub fn dilate(o: &Region, d: f64) -> Vec<Region> {
    if o.holes.is_empty()
        && let Some(c) = o.outer.as_circle(1e-9)
    {
        return vec![Region::circle(c.c, c.r + d)];
    }
    offset_region(o, -d)
}

/// Résidu : F ∖ ⋃ dilate(élément, gap), copeaux supprimés.
pub fn carve(f: &Region, items: &[Region], gap: f64, min_r: f64) -> Vec<Region> {
    let mut res = vec![f.clone()];
    for it in items {
        for d in dilate(it, gap) {
            res = subtract(&res, &d);
            if res.is_empty() {
                return res;
            }
        }
    }
    drop_chips(res, min_r)
}

/// Retire des bandes (boucles) d'une région.
pub fn cut_bands(f: &Region, bands: &[Loop], min_r: f64) -> Vec<Region> {
    let mut res = vec![f.clone()];
    for b in bands {
        res = subtract_loop(&res, b);
    }
    drop_chips(res, min_r)
}

/// Suppression des copeaux : pièces dont le cercle inscrit est trop petit
/// (elles deviennent de la pierre). Ordre déterministe : haut → bas,
/// gauche → droite.
pub fn drop_chips(v: Vec<Region>, min_r: f64) -> Vec<Region> {
    let mut out: Vec<Region> = v
        .into_iter()
        .filter(|r| {
            let a = r.area();
            if a < PI * min_r * min_r {
                return false;
            }
            match max_inscribed(r, &[], 0.0, 14) {
                Some((c, _)) => c.r >= min_r,
                None => false,
            }
        })
        .collect();
    sort_regions(&mut out);
    out
}

pub fn sort_regions(v: &mut [Region]) {
    v.sort_by(|a, b| {
        let (ca, cb) = (a.bbox().center(), b.bbox().center());
        let ka = (round(-ca.y * 1e6), round(ca.x * 1e6));
        let kb = (round(-cb.y * 1e6), round(cb.x * 1e6));
        ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
    });
}

/// Le contour `l` est-il dans F avec une marge ≥ `margin` ?
pub fn fits_inside(f: &Region, l: &Loop, margin: f64) -> bool {
    let step = l.length() / 96.0;
    l.sample(step.max(1e-9))
        .into_iter()
        .all(|p| f.sdf(p) >= margin)
}

/// Bande autour d'un segment de droite (rectangle prolongé de `ext`).
pub fn line_band(a: P2, b: P2, w: f64, ext: f64) -> Loop {
    let u = unit(b - a);
    let n = perp(u) * (w / 2.0);
    let a2 = a - u * ext;
    let b2 = b + u * ext;
    Loop::polygon(&[a2 - n, b2 - n, b2 + n, a2 + n])
}

/// Bande autour d'un arc (secteur de couronne, arcs concentriques R ± w/2),
/// prolongée angulairement de `ext` (longueur) à chaque extrémité.
pub fn arc_band(c: P2, r: f64, theta0: f64, sweep: f64, w: f64, ext: f64) -> Loop {
    let s = sweep.signum();
    let da = ext / r.max(1e-12);
    let t0 = theta0 - s * da;
    let sw = sweep + s * 2.0 * da;
    let (ro, ri) = (r + w / 2.0, (r - w / 2.0).max(1e-9));
    let l = Loop::new(vec![
        geom::Seg::arc(c, ro, t0, sw),
        geom::Seg::line(c + dir(t0 + sw) * ro, c + dir(t0 + sw) * ri),
        geom::Seg::arc(c, ri, t0 + sw, -sw),
        geom::Seg::line(c + dir(t0) * ri, c + dir(t0) * ro),
    ]);
    l.oriented(true)
}

/// Cercle inscrit exact avec écart au bord.
pub fn inscribed_with_gap(f: &Region, gap: f64) -> Option<Circle> {
    max_inscribed(f, &[], gap, 28).map(|(c, _)| c)
}

/// Jumelage par translation/symétrie : l'enfant `j` devient copie de `i`.
pub fn twin(children: &mut [ChildSpec], j: usize, i: usize, x: Xf) {
    if j < children.len() && i < j {
        children[j].twin = Some((i, x));
    }
}

/// Convertit les jours en `Opening` (utilisé par l'assemblage).
pub fn to_openings(regs: &[Region], depth: u8, path: &str, rule: &str) -> Vec<Opening> {
    regs.iter()
        .map(|r| Opening {
            region: r.clone(),
            depth,
            path: path.to_string(),
            rule: rule.to_string(),
        })
        .collect()
}
