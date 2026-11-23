//! Fichiers de forme et fichier projet `.tracery` (annexe B ; les anciens
//! fichiers `.remplage`, au format identique, restent lisibles).

use engine::refshapes::ref_shape;
use engine::{GENERATOR_VERSION, Genome};
use geom::math::*;
use geom::{Loop, Region, Seg};
use motifs::shapes::*;
use motifs::{ArchKind, arch_window};
use serde::{Deserialize, Serialize};

/// Région au format de l'annexe B : listes de segments.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegionJson {
    pub outer: Vec<Seg>,
    #[serde(default)]
    pub holes: Vec<Vec<Seg>>,
}

impl RegionJson {
    pub fn from_region(r: &Region) -> Self {
        RegionJson {
            outer: r.outer.segs.clone(),
            holes: r.holes.iter().map(|h| h.segs.clone()).collect(),
        }
    }
    pub fn to_region(&self) -> Region {
        Region::new(
            Loop::new(self.outer.clone()),
            self.holes.iter().map(|h| Loop::new(h.clone())).collect(),
        )
    }
}

/// Description paramétrique d'une primitive (EF-10, EF-11).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "primitive", rename_all = "kebab-case")]
pub enum Primitive {
    Arch {
        #[serde(default = "default_arch")]
        kind: String,
        #[serde(default = "one")]
        rho: f64,
        span: f64,
        #[serde(default)]
        jambs: f64,
    },
    Circle {
        r: f64,
    },
    Rect {
        w: f64,
        h: f64,
    },
    Polygon {
        n: usize,
        r: f64,
    },
    Points {
        points: Vec<[f64; 2]>,
    },
    Smooth {
        points: Vec<[f64; 2]>,
    },
    Reuleaux {
        s: f64,
    },
    Vesica {
        r: f64,
    },
    Annulus {
        r: f64,
        r_in: f64,
    },
    Teardrop {
        r: f64,
        length: f64,
    },
    Oval {
        a: f64,
        b: f64,
    },
    Foil {
        r: f64,
        n: usize,
        #[serde(default = "kdef")]
        k: f64,
    },
}

fn default_arch() -> String {
    "tiers-point".into()
}
fn one() -> f64 {
    1.0
}
fn kdef() -> f64 {
    1.15
}

pub fn arch_kind(kind: &str, rho: f64) -> Option<ArchKind> {
    Some(match kind {
        "plein-cintre" | "round" => ArchKind::Round,
        "tiers-point" | "equilateral" => ArchKind::equilateral(),
        "lancette" | "lancet" => ArchKind::Pointed { rho: rho.max(1.0) },
        "surbaisse" | "surbaissé" => ArchKind::Pointed {
            rho: rho.clamp(0.51, 1.0),
        },
        "brise" | "brisé" | "pointed" => ArchKind::Pointed { rho },
        "tudor" => ArchKind::tudor(),
        "accolade" | "ogee" => ArchKind::ogee(),
        _ => return None,
    })
}

impl Primitive {
    pub fn to_region(&self) -> Result<Region, String> {
        let p = |v: &[[f64; 2]]| v.iter().map(|q| P2::new(q[0], q[1])).collect::<Vec<_>>();
        Ok(match self {
            Primitive::Arch {
                kind,
                rho,
                span,
                jambs,
            } => {
                let k = arch_kind(kind, *rho).ok_or(format!("type d'arc inconnu : {kind}"))?;
                Region::simple(arch_window(k, *span, *jambs).0)
            }
            Primitive::Circle { r } => Region::circle(P2::ZERO, *r),
            Primitive::Rect { w, h } => Region::simple(rectangle(P2::ZERO, *w, *h)),
            Primitive::Polygon { n, r } => {
                Region::simple(regular_polygon(P2::ZERO, *r, (*n).max(3)))
            }
            Primitive::Points { points } => Region::simple(Loop::polygon(&p(points))),
            Primitive::Smooth { points } => Region::simple(smooth_closed(&p(points))),
            Primitive::Reuleaux { s } => Region::simple(curved_triangle(P2::ZERO, *s, 1.0)),
            Primitive::Vesica { r } => Region::simple(vesica(P2::ZERO, *r)),
            Primitive::Annulus { r, r_in } => annulus(P2::ZERO, *r, *r_in),
            Primitive::Teardrop { r, length } => Region::simple(
                teardrop(P2::ZERO, *r, P2::new(0.0, -length)).ok_or("larme invalide")?,
            ),
            Primitive::Oval { a, b } => {
                Region::simple(oval4(P2::ZERO, a.max(*b), a.min(*b), 0.55 * a.min(*b)))
            }
            Primitive::Foil { r, n, k } => Region::simple(foil_outline(P2::ZERO, *r, *n, *k)),
        })
    }
}

/// Forme d'un projet.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectShape {
    pub id: String,
    pub region: RegionJson,
    pub genome: Genome,
    /// Données propres à l'application (primitive paramétrique…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Favorite {
    pub shape: String,
    pub genome: String,
    #[serde(default)]
    pub label: String,
}

/// Fichier projet `.tracery` (JSON versionné, annexe B).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub format: String,
    pub version: String,
    pub generator: String,
    pub units: String,
    pub shapes: Vec<ProjectShape>,
    #[serde(default)]
    pub favorites: Vec<Favorite>,
}

impl Project {
    pub fn new(units: &str) -> Self {
        Project {
            format: "tracery-project".into(),
            version: "1.0.0".into(),
            generator: GENERATOR_VERSION.into(),
            units: units.into(),
            shapes: vec![],
            favorites: vec![],
        }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ShapeFile {
    Project(Project),
    Region(RegionJson),
    Prim(Primitive),
    Ref {
        #[serde(rename = "ref")]
        r: String,
    },
}

/// Forme chargée, avec un génome éventuel (fichier projet).
pub struct LoadedShape {
    pub name: String,
    pub region: Region,
    pub genome: Option<Genome>,
}

/// Charge une forme : identifiant de référence (S01…S16), fichier JSON
/// (région, primitive ou projet).
pub fn load_shape(spec: &str) -> Result<LoadedShape, String> {
    if let Some(r) = ref_shape(spec) {
        return Ok(LoadedShape {
            name: r.id.to_string(),
            region: r.region,
            genome: None,
        });
    }
    let txt = std::fs::read_to_string(spec).map_err(|e| format!("{spec} : {e}"))?;
    let name = std::path::Path::new(spec)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| spec.to_string());
    let sf: ShapeFile = serde_json::from_str(&txt).map_err(|e| format!("{spec} : {e}"))?;
    Ok(match sf {
        ShapeFile::Project(p) => {
            let s = p.shapes.into_iter().next().ok_or("projet sans forme")?;
            LoadedShape {
                name: s.id,
                region: s.region.to_region(),
                genome: Some(s.genome),
            }
        }
        ShapeFile::Region(r) => LoadedShape {
            name,
            region: r.to_region(),
            genome: None,
        },
        ShapeFile::Prim(p) => LoadedShape {
            name,
            region: p.to_region()?,
            genome: None,
        },
        ShapeFile::Ref { r } => {
            let s = ref_shape(&r).ok_or(format!("forme de référence inconnue : {r}"))?;
            LoadedShape {
                name: s.id.to_string(),
                region: s.region,
                genome: None,
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annex_b_project_parses() {
        let txt = r#"{
  "format": "remplage-project", "version": "1.0.0", "generator": "0.4.2", "units": "mm",
  "shapes": [{
    "id": "baie-1",
    "region": { "outer": [
        { "line": { "a": [-500, -1500], "b": [500, -1500] } },
        { "line": { "a": [500, -1500],  "b": [500, 0] } },
        { "arc":  { "c": [-500, 0], "r": 1000, "theta0": 0.0, "sweep": 1.0472 } },
        { "arc":  { "c": [500, 0],  "r": 1000, "theta0": 2.0944, "sweep": 1.0472 } },
        { "line": { "a": [-500, 0], "b": [-500, -1500] } } ],
      "holes": [] },
    "genome": {
      "seed": "7QK3-M2XD-9A1P-F",
      "style": { "family": "rayonnant", "depth": 4, "density": 0.60, "w0": 0.035,
                 "lambda": 0.72, "foils": [3, 4, 5, 6], "k": 1.15, "symmetry": "strict",
                 "temperature": 0.5, "canon": "soft", "candidates": 8 },
      "overrides": {
        "0/2": { "salt": 3 },
        "0/0": { "lock": { "rule": "R04", "params": { "n": 4, "k": 1.2, "phase": 0.785 } } }
      } } }],
  "favorites": [ { "shape": "baie-1", "genome": "RMP1.rayonnant.7QK3M2XD9A1PF.oWEw", "label": "quadrilobes" } ]
}"#;
        let p: Project = serde_json::from_str(txt).expect("annexe B");
        assert_eq!(
            p.shapes[0].genome.seed,
            engine::seed::parse_seed("7QK3-M2XD-9A1P-F")
        );
        assert_eq!(p.shapes[0].genome.overrides.len(), 2);
        let r = p.shapes[0].region.to_region();
        assert!(r.area() > 0.0);
    }
}
