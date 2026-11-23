//! Cœur géométrique de Tracery : segments et arcs à centre et rayon
//! explicites (INV-1), constructions à la règle et au compas, problème
//! d'Apollonius, décalages et booléens, distance signée.
//!
//! Aucune dépendance à l'interface ni au GPU (INV-5).
#![forbid(unsafe_code)]

pub mod clip;
pub mod constr;
pub mod fit;
pub mod math;
pub mod sdf;
pub mod seg;

pub use math::{P2, Tol};
pub use seg::{BBox, Circle, Joint, Loop, Region, Seg, Xf};

use serde::{Deserialize, Serialize};

/// Opération de construction ayant produit une entité (INV-6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstructionOp {
    Input,
    Intersect,
    Offset,
    Apollonius,
    Inscribed,
    FoilLobe,
    ArchCenter,
    Tangent,
    Fit,
    Mouchette,
    Soufflet,
    Rose,
    Cusp,
    Axis,
}

/// Entité de construction (épure) : centres, cercles, droites.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ConstructionEntity {
    Point { p: P2, op: ConstructionOp },
    Circle { c: P2, r: f64, op: ConstructionOp },
    Line { a: P2, b: P2, op: ConstructionOp },
}

impl ConstructionEntity {
    pub fn transform(&self, x: &Xf) -> ConstructionEntity {
        match *self {
            ConstructionEntity::Point { p, op } => ConstructionEntity::Point { p: x.apply(p), op },
            ConstructionEntity::Circle { c, r, op } => ConstructionEntity::Circle {
                c: x.apply(c),
                r: r * x.scale,
                op,
            },
            ConstructionEntity::Line { a, b, op } => ConstructionEntity::Line {
                a: x.apply(a),
                b: x.apply(b),
                op,
            },
        }
    }
    pub fn circle(c: Circle, op: ConstructionOp) -> Self {
        ConstructionEntity::Circle { c: c.c, r: c.r, op }
    }
    pub fn point(p: P2, op: ConstructionOp) -> Self {
        ConstructionEntity::Point { p, op }
    }
}
