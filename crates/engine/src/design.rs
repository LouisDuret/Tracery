//! Résultat d'une génération (EF-31) : arbre de dérivation, jours, entités
//! de construction, détail du score.

use crate::score::ScoreBreakdown;
use geom::{ConstructionEntity, Region, Xf};
use serde::{Deserialize, Serialize};

/// Jour (ouverture) terminal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Opening {
    pub region: Region,
    /// Profondeur du nœud qui l'a produit.
    pub depth: u8,
    /// Chemin du nœud producteur.
    pub path: String,
    pub rule: String,
}

impl Opening {
    pub fn transform(&self, x: &Xf, path: String) -> Opening {
        Opening {
            region: self.region.transform(x),
            depth: self.depth,
            path,
            rule: self.rule.clone(),
        }
    }
}

/// Nature d'un ornement de porte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrnKind {
    /// Pierre en saillie, au niveau `level` (0 : mur, 1 : encadrement,
    /// 2 : moulures, 3 : sculpture).
    Relief,
    /// Creux (gorge de moulure, panneau aveugle) taillé dans la pierre
    /// sous-jacente ; `level` donne la profondeur (1 : gorge, 2 : panneau).
    Creux,
}

/// Ornement d'une porte : région de pierre en relief ou en creux, posée
/// hors de la forme (archivolte, accolade, crochets, pinacles…) ou dans son
/// tympan.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ornament {
    pub region: Region,
    pub kind: OrnKind,
    pub level: u8,
    /// Chemin du nœud producteur (panneau des objets).
    pub path: String,
}

impl Ornament {
    pub fn transform(&self, x: &Xf) -> Ornament {
        Ornament {
            region: self.region.transform(x),
            kind: self.kind,
            level: self.level,
            path: self.path.clone(),
        }
    }
}

/// Nœud de l'arbre de dérivation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TreeNode {
    pub path: String,
    pub depth: u8,
    pub rule: String,
    pub params: crate::genome::Params,
    pub types: Vec<String>,
    pub area: f64,
    pub inscribed: f64,
    pub local_score: f64,
    /// Chemin du jumeau dont ce sous-arbre est la copie (isométrie).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twin_of: Option<String>,
    pub bbox: geom::BBox,
    pub locked: bool,
    /// Rapport clé de la règle (terme de proportion).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f64>,
    /// Nombre de candidats valides évalués.
    pub candidates: usize,
    /// Région de la cellule (sélection dans l'interface ; non sérialisée).
    #[serde(skip)]
    pub region: Option<geom::Region>,
}

/// Statistiques de génération.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub millis: f64,
    pub nodes: usize,
    pub primitives: usize,
    pub candidates_evaluated: usize,
    pub candidates_failed: usize,
}

/// Remplage complet.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Design {
    pub version: String,
    /// Forme source (unités du document).
    pub shape: Region,
    pub openings: Vec<Opening>,
    pub construction: Vec<ConstructionEntity>,
    pub tree: Vec<TreeNode>,
    pub score: ScoreBreakdown,
    pub stats: Stats,
    /// Largeur des meneaux par ordre (unités du document).
    pub widths: Vec<f64>,
    pub warnings: Vec<String>,
    /// Porte (ADR-23) : la forme est la baie, le décor est à l'extérieur.
    #[serde(default, skip_serializing_if = "is_false")]
    pub door: bool,
    /// Ferronnerie (ADR-26) : vantaux de bois et pentures de fer ; les
    /// ornements bas (niveaux 0–1) sont du bois, les autres du fer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub iron: bool,
    /// Ornements de porte (relief et creux), du fond vers l'avant.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ornaments: Vec<Ornament>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Design {
    pub fn primitive_count(&self) -> usize {
        self.openings
            .iter()
            .map(|o| o.region.segs().count())
            .sum::<usize>()
            + self
                .ornaments
                .iter()
                .map(|o| o.region.segs().count())
                .sum::<usize>()
            + self.shape.segs().count()
    }
    /// Boîte englobante de la forme et des ornements.
    pub fn bbox(&self) -> geom::BBox {
        self.ornaments
            .iter()
            .fold(self.shape.bbox(), |b, o| b.union(&o.region.bbox()))
    }
    pub fn opening_area(&self) -> f64 {
        self.openings.iter().map(|o| o.region.area()).sum()
    }
}
