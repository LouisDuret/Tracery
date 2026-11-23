//! Analyse et classification des régions (§ 5.2).

use geom::math::*;
use geom::sdf::max_inscribed;
use geom::{Circle, Joint, Loop, Region, Seg};
use serde::{Deserialize, Serialize};

/// Type de cellule (classification déterministe à règles).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CellType {
    Circle,
    Annulus,
    Arch,
    Lancet,
    Tympan,
    Panel,
    Triangle,
    Quad,
    Lunule,
    Lens,
    Tapered,
    Sector,
    Polygon,
    Band,
    Irregular,
}

/// Données géométriques transmises par la règle parente.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum CellMeta {
    /// Lancette en arc brisé : axe, portée, rayon d'arc, naissances, base.
    Lancet {
        x: f64,
        span: f64,
        radius: f64,
        y_spring: f64,
        y_bottom: f64,
    },
}

/// Description d'une baie (arc sur piédroits) détectée.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArchInfo {
    pub x_axis: f64,
    pub x0: f64,
    pub x1: f64,
    pub y_base: f64,
    pub y_spring: f64,
    pub y_top: f64,
}

/// Coin de la boucle extérieure.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Corner {
    pub p: P2,
    /// Angle intérieur (0 = rebroussement, π = plat).
    pub interior: f64,
}

#[derive(Clone, Debug)]
pub struct Analysis {
    pub region: Region,
    pub area: f64,
    pub perimeter: f64,
    pub bbox: geom::BBox,
    pub centroid: P2,
    pub inscribed: Circle,
    pub circularity: f64,
    pub convexity: f64,
    pub elongation: f64,
    pub corners: Vec<Corner>,
    /// Axe de symétrie vertical x = a (validé par Hausdorff).
    pub sym_x: Option<f64>,
    pub circle: Option<Circle>,
    pub arch: Option<ArchInfo>,
    pub types: Vec<(CellType, f64)>,
    pub meta: Option<CellMeta>,
}

impl Analysis {
    pub fn confidence(&self, t: CellType) -> f64 {
        self.types
            .iter()
            .find(|(u, _)| *u == t)
            .map(|(_, c)| *c)
            .unwrap_or(0.0)
    }
    pub fn is(&self, t: CellType) -> bool {
        self.confidence(t) > 0.0
    }
    pub fn sharp_corners(&self, max_interior: f64) -> Vec<Corner> {
        self.corners
            .iter()
            .copied()
            .filter(|c| c.interior < max_interior)
            .collect()
    }
}

/// Enveloppe convexe (chaîne monotone d'Andrew).
fn convex_hull_area(pts: &[P2]) -> f64 {
    let mut p: Vec<P2> = pts.to_vec();
    p.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    if p.len() < 3 {
        return 0.0;
    }
    let mut lower: Vec<P2> = Vec::new();
    for &q in &p {
        while lower.len() >= 2 && side_of(lower[lower.len() - 2], lower[lower.len() - 1], q) <= 0.0
        {
            lower.pop();
        }
        lower.push(q);
    }
    let mut upper: Vec<P2> = Vec::new();
    for &q in p.iter().rev() {
        while upper.len() >= 2 && side_of(upper[upper.len() - 2], upper[upper.len() - 1], q) <= 0.0
        {
            upper.pop();
        }
        upper.push(q);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    let n = lower.len();
    let mut a = 0.0;
    for i in 0..n {
        a += cross(lower[i], lower[(i + 1) % n]);
    }
    a.abs() * 0.5
}

/// Test de symétrie miroir par rapport à x = a (distance de Hausdorff
/// unilatérale du bord échantillonné, < `tol`).
pub fn mirror_symmetric(r: &Region, a: f64, tol: f64) -> bool {
    let step = r.perimeter() / 160.0;
    for l in r.loops() {
        for p in l.sample(step) {
            let q = P2::new(2.0 * a - p.x, p.y);
            if r.boundary_dist(q) > tol {
                return false;
            }
        }
    }
    true
}

fn corners_of(l: &Loop) -> Vec<Corner> {
    let n = l.segs.len();
    let mut out = Vec::new();
    for i in 0..n {
        if l.joints.get(i) == Some(&Joint::Tangent) {
            continue;
        }
        let prev = &l.segs[(i + n - 1) % n];
        let cur = &l.segs[i];
        let turn = geom::seg::turn_angle(prev.tangent_end(), cur.tangent_start());
        // seuil de 30° (§ 5.2)
        if turn.abs() > PI / 6.0 {
            out.push(Corner {
                p: cur.start(),
                interior: PI - turn,
            });
        }
    }
    out
}

fn detect_arch(r: &Region, sym_x: Option<f64>) -> Option<ArchInfo> {
    let ax = sym_x?;
    if !r.holes.is_empty() {
        return None;
    }
    let bb = r.bbox();
    let width = bb.size().x;
    let eps = 1e-6 * bb.diag();
    // base horizontale
    let has_base = r.outer.segs.iter().any(|s| match *s {
        Seg::Line { a, b } => {
            (a.y - bb.min.y).abs() < eps
                && (b.y - bb.min.y).abs() < eps
                && (a.x - b.x).abs() > 0.5 * width
        }
        _ => false,
    });
    if !has_base {
        return None;
    }
    // piédroits verticaux aux abscisses extrêmes
    let mut spring = bb.min.y;
    for s in &r.outer.segs {
        if let Seg::Line { a, b } = *s
            && (a.x - b.x).abs() < eps
            && ((a.x - bb.min.x).abs() < eps || (a.x - bb.max.x).abs() < eps)
        {
            spring = spring.max(a.y.max(b.y));
        }
    }
    // le sommet doit être sur l'axe
    let top_on_axis = r.outer.segs.iter().any(|s| {
        let p = s.start();
        (p.y - bb.max.y).abs() < 1e-4 * bb.diag() && (p.x - ax).abs() < 1e-4 * bb.diag()
    }) || r.outer.segs.iter().any(|s| {
        let b = s.bbox();
        b.max.y >= bb.max.y - eps && b.min.x <= ax && b.max.x >= ax
    });
    if !top_on_axis {
        return None;
    }
    Some(ArchInfo {
        x_axis: ax,
        x0: bb.min.x,
        x1: bb.max.x,
        y_base: bb.min.y,
        y_spring: spring,
        y_top: bb.max.y,
    })
}

/// Analyse complète d'une région.
pub fn analyze(region: &Region, meta: Option<CellMeta>, hint: Option<CellType>) -> Analysis {
    let area = region.area();
    let perimeter = region.perimeter();
    let bbox = region.bbox();
    let diag = bbox.diag();
    let centroid = region.centroid();
    let inscribed = max_inscribed(region, &[], 0.0, 28)
        .map(|(c, _)| c)
        .unwrap_or(Circle::new(centroid, 0.0));
    let circularity = 4.0 * PI * area / (perimeter * perimeter).max(1e-300);
    let pts = region.outer.sample(region.outer.length() / 200.0);
    let hull = convex_hull_area(&pts);
    let convexity = if hull > 0.0 {
        region.outer.area() / hull
    } else {
        1.0
    };
    let elongation = area / (PI * inscribed.r * inscribed.r).max(1e-300);
    let corners = corners_of(&region.outer);
    let tol = 0.005 * diag;
    let mut sym_x = None;
    for a in [bbox.center().x, centroid.x] {
        if mirror_symmetric(region, a, tol) {
            sym_x = Some(a);
            break;
        }
    }
    let circle = if region.holes.is_empty() {
        region.outer.as_circle(1e-9 * diag)
    } else {
        None
    };
    let arch = detect_arch(region, sym_x);

    let mut types: Vec<(CellType, f64)> = Vec::new();
    if let Some(h) = hint {
        types.push((h, 1.0));
    }
    let n_sharp = corners.iter().filter(|c| c.interior < 0.85 * PI).count();
    if circle.is_some() || (circularity > 0.97 && region.holes.is_empty()) {
        types.push((CellType::Circle, 1.0));
    }
    if region.holes.len() == 1 {
        types.push((CellType::Annulus, 0.9));
    }
    if arch.is_some() {
        types.push((CellType::Arch, 0.9));
    }
    let polygon = region.outer.is_polygon();
    if region.holes.is_empty()
        && !types
            .iter()
            .any(|t| matches!(t.0, CellType::Circle | CellType::Lancet))
    {
        match n_sharp {
            2 => {
                // lunule (un côté concave) ou lentille (deux côtés convexes)
                let concave = region
                    .outer
                    .segs
                    .iter()
                    .any(|s| matches!(s, Seg::Arc { sweep, .. } if *sweep < 0.0));
                if concave {
                    types.push((CellType::Lunule, 0.8));
                } else {
                    types.push((CellType::Lens, 0.8));
                }
            }
            3 if convexity > 0.8 => types.push((CellType::Triangle, 0.85)),
            3 => types.push((CellType::Triangle, 0.5)),
            4 => types.push((CellType::Quad, if convexity > 0.85 { 0.8 } else { 0.5 })),
            _ => {}
        }
        if polygon {
            types.push((CellType::Polygon, 0.7));
        }
        if elongation > 2.6 && corners.iter().any(|c| c.interior < 0.5 * PI) {
            types.push((CellType::Tapered, (elongation / 4.0).min(1.0)));
        }
        if elongation > 4.5 {
            types.push((CellType::Band, (elongation / 8.0).min(1.0)));
        }
    }
    if types.iter().all(|t| t.1 <= 0.5) {
        types.push((CellType::Irregular, 0.8));
    }
    // ordre déterministe, sans doublon
    types.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)));
    types.dedup_by(|a, b| a.0 == b.0);

    Analysis {
        region: region.clone(),
        area,
        perimeter,
        bbox,
        centroid,
        inscribed,
        circularity,
        convexity,
        elongation,
        corners,
        sym_x,
        circle,
        arch,
        types,
        meta,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use motifs::{ArchKind, arch_window};

    #[test]
    fn classify_window() {
        let (l, _) = arch_window(ArchKind::equilateral(), 2.0, 3.0);
        let a = analyze(&Region::simple(l), None, None);
        assert!(a.is(CellType::Arch), "{:?}", a.types);
        assert!(a.sym_x.is_some());
        let ai = a.arch.unwrap();
        assert!((ai.y_spring - 0.0).abs() < 1e-9);
        assert!((ai.x1 - ai.x0 - 2.0).abs() < 1e-9);
    }

    #[test]
    fn classify_circle_and_triangle() {
        let a = analyze(&Region::circle(P2::ZERO, 3.0), None, None);
        assert!(a.circle.is_some());
        assert!(a.is(CellType::Circle));
        let t = analyze(
            &Region::simple(Loop::polygon(&[
                P2::ZERO,
                P2::new(4.0, 0.0),
                P2::new(2.0, 3.0),
            ])),
            None,
            None,
        );
        assert!(t.is(CellType::Triangle), "{:?}", t.types);
        assert_eq!(t.corners.len(), 3);
    }
}
