//! R22 — Double mouchette : la cellule est partagée par un barreau en S en
//! deux mouchettes affrontées, tête-bêche (§ 4.7).
//!
//! Le S naît tangentiellement des grandes formes (principe : tracer d'abord
//! les grandes formes, puis les petites en suivant leurs tangentes) :
//! - **arrondie** (cellule circulaire) : deux demi-cercles centrés sur un
//!   diamètre, tangents intérieurement au cercle aux extrémités et entre eux
//!   au point d'inflexion (« yin-yang ») ;
//! - **brisée** (lentille, tympan, écoinçon…) : biarc G1 entre deux coins de
//!   la cellule (pointes de la lentille, ou pied du meneau et clef du
//!   tympan). Les tangentes de départ sont prises dans l'angle du coin,
//!   entre sa bissectrice et l'un des côtés, si bien que le barreau prolonge
//!   le meneau ou épouse le flanc de la forme englobante.
//!
//! Les deux moitiés sont des jours : rien n'est généré à l'intérieur d'une
//! double mouchette.

use super::*;
use crate::analysis::CellType;
use crate::genome::Params;
use geom::constr::biarc;
use geom::{ConstructionOp, Seg};
use motifs::flamboyant::s_curve_round;

pub struct DoubleMouchette;

/// R24 — Soufflet et mouchettes (voir `SouffletHead`).
pub struct SouffletHead;

/// Tangentes d'arrivée et de départ du contour au coin `p`.
fn corner_tangents(l: &Loop, p: P2, eps: f64) -> Option<(P2, P2)> {
    let n = l.segs.len();
    (0..n).find_map(|i| {
        (dist(l.segs[i].start(), p) < eps).then(|| {
            (
                l.segs[(i + n - 1) % n].tangent_end(),
                l.segs[i].tangent_start(),
            )
        })
    })
}

/// Direction intérieure (bissectrice) et demi-ouverture d'un coin.
fn inward(tin: P2, tout: P2) -> (P2, f64) {
    let lin = perp(unit(tin));
    let lout = perp(unit(tout));
    let s = lin + lout;
    let d = if norm(s) > 1e-6 {
        unit(s)
    } else {
        unit(unit(tout) - unit(tin))
    };
    // demi-ouverture : angle entre la bissectrice et le côté sortant
    let half = acos(dot(d, unit(tout)).clamp(-1.0, 1.0));
    (d, half)
}

/// Bande (épaisseur w) autour d'un segment.
fn seg_band(s: &Seg, w: f64, ext: f64) -> Loop {
    match *s {
        Seg::Line { a, b } => line_band(a, b, w, ext),
        Seg::Arc {
            c,
            r,
            theta0,
            sweep,
        } => arc_band(c, r, theta0, sweep, w, ext),
    }
}

impl DoubleMouchette {
    /// Paires d'ancres du S pour une cellule non circulaire : couples de
    /// coins (sur l'axe de symétrie s'il existe), du plus éloigné au plus
    /// proche. Dans une cellule non convexe, la paire la plus éloignée peut
    /// donner un S qui sort de la cellule : les suivantes sont essayées.
    fn anchors(a: &Analysis) -> Vec<(P2, P2)> {
        let cs = &a.corners;
        let on_axis = |p: P2| {
            a.sym_x
                .is_none_or(|ax| (p.x - ax).abs() < 1e-4 * a.bbox.diag())
        };
        let mut v: Vec<(P2, P2, f64)> = Vec::new();
        for i in 0..cs.len() {
            for j in i + 1..cs.len() {
                let (p, q) = (cs[i].p, cs[j].p);
                let d = dist(p, q);
                if !(on_axis(p) && on_axis(q)) || d < 2.0 * a.inscribed.r {
                    continue;
                }
                // bas → haut, puis gauche → droite (déterminisme)
                let (p, q) = if (p.y, p.x) <= (q.y, q.x) {
                    (p, q)
                } else {
                    (q, p)
                };
                v.push((p, q, d));
            }
        }
        v.sort_by(|x, y| y.2.total_cmp(&x.2));
        v.into_iter().take(6).map(|(p, q, _)| (p, q)).collect()
    }

    /// S candidats (le premier qui partage bien la cellule est retenu).
    fn s_curves(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Vec<Vec<Seg>> {
        if let Some(c) = a.circle {
            // arrondie : l'axe du S est tangent à l'axe du barreau englobant
            // (cercle R + w/2), et non au bord du jour. Le bord intérieur de
            // la bande (± w/2) affleure alors le bord de la cellule au point
            // de tangence : le raccord est G1, sans marche d'une demi-épaisseur
            // (l'about de la bande tombe entièrement dans la pierre).
            let theta = p.get("theta", FRAC_PI_2);
            let r = c.r + 0.5 * ctx.w;
            return vec![s_curve_round(c.c, r, theta, p.get("a", 0.5)).to_vec()];
        }
        let eps = 1e-6 * a.bbox.diag();
        let flip = if p.get("flip", 0.0) > 0.5 { -1.0 } else { 1.0 };
        let lam = p.get("beta", 0.6);
        let mut out = Vec::new();
        for (pp, qq) in Self::anchors(a) {
            let (Some((ti, to)), Some((ui, uo))) = (
                corner_tangents(&a.region.outer, pp, eps),
                corner_tangents(&a.region.outer, qq, eps),
            ) else {
                continue;
            };
            let (dp, hp) = inward(ti, to);
            let (dq, hq) = inward(ui, uo);
            // un coin rentrant (pied de meneau) admet une plus grande déviation
            let bp = lam * hp.min(0.3 * PI);
            let bq = lam * hq.min(0.3 * PI);
            // symétrie centrale du S : même sens de rotation aux deux bouts
            let tp = rotate(dp, flip * bp);
            let tq = -rotate(dq, flip * bq);
            let segs = biarc(pp, tp, qq, tq);
            if segs.len() == 2 {
                out.push(segs);
            }
        }
        out
    }

    /// Partage de la cellule par le S `s` : deux moitiés équilibrées.
    fn split(ctx: &Ctx, a: &Analysis, s: &[Seg]) -> Result<Vec<Region>, RuleError> {
        let f = &a.region;
        // le S doit rester dans la cellule (hors extrémités) ; arrondi, ses
        // extrémités sont sur l'axe du barreau englobant, à w/2 du bord
        let slack = if a.circle.is_some() { 0.5 * ctx.w } else { 0.0 };
        let margin = -slack - 1e-6 * ctx.tol.l;
        for seg in s {
            if seg.sample(40).iter().skip(1).any(|q| f.sdf(*q) < margin) {
                return Err(RuleError::Constraint("le S sort de la cellule"));
            }
        }
        // épaisseurs presque uniformes : le S a celle des autres barreaux
        let w = ctx.w;
        let bands: Vec<Loop> = s.iter().map(|g| seg_band(g, w, 0.02 * w)).collect();
        let pieces = cut_bands(f, &bands, ctx.min_opening_r());
        if pieces.len() != 2 {
            return Err(RuleError::Degenerate("partage en S ≠ 2 moitiés"));
        }
        let (a0, a1) = (pieces[0].area(), pieces[1].area());
        if a0.min(a1) < 0.3 * a0.max(a1) {
            return Err(RuleError::Constraint("moitiés déséquilibrées"));
        }
        Ok(pieces)
    }
}

impl Rule for DoubleMouchette {
    fn id(&self) -> &'static str {
        "R22"
    }
    fn name(&self) -> &'static str {
        "Double mouchette"
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only || !a.region.holes.is_empty() {
            return 0.0;
        }
        // chaque moitié doit garder un jour utile
        if a.inscribed.r < 2.0 * ctx.w + 3.0 * ctx.min_opening_r() {
            return 0.0;
        }
        if a.circle.is_some() {
            return 1.0;
        }
        if Self::anchors(a).is_empty() {
            return 0.0;
        }
        let t = |c: CellType| a.confidence(c);
        t(CellType::Lens)
            .max(0.9 * t(CellType::Tympan))
            .max(0.6 * t(CellType::Lunule))
            .max(0.5 * t(CellType::Tapered))
            .max(0.4 * t(CellType::Irregular))
            .max(0.4 * t(CellType::Triangle))
            .max(0.3 * t(CellType::Quad))
    }
    fn sample(&self, _ctx: &Ctx, a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        // orientations canoniques du S dans un oculus
        let thetas = [FRAC_PI_2, 0.0, 0.25 * PI, 0.75 * PI];
        let ti = weighted_index(rng, &[3.0, 1.5, 1.0, 1.0]).unwrap_or(0);
        let mut p = Params::new()
            .with("flip", if unif(rng) < 0.5 { 0.0 } else { 1.0 })
            .with("beta", unif_in(rng, 0.6, 0.95));
        if a.circle.is_some() {
            p = p.with("theta", thetas[ti]).with(
                "a",
                if unif(rng) < 0.6 {
                    0.5
                } else {
                    unif_in(rng, 0.42, 0.58)
                },
            );
        }
        p
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let mut err = RuleError::NotApplicable("pas de S possible");
        let mut found = None;
        for s in self.s_curves(ctx, a, p) {
            match Self::split(ctx, a, &s) {
                Ok(pieces) => {
                    found = Some((s, pieces));
                    break;
                }
                Err(e) => err = e,
            }
        }
        let (s, pieces) = found.ok_or(err)?;
        let mut construction = Vec::new();
        for g in &s {
            if let Seg::Arc { c, r, .. } = *g {
                construction.push(ConstructionEntity::Circle {
                    c,
                    r,
                    op: ConstructionOp::Mouchette,
                });
                construction.push(ConstructionEntity::point(c, ConstructionOp::Mouchette));
            }
        }
        construction.push(ConstructionEntity::point(
            s[0].end(),
            ConstructionOp::Tangent,
        ));
        let ratio = match (s[0], s[1]) {
            (Seg::Arc { r: r1, .. }, Seg::Arc { r: r2, .. }) => Some(r1.min(r2) / r1.max(r2)),
            _ => None,
        };
        // les deux mouchettes restent des jours : rien n'est généré dedans
        Ok(Expansion {
            children: vec![],
            openings: pieces,
            construction,
            key_ratio: ratio,
        })
    }
}

/// R24 — Soufflet et mouchettes : à son sommet, le meneau se dédouble en
/// deux barreaux qui en partent tangentiellement, s'écartent puis se
/// rejoignent à la clef. Ils dessinent un soufflet central, et les deux
/// résidus contre l'arc principal prennent d'eux-mêmes la forme de
/// mouchettes. C'est la composition flamboyante classique du tympan d'une
/// baie à deux lancettes. Chaque barreau est un biarc G1 : il part dans
/// l'axe du meneau (déviation β ≤ 0,3 rad) et arrive à la clef dans l'angle
/// de l'arc.
impl Rule for SouffletHead {
    fn id(&self) -> &'static str {
        "R24"
    }
    fn name(&self) -> &'static str {
        "Soufflet et mouchettes"
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        if ctx.terminal_only || !a.region.holes.is_empty() || a.circle.is_some() {
            return 0.0;
        }
        if a.sym_x.is_none() || a.inscribed.r < 3.0 * ctx.w + 4.0 * ctx.min_opening_r() {
            return 0.0;
        }
        if DoubleMouchette::anchors(a).is_empty() {
            return 0.0;
        }
        let t = |c: CellType| a.confidence(c);
        t(CellType::Tympan)
            .max(0.8 * t(CellType::Lens))
            .max(0.5 * t(CellType::Triangle))
            .max(0.4 * t(CellType::Quad))
    }
    fn sample(&self, _ctx: &Ctx, _a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        Params::new()
            .with("beta", unif_in(rng, 0.15, 0.5))
            .with("gamma", unif_in(rng, 0.5, 0.95))
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let f = &a.region;
        let ax = a.sym_x.ok_or(RuleError::NotApplicable("pas d'axe"))?;
        let eps = 1e-6 * a.bbox.diag();
        let (beta, lam) = (p.get("beta", 0.15), p.get("gamma", 0.5));
        let w = ctx.w;
        let margin = -1e-6 * ctx.tol.l;
        let mut err = RuleError::NotApplicable("pas de soufflet possible");
        for (pp, qq) in DoubleMouchette::anchors(a) {
            let (Some((ti, to)), Some((ui, uo))) = (
                corner_tangents(&f.outer, pp, eps),
                corner_tangents(&f.outer, qq, eps),
            ) else {
                continue;
            };
            let (dp, _) = inward(ti, to);
            let (dq, hq) = inward(ui, uo);
            let gamma = lam * hq.min(0.3 * PI);
            // barreau gauche, puis son miroir exact
            let left = biarc(pp, rotate(dp, beta), qq, rotate(-dq, -gamma));
            if left.len() != 2 {
                err = RuleError::Degenerate("barreau rectiligne");
                continue;
            }
            let m = Xf::mirror_x(ax);
            let right: Vec<Seg> = left.iter().map(|g| g.transform(&m).reversed()).collect();
            let bars: Vec<Seg> = left.iter().chain(right.iter()).copied().collect();
            if bars
                .iter()
                .any(|g| g.sample(40).iter().skip(1).any(|q| f.sdf(*q) < margin))
            {
                err = RuleError::Constraint("barreau hors cellule");
                continue;
            }
            // le soufflet doit s'ouvrir vers la gauche pour le barreau gauche
            let mid = left[0].end();
            // un soufflet, pas une fente : largeur ≥ 1/5 de sa hauteur
            if ax - mid.x < 0.1 * dist(pp, qq) {
                err = RuleError::Constraint("soufflet trop étroit");
                continue;
            }
            let bands: Vec<Loop> = bars.iter().map(|g| seg_band(g, w, 0.02 * w)).collect();
            let pieces = cut_bands(f, &bands, ctx.min_opening_r());
            if pieces.len() != 3 {
                err = RuleError::Degenerate("partage ≠ soufflet + 2 mouchettes");
                continue;
            }
            let mut construction = Vec::new();
            for g in &bars {
                if let Seg::Arc { c, r, .. } = *g {
                    construction.push(ConstructionEntity::point(c, ConstructionOp::Soufflet));
                    construction.push(ConstructionEntity::Circle {
                        c,
                        r,
                        op: ConstructionOp::Soufflet,
                    });
                }
            }
            construction.push(ConstructionEntity::Line {
                a: pp,
                b: qq,
                op: ConstructionOp::Axis,
            });
            let soufflet_w = 2.0 * (ax - mid.x);
            return Ok(Expansion {
                children: pieces
                    .into_iter()
                    .map(|r| ChildSpec::new(r, Some(CellType::Tapered)))
                    .collect(),
                openings: vec![],
                construction,
                key_ratio: Some(soufflet_w / dist(pp, qq)),
            });
        }
        Err(err)
    }
}
