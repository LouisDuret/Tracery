//! Moteur de génération de remplages : analyse, grammaire de formes
//! stochastique, règles, score, seeds hiérarchiques et génome (§ 5).
//!
//! Aucune dépendance à l'interface ni au GPU (INV-5).
#![forbid(unsafe_code)]

pub mod analysis;
pub mod derive;
pub mod design;
pub mod door;
pub mod genome;
pub mod grammar;
pub mod hidden;
pub mod iron;
pub mod refshapes;
pub mod rules;
pub mod score;
pub mod seed;
pub mod style;
pub mod validate;

pub use derive::CancelToken;
pub use design::{Design, Opening, OrnKind, Ornament, Stats, TreeNode};
pub use genome::{GENERATOR_VERSION, Genome, NodePath, Params};
pub use style::{Family, StyleParams};

use geom::clip::offset_region;
use geom::math::*;
use geom::{Region, Tol, Xf};

#[derive(Debug, thiserror::Error)]
pub enum GenError {
    #[error("forme invalide : {0}")]
    InvalidShape(String),
    #[error("génération annulée")]
    Cancelled,
}

/// Vérifie qu'une forme d'entrée est exploitable.
pub fn check_shape(shape: &Region) -> Result<(), GenError> {
    if !shape.is_finite() {
        return Err(GenError::InvalidShape("coordonnées non finies".into()));
    }
    if shape.outer.segs.is_empty() || shape.area() <= 0.0 {
        return Err(GenError::InvalidShape("aire nulle".into()));
    }
    if geom::clip::self_intersects(&shape.outer) {
        return Err(GenError::InvalidShape("contour auto-intersectant".into()));
    }
    Ok(())
}

/// Facteur d'échelle exact (puissance de 2) ramenant L vers 100.
fn norm_scale(l: f64) -> f64 {
    let k = round(ln(100.0 / l) / ln(2.0));
    powf(2.0, k)
}

/// Génère le remplage d'une forme pour un génome donné.
///
/// (forme, génome, version) ⇒ géométrie identique (INV-3) : seul l'ordre
/// fixe des règles et des candidats, le RNG ChaCha8 et `libm` interviennent.
pub fn generate(shape: &Region, genome: &Genome, cancel: &CancelToken) -> Result<Design, GenError> {
    #[cfg(not(target_arch = "wasm32"))]
    let t0 = std::time::Instant::now();
    check_shape(shape)?;
    if genome.style.iron {
        return iron::generate_iron(shape, genome, cancel);
    }
    if genome.style.door {
        return door::generate_door(shape, genome, cancel);
    }
    let shape = Region::new(shape.outer.clone(), shape.holes.clone());
    let l = shape.bbox().diag();
    let s = norm_scale(l);
    let fwd = Xf::scale(s);
    let back = Xf::scale(1.0 / s);
    let shape_n = shape.transform(&fwd);
    let style = genome.style.clamped();
    let def = style::style_def(style.family);
    let area = shape_n.area();
    let w_min = style.w_min * s;
    let w_base = (style.w0 * area.sqrt()).max(0.012 * area.sqrt());
    // Épaisseurs presque uniformes : le cadre (ordre 0) garde w₀, tous les
    // barreaux du remplage partent de 0,55·w₀ et ne décroissent que
    // doucement (λ par ordre, jamais sous 80 % du premier ordre), pour
    // éviter des écarts d'épaisseur disgracieux.
    let w_bar = 0.55 * w_base;
    let widths: Vec<f64> = (0..10)
        .map(|k| {
            if k == 0 {
                w_base.max(w_min)
            } else {
                (w_bar * powf(style.lambda, (k - 1) as f64).max(0.8)).max(w_min)
            }
        })
        .collect();
    let frame_w = if style.frame {
        (style.w0 * area.sqrt()).max(w_min)
    } else {
        0.0
    };
    let tol = Tol::new(shape_n.bbox().diag());

    let mut deriver = derive::Deriver {
        genome,
        style: style.clone(),
        def,
        tol,
        widths: widths.clone(),
        w_min,
        root_area: area,
        cancel,
        stats: Stats::default(),
    };
    let mut pieces = if frame_w > 0.0 {
        offset_region(&shape_n, frame_w)
    } else {
        vec![shape_n.clone()]
    };
    rules::sort_regions(&mut pieces);
    let root = NodePath::root();
    let mut sub = derive::Sub {
        tree: vec![TreeNode {
            path: root.to_string(),
            depth: 0,
            rule: if frame_w > 0.0 {
                "R00".into()
            } else {
                "R00-".into()
            },
            params: Params::new().with("w0", frame_w),
            types: vec![],
            area,
            inscribed: 0.0,
            local_score: 1.0,
            twin_of: None,
            bbox: shape_n.bbox(),
            locked: false,
            ratio: None,
            candidates: 1,
            region: Some(shape_n.clone()),
        }],
        ..Default::default()
    };
    for (i, p) in pieces.iter().enumerate() {
        let s = deriver.derive(p, &root.child(i), 1, None, None);
        sub.openings.extend(s.openings);
        sub.construction.extend(s.construction);
        sub.tree.extend(s.tree);
        sub.warnings.extend(s.warnings);
    }
    if cancel.is_cancelled() {
        return Err(GenError::Cancelled);
    }
    let sym_x = analysis::analyze(&shape_n, None, None).sym_x;
    let score = score::global_score(
        &shape_n,
        &sub.openings,
        &sub.tree,
        &widths,
        &style,
        def,
        sym_x,
    );

    // retour aux unités du document (facteur exact)
    let openings = sub
        .openings
        .iter()
        .map(|o| o.transform(&back, o.path.clone()))
        .collect();
    let construction = sub
        .construction
        .iter()
        .map(|c| c.transform(&back))
        .collect();
    let tree = sub
        .tree
        .into_iter()
        .map(|mut n| {
            n.area /= s * s;
            n.inscribed /= s;
            n.bbox = geom::BBox {
                min: back.apply(n.bbox.min),
                max: back.apply(n.bbox.max),
            };
            n.region = n.region.map(|r| r.transform(&back));
            n
        })
        .collect();
    let mut stats = deriver.stats;
    #[cfg(not(target_arch = "wasm32"))]
    {
        stats.millis = t0.elapsed().as_secs_f64() * 1000.0;
    }
    let mut d = Design {
        version: GENERATOR_VERSION.to_string(),
        shape,
        openings,
        construction,
        tree,
        score,
        stats,
        widths: std::iter::once(frame_w / s)
            .chain(widths.iter().skip(1).map(|w| w / s))
            .collect(),
        warnings: sub.warnings,
        door: false,
        iron: false,
        ornaments: Vec::new(),
    };
    d.stats.primitives = d.primitive_count();
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gen_ref(id: &str, seed: u64, fam: Family) -> Design {
        let s = refshapes::ref_shape(id).unwrap();
        let g = Genome::new(seed, StyleParams::for_family(fam));
        generate(&s.region, &g, &CancelToken::new()).unwrap()
    }

    #[test]
    fn s01_generates_valid() {
        for seed in 0..4 {
            let d = gen_ref("S01", seed, Family::Rayonnant);
            assert!(!d.openings.is_empty());
            let v = validate::validate(&d, 0.0, 0.0);
            assert!(v.is_empty(), "seed {seed}: {v:?}");
        }
    }

    #[test]
    fn deterministic() {
        let a = gen_ref("S10", 7, Family::Flamboyant);
        let b = gen_ref("S10", 7, Family::Flamboyant);
        assert_eq!(
            serde_json::to_string(&a.openings).unwrap(),
            serde_json::to_string(&b.openings).unwrap()
        );
    }
}
