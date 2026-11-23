//! Invariants vérifiés automatiquement (§ 9.2) : EQ-01 à EQ-05.

use crate::design::Design;
use geom::BBox;
use geom::clip::self_intersects;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Violation {
    pub code: String,
    pub detail: String,
}

fn v(code: &str, detail: String) -> Violation {
    Violation {
        code: code.to_string(),
        detail,
    }
}

/// Vérifie un remplage. `w_min` et `j_min` en unités du document
/// (0 = contrainte désactivée).
pub fn validate(d: &Design, w_min: f64, j_min: f64) -> Vec<Violation> {
    let mut out = Vec::new();
    let l = d.shape.bbox().diag();
    let eps = 1e-6 * l;
    for (i, o) in d.openings.iter().enumerate() {
        // EQ-01 : segments et arcs finis, r > 0
        for s in o.region.segs() {
            if !s.is_finite() {
                out.push(v("EQ-01", format!("jour {i} : segment non fini")));
            }
            if let geom::Seg::Arc { r, .. } = s
                && *r <= 1e-9 * l
            {
                out.push(v("EQ-01", format!("jour {i} : rayon nul")));
            }
        }
        // EQ-05 : régions valides
        if !o.region.outer.is_ccw() || o.region.area() <= 0.0 {
            out.push(v(
                "EQ-05",
                format!("jour {i} ({}) : orientation ou aire", o.path),
            ));
        }
        if self_intersects(&o.region.outer) {
            out.push(v(
                "EQ-05",
                format!("jour {i} ({}) : auto-intersection", o.path),
            ));
        }
        // EQ-02 : inclusion dans la forme (hors des trous)
        let step = o.region.perimeter() / 96.0;
        if o.region
            .outer
            .sample(step)
            .iter()
            .any(|p| d.shape.sdf(*p) < -eps)
        {
            out.push(v(
                "EQ-02",
                format!("jour {i} ({}) : hors de la forme", o.path),
            ));
        }
    }
    // EQ-02 (ferronnerie) : bois et ferrures restent dans la forme
    if d.iron {
        for (i, o) in d.ornaments.iter().enumerate() {
            let step = o.region.perimeter() / 64.0;
            if o.region
                .outer
                .sample(step)
                .iter()
                .any(|p| d.shape.sdf(*p) < -eps)
            {
                out.push(v(
                    "EQ-02",
                    format!("ferrure {i} ({}) : hors de la forme", o.path),
                ));
            }
        }
    }
    // EQ-02 (disjonction) et EQ-03 (épaisseur de pierre entre jours)
    let bbs: Vec<BBox> = d.openings.iter().map(|o| o.region.bbox()).collect();
    let margin = w_min.max(0.0) + eps;
    for i in 0..d.openings.len() {
        for j in (i + 1)..d.openings.len() {
            if !bbs[i].overlaps(&bbs[j], margin) {
                continue;
            }
            let (a, b) = (&d.openings[i].region, &d.openings[j].region);
            let pa = a.outer.sample(a.perimeter() / 96.0);
            let pb = b.outer.sample(b.perimeter() / 96.0);
            if pa.iter().any(|p| b.sdf(*p) > eps) || pb.iter().any(|p| a.sdf(*p) > eps) {
                out.push(v(
                    "EQ-02",
                    format!(
                        "jours {i} et {j} ({} / {}) : chevauchement",
                        d.openings[i].path, d.openings[j].path
                    ),
                ));
                continue;
            }
            if w_min > 0.0 {
                let md = pa
                    .iter()
                    .map(|p| b.boundary_dist(*p))
                    .fold(f64::INFINITY, f64::min);
                if md < w_min - eps {
                    out.push(v(
                        "EQ-03",
                        format!("jours {i} et {j} : pierre {md:.3} < {w_min}"),
                    ));
                }
            }
        }
    }
    // EQ-04 : largeur des jours
    if j_min > 0.0 {
        for (i, o) in d.openings.iter().enumerate() {
            let r = geom::sdf::max_inscribed(&o.region, &[], 0.0, 16)
                .map(|c| c.0.r)
                .unwrap_or(0.0);
            if 2.0 * r < j_min - eps {
                out.push(v(
                    "EQ-04",
                    format!("jour {i} : largeur {:.3} < {j_min}", 2.0 * r),
                ));
            }
        }
    }
    out
}
