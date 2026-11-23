//! Dérivation stochastique (annexe A.1).
//!
//! Pour chaque nœud : analyse, initialisation du RNG à partir de la seed
//! hiérarchique, tirage de K candidats (règle ∝ poids_style × poids
//! d'application, puis paramètres), application, score local, choix par
//! softmax(score/T), puis dérivation récursive des enfants (ou copie exacte
//! d'un jumeau par isométrie).

use crate::analysis::{Analysis, CellMeta, CellType, analyze};
use crate::design::{Opening, Stats, TreeNode};
use crate::genome::{Genome, NodePath, Params};
use crate::rules::{
    ChildSpec, Ctx, Expansion, Rule, fits_inside, registry, rule_by_id, to_openings, unif,
    weighted_index,
};
use crate::score::local_score;
use crate::seed::{derived, node_seed};
use crate::style::{StyleDef, StyleParams, SymMode};
use geom::math::*;
use geom::{ConstructionEntity, Region, Tol, Xf};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::SeedableRng;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Jeton d'annulation partagé.
#[derive(Clone, Default)]
pub struct CancelToken(pub Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Sous-résultat d'un nœud.
#[derive(Clone, Debug, Default)]
pub struct Sub {
    pub openings: Vec<Opening>,
    pub construction: Vec<ConstructionEntity>,
    pub tree: Vec<TreeNode>,
    pub warnings: Vec<String>,
}

fn rename(path: &str, from: &str, to: &str) -> String {
    if let Some(rest) = path.strip_prefix(from) {
        format!("{to}{rest}")
    } else {
        path.to_string()
    }
}

impl Sub {
    /// Copie par isométrie d'un sous-arbre jumeau, chemins renommés.
    fn transformed(&self, x: &Xf, from: &str, to: &str) -> Sub {
        Sub {
            openings: self
                .openings
                .iter()
                .map(|o| o.transform(x, rename(&o.path, from, to)))
                .collect(),
            construction: self.construction.iter().map(|c| c.transform(x)).collect(),
            tree: self
                .tree
                .iter()
                .map(|n| {
                    let mut m = n.clone();
                    m.twin_of = Some(n.path.clone());
                    m.path = rename(&n.path, from, to);
                    let b = n.bbox;
                    let (p, q) = (x.apply(b.min), x.apply(b.max));
                    m.bbox = geom::BBox::from_pts(p, q);
                    m.region = n.region.as_ref().map(|r| r.transform(x));
                    m
                })
                .collect(),
            warnings: vec![],
        }
    }
    fn append(&mut self, o: Sub) {
        self.openings.extend(o.openings);
        self.construction.extend(o.construction);
        self.tree.extend(o.tree);
        self.warnings.extend(o.warnings);
    }
}

/// Générateur : état partagé d'une dérivation.
pub struct Deriver<'a> {
    pub genome: &'a Genome,
    pub style: StyleParams,
    pub def: &'static StyleDef,
    pub tol: Tol,
    pub widths: Vec<f64>,
    pub w_min: f64,
    pub root_area: f64,
    pub cancel: &'a CancelToken,
    pub stats: Stats,
}

impl Deriver<'_> {
    pub fn width(&self, order: usize) -> f64 {
        self.widths[order.min(self.widths.len() - 1)]
    }

    fn ctx(&self, depth: u8, a: &Analysis) -> Ctx<'_> {
        let w = self.width(depth as usize);
        let terminal_only = depth >= self.style.depth || a.inscribed.r < 1.5 * w;
        Ctx {
            style: &self.style,
            def: self.def,
            tol: self.tol,
            depth,
            max_depth: self.style.depth,
            w,
            w_next: self.width(depth as usize + 1),
            w_min: self.w_min,
            root_area: self.root_area,
            sym: self.style.sym_mode(),
            terminal_only,
        }
    }

    /// Dérive un nœud.
    pub fn derive(
        &mut self,
        region: &Region,
        path: &NodePath,
        depth: u8,
        hint: Option<CellType>,
        meta: Option<CellMeta>,
    ) -> Sub {
        let pstr = path.to_string();
        let a = analyze(region, meta, hint);
        let ctx = self.ctx(depth, &a);
        let role = crate::grammar::role(&a, hint, self.root_area, ctx.w);
        let ov = self.genome.override_for(path).cloned().unwrap_or_default();
        let seed = node_seed(self.genome.seed, &path.0, ov.salt);
        let mut rng = ChaCha8Rng::seed_from_u64(seed);

        // (1) candidats, (2) paramètres
        let locked = ov.lock.is_some();
        let forced = ov.force_rule.clone();
        let sample = |rng: &mut ChaCha8Rng, force: Option<&str>| {
            let mut cands: Vec<(&'static dyn Rule, Params)> = Vec::new();
            let rules: Vec<&'static dyn Rule> = registry()
                .iter()
                .map(|b| b.as_ref())
                .filter(|r| !ctx.terminal_only || r.terminal())
                .filter(|r| force.is_none_or(|f| r.id() == f))
                .collect();
            let weights: Vec<f64> = rules
                .iter()
                .map(|r| {
                    // une règle imposée par l'utilisateur ignore le poids du style
                    let sw = if force.is_some() {
                        1.0
                    } else {
                        self.def.weight(r.id()) as f64
                    };
                    // une grande cellule de premier niveau doit être
                    // subdivisée : les règles terminales y sont rarissimes
                    let big_root = force.is_none()
                        && ctx.depth <= 1
                        && !ctx.terminal_only
                        && r.terminal()
                        && a.inscribed.r > 6.0 * ctx.w;
                    // place de l'ornement dans la grammaire gothique
                    let gf = if force.is_some() {
                        1.0
                    } else {
                        crate::grammar::fit(role, r.id())
                    };
                    if sw <= 0.0 || gf <= 0.0 {
                        0.0
                    } else if big_root {
                        0.02 * sw * gf * r.accepts(&ctx, &a)
                    } else {
                        sw * gf * r.accepts(&ctx, &a)
                    }
                })
                .collect();
            let wmax = weights.iter().copied().fold(0.0, f64::max).max(1e-12);
            let priors: Vec<(&'static str, f64)> = rules
                .iter()
                .zip(&weights)
                .map(|(r, w)| (r.id(), w / wmax))
                .collect();
            for _ in 0..self.style.candidates.max(1) {
                if let Some(i) = weighted_index(rng, &weights) {
                    let p = rules[i].sample(&ctx, &a, rng);
                    cands.push((rules[i], p));
                }
            }
            (cands, priors)
        };
        // candidats évalués en parallèle (application, validité, score
        // local) ; les résultats sont repris dans l'ordre du tirage, ce qui
        // garde la génération déterministe
        let evaluate = |cands: Vec<(&'static dyn Rule, Params)>,
                        priors: &[(&'static str, f64)],
                        n_eval: &mut usize,
                        n_fail: &mut usize| {
            use rayon::prelude::*;
            type Scored = (&'static dyn Rule, Params, Expansion, f64);
            let cancel = self.cancel;
            let results: Vec<Option<Result<Scored, (&'static str, String)>>> = cands
                .into_par_iter()
                .map(|(r, p)| {
                    if cancel.is_cancelled() {
                        return None;
                    }
                    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        r.apply(&ctx, &a, &p)
                    }));
                    Some(match res {
                        Ok(Ok(e)) if expansion_valid(&e, &a, &ctx) => {
                            let prior = priors
                                .iter()
                                .find(|p| p.0 == r.id())
                                .map(|p| p.1)
                                .unwrap_or(1.0);
                            let s = local_score(&ctx, &a, r.id(), &e, prior);
                            Ok((r, p, e, s))
                        }
                        Ok(Ok(_)) => Err((r.id(), "expansion invalide".to_string())),
                        Ok(Err(e)) => Err((r.id(), e.to_string())),
                        Err(_) => Err((r.id(), "panique".to_string())),
                    })
                })
                .collect();
            let mut scored: Vec<Scored> = Vec::new();
            for res in results.into_iter().flatten() {
                *n_eval += 1;
                match res {
                    Ok(x) => scored.push(x),
                    Err((id, why)) => {
                        *n_fail += 1;
                        if trace() {
                            eprintln!("[trace] {pstr} {id} : {why}");
                        }
                    }
                }
            }
            scored
        };
        let (mut n_eval, mut n_fail) = (0usize, 0usize);
        let mut warnings = Vec::new();
        let mut scored = if let Some(lock) = &ov.lock {
            match rule_by_id(&lock.rule) {
                Some(r) => evaluate(
                    vec![(r, lock.params.clone())],
                    &[],
                    &mut n_eval,
                    &mut n_fail,
                ),
                None => vec![],
            }
        } else {
            let (c, p) = sample(&mut rng, forced.as_deref());
            evaluate(c, &p, &mut n_eval, &mut n_fail)
        };
        if scored.is_empty() && (locked || forced.is_some()) && !self.cancel.is_cancelled() {
            // surcharge devenue inapplicable (forme modifiée) : tirage normal
            warnings.push(format!("{pstr} : surcharge inapplicable, ignorée"));
            let (c, p) = sample(&mut rng, None);
            scored = evaluate(c, &p, &mut n_eval, &mut n_fail);
        }

        let n_valid = scored.len();
        let (rule_id, params, exp, lscore) = if scored.is_empty() {
            // dernier recours (INV-4) : jour simple, ou plein si trop petit
            let e = Expansion {
                openings: if a.inscribed.r >= ctx.min_opening_r() {
                    vec![region.clone()]
                } else {
                    vec![]
                },
                ..Default::default()
            };
            ("fallback".to_string(), Params::new(), e, 0.0)
        } else {
            let t = (self.style.temperature * 0.08).max(1e-6);
            let m = scored.iter().map(|s| s.3).fold(f64::NEG_INFINITY, f64::max);
            let pick = if self.style.temperature <= 1e-9 {
                // premier meilleur (ordre fixe)
                scored.iter().position(|s| s.3 == m).unwrap_or(0)
            } else {
                let w: Vec<f64> = scored.iter().map(|s| exp((s.3 - m) / t)).collect();
                weighted_index(&mut rng, &w).unwrap_or(0)
            };
            let (r, p, e, s) = scored.swap_remove(pick);
            (r.id().to_string(), p, e, s)
        };
        // fin de l'emprunt de `ctx` (NLL) avant la récursion
        self.stats.nodes += 1;
        self.stats.candidates_evaluated += n_eval;
        self.stats.candidates_failed += n_fail;

        let mut sub = Sub {
            openings: to_openings(&exp.openings, depth, &pstr, &rule_id),
            construction: exp.construction.clone(),
            tree: vec![TreeNode {
                path: pstr.clone(),
                depth,
                rule: rule_id.clone(),
                params,
                types: a
                    .types
                    .iter()
                    .map(|t| format!("{:?}", t.0).to_lowercase())
                    .collect(),
                area: a.area,
                inscribed: a.inscribed.r,
                local_score: lscore,
                twin_of: None,
                bbox: a.bbox,
                locked,
                ratio: exp.key_ratio,
                candidates: n_valid,
                region: Some(region.clone()),
            }],
            warnings,
        };

        // jumelage automatique des cellules congruentes (miroir / rotation)
        let mut children = exp.children;
        let twin_roll = unif(&mut ChaCha8Rng::seed_from_u64(derived(seed, 0x7417)));
        // une baie gothique est symétrique : hors style libre, les cellules
        // congruentes (miroir, rotation) reçoivent toujours le même dessin
        if twin_roll < self.def.twin_prob || self.style.sym_mode() != SymMode::Free {
            auto_twins(&mut children, &a, self.tol.l);
        }

        let mut subs: Vec<Sub> = Vec::with_capacity(children.len());
        for (i, ch) in children.iter().enumerate() {
            let cpath = path.child(i);
            let has_override = self
                .genome
                .overrides
                .keys()
                .any(|k| NodePath::parse(k).is_some_and(|p| p.starts_with(&cpath)));
            let s = match ch.twin {
                Some((j, x)) if j < i && !has_override => {
                    let from = path.child(j).to_string();
                    subs[j].transformed(&x, &from, &cpath.to_string())
                }
                _ => self.derive(&ch.region, &cpath, depth + 1, ch.hint, ch.meta),
            };
            subs.push(s);
        }
        for s in subs {
            sub.append(s);
        }
        sub
    }
}

/// Journal des candidats rejetés (variable d'environnement TRACERY_TRACE).
fn trace() -> bool {
    static T: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *T.get_or_init(|| std::env::var_os("TRACERY_TRACE").is_some())
}

/// Contrôles rapides d'une expansion (contraintes dures locales).
fn expansion_valid(e: &Expansion, a: &Analysis, ctx: &Ctx) -> bool {
    let eps = 1e-6 * ctx.tol.l;
    for r in e
        .openings
        .iter()
        .chain(e.children.iter().map(|c| &c.region))
    {
        if !r.is_finite() || r.area() <= 0.0 || r.outer.segs.is_empty() {
            return false;
        }
        // contour pincé ou croisé (EQ-05) : candidat rejeté, une autre
        // alternative sera retenue
        if geom::clip::self_intersects(&r.outer) || r.holes.iter().any(geom::clip::self_intersects)
        {
            return false;
        }
        for s in r.segs() {
            if let geom::Seg::Arc { r: rr, .. } = s
                && *rr < 1e-6 * ctx.tol.l
            {
                return false;
            }
        }
    }
    for o in &e.openings {
        if !fits_inside(&a.region, &o.outer, -eps) {
            return false;
        }
    }
    // une expansion non terminale doit produire au moins un enfant ou un jour
    !(e.children.is_empty() && e.openings.is_empty() && a.inscribed.r > 3.0 * ctx.w)
}

/// Détection des cellules sœurs congruentes : image miroir par rapport à
/// l'axe vertical de la cellule, ou rotation autour du centre d'un cercle.
fn auto_twins(children: &mut [ChildSpec], a: &Analysis, l: f64) {
    let info: Vec<(P2, f64, f64)> = children
        .iter()
        .map(|c| {
            (
                c.region.bbox().center(),
                c.region.area(),
                c.region.perimeter(),
            )
        })
        .collect();
    let tol = 2e-3 * l;
    let is_src: Vec<bool> = children.iter().map(|c| c.twin.is_none()).collect();
    for j in 0..children.len() {
        if children[j].twin.is_some() {
            continue;
        }
        for i in 0..j {
            if !is_src[i] || children[i].twin.is_some() {
                continue;
            }
            let (ci, ai, pi) = info[i];
            let (cj, aj, pj) = info[j];
            if (ai - aj).abs() > 0.02 * ai.max(aj) || (pi - pj).abs() > 0.02 * pi.max(pj) {
                continue;
            }
            if let Some(ax) = a.sym_x {
                let m = Xf::mirror_x(ax);
                if dist(m.apply(ci), cj) < tol
                    && same_shape(&children[i].region, &children[j].region, &m, tol)
                {
                    children[j].twin = Some((i, m));
                    break;
                }
            }
            if let Some(c) = a.circle {
                let ang = angle_of(cj - c.c) - angle_of(ci - c.c);
                let x = Xf::rotate_about(c.c, ang);
                if dist(x.apply(ci), cj) < tol
                    && (dist(ci, c.c) - dist(cj, c.c)).abs() < tol
                    && same_shape(&children[i].region, &children[j].region, &x, tol)
                {
                    children[j].twin = Some((i, x));
                    break;
                }
            }
        }
    }
}

/// L'image de `a` par `x` coïncide-t-elle avec `b` (bords échantillonnés) ?
fn same_shape(a: &Region, b: &Region, x: &Xf, tol: f64) -> bool {
    let step = a.perimeter() / 48.0;
    a.outer
        .sample(step)
        .into_iter()
        .all(|p| b.boundary_dist(x.apply(p)) < tol)
}
