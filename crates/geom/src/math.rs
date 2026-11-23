//! Fonctions mathématiques portables (INV-3) et utilitaires de tolérance.
//!
//! Toutes les fonctions transcendantes passent par la crate `libm`, dont
//! l'implémentation est identique en natif et en WASM. `sqrt` est exacte
//! selon IEEE 754 et peut être prise dans `std`.

pub use glam::DVec2 as P2;

pub const PI: f64 = std::f64::consts::PI;
pub const TAU: f64 = std::f64::consts::TAU;
pub const FRAC_PI_2: f64 = std::f64::consts::FRAC_PI_2;

#[inline]
pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}
#[inline]
pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}
#[inline]
pub fn tan(x: f64) -> f64 {
    libm::tan(x)
}
#[inline]
pub fn atan(x: f64) -> f64 {
    libm::atan(x)
}
#[inline]
pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}
#[inline]
pub fn acos(x: f64) -> f64 {
    libm::acos(x.clamp(-1.0, 1.0))
}
#[inline]
pub fn asin(x: f64) -> f64 {
    libm::asin(x.clamp(-1.0, 1.0))
}
#[inline]
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}
#[inline]
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}
#[inline]
pub fn powf(x: f64, y: f64) -> f64 {
    libm::pow(x, y)
}
#[inline]
pub fn round(x: f64) -> f64 {
    libm::round(x)
}

/// Vecteur unitaire d'angle `a`.
#[inline]
pub fn dir(a: f64) -> P2 {
    P2::new(cos(a), sin(a))
}

/// Angle d'un vecteur, dans ]-π, π].
#[inline]
pub fn angle_of(v: P2) -> f64 {
    atan2(v.y, v.x)
}

/// Norme euclidienne (sans FMA).
#[inline]
pub fn norm(v: P2) -> f64 {
    (v.x * v.x + v.y * v.y).sqrt()
}

#[inline]
pub fn dist(a: P2, b: P2) -> f64 {
    norm(b - a)
}

#[inline]
pub fn cross(a: P2, b: P2) -> f64 {
    a.x * b.y - a.y * b.x
}

#[inline]
pub fn dot(a: P2, b: P2) -> f64 {
    a.x * b.x + a.y * b.y
}

/// Perpendiculaire à gauche (rotation de +90°).
#[inline]
pub fn perp(v: P2) -> P2 {
    P2::new(-v.y, v.x)
}

/// Normalisation sûre : renvoie le vecteur nul si la norme est nulle.
#[inline]
pub fn unit(v: P2) -> P2 {
    let n = norm(v);
    if n > 0.0 { v / n } else { P2::ZERO }
}

/// Rotation d'un vecteur.
#[inline]
pub fn rotate(v: P2, a: f64) -> P2 {
    let (s, c) = (sin(a), cos(a));
    P2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Ramène un angle dans [0, 2π).
#[inline]
pub fn norm_angle(a: f64) -> f64 {
    let mut r = a % TAU;
    if r < 0.0 {
        r += TAU;
    }
    if r >= TAU { 0.0 } else { r }
}

/// Écart angulaire orienté positif de `a` vers `b` dans [0, 2π).
#[inline]
pub fn ccw_delta(a: f64, b: f64) -> f64 {
    norm_angle(b - a)
}

/// Tolérances relatives à L, la diagonale de la boîte englobante de la forme
/// racine (§ 4.2).
#[derive(Clone, Copy, Debug)]
pub struct Tol {
    /// Diagonale de référence.
    pub l: f64,
    /// ε_geom = 1e-9·L : égalité de points.
    pub geom: f64,
    /// ε_tan = 1e-7·L : détection de tangence.
    pub tan: f64,
}

impl Tol {
    pub fn new(l: f64) -> Self {
        Self {
            l,
            geom: 1e-9 * l,
            tan: 1e-7 * l,
        }
    }
    #[inline]
    pub fn approx_eq(&self, a: f64, b: f64) -> bool {
        (a - b).abs() <= self.geom
    }
    #[inline]
    pub fn pt_eq(&self, a: P2, b: P2) -> bool {
        dist(a, b) <= self.geom
    }
    /// Deux distances sont-elles égales au sens de la tangence ?
    #[inline]
    pub fn is_tangent(&self, d: f64, target: f64) -> bool {
        (d - target).abs() <= self.tan
    }
}

impl Default for Tol {
    fn default() -> Self {
        Tol::new(100.0)
    }
}

/// Côté d'un point par rapport à la droite orientée (a→b) : >0 à gauche.
#[inline]
pub fn side_of(a: P2, b: P2, p: P2) -> f64 {
    cross(b - a, p - a)
}
