//! Document : formes (primitives paramétriques ou contours libres), chacune
//! avec son génome (EF-26), favoris ; conversion vers le fichier projet.

use engine::{Genome, StyleParams, WindowKind};
use geom::clip;
use geom::math::*;
use geom::{Loop, Region, Seg, Xf};
use io::project::{Favorite, Project, ProjectShape, RegionJson};
use motifs::ArchKind;
use motifs::shapes::*;
use serde::{Deserialize, Serialize};

/// Type de primitive (EF-10, EF-11).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PrimKind {
    Arch(ArchKind),
    Rect,
    Circle,
    Polygon(usize),
    Vesica,
    Reuleaux,
    Oval,
    Annulus,
    Teardrop,
    Foil(usize),
}

impl PrimKind {
    pub fn label(&self) -> String {
        match self {
            PrimKind::Arch(k) => format!("Arc {}", k.name()),
            PrimKind::Rect => "Rectangle".into(),
            PrimKind::Circle => "Cercle".into(),
            PrimKind::Polygon(n) => format!("Polygone à {n} côtés"),
            PrimKind::Vesica => "Vesica".into(),
            PrimKind::Reuleaux => "Triangle curviligne".into(),
            PrimKind::Oval => "Ovale à quatre centres".into(),
            PrimKind::Annulus => "Couronne".into(),
            PrimKind::Teardrop => "Larme".into(),
            PrimKind::Foil(n) => format!("Contour polylobé ({n})"),
        }
    }
}

/// Primitive paramétrique inscrite dans une boîte (centre, largeur,
/// hauteur), tournée de `angle` autour de son centre (EF-12).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrimSpec {
    pub kind: PrimKind,
    pub center: P2,
    pub w: f64,
    pub h: f64,
    pub angle: f64,
}

impl PrimSpec {
    pub fn build(&self) -> Option<Region> {
        let (w, h) = (self.w.abs().max(1e-6), self.h.abs().max(1e-6));
        let c = self.center;
        let m = w.min(h);
        let r = match self.kind {
            PrimKind::Arch(k) => {
                let ha = motifs::arch_curve(k, w).height;
                let jamb = (h - ha).max(0.0);
                let (l, _) = motifs::arch_window(k, w, jamb);
                // centre de la boîte : piédroits en bas, clé en haut
                let mid = 0.5 * (ha - jamb);
                Region::simple(l.transform(&Xf::translate(c - P2::new(0.0, mid))))
            }
            PrimKind::Rect => Region::simple(rectangle(c, w, h)),
            PrimKind::Circle => Region::circle(c, m / 2.0),
            PrimKind::Polygon(n) => Region::simple(regular_polygon(c, m / 2.0, n.max(3))),
            PrimKind::Vesica => {
                let rr = w.min(h / 3f64.sqrt());
                Region::simple(vesica(c, rr))
            }
            PrimKind::Reuleaux => {
                // le centre du triangle est aux 2/3 de la hauteur s·√3/2 :
                // on recentre la boîte
                let s = m;
                let l = curved_triangle(P2::ZERO, s, 1.0);
                let bb = l.bbox();
                Region::simple(l.transform(&Xf::translate(c - bb.center())))
            }
            PrimKind::Oval => {
                let (a, b) = (w.max(h) / 2.0, w.min(h) / 2.0);
                if b >= 0.97 * a {
                    Region::circle(c, a)
                } else {
                    let l = oval4(P2::ZERO, a, b, 0.55 * b);
                    let l = if h > w {
                        l.transform(&Xf::rotate_about(P2::ZERO, FRAC_PI_2))
                    } else {
                        l
                    };
                    Region::simple(l.transform(&Xf::translate(c)))
                }
            }
            PrimKind::Annulus => annulus(c, m / 2.0, 0.33 * m / 2.0),
            PrimKind::Teardrop => {
                let rr = (w / 2.0).min(h / 2.4);
                let l = teardrop(
                    c + P2::new(0.0, h / 2.0 - rr),
                    rr,
                    c - P2::new(0.0, h / 2.0),
                )?;
                Region::simple(l)
            }
            PrimKind::Foil(n) => Region::simple(foil_outline(c, m / 2.0, n.max(3), 1.15)),
        };
        Some(if self.angle != 0.0 {
            r.transform(&Xf::rotate_about(c, self.angle))
        } else {
            r
        })
    }
}

/// Forme du document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapeItem {
    pub id: u64,
    pub name: String,
    pub prim: Option<PrimSpec>,
    pub free: Option<Region>,
    pub genome: Genome,
}

impl ShapeItem {
    pub fn region(&self) -> Option<Region> {
        match (&self.prim, &self.free) {
            (Some(p), _) => p.build(),
            (None, Some(r)) => Some(r.clone()),
            _ => None,
        }
    }
    /// Conversion explicite en contour libre.
    pub fn make_free(&mut self) {
        if let Some(r) = self.region() {
            self.prim = None;
            self.free = Some(r);
        }
    }
    pub fn transform(&mut self, x: &Xf) {
        if let Some(p) = &mut self.prim
            && x.scale == 1.0
            && !x.mirror
        {
            // déplacement / rotation : la primitive reste paramétrique
            p.center = x.apply(p.center);
            p.angle += x.angle;
            return;
        }
        self.make_free();
        if let Some(r) = &mut self.free {
            *r = r.transform(x);
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub shapes: Vec<ShapeItem>,
    pub next_id: u64,
    pub favorites: Vec<Favorite>,
}

impl Document {
    pub fn add(
        &mut self,
        name: &str,
        prim: Option<PrimSpec>,
        free: Option<Region>,
        style: StyleParams,
        seed: u64,
    ) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.shapes.push(ShapeItem {
            id,
            name: format!("{name} {id}"),
            prim,
            free,
            genome: Genome::new(seed, style),
        });
        id
    }
    pub fn get(&self, id: u64) -> Option<&ShapeItem> {
        self.shapes.iter().find(|s| s.id == id)
    }
    pub fn get_mut(&mut self, id: u64) -> Option<&mut ShapeItem> {
        self.shapes.iter_mut().find(|s| s.id == id)
    }
    pub fn remove(&mut self, id: u64) {
        self.shapes.retain(|s| s.id != id);
    }

    pub fn to_project(&self) -> Project {
        let mut p = Project::new("mm");
        for s in &self.shapes {
            if let Some(r) = s.region() {
                p.shapes.push(ProjectShape {
                    id: s.name.clone(),
                    region: RegionJson::from_region(&r),
                    genome: s.genome.clone(),
                    extra: s.prim.and_then(|pr| serde_json::to_value(pr).ok()),
                });
            }
        }
        p.favorites = self.favorites.clone();
        p
    }

    pub fn from_project(p: &Project) -> Document {
        let mut d = Document::default();
        for s in &p.shapes {
            d.next_id += 1;
            let prim: Option<PrimSpec> =
                s.extra.clone().and_then(|v| serde_json::from_value(v).ok());
            d.shapes.push(ShapeItem {
                id: d.next_id,
                name: s.id.clone(),
                free: if prim.is_some() {
                    None
                } else {
                    Some(s.region.to_region())
                },
                prim,
                genome: s.genome.clone(),
            });
        }
        d.favorites = p.favorites.clone();
        d
    }
}

// ------------------------------------------------------------ ouvrage

/// Catégorie d'ouvrage d'une forme (ADR-29).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Fenetre,
    Porte,
}

impl Category {
    pub const ALL: [Category; 2] = [Category::Fenetre, Category::Porte];
    pub fn label(self) -> &'static str {
        match self {
            Category::Fenetre => "Fenêtre",
            Category::Porte => "Porte",
        }
    }
    pub fn types(self) -> &'static [Ouvrage] {
        match self {
            Category::Fenetre => &[Ouvrage::Remplage, Ouvrage::Meneaux, Ouvrage::Vitrail],
            Category::Porte => &[Ouvrage::Encadrement, Ouvrage::PorteBois],
        }
    }
}

/// Type d'ouvrage. Il est stocké dans le style du génome : `door` pour
/// l'encadrement de porte (ADR-23), `iron` pour la porte en bois (ADR-26),
/// `window` pour les fenêtres.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ouvrage {
    Remplage,
    Meneaux,
    Vitrail,
    Encadrement,
    PorteBois,
}

impl Ouvrage {
    pub fn of(st: &StyleParams) -> Ouvrage {
        if st.iron {
            Ouvrage::PorteBois
        } else if st.door {
            Ouvrage::Encadrement
        } else {
            match st.window {
                WindowKind::Remplage => Ouvrage::Remplage,
                WindowKind::Meneaux => Ouvrage::Meneaux,
                WindowKind::Vitrail => Ouvrage::Vitrail,
            }
        }
    }
    /// Règle le style pour ce type (le type de fenêtre est retenu quand la
    /// forme devient porte, pour le retrouver en revenant à « Fenêtre »).
    pub fn apply(self, st: &mut StyleParams) {
        st.door = self == Ouvrage::Encadrement;
        st.iron = self == Ouvrage::PorteBois;
        match self {
            Ouvrage::Remplage => st.window = WindowKind::Remplage,
            Ouvrage::Meneaux => st.window = WindowKind::Meneaux,
            Ouvrage::Vitrail => st.window = WindowKind::Vitrail,
            Ouvrage::Encadrement | Ouvrage::PorteBois => {}
        }
    }
    pub fn category(self) -> Category {
        match self {
            Ouvrage::Remplage | Ouvrage::Meneaux | Ouvrage::Vitrail => Category::Fenetre,
            Ouvrage::Encadrement | Ouvrage::PorteBois => Category::Porte,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Ouvrage::Remplage => "Remplage (tracerie)",
            Ouvrage::Meneaux => "Fenêtre à meneaux et croisées",
            Ouvrage::Vitrail => "Vitrail",
            Ouvrage::Encadrement => "Encadrement de porte",
            Ouvrage::PorteBois => "Porte en bois",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Ouvrage::Remplage => {
                "Remplage gothique : réseau de pierre construit à la règle et au compas."
            }
            Ouvrage::Meneaux => {
                "Générateur à venir : la forme est pour l'instant remplie d'un remplage."
            }
            Ouvrage::Vitrail => {
                "Générateur à venir : la forme est pour l'instant remplie d'un remplage."
            }
            Ouvrage::Encadrement => {
                "L'arc devient une baie de porte, décorée à l'extérieur : archivolte, accolade à crochets, fleuron, pinacles, lancettes aveugles, tympan."
            }
            Ouvrage::PorteBois => {
                "Porte de bois à un ou deux vantaux (selon les proportions), ornée de ferrures gothiques : pentures à volutes, fourches et fleurs de lis, rinceaux, fleurons, clous, anneau et serrure."
            }
        }
    }
    /// Le générateur a-t-il son propre algorithme ?
    pub fn has_generator(self) -> bool {
        !matches!(self, Ouvrage::Meneaux | Ouvrage::Vitrail)
    }
    /// Formes qui conviennent : l'encadrement de porte demande un arc, la
    /// porte en bois un arc ou un rectangle ; les fenêtres, toute forme.
    pub fn fits(self, prim: Option<&PrimSpec>) -> bool {
        let kind = prim.map(|p| p.kind);
        match self {
            Ouvrage::Encadrement => matches!(kind, Some(PrimKind::Arch(_))),
            Ouvrage::PorteBois => matches!(kind, Some(PrimKind::Arch(_) | PrimKind::Rect)),
            _ => true,
        }
    }
    /// Premier type de la catégorie qui convient à la forme.
    pub fn first_fitting(
        c: Category,
        st: &StyleParams,
        prim: Option<&PrimSpec>,
    ) -> Option<Ouvrage> {
        if c == Category::Fenetre {
            // retrouver le type de fenêtre retenu
            let mut w = st.clone();
            w.door = false;
            w.iron = false;
            return Some(Ouvrage::of(&w));
        }
        c.types().iter().copied().find(|o| o.fits(prim))
    }
}

// ------------------------------------------------------ copier-coller

/// En-tête du texte placé dans le presse-papiers par « Copier » (ADR-27) :
/// le collage le reconnaît, y compris d'une fenêtre Tracery à l'autre.
pub const CLIP_PREFIX: &str = "TRACERY-OBJETS 1\n";

/// Texte du presse-papiers pour une liste de formes.
pub fn objects_to_text(items: &[ShapeItem]) -> String {
    format!(
        "{CLIP_PREFIX}{}",
        serde_json::to_string(items).unwrap_or_else(|_| "[]".into())
    )
}

/// Formes lues dans un texte du presse-papiers (None : pas des formes).
pub fn objects_from_text(t: &str) -> Option<Vec<ShapeItem>> {
    let body = t.trim_start().strip_prefix(CLIP_PREFIX.trim_end())?;
    let v: Vec<ShapeItem> = serde_json::from_str(body.trim()).ok()?;
    (!v.is_empty()).then_some(v)
}

/// Boîte englobante d'un groupe de formes.
pub fn group_bbox(items: &[ShapeItem]) -> geom::BBox {
    items
        .iter()
        .filter_map(|s| s.region())
        .fold(geom::BBox::EMPTY, |b, r| b.union(&r.bbox()))
}

impl Document {
    /// Formes d'identifiants `ids`, dans l'ordre du document.
    pub fn items(&self, ids: &[u64]) -> Vec<ShapeItem> {
        self.shapes
            .iter()
            .filter(|s| ids.contains(&s.id))
            .cloned()
            .collect()
    }

    /// Ajoute des copies des formes, décalées de `offset` ; renvoie leurs
    /// nouveaux identifiants (génome conservé : même remplage).
    pub fn paste(&mut self, items: &[ShapeItem], offset: P2) -> Vec<u64> {
        let mut ids = Vec::with_capacity(items.len());
        for it in items {
            let mut s = it.clone();
            self.next_id += 1;
            s.id = self.next_id;
            if !s.name.ends_with("(copie)") {
                s.name = format!("{} (copie)", s.name);
            }
            if offset != P2::ZERO {
                s.transform(&Xf::translate(offset));
            }
            ids.push(s.id);
            self.shapes.push(s);
        }
        ids
    }

    /// Déplace un groupe de formes.
    pub fn translate(&mut self, ids: &[u64], d: P2) {
        if d == P2::ZERO {
            return;
        }
        for s in self.shapes.iter_mut().filter(|s| ids.contains(&s.id)) {
            s.transform(&Xf::translate(d));
        }
    }

    /// Formes entièrement contenues dans la boîte (rectangle de sélection).
    pub fn inside(&self, b: &geom::BBox, skip: impl Fn(u64) -> bool) -> Vec<u64> {
        self.shapes
            .iter()
            .filter(|s| !skip(s.id))
            .filter(|s| {
                s.region().is_some_and(|r| {
                    let r = r.bbox();
                    r.min.x >= b.min.x
                        && r.min.y >= b.min.y
                        && r.max.x <= b.max.x
                        && r.max.y <= b.max.y
                })
            })
            .map(|s| s.id)
            .collect()
    }
}

/// Opération booléenne entre formes (EF-22).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Difference,
    Intersection,
    Xor,
}

pub fn boolean(a: &Region, b: &Region, op: BoolOp) -> Vec<Region> {
    let av = std::slice::from_ref(a);
    match op {
        BoolOp::Union => clip::union(av, b),
        BoolOp::Difference => clip::subtract(av, b),
        BoolOp::Intersection => clip::intersect(av, b),
        BoolOp::Xor => {
            let mut v = clip::subtract(av, b);
            v.extend(clip::subtract(std::slice::from_ref(b), a));
            v
        }
    }
}

/// Poignée d'édition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Handle {
    /// Coin de la boîte d'une primitive (signes x, y dans le repère local).
    Corner(f64, f64),
    /// Sommet `i` d'un contour libre (boucle `l` : 0 extérieur, 1.. trous).
    Vertex(usize, usize),
    /// Milieu du segment `i` (courbure).
    Mid(usize, usize),
}

impl ShapeItem {
    /// Poignées et leur position (document).
    pub fn handles(&self) -> Vec<(Handle, P2)> {
        let mut out = Vec::new();
        if let Some(p) = &self.prim {
            let rot = Xf::rotate_about(p.center, p.angle);
            for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
                out.push((
                    Handle::Corner(sx, sy),
                    rot.apply(p.center + P2::new(sx * p.w / 2.0, sy * p.h / 2.0)),
                ));
            }
        } else if let Some(r) = &self.free {
            for (li, l) in r.loops().enumerate() {
                for (i, s) in l.segs.iter().enumerate() {
                    out.push((Handle::Vertex(li, i), s.start()));
                    out.push((Handle::Mid(li, i), s.point_at(0.5)));
                }
            }
        }
        out
    }

    /// Déplace une poignée en `q` (document).
    pub fn drag_handle(&mut self, h: Handle, q: P2) {
        match h {
            Handle::Corner(_, _) => {
                if let Some(p) = &mut self.prim {
                    let local = Xf::rotate_about(p.center, -p.angle).apply(q) - p.center;
                    p.w = (2.0 * local.x.abs()).max(1e-3);
                    p.h = (2.0 * local.y.abs()).max(1e-3);
                }
            }
            Handle::Vertex(li, i) => {
                if let Some(r) = &mut self.free {
                    let l = if li == 0 {
                        &mut r.outer
                    } else {
                        &mut r.holes[li - 1]
                    };
                    let n = l.segs.len();
                    let prev = (i + n - 1) % n;
                    let sp = l.segs[prev];
                    let si = l.segs[i];
                    l.segs[prev] = Seg::from_sweep(sp.start(), q, sp.sweep());
                    l.segs[i] = Seg::from_sweep(q, si.end(), si.sweep());
                    l.compute_joints();
                }
            }
            Handle::Mid(li, i) => {
                if let Some(r) = &mut self.free {
                    let l = if li == 0 {
                        &mut r.outer
                    } else {
                        &mut r.holes[li - 1]
                    };
                    let s = l.segs[i];
                    l.segs[i] = Seg::through(s.start(), q, s.end());
                    l.compute_joints();
                }
            }
        }
    }

    /// Insère un sommet au point le plus proche de `q` (EF-21).
    pub fn insert_vertex(&mut self, q: P2) -> bool {
        let Some(r) = &mut self.free else {
            return false;
        };
        let mut best: Option<(usize, usize, f64, f64)> = None;
        for (li, l) in r.loops().enumerate() {
            for (i, s) in l.segs.iter().enumerate() {
                let (_, t, d) = s.closest(q);
                if best.is_none_or(|b| d < b.3) {
                    best = Some((li, i, t, d));
                }
            }
        }
        let Some((li, i, t, _)) = best else {
            return false;
        };
        if !(0.02..=0.98).contains(&t) {
            return false;
        }
        let l = if li == 0 {
            &mut r.outer
        } else {
            &mut r.holes[li - 1]
        };
        let s = l.segs[i];
        l.segs[i] = s.sub(0.0, t);
        l.segs.insert(i + 1, s.sub(t, 1.0));
        l.compute_joints();
        true
    }

    /// Supprime un sommet (fusion des deux segments adjacents en droite).
    pub fn delete_vertex(&mut self, li: usize, i: usize) -> bool {
        let Some(r) = &mut self.free else {
            return false;
        };
        let l = if li == 0 {
            &mut r.outer
        } else {
            &mut r.holes[li - 1]
        };
        let n = l.segs.len();
        if n <= 3 {
            return false;
        }
        let prev = (i + n - 1) % n;
        let a = l.segs[prev].start();
        let b = l.segs[i].end();
        l.segs[prev] = Seg::line(a, b);
        l.segs.remove(i);
        l.compute_joints();
        true
    }

    /// Bascule segment ↔ arc (EF-21).
    pub fn toggle_arc(&mut self, li: usize, i: usize) {
        let Some(r) = &mut self.free else { return };
        let l: &mut Loop = if li == 0 {
            &mut r.outer
        } else {
            &mut r.holes[li - 1]
        };
        let s = l.segs[i];
        l.segs[i] = if s.is_arc() {
            Seg::line(s.start(), s.end())
        } else {
            Seg::from_sweep(s.start(), s.end(), FRAC_PI_2 / 2.0)
        };
        l.compute_joints();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_two() -> Document {
        let mut d = Document::default();
        let prim = |x: f64| PrimSpec {
            kind: PrimKind::Rect,
            center: P2::new(x, 0.0),
            w: 100.0,
            h: 200.0,
            angle: 0.0,
        };
        d.add(
            "Rectangle",
            Some(prim(0.0)),
            None,
            StyleParams::default(),
            1,
        );
        let free = Region::simple(rectangle(P2::new(500.0, 0.0), 80.0, 80.0));
        d.add("Tracé", None, Some(free), StyleParams::default(), 2);
        d
    }

    #[test]
    fn copier_coller_aller_retour() {
        let mut d = doc_two();
        let items = d.items(&[2, 1]);
        assert_eq!(items.iter().map(|s| s.id).collect::<Vec<_>>(), vec![1, 2]);
        let txt = objects_to_text(&items);
        assert!(objects_from_text("RMP1abc").is_none());
        assert!(objects_from_text("texte quelconque").is_none());
        let back = objects_from_text(&txt).expect("formes relues");
        // égalité au sens du fichier (les angles de raccord, recalculés à la
        // lecture, ne sont pas enregistrés)
        let json = |v: &[ShapeItem]| serde_json::to_string(v).unwrap();
        assert_eq!(json(&back), json(&items));
        let off = P2::new(10.0, -10.0);
        let ids = d.paste(&back, off);
        assert_eq!(ids, vec![3, 4]);
        assert_eq!(d.shapes.len(), 4);
        // copies décalées, génome et nature (primitive / libre) conservés
        for (src, id) in items.iter().zip(&ids) {
            let c = d.get(*id).unwrap();
            assert_eq!(c.genome, src.genome);
            assert_eq!(c.prim.is_some(), src.prim.is_some());
            assert!(c.name.ends_with("(copie)"));
            let (a, b) = (src.region().unwrap().bbox(), c.region().unwrap().bbox());
            assert!(dist(b.min, a.min + off) < 1e-9);
        }
        // recoller une copie n'empile pas les suffixes
        let again = d.items(&[3]);
        let id = d.paste(&again, P2::ZERO)[0];
        assert_eq!(d.get(id).unwrap().name, d.get(3).unwrap().name);
    }

    #[test]
    fn deplacement_groupe_et_rectangle() {
        let mut d = doc_two();
        let all = geom::BBox::from_pts(P2::new(-100.0, -150.0), P2::new(600.0, 150.0));
        assert_eq!(d.inside(&all, |_| false), vec![1, 2]);
        assert_eq!(d.inside(&all, |id| id == 1), vec![2]);
        // rectangle qui ne couvre qu'en partie la première forme
        let part = geom::BBox::from_pts(P2::new(0.0, -150.0), P2::new(600.0, 150.0));
        assert_eq!(d.inside(&part, |_| false), vec![2]);
        let before = group_bbox(&d.items(&[1, 2]));
        d.translate(&[1, 2], P2::new(25.0, 5.0));
        let after = group_bbox(&d.items(&[1, 2]));
        assert!(dist(after.min, before.min + P2::new(25.0, 5.0)) < 1e-9);
        assert!(
            d.get(1).unwrap().prim.is_some(),
            "la primitive reste paramétrique"
        );
    }

    #[test]
    fn ouvrages() {
        let arc = PrimSpec {
            kind: PrimKind::Arch(ArchKind::equilateral()),
            center: P2::ZERO,
            w: 1000.0,
            h: 2000.0,
            angle: 0.0,
        };
        let cercle = PrimSpec {
            kind: PrimKind::Circle,
            ..arc
        };
        let mut st = StyleParams::default();
        assert_eq!(Ouvrage::of(&st), Ouvrage::Remplage);
        // chaque type se relit tel quel
        for c in Category::ALL {
            for &o in c.types() {
                o.apply(&mut st);
                assert_eq!(Ouvrage::of(&st), o);
                assert_eq!(o.category(), c);
                assert!(!(st.door && st.iron));
            }
        }
        // le type de fenêtre est retenu pendant que la forme est une porte
        Ouvrage::Vitrail.apply(&mut st);
        Ouvrage::PorteBois.apply(&mut st);
        assert_eq!(
            Ouvrage::first_fitting(Category::Fenetre, &st, Some(&arc)),
            Some(Ouvrage::Vitrail)
        );
        // formes qui conviennent aux portes
        assert_eq!(
            Ouvrage::first_fitting(Category::Porte, &st, Some(&arc)),
            Some(Ouvrage::Encadrement)
        );
        let rect = PrimSpec {
            kind: PrimKind::Rect,
            ..arc
        };
        assert_eq!(
            Ouvrage::first_fitting(Category::Porte, &st, Some(&rect)),
            Some(Ouvrage::PorteBois)
        );
        assert_eq!(
            Ouvrage::first_fitting(Category::Porte, &st, Some(&cercle)),
            None
        );
        assert_eq!(Ouvrage::first_fitting(Category::Porte, &st, None), None);
        // un ancien génome (sans champ `window`) reste un remplage
        let g: StyleParams = serde_json::from_str("{\"door\": false}").unwrap();
        assert_eq!(Ouvrage::of(&g), Ouvrage::Remplage);
    }
}
