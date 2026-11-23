//! Export DXF ASCII (entités LINE et ARC, calques, unités mm).
//!
//! Écrivain minimal au format AC1009 (R12), lu par LibreCAD, Inkscape et la
//! plupart des logiciels de découpe. Écart au § 3.8 (R2000) consigné dans
//! docs/DECISIONS.md. Option « profils de découpe » : contours fermés
//! orientés uniquement (pas de calque de construction).

use engine::{Design, OrnKind};
use geom::math::*;
use geom::{ConstructionEntity, Loop, Seg};
use std::fmt::Write;

fn pair(out: &mut String, code: i32, v: impl std::fmt::Display) {
    let _ = writeln!(out, "{code}\n{v}");
}

fn num(v: f64) -> String {
    format!("{v:.6}")
}

fn line(out: &mut String, layer: &str, a: P2, b: P2) {
    pair(out, 0, "LINE");
    pair(out, 8, layer);
    pair(out, 10, num(a.x));
    pair(out, 20, num(a.y));
    pair(out, 30, num(0.0));
    pair(out, 11, num(b.x));
    pair(out, 21, num(b.y));
    pair(out, 31, num(0.0));
}

fn arc(out: &mut String, layer: &str, c: P2, r: f64, a0: f64, a1: f64) {
    pair(out, 0, "ARC");
    pair(out, 8, layer);
    pair(out, 10, num(c.x));
    pair(out, 20, num(c.y));
    pair(out, 30, num(0.0));
    pair(out, 40, num(r));
    pair(out, 50, num(a0.to_degrees()));
    pair(out, 51, num(a1.to_degrees()));
}

fn circle(out: &mut String, layer: &str, c: P2, r: f64) {
    pair(out, 0, "CIRCLE");
    pair(out, 8, layer);
    pair(out, 10, num(c.x));
    pair(out, 20, num(c.y));
    pair(out, 30, num(0.0));
    pair(out, 40, num(r));
}

fn emit_loop(out: &mut String, layer: &str, l: &Loop) {
    if let Some(c) = l.as_circle(1e-9) {
        circle(out, layer, c.c, c.r);
        return;
    }
    for s in &l.segs {
        match *s {
            Seg::Line { a, b } => line(out, layer, a, b),
            Seg::Arc {
                c,
                r,
                theta0,
                sweep,
            } => {
                // ARC DXF : toujours trigonométrique de l'angle 50 à l'angle 51
                let (a0, a1) = if sweep >= 0.0 {
                    (theta0, theta0 + sweep)
                } else {
                    (theta0 + sweep, theta0)
                };
                arc(out, layer, c, r, norm_angle(a0), norm_angle(a1));
            }
        }
    }
}

/// Export d'un remplage en DXF.
pub fn design_to_dxf(d: &Design, cut_profiles: bool) -> String {
    let mut s = String::new();
    pair(&mut s, 0, "SECTION");
    pair(&mut s, 2, "HEADER");
    pair(&mut s, 9, "$ACADVER");
    pair(&mut s, 1, "AC1009");
    pair(&mut s, 9, "$INSUNITS");
    pair(&mut s, 70, 4);
    pair(&mut s, 0, "ENDSEC");
    // table des calques
    pair(&mut s, 0, "SECTION");
    pair(&mut s, 2, "TABLES");
    pair(&mut s, 0, "TABLE");
    pair(&mut s, 2, "LAYER");
    let layers: &[(&str, i32)] = if cut_profiles {
        &[("CONTOUR", 7), ("JOURS", 5), ("RELIEF", 3), ("CREUX", 4)]
    } else {
        &[
            ("CONTOUR", 7),
            ("JOURS", 5),
            ("RELIEF", 3),
            ("CREUX", 4),
            ("CONSTRUCTION", 1),
        ]
    };
    pair(&mut s, 70, layers.len());
    for (name, color) in layers {
        pair(&mut s, 0, "LAYER");
        pair(&mut s, 2, name);
        pair(&mut s, 70, 0);
        pair(&mut s, 62, color);
        pair(&mut s, 6, "CONTINUOUS");
    }
    pair(&mut s, 0, "ENDTAB");
    pair(&mut s, 0, "ENDSEC");
    pair(&mut s, 0, "SECTION");
    pair(&mut s, 2, "ENTITIES");
    for l in d.shape.loops() {
        emit_loop(&mut s, "CONTOUR", l);
    }
    for o in &d.openings {
        for l in o.region.loops() {
            // profils de découpe : jours en sens horaire (trous dans la pierre)
            let l = if cut_profiles {
                l.oriented(false)
            } else {
                l.clone()
            };
            emit_loop(&mut s, "JOURS", &l);
        }
    }
    // ornements de porte (ADR-23)
    for o in &d.ornaments {
        let layer = match o.kind {
            OrnKind::Relief => "RELIEF",
            OrnKind::Creux => "CREUX",
        };
        for l in o.region.loops() {
            emit_loop(&mut s, layer, l);
        }
    }
    if !cut_profiles {
        for c in &d.construction {
            match *c {
                ConstructionEntity::Circle { c, r, .. } => circle(&mut s, "CONSTRUCTION", c, r),
                ConstructionEntity::Line { a, b, .. } => line(&mut s, "CONSTRUCTION", a, b),
                ConstructionEntity::Point { p, .. } => {
                    pair(&mut s, 0, "POINT");
                    pair(&mut s, 8, "CONSTRUCTION");
                    pair(&mut s, 10, num(p.x));
                    pair(&mut s, 20, num(p.y));
                    pair(&mut s, 30, num(0.0));
                }
            }
        }
    }
    pair(&mut s, 0, "ENDSEC");
    pair(&mut s, 0, "EOF");
    s
}
