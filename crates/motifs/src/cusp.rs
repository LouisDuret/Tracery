//! Redentage générique (§ 5.6) et têtes trilobées.
//!
//! Les centres cᵢ des lobes sont placés sur Γ = ∂(O ⊖ ρ) ; chaque disque
//! D(cᵢ, ρ) est alors tangent au bord. L'espacement le long de Γ vaut 2ρ/k′,
//! ajusté pour diviser exactement la longueur de Γ :
//!   O_redenté = (O ⊖ ρ) ∪ ⋃ᵢ D(cᵢ, ρ).

use geom::clip::{intersect, offset_region, union, union_loop};
use geom::constr::inter_circle_circle;
use geom::math::*;
use geom::{Circle, ConstructionEntity, ConstructionOp, Loop, Region, Tol};

/// Abscisse curviligne du point le plus haut d'une boucle (départage par la
/// proximité de l'axe `axis_x` s'il est donné).
fn top_param(l: &Loop, axis_x: Option<f64>) -> f64 {
    let mut best = (f64::NEG_INFINITY, 0.0);
    let mut acc = 0.0;
    for s in &l.segs {
        let len = s.length();
        let n = ((len / (l.length() / 512.0)).ceil() as usize).clamp(1, 512);
        for i in 0..n {
            let t = i as f64 / n as f64;
            let p = s.point_at(t);
            let key = match axis_x {
                Some(x) => p.y - 4.0 * (p.x - x).abs(),
                None => p.y,
            };
            if key > best.0 {
                best = (key, acc + t * len);
            }
        }
        acc += len;
    }
    best.1
}

/// Résultat d'un redentage.
pub struct Cusped {
    pub regions: Vec<Region>,
    pub lobes: Vec<Circle>,
}

/// Redente le jour `o` avec des lobes de rayon `rho` et un chevauchement
/// `kp` ∈ [1,05 ; 1,4]. Renvoie `None` si l'offset intérieur est vide.
pub fn cusp_region(o: &Region, rho: f64, kp: f64, axis_x: Option<f64>) -> Option<Cusped> {
    let g = offset_region(o, rho);
    if g.is_empty() {
        return None;
    }
    let mut lobes = Vec::new();
    for gr in &g {
        let len = gr.outer.length();
        let n = (round(len * kp / (2.0 * rho)) as usize).max(3);
        let u0 = top_param(&gr.outer, axis_x);
        for i in 0..n {
            let (p, _) = gr.outer.point_at_length(u0 + len * i as f64 / n as f64);
            lobes.push(Circle::new(p, rho));
        }
    }
    let mut res = g;
    for l in &lobes {
        res = union_loop(&res, &l.to_loop());
    }
    Some(Cusped {
        regions: res,
        lobes,
    })
}

/// Entités de construction d'un redentage.
pub fn cusp_construction(c: &Cusped) -> Vec<ConstructionEntity> {
    c.lobes
        .iter()
        .map(|l| ConstructionEntity::point(l.c, ConstructionOp::Cusp))
        .collect()
}

/// Tête trilobée brisée d'une lancette en arc brisé de portée `s` et de
/// rayon `r` (naissances à y = 0, axe x = 0) :
/// - lobe supérieur **brisé** : l'arc de la lancette rentré de δ (arcs de
///   mêmes centres, de rayon r − δ), qui garde donc la pointe gothique ;
/// - deux lobes d'épaule de rayon ρₛ, tangents à la fois au piédroit et à
///   l'arc aux naissances (centre sur la ligne des naissances) ;
/// - corps rectangulaire jusqu'à `y_bottom`.
///
/// Les redents naissent à l'intersection des lobes d'épaule et du lobe
/// supérieur.
pub fn trefoil_lancet(
    s: f64,
    r: f64,
    y_bottom: f64,
    shoulder: f64,
    delta: f64,
) -> Option<(Vec<Region>, Vec<ConstructionEntity>)> {
    let a = s / 2.0;
    if r < a || delta <= 0.0 || delta >= 0.3 * s {
        return None;
    }
    let d = delta;
    let rs = shoulder * s;
    // centres des arcs de la lancette : l'arc droit a son centre à gauche
    let c_right = P2::new(a - r, 0.0);
    let c_left = P2::new(r - a, 0.0);
    let left = Circle::new(P2::new(-a + rs, 0.0), rs);
    let right = Circle::new(P2::new(a - rs, 0.0), rs);
    // le lobe d'épaule doit couper le lobe supérieur au-dessus des naissances
    // (redent lisible) sans l'englober
    let tol = Tol::new(s * 4.0);
    let xs = inter_circle_circle(&right, &Circle::new(c_right, r - d), &tol);
    let cusp = xs.into_iter().find(|p| p.y > 1e-9 * s)?;
    if cusp.y < 0.15 * rs {
        return None;
    }
    let top = intersect(
        &[Region::circle(c_right, r - d)],
        &Region::circle(c_left, r - d),
    );
    let body = Region::simple(Loop::polygon(&[
        P2::new(-a, y_bottom),
        P2::new(a, y_bottom),
        P2::new(a, 0.0),
        P2::new(-a, 0.0),
    ]));
    let mut res = vec![body];
    for t in &top {
        res = union(&res, t);
    }
    for c in [&left, &right] {
        res = union_loop(&res, &c.to_loop());
    }
    let cons = vec![
        ConstructionEntity::point(c_right, ConstructionOp::Cusp),
        ConstructionEntity::point(c_left, ConstructionOp::Cusp),
        ConstructionEntity::point(left.c, ConstructionOp::Cusp),
        ConstructionEntity::point(right.c, ConstructionOp::Cusp),
        ConstructionEntity::point(cusp, ConstructionOp::Cusp),
        ConstructionEntity::point(P2::new(-cusp.x, cusp.y), ConstructionOp::Cusp),
    ];
    Some((res, cons))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cusp_square() {
        let o = Region::simple(crate::shapes::rectangle(P2::ZERO, 4.0, 4.0));
        let c = cusp_region(&o, 0.5, 1.15, Some(0.0)).expect("redentage");
        assert_eq!(c.regions.len(), 1);
        let a = c.regions[0].area();
        assert!(a < o.area() && a > 0.5 * o.area(), "{a}");
    }

    #[test]
    fn trefoil_head() {
        let (r, _) = trefoil_lancet(2.0, 2.0, -3.0, 0.25, 0.2).expect("tête");
        assert_eq!(r.len(), 1);
        assert!(r[0].holes.is_empty());
    }
}
