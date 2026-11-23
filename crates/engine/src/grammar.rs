//! Grammaire gothique : rôle architectural d'une cellule et règles admises
//! pour ce rôle (ADR-14).
//!
//! La dérivation trace d'abord les grandes formes (baie → lancettes et
//! tympan → oculus → écoinçons) puis les petites à l'intérieur. Chaque
//! ornement n'a de sens qu'à sa place : un polylobe garnit un oculus, pas
//! une lancette ; une lancette se subdivise ou se redente, elle ne se remplit
//! pas de cercles ; les écoinçons reçoivent de petits motifs ajustés. Le
//! tableau ci-dessous multiplie le poids du style (0 = interdit).

use crate::analysis::{Analysis, CellType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Baie complète (racine en arc).
    Window,
    /// Lancette (jour vertical sous arc).
    Light,
    /// Tympan : espace sous l'arc principal, au-dessus des lancettes.
    Head,
    /// Oculus (cellule circulaire).
    Oculus,
    /// Couronne (rose, roue).
    Ring,
    /// Secteur de rose.
    Sector,
    /// Panneau (perpendiculaire).
    Panel,
    /// Moitié de double mouchette, rayon de roue : cellule effilée.
    Tapered,
    /// Petit résidu entre grandes formes.
    Spandrel,
    /// Grande cellule quelconque (forme libre, grand résidu).
    Field,
}

pub fn role(a: &Analysis, hint: Option<CellType>, root_area: f64, w: f64) -> Role {
    if a.circle.is_some() || hint == Some(CellType::Circle) {
        return Role::Oculus;
    }
    match hint {
        Some(CellType::Lancet) => return Role::Light,
        Some(CellType::Tympan) => return Role::Head,
        Some(CellType::Sector) => return Role::Sector,
        Some(CellType::Panel) => return Role::Panel,
        Some(CellType::Tapered) => return Role::Tapered,
        _ => {}
    }
    if !a.region.holes.is_empty() {
        return Role::Ring;
    }
    // forme racine (sous le cadre)
    if a.area > 0.5 * root_area {
        return if a.arch.is_some() {
            Role::Window
        } else {
            Role::Field
        };
    }
    if a.inscribed.r < 5.0 * w {
        Role::Spandrel
    } else {
        Role::Field
    }
}

/// Facteur d'adéquation règle × rôle.
pub fn fit(role: Role, rule: &str) -> f64 {
    use Role::*;
    match (role, rule) {
        // baie : lancettes sous arcs (ou panneaux perpendiculaires)
        (Window, "R01") => 1.0,
        (Window, "R14") => 0.15,
        (Window, _) => 0.0,

        // lancette : subdivision, tête tréflée, redents, jour simple
        (Light, "R01") => 1.0,
        (Light, "R21") => 1.0,
        (Light, "R09") => 0.3,
        (Light, "R19") => 0.5,
        (Light, "R14") => 0.3,
        (Light, _) => 0.0,

        // tympan : oculus, cercles de tête, mouchettes, soufflets, S
        (Head, "R02") => 1.0,
        (Head, "R03") => 0.6,
        (Head, "R22") => 0.8,
        (Head, "R24") => 1.0,
        (Head, "R11") => 0.8,
        (Head, "R10") => 0.5,
        (Head, "R17") => 0.6,
        (Head, "R16") => 0.4,
        (Head, "R07" | "R08") => 0.5,
        (Head, "R14") => 0.3,
        (Head, "R19") => 0.05,
        (Head, _) => 0.0,

        // oculus : polylobe, rose, roue, couronne, double mouchette arrondie
        (Oculus, "R04") => 1.0,
        (Oculus, "R05") => 0.8,
        (Oculus, "R06") => 0.6,
        (Oculus, "R12") => 0.8,
        (Oculus, "R22") => 0.9,
        (Oculus, "R09") => 0.4,
        (Oculus, "R19") => 0.15,
        (Oculus, _) => 0.0,

        (Ring, "R05" | "R06" | "R20" | "R12") => 1.0,
        (Ring, "R15") => 0.2,
        (Ring, "R19") => 0.1,
        (Ring, _) => 0.0,

        // secteur de rose : petite lancette, soufflet, mouchette, trilobe
        (Sector, "R01") => 0.8,
        (Sector, "R21") => 0.8,
        (Sector, "R07" | "R08") => 0.7,
        (Sector, "R11") => 0.8,
        (Sector, "R10") => 0.6,
        (Sector, "R02") => 0.4,
        (Sector, "R23") => 0.5,
        (Sector, "R09") => 0.6,
        (Sector, "R19") => 0.3,
        (Sector, _) => 0.0,

        (Panel, "R01") => 1.0,
        (Panel, "R21") => 1.0,
        (Panel, "R14") => 0.6,
        (Panel, "R09") => 0.2,
        (Panel, "R19") => 0.4,
        (Panel, _) => 0.0,

        // soufflet, mouchette, rayon de roue : redenté ou laissé en jour
        (Tapered, "R23") => 1.0,
        // … ou repartagé par une branche tangente (complexité suffisante)
        (Tapered, "R25") => 1.0,
        (Tapered, "R19") => 0.4,
        (Tapered, _) => 0.0,

        // écoinçon : petit motif ajusté, jamais de remplissage par cercles
        (Spandrel, "R07" | "R08") => 1.0,
        (Spandrel, "R10" | "R11") => 1.0,
        (Spandrel, "R23") => 0.6,
        (Spandrel, "R25") => 0.5,
        (Spandrel, "R09") => 0.8,
        (Spandrel, "R04") => 0.6,
        (Spandrel, "R02") => 0.25,
        (Spandrel, "R19") => 0.5,
        (Spandrel, _) => 0.0,

        // grande cellule libre : découpes structurantes d'abord
        (Field, "R01") => 1.0,
        (Field, "R02") => 0.9,
        (Field, "R22") => 0.9,
        (Field, "R24") => 0.6,
        (Field, "R25") => 0.5,
        (Field, "R17") => 0.8,
        (Field, "R16") => 0.7,
        (Field, "R14") => 0.5,
        (Field, "R11" | "R10") => 0.5,
        (Field, "R07" | "R08") => 0.4,
        (Field, "R03") => 0.4,
        (Field, "R15") => 0.15,
        (Field, "R09") => 0.1,
        (Field, "R20") => 0.2,
        (Field, "R19") => 0.1,
        (Field, _) => 0.0,
    }
}
