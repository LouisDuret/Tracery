//! Styles (§ 3.6, § 5.10) : paramètres utilisateur et définitions de style
//! (poids des règles, symétrie, brisure, poids du score) chargées depuis des
//! fichiers RON embarqués.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Rayonnant,
    Geometrique,
    Flamboyant,
    Curvilineaire,
    Perpendiculaire,
    Plaque,
    Libre,
}

impl Family {
    pub const ALL: [Family; 7] = [
        Family::Rayonnant,
        Family::Geometrique,
        Family::Flamboyant,
        Family::Curvilineaire,
        Family::Perpendiculaire,
        Family::Plaque,
        Family::Libre,
    ];
    pub fn key(&self) -> &'static str {
        match self {
            Family::Rayonnant => "rayonnant",
            Family::Geometrique => "geometrique",
            Family::Flamboyant => "flamboyant",
            Family::Curvilineaire => "curvilineaire",
            Family::Perpendiculaire => "perpendiculaire",
            Family::Plaque => "plaque",
            Family::Libre => "libre",
        }
    }
    pub fn parse(s: &str) -> Option<Family> {
        let s = s.to_lowercase();
        let s = s.replace(['é', 'è'], "e").replace(['-', ' '], "");
        Family::ALL.iter().copied().find(|f| {
            let k = f.key();
            k == s || (s.len() >= 4 && k.starts_with(&s))
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymMode {
    Strict,
    #[serde(alias = "preferee", alias = "preferred")]
    Preferred,
    #[serde(alias = "libre")]
    Free,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Canon {
    Off,
    #[serde(alias = "doux")]
    Soft,
    Strict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalMode {
    Jour,
    Plein,
    Auto,
}

/// Paramètres de style exposés à l'utilisateur (§ 3.6), partie du génome.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StyleParams {
    pub family: Family,
    /// Complexité : profondeur maximale (1–7).
    pub depth: u8,
    /// Taux de jours visé (0,35–0,80).
    pub density: f64,
    /// Largeur du cadre w₀ en fraction de √aire (0–0,08).
    pub w0: f64,
    /// Rapport hiérarchique λ : w(k+1) = λ·w(k).
    pub lambda: f64,
    /// Largeur minimale de meneau, en unités du document (0 = désactivée).
    pub w_min: f64,
    /// Nombres de lobes autorisés.
    pub foils: Vec<u8>,
    /// Chevauchement des lobes k.
    pub k: f64,
    /// Brisure préférée ρ = R/s.
    pub rho: f64,
    /// Symétrie (None : valeur par défaut de la famille).
    pub symmetry: Option<SymMode>,
    /// Probabilité de redentage des jours.
    pub cusping: f64,
    /// Température (0 = toujours le meilleur candidat).
    pub temperature: f64,
    pub canon: Canon,
    /// Candidats évalués par nœud.
    pub candidates: u8,
    pub terminal: TerminalMode,
    /// Cadre de pierre d'ordre 0 (sinon remplage « à fleur »).
    pub frame: bool,
    /// Porte (ADR-23) : la forme devient une baie de porte, décorée à
    /// l'extérieur (archivolte, accolade, crochets, fleuron, pinacles…).
    pub door: bool,
    /// Ferronnerie (ADR-26) : la forme (rectangle, arc) devient une porte
    /// de bois à un ou deux vantaux, ornée de pentures à volutes.
    pub iron: bool,
}

impl Default for StyleParams {
    fn default() -> Self {
        StyleParams {
            family: Family::Rayonnant,
            depth: 4,
            density: 0.60,
            w0: 0.035,
            lambda: 0.95,
            w_min: 0.0,
            foils: vec![3, 4, 5, 6],
            k: 1.15,
            rho: 1.0,
            symmetry: None,
            cusping: 0.6,
            temperature: 0.5,
            canon: Canon::Soft,
            candidates: 8,
            terminal: TerminalMode::Auto,
            frame: true,
            door: false,
            iron: false,
        }
    }
}

impl StyleParams {
    pub fn for_family(f: Family) -> Self {
        let def = style_def(f);
        StyleParams {
            family: f,
            rho: def.rho,
            cusping: def.cusping,
            ..Default::default()
        }
    }
    pub fn sym_mode(&self) -> SymMode {
        self.symmetry.unwrap_or(style_def(self.family).symmetry)
    }
    /// Bornes de validité (§ 3.6).
    pub fn clamped(&self) -> Self {
        let mut s = self.clone();
        s.depth = s.depth.clamp(1, 7);
        s.density = s.density.clamp(0.35, 0.80);
        s.w0 = s.w0.clamp(0.0, 0.08);
        s.lambda = s.lambda.clamp(0.5, 1.0);
        s.w_min = s.w_min.max(0.0);
        s.foils.retain(|n| (3..=8).contains(n));
        if s.foils.is_empty() {
            s.foils = vec![3, 4];
        }
        s.k = s.k.clamp(1.0, 1.5);
        s.rho = s.rho.clamp(0.5, 1.5);
        s.cusping = s.cusping.clamp(0.0, 1.0);
        s.temperature = s.temperature.clamp(0.0, 2.0);
        s.candidates = s.candidates.clamp(1, 64);
        s
    }
}

/// Définition d'un style (fichier RON).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StyleDef {
    pub name: String,
    /// Poids des règles (identifiant → poids, 0 = exclue).
    pub rules: BTreeMap<String, f32>,
    pub symmetry: SymMode,
    /// Brisure par défaut et plage tirée pour les sous-arcs.
    pub rho: f64,
    pub rho_range: (f64, f64),
    /// Probabilité de jumelage des cellules sœurs congruentes.
    pub twin_prob: f64,
    /// Probabilité de redentage par défaut.
    pub cusping: f64,
    /// Poids des termes du score.
    pub score: BTreeMap<String, f64>,
}

const STYLE_FILES: [(Family, &str); 7] = [
    (
        Family::Rayonnant,
        include_str!("../../../assets/styles/rayonnant.ron"),
    ),
    (
        Family::Geometrique,
        include_str!("../../../assets/styles/geometrique.ron"),
    ),
    (
        Family::Flamboyant,
        include_str!("../../../assets/styles/flamboyant.ron"),
    ),
    (
        Family::Curvilineaire,
        include_str!("../../../assets/styles/curvilineaire.ron"),
    ),
    (
        Family::Perpendiculaire,
        include_str!("../../../assets/styles/perpendiculaire.ron"),
    ),
    (
        Family::Plaque,
        include_str!("../../../assets/styles/plaque.ron"),
    ),
    (
        Family::Libre,
        include_str!("../../../assets/styles/libre.ron"),
    ),
];

fn defs() -> &'static Vec<(Family, StyleDef)> {
    static DEFS: OnceLock<Vec<(Family, StyleDef)>> = OnceLock::new();
    DEFS.get_or_init(|| {
        STYLE_FILES
            .iter()
            .map(|(f, src)| {
                // Les fichiers embarqués sont validés par les tests ; un
                // fichier invalide est un défaut de compilation du produit.
                let d: StyleDef =
                    ron::from_str(src).unwrap_or_else(|e| panic!("style {f:?} invalide : {e}"));
                (*f, d)
            })
            .collect()
    })
}

/// Définition de style d'une famille.
pub fn style_def(f: Family) -> &'static StyleDef {
    let d = defs();
    &d.iter().find(|(g, _)| *g == f).unwrap_or(&d[0]).1
}

impl StyleDef {
    pub fn weight(&self, rule: &str) -> f32 {
        self.rules.get(rule).copied().unwrap_or(0.0)
    }
    pub fn score_weight(&self, term: &str, default: f64) -> f64 {
        self.score.get(term).copied().unwrap_or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_load() {
        for f in Family::ALL {
            let d = style_def(f);
            assert!(!d.rules.is_empty(), "{f:?}");
        }
        assert_eq!(Family::parse("Flamboyant"), Some(Family::Flamboyant));
        assert_eq!(Family::parse("géométrique"), Some(Family::Geometrique));
    }
}
