//! Rendu CPU déterministe (tiny-skia) : modes Trait, Pierre (ombrage 2,5D),
//! Épure, Vitrail ; planches de contact. Les arcs sont convertis en
//! Bézier cubiques au rendu uniquement (INV-1).

use crate::font::draw_text;
use engine::hidden::{DoorDrawing, door_drawing};
use engine::score::edt;
use engine::{Design, OrnKind};
use geom::math::*;
use geom::{BBox, ConstructionEntity, Loop, Region, Seg};
use rayon::prelude::*;
use tiny_skia::{
    Color, FillRule, LineCap, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash,
    Transform,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Trait,
    Pierre,
    Epure,
    Vitrail,
    /// Épure sur fond sombre (« mode nuit »).
    EpureSombre,
}

impl Mode {
    pub fn parse(s: &str) -> Option<Mode> {
        match s.to_lowercase().as_str() {
            "trait" | "1" => Some(Mode::Trait),
            "pierre" | "2" => Some(Mode::Pierre),
            "epure" | "épure" | "3" => Some(Mode::Epure),
            "vitrail" | "4" => Some(Mode::Vitrail),
            "epure-sombre" | "épure-sombre" | "epure_sombre" | "sombre" | "nuit" | "5" => {
                Some(Mode::EpureSombre)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RenderOpts {
    /// Plus grand côté en pixels.
    pub size: u32,
    pub mode: Mode,
    pub transparent: bool,
    /// Marge en fraction de la taille.
    pub margin: f64,
}

impl Default for RenderOpts {
    fn default() -> Self {
        RenderOpts {
            size: 1024,
            mode: Mode::Pierre,
            transparent: false,
            margin: 0.04,
        }
    }
}

/// Repère document → pixels (y vers le bas).
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub scale: f64,
    pub min_x: f64,
    pub max_y: f64,
    pub off: f64,
    pub w: u32,
    pub h: u32,
}

impl View {
    pub fn fit(bb: &BBox, size: u32, margin: f64) -> View {
        let s = bb.size();
        let m = margin * size as f64;
        let scale = (size as f64 - 2.0 * m) / s.x.max(s.y).max(1e-300);
        let w = (s.x * scale + 2.0 * m).ceil().max(1.0) as u32;
        let h = (s.y * scale + 2.0 * m).ceil().max(1.0) as u32;
        View {
            scale,
            min_x: bb.min.x,
            max_y: bb.max.y,
            off: m,
            w,
            h,
        }
    }
    pub fn p(&self, p: P2) -> (f32, f32) {
        (
            ((p.x - self.min_x) * self.scale + self.off) as f32,
            ((self.max_y - p.y) * self.scale + self.off) as f32,
        )
    }
}

fn add_loop(pb: &mut PathBuilder, l: &Loop, v: &View) {
    let Some(first) = l.segs.first() else { return };
    let (x, y) = v.p(first.start());
    pb.move_to(x, y);
    for s in &l.segs {
        match *s {
            Seg::Line { b, .. } => {
                let (x, y) = v.p(b);
                pb.line_to(x, y);
            }
            Seg::Arc { c, r, .. } => {
                for piece in s.split_arc(FRAC_PI_2) {
                    if let Seg::Arc { theta0, sweep, .. } = piece {
                        let k = 4.0 / 3.0 * tan(sweep / 4.0) * r;
                        let a = theta0;
                        let b = theta0 + sweep;
                        let p0 = c + dir(a) * r;
                        let p3 = c + dir(b) * r;
                        let p1 = p0 + perp(dir(a)) * k;
                        let p2 = p3 - perp(dir(b)) * k;
                        let (x1, y1) = v.p(p1);
                        let (x2, y2) = v.p(p2);
                        let (x3, y3) = v.p(p3);
                        pb.cubic_to(x1, y1, x2, y2, x3, y3);
                    }
                }
            }
        }
    }
    pb.close();
}

pub fn region_path(regions: &[&Region], v: &View) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for r in regions {
        for l in r.loops() {
            add_loop(&mut pb, l, v);
        }
    }
    pb.finish()
}

fn paint(rgb: [u8; 3], a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(rgb[0], rgb[1], rgb[2], a);
    p.anti_alias = true;
    p
}

fn stroke(w: f32) -> Stroke {
    Stroke {
        width: w,
        line_cap: LineCap::Round,
        ..Default::default()
    }
}

const STONE: [u8; 3] = [232, 223, 200];
const GLASS: [u8; 3] = [47, 70, 96];
const INK: [u8; 3] = [40, 36, 30];
const RED: [u8; 3] = [176, 58, 46];
const BG: [u8; 3] = [250, 248, 243];
/// Baie de porte (ombre de l'intérieur).
const DOOR_VOID: [u8; 3] = [34, 29, 25];
/// Mur autour d'une porte.
const WALL: [u8; 3] = [222, 212, 190];
/// Bois des vantaux (chêne grisé) et fer forgé (ADR-26).
const WOOD: [u8; 3] = [146, 118, 88];
const IRON: [u8; 3] = [64, 56, 50];

/// Ornement de fer (ferronnerie : niveaux 2 et plus) ?
fn is_iron(d: &Design, o: &engine::Ornament) -> bool {
    d.iron && o.kind == OrnKind::Relief && o.level >= 2
}

/// Aplats des portes, du fond vers l'avant (ordre de peinture de
/// `door_drawing`), puis la baie.
fn fill_doors(
    pm: &mut Pixmap,
    ds: &[&Design],
    draws: &[DoorDrawing],
    v: &View,
    stone: [u8; 3],
    creux: [u8; 3],
    void: [u8; 3],
) {
    let id = Transform::identity();
    for (d, dr) in ds.iter().filter(|d| d.door).zip(draws) {
        for &i in &dr.order {
            let o = &d.ornaments[i];
            let c = if is_iron(d, o) {
                // fer : nettement plus sombre que le bois ou la pierre
                stone.map(|x| (x as f32 * 0.5) as u8)
            } else if o.kind == OrnKind::Relief {
                stone
            } else {
                creux
            };
            if let Some(p) = region_path(&[&o.region], v) {
                pm.fill_path(&p, &paint(c, 255), FillRule::Winding, id, None);
            }
        }
        let op: Vec<&Region> = d.openings.iter().map(|o| &o.region).collect();
        if let Some(p) = region_path(&op, v) {
            pm.fill_path(&p, &paint(void, 255), FillRule::Winding, id, None);
        }
        // reliefs de niveau 3 posés dans une baie (plombs des vitreries,
        // ADR-30) : devant le vide
        let obb: Vec<geom::BBox> = op.iter().map(|r| r.bbox()).collect();
        for o in &d.ornaments {
            let b = o.region.bbox();
            let inside = obb.iter().any(|q| {
                b.min.x >= q.min.x && b.min.y >= q.min.y && b.max.x <= q.max.x && b.max.y <= q.max.y
            });
            if o.kind == OrnKind::Relief
                && o.level >= 3
                && inside
                && let Some(p) = region_path(&[&o.region], v)
            {
                pm.fill_path(&p, &paint(stone, 255), FillRule::Winding, id, None);
            }
        }
    }
}

/// Traits vus des portes (ornements sans parties cachées), forme et baie.
fn door_line_path(ds: &[&Design], draws: &[DoorDrawing], v: &View) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    for (d, dr) in ds.iter().filter(|d| d.door).zip(draws) {
        for l in &dr.lines {
            let Some(first) = l.first() else { continue };
            let (x, y) = v.p(*first);
            pb.move_to(x, y);
            for q in &l[1..] {
                let (x, y) = v.p(*q);
                pb.line_to(x, y);
            }
        }
        for r in std::iter::once(&d.shape).chain(d.openings.iter().map(|o| &o.region)) {
            for l in r.loops() {
                add_loop(&mut pb, l, v);
            }
        }
    }
    pb.finish()
}

/// Palette d'une épure (claire ou sombre).
#[derive(Clone, Copy)]
pub struct EpurePalette {
    pub bg: [u8; 3],
    pub stone: [u8; 3],
    pub ink: [u8; 3],
    /// Cercles et points de construction.
    pub cons: [u8; 3],
    /// Droites de construction (axes).
    pub axis: [u8; 3],
}

pub const EPURE_CLAIRE: EpurePalette = EpurePalette {
    bg: BG,
    stone: [238, 234, 224],
    ink: INK,
    cons: RED,
    axis: [214, 125, 40],
};

/// Épure sombre : fond bleu nuit, pierre ardoise, traits clairs, tracés de
/// construction cyan et ambre (lisibles sur fond sombre).
pub const EPURE_SOMBRE: EpurePalette = EpurePalette {
    bg: [21, 24, 31],
    stone: [36, 41, 52],
    ink: [222, 226, 234],
    cons: [94, 196, 214],
    axis: [232, 168, 72],
};

impl Mode {
    /// Couleur de fond du mode.
    pub fn background(self) -> [u8; 3] {
        match self {
            Mode::EpureSombre => EPURE_SOMBRE.bg,
            _ => BG,
        }
    }
}

/// Rendu d'un remplage.
pub fn render(d: &Design, o: &RenderOpts) -> Pixmap {
    let v = View::fit(&d.bbox(), o.size, o.margin);
    let lw = (o.size as f32 / 900.0).max(1.0);
    render_view(&[d], o.mode, o.transparent, &v, lw)
}

/// Rendu de plusieurs remplages dans une vue quelconque (canevas).
pub fn render_view(ds: &[&Design], mode: Mode, transparent: bool, v: &View, line_w: f32) -> Pixmap {
    let mut pm = Pixmap::new(v.w.max(1), v.h.max(1))
        .unwrap_or_else(|| Pixmap::new(1, 1).expect("pixmap 1×1"));
    if !transparent {
        let bg = mode.background();
        pm.fill(Color::from_rgba8(bg[0], bg[1], bg[2], 255));
    }
    let id = Transform::identity();
    let mut all: Vec<&Region> = Vec::new();
    for d in ds.iter().filter(|d| !d.door) {
        all.push(&d.shape);
        all.extend(d.openings.iter().map(|o| &o.region));
    }
    // portes : ordre de peinture et traits vus (parties cachées retirées)
    let draws: Vec<DoorDrawing> = if matches!(
        mode,
        Mode::Trait | Mode::Epure | Mode::EpureSombre | Mode::Vitrail
    ) {
        ds.iter()
            .filter(|d| d.door)
            .map(|d| door_drawing(d))
            .collect()
    } else {
        Vec::new()
    };
    let shapes: Vec<&Region> = ds.iter().filter(|d| !d.door).map(|d| &d.shape).collect();
    match mode {
        Mode::Trait => {
            if let Some(p) = region_path(&all, v) {
                pm.stroke_path(&p, &paint([0, 0, 0], 255), &stroke(line_w), id, None);
            }
            if let Some(p) = door_line_path(ds, &draws, v) {
                pm.stroke_path(&p, &paint([0, 0, 0], 255), &stroke(0.8 * line_w), id, None);
            }
        }
        Mode::Epure | Mode::EpureSombre => {
            let pal = if mode == Mode::EpureSombre {
                EPURE_SOMBRE
            } else {
                EPURE_CLAIRE
            };
            if let Some(p) = region_path(&all, v) {
                pm.fill_path(&p, &paint(pal.stone, 255), FillRule::EvenOdd, id, None);
            }
            let creux = pal.stone.map(|x| (x as f32 * 0.9) as u8);
            fill_doors(&mut pm, ds, &draws, v, pal.stone, creux, pal.bg);
            for d in ds {
                draw_construction(&mut pm, &d.construction, v, line_w, &pal);
            }
            if let Some(p) = region_path(&all, v) {
                pm.stroke_path(&p, &paint(pal.ink, 255), &stroke(line_w), id, None);
            }
            if let Some(p) = door_line_path(ds, &draws, v) {
                pm.stroke_path(&p, &paint(pal.ink, 255), &stroke(0.8 * line_w), id, None);
            }
        }
        Mode::Pierre => {
            if ds.iter().any(|d| d.door) {
                render_stone_layered(&mut pm, ds, v, transparent);
            } else {
                render_stone(&mut pm, ds, v, transparent);
            }
            if let Some(p) = region_path(&shapes, v) {
                pm.stroke_path(&p, &paint([90, 80, 64], 255), &stroke(line_w), id, None);
            }
        }
        Mode::Vitrail => {
            if let Some(p) = region_path(&all, v) {
                pm.fill_path(&p, &paint([58, 56, 54], 255), FillRule::EvenOdd, id, None);
            }
            fill_doors(
                &mut pm,
                ds,
                &draws,
                v,
                [58, 56, 54],
                [44, 42, 40],
                DOOR_VOID,
            );
            for d in ds.iter().filter(|d| !d.door) {
                for op in &d.openings {
                    let c = glass_color(&op.path, op.depth);
                    if let Some(p) = region_path(&[&op.region], v) {
                        pm.fill_path(&p, &paint(c, 255), FillRule::EvenOdd, id, None);
                        let lead =
                            (d.widths.last().copied().unwrap_or(1.0) * v.scale * 0.35) as f32;
                        pm.stroke_path(
                            &p,
                            &paint([20, 20, 20], 255),
                            &stroke(lead.max(1.0)),
                            id,
                            None,
                        );
                    }
                }
            }
        }
    }
    pm
}

fn glass_color(path: &str, depth: u8) -> [u8; 3] {
    let h = engine::seed::hash_text(path);
    let palette: [[u8; 3]; 8] = [
        [32, 68, 140],
        [160, 30, 40],
        [40, 120, 70],
        [200, 150, 40],
        [90, 50, 130],
        [30, 110, 140],
        [180, 90, 40],
        [60, 90, 170],
    ];
    let base = palette[((h >> 7) as usize + depth as usize * 3) % palette.len()];
    let j = (h & 31) as i32 - 16;
    [
        (base[0] as i32 + j).clamp(0, 255) as u8,
        (base[1] as i32 + j).clamp(0, 255) as u8,
        (base[2] as i32 + j).clamp(0, 255) as u8,
    ]
}

fn draw_construction(
    pm: &mut Pixmap,
    cons: &[ConstructionEntity],
    v: &View,
    lw: f32,
    pal: &EpurePalette,
) {
    let id = Transform::identity();
    let mut st = stroke(lw * 0.8);
    st.dash = StrokeDash::new(vec![lw * 6.0, lw * 4.0], 0.0);
    let red = paint(pal.cons, 170);
    let orange = paint(pal.axis, 160);
    for c in cons {
        match *c {
            ConstructionEntity::Circle { c, r, .. } => {
                let (x, y) = v.p(c);
                if let Some(p) = PathBuilder::from_circle(x, y, (r * v.scale) as f32) {
                    pm.stroke_path(&p, &red, &st, id, None);
                }
            }
            ConstructionEntity::Line { a, b, .. } => {
                let mut pb = PathBuilder::new();
                let (x1, y1) = v.p(a);
                let (x2, y2) = v.p(b);
                pb.move_to(x1, y1);
                pb.line_to(x2, y2);
                if let Some(p) = pb.finish() {
                    pm.stroke_path(&p, &orange, &st, id, None);
                }
            }
            ConstructionEntity::Point { p, .. } => {
                let (x, y) = v.p(p);
                if let Some(path) = PathBuilder::from_circle(x, y, lw * 2.2) {
                    pm.fill_path(&path, &paint(pal.cons, 230), FillRule::Winding, id, None);
                }
            }
        }
    }
}

/// Ombrage 2,5D : distance au bord des jours (transformée exacte), profil
/// de moulure en quart de rond, normale, éclairage de Lambert.
fn render_stone(pm: &mut Pixmap, ds: &[&Design], v: &View, transparent: bool) {
    let (w, h) = (v.w as usize, v.h as usize);
    let id = Transform::identity();
    let mut all: Vec<&Region> = Vec::new();
    let mut openings: Vec<&Region> = Vec::new();
    for d in ds {
        all.push(&d.shape);
        all.extend(d.openings.iter().map(|o| &o.region));
        openings.extend(d.openings.iter().map(|o| &o.region));
    }
    // masque de pierre (antialiasé)
    let mut mask =
        Pixmap::new(v.w.max(1), v.h.max(1)).unwrap_or_else(|| Pixmap::new(1, 1).expect("pixmap"));
    if let Some(p) = region_path(&all, v) {
        mask.fill_path(
            &p,
            &paint([255, 255, 255], 255),
            FillRule::EvenOdd,
            id,
            None,
        );
    }
    // jours
    let mut glass =
        Pixmap::new(v.w.max(1), v.h.max(1)).unwrap_or_else(|| Pixmap::new(1, 1).expect("pixmap"));
    if let Some(p) = region_path(&openings, v) {
        glass.fill_path(&p, &paint(GLASS, 255), FillRule::EvenOdd, id, None);
    }
    let md = mask.data();
    let inside = |i: usize| md[i * 4 + 3] > 127;
    let dist = edt(w, h, inside);
    // profil : demi-largeur de référence = meneau d'ordre 1
    let w1 = ds
        .first()
        .and_then(|d| d.widths.get(1).copied())
        .unwrap_or(1.0);
    let half = (w1 * v.scale * 0.5).max(1.5);
    let height = |i: usize| {
        let t = (dist[i] / half).min(1.0);
        (1.0 - (1.0 - t) * (1.0 - t)).sqrt() * half * 0.9
    };
    let hmap: Vec<f64> = (0..w * h).into_par_iter().map(height).collect();
    let light = {
        let l = (-0.55f64, -0.6f64, 0.75f64);
        let n = (l.0 * l.0 + l.1 * l.1 + l.2 * l.2).sqrt();
        (l.0 / n, l.1 / n, l.2 / n)
    };
    let gd = glass.data();
    let data = pm.data_mut();
    data.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let i = y * w + x;
            let px = &mut row[x * 4..x * 4 + 4];
            let a_st = md[i * 4 + 3] as f64 / 255.0;
            let a_gl = gd[i * 4 + 3] as f64 / 255.0;
            let mut col = [px[0] as f64, px[1] as f64, px[2] as f64];
            let mut alpha = px[3] as f64 / 255.0;
            if a_gl > 0.0 {
                // verre sombre, léger dégradé vertical
                let g = 0.85 + 0.25 * (y as f64 / h as f64);
                let c = [
                    GLASS[0] as f64 * g,
                    GLASS[1] as f64 * g,
                    GLASS[2] as f64 * g,
                ];
                for k in 0..3 {
                    col[k] = col[k] * (1.0 - a_gl) + c[k] * a_gl;
                }
                alpha = alpha.max(a_gl);
            }
            if a_st > 0.0 {
                let hx = |xx: usize| hmap[y * w + xx];
                let hy = |yy: usize| hmap[yy * w + x];
                let dx = (hx((x + 1).min(w - 1)) - hx(x.saturating_sub(1))) * 0.5;
                let dy = (hy((y + 1).min(h - 1)) - hy(y.saturating_sub(1))) * 0.5;
                let n = {
                    let (a, b, c) = (-dx, -dy, 1.0);
                    let l = (a * a + b * b + c * c).sqrt();
                    (a / l, b / l, c / l)
                };
                let lam = (n.0 * light.0 + n.1 * light.1 + n.2 * light.2).max(0.0);
                let ao = 0.82 + 0.18 * (dist[i] / half).min(1.0);
                let shade = (0.32 + 0.78 * lam) * ao;
                let c = [
                    (STONE[0] as f64 * shade).min(255.0),
                    (STONE[1] as f64 * shade).min(255.0),
                    (STONE[2] as f64 * shade).min(255.0),
                ];
                for k in 0..3 {
                    col[k] = col[k] * (1.0 - a_st) + c[k] * a_st;
                }
                alpha = alpha.max(a_st);
            }
            if transparent {
                // données prémultipliées
                for k in 0..3 {
                    px[k] = (col[k] * alpha).clamp(0.0, 255.0) as u8;
                }
            } else {
                for k in 0..3 {
                    px[k] = col[k].clamp(0.0, 255.0) as u8;
                }
            }
            px[3] = (alpha * 255.0).round() as u8;
        }
    });
}

/// Masque antialiasé (couverture 0–1) d'un ensemble de régions, rastérisé
/// par bandes horizontales en parallèle.
fn coverage(regions: &[&Region], v: &View, rule: FillRule) -> Vec<f32> {
    let (w, h) = (v.w.max(1) as usize, v.h.max(1) as usize);
    let mut out = vec![0f32; w * h];
    let Some(path) = region_path(regions, v) else {
        return out;
    };
    const BAND: usize = 64;
    out.par_chunks_mut(w * BAND)
        .enumerate()
        .for_each(|(k, chunk)| {
            let bh = chunk.len() / w;
            let y0 = (k * BAND) as f32;
            let Some(mut pm) = Pixmap::new(w as u32, bh as u32) else {
                return;
            };
            pm.fill_path(
                &path,
                &paint([255, 255, 255], 255),
                rule,
                Transform::from_translate(0.0, -y0),
                None,
            );
            for (o, c) in chunk.iter_mut().zip(pm.data().as_chunks::<4>().0) {
                *o = c[3] as f32 / 255.0;
            }
        });
    out
}

/// Ombrage 2,5D par niveaux (portes, ADR-23) : chaque niveau de relief a sa
/// hauteur et son profil en quart de rond (distance exacte au bord), les
/// creux (gorges, panneaux aveugles) sont taillés dans la pierre, la baie
/// est un vide sombre. Éclairage de Lambert et ombres portées calculées
/// par balayage de l'horizon le long de la direction de la lumière.
fn render_stone_layered(pm: &mut Pixmap, ds: &[&Design], v: &View, transparent: bool) {
    let (w, h) = (v.w as usize, v.h as usize);
    if w < 2 || h < 2 {
        return;
    }
    let n = w * h;
    // unité : largeur de l'archivolte de la première porte, en pixels
    let u = ds
        .iter()
        .find(|d| d.door)
        .and_then(|d| d.widths.first().copied())
        .unwrap_or(1.0)
        * v.scale;
    let u = u.max(4.0);
    let base = [0.12, 0.35, 0.65, 1.05];
    let bevel = [0.22, 0.22, 0.32, 0.45];
    let prof = |t: f64| {
        let t = t.min(1.0);
        (1.0 - (1.0 - t) * (1.0 - t)).max(0.0).sqrt()
    };
    let mut hmap = vec![f64::NEG_INFINITY; n];
    let mut stone_a = vec![0f32; n];
    let mut wall_a = vec![0f32; n];
    // couverture des reliefs de niveau 3 : ils passent devant les baies
    // (plombs des vitreries, ADR-30)
    let mut top_cov: Vec<f32> = Vec::new();
    // reliefs des portes, niveau par niveau
    for lv in 0u8..=3 {
        let regs: Vec<&Region> = ds
            .iter()
            .filter(|d| d.door)
            .flat_map(|d| d.ornaments.iter())
            .filter(|o| o.kind == OrnKind::Relief && o.level.min(3) == lv)
            .map(|o| &o.region)
            .collect();
        if regs.is_empty() {
            continue;
        }
        let cov = coverage(&regs, v, FillRule::Winding);
        let dist = edt(w, h, |i| cov[i] > 0.5);
        let (b, bv) = (base[lv as usize] * u, bevel[lv as usize] * u);
        if lv == 3 {
            top_cov = cov.clone();
        }
        hmap.par_iter_mut()
            .zip(stone_a.par_iter_mut())
            .zip(wall_a.par_iter_mut())
            .enumerate()
            .for_each(|(i, ((hm, sa), wa))| {
                let c = cov[i];
                if c > 0.0 {
                    *sa = sa.max(c);
                    if lv == 0 {
                        *wa = c;
                    } else if c > 0.5 {
                        *wa = 0.0;
                    }
                    if c > 0.5 {
                        *hm = hm.max(b * prof(dist[i] / bv));
                    }
                }
            });
    }
    // remplages ordinaires présents dans la même vue
    let win: Vec<&Region> = ds
        .iter()
        .filter(|d| !d.door)
        .flat_map(|d| std::iter::once(&d.shape).chain(d.openings.iter().map(|o| &o.region)))
        .collect();
    if !win.is_empty() {
        let cov = coverage(&win, v, FillRule::EvenOdd);
        let dist = edt(w, h, |i| cov[i] > 0.5);
        let half = (ds
            .iter()
            .find(|d| !d.door)
            .and_then(|d| d.widths.get(1).copied())
            .unwrap_or(1.0)
            * v.scale
            * 0.5)
            .max(1.5);
        for i in 0..n {
            if cov[i] > 0.0 {
                stone_a[i] = stone_a[i].max(cov[i]);
                if cov[i] > 0.5 {
                    let t = (dist[i] / half).min(1.0);
                    hmap[i] = hmap[i].max((1.0 - (1.0 - t) * (1.0 - t)).sqrt() * half * 0.9);
                    wall_a[i] = 0.0;
                }
            }
        }
    }
    // creux : gorges (niveau 1) et panneaux (niveau 2)
    for (lv, depth, bv) in [(1u8, 0.14, 0.05), (2u8, 0.24, 0.07)] {
        let regs: Vec<&Region> = ds
            .iter()
            .filter(|d| d.door)
            .flat_map(|d| d.ornaments.iter())
            .filter(|o| o.kind == OrnKind::Creux && o.level.clamp(1, 2) == lv)
            .map(|o| &o.region)
            .collect();
        if regs.is_empty() {
            continue;
        }
        let cov = coverage(&regs, v, FillRule::Winding);
        let dist = edt(w, h, |i| cov[i] > 0.5);
        hmap.par_iter_mut().enumerate().for_each(|(i, hm)| {
            if cov[i] > 0.5 && hm.is_finite() {
                *hm -= depth * u * (dist[i] / (bv * u)).min(1.0);
            }
        });
    }
    // baies de porte et verre des fenêtres
    let doors: Vec<&Region> = ds
        .iter()
        .filter(|d| d.door)
        .flat_map(|d| d.openings.iter().map(|o| &o.region))
        .collect();
    let glass_regs: Vec<&Region> = ds
        .iter()
        .filter(|d| !d.door)
        .flat_map(|d| d.openings.iter().map(|o| &o.region))
        .collect();
    let (mut void_a, glass_a) = rayon::join(
        || coverage(&doors, v, FillRule::Winding),
        || coverage(&glass_regs, v, FillRule::EvenOdd),
    );
    if !top_cov.is_empty() {
        for (va, tc) in void_a.iter_mut().zip(&top_cov) {
            *va *= 1.0 - tc;
        }
    }
    let floor = -0.6 * u;
    for i in 0..n {
        if void_a[i] > 0.5 {
            hmap[i] = floor;
        }
    }
    // lissage 3×3 : efface l'escalier de la distance discrète sur les
    // bords obliques (flèches, gâbles) ; le fond reste hors pierre
    {
        let src: Vec<f64> = hmap
            .iter()
            .map(|h| if h.is_finite() { *h } else { floor })
            .collect();
        let keep: Vec<bool> = hmap
            .iter()
            .zip(&void_a)
            .map(|(h, va)| h.is_finite() && *va <= 0.5)
            .collect();
        hmap.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            for (x, hm) in row.iter_mut().enumerate() {
                if !keep[y * w + x] {
                    continue;
                }
                let mut acc = 0.0;
                let mut k = 0.0;
                for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                        let j = yy * w + xx;
                        if keep[j] {
                            acc += src[j];
                            k += 1.0;
                        }
                    }
                }
                *hm = acc / k;
            }
        });
    }
    // ferronneries : couverture du bois et du fer (couleur, grain, reflet)
    let iron_regs: Vec<&Region> = ds
        .iter()
        .flat_map(|d| {
            d.ornaments
                .iter()
                .filter(|o| is_iron(d, o))
                .map(|o| &o.region)
        })
        .collect();
    let wood_regs: Vec<&Region> = ds
        .iter()
        .filter(|d| d.iron)
        .flat_map(|d| {
            d.ornaments
                .iter()
                .filter(|o| o.kind == OrnKind::Relief && o.level <= 1)
                .map(|o| &o.region)
        })
        .collect();
    let (iron_a, wood_a) = if wood_regs.is_empty() && iron_regs.is_empty() {
        (Vec::new(), Vec::new())
    } else {
        rayon::join(
            || coverage(&iron_regs, v, FillRule::Winding),
            || coverage(&wood_regs, v, FillRule::Winding),
        )
    };
    // fil du bois : stries verticales, en unités du document
    let grain_u = ds
        .iter()
        .find(|d| d.iron)
        .and_then(|d| d.widths.first().copied())
        .unwrap_or(1.0);
    let hv = |i: usize| if hmap[i].is_finite() { hmap[i] } else { floor };
    // ombres portées : horizon balayé ligne par ligne (lumière en haut à
    // gauche ; le prédécesseur vers la lumière est sur la ligne du dessus)
    let l = {
        let l = (-0.55f64, -0.6f64, 0.75f64);
        let k = (l.0 * l.0 + l.1 * l.1 + l.2 * l.2).sqrt();
        (l.0 / k, l.1 / k, l.2 / k)
    };
    let fx = l.0 / l.1; // décalage en x par ligne vers la lumière
    let step = (fx * fx + 1.0).sqrt();
    let rise = step * l.2 / (l.0 * l.0 + l.1 * l.1).sqrt();
    let mut horizon = vec![floor; n];
    let mut shadow = vec![0f32; n];
    let soft = 0.04 * u;
    // chaque ligne ne dépend que de la précédente : lignes en séquence,
    // pixels d'une ligne en parallèle
    for y in 0..h {
        let (done, rest) = horizon.split_at_mut(y * w);
        let above: &[f64] = if y == 0 { &[] } else { &done[(y - 1) * w..] };
        rest[..w]
            .par_iter_mut()
            .zip(shadow[y * w..(y + 1) * w].par_iter_mut())
            .enumerate()
            .for_each(|(x, (hz, sh))| {
                let hi = hv(y * w + x);
                let prev = if y == 0 {
                    floor
                } else {
                    let xs = x as f64 - fx;
                    let xf = xs.floor();
                    let t = xs - xf;
                    let at = |xx: f64| {
                        if xx < 0.0 || xx >= w as f64 {
                            floor
                        } else {
                            above[xx as usize]
                        }
                    };
                    at(xf) * (1.0 - t) + at(xf + 1.0) * t - rise
                };
                if prev > hi {
                    *sh = ((prev - hi) / soft).min(1.0) as f32;
                }
                *hz = hi.max(prev);
            });
    }
    let data = pm.data_mut();
    data.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            let i = y * w + x;
            let px = &mut row[x * 4..x * 4 + 4];
            let mut col = [px[0] as f64, px[1] as f64, px[2] as f64];
            let mut alpha = px[3] as f64 / 255.0;
            let (a_st, a_void, a_gl) = (stone_a[i] as f64, void_a[i] as f64, glass_a[i] as f64);
            if a_st > 0.0 {
                let hx = |xx: usize| hv(y * w + xx);
                let hy = |yy: usize| hv(yy * w + x);
                let dx = (hx((x + 1).min(w - 1)) - hx(x.saturating_sub(1))) * 0.5;
                let dy = (hy((y + 1).min(h - 1)) - hy(y.saturating_sub(1))) * 0.5;
                // le vide de la baie ne crée pas de pente démesurée
                let (dx, dy) = (dx.clamp(-1.5, 1.5), dy.clamp(-1.5, 1.5));
                let nn = {
                    let (a, b, c) = (-dx, -dy, 1.0);
                    let k = (a * a + b * b + c * c).sqrt();
                    (a / k, b / k, c / k)
                };
                let lam = (nn.0 * l.0 + nn.1 * l.1 + nn.2 * l.2).max(0.0);
                let sh = 1.0 - 0.55 * shadow[i] as f64;
                let shade = 0.30 + 0.80 * lam * sh;
                let wa = wall_a[i] as f64;
                let (fe, bo) = if iron_a.is_empty() {
                    (0.0, 0.0)
                } else {
                    (
                        iron_a[i] as f64,
                        wood_a[i] as f64 * (1.0 - iron_a[i] as f64),
                    )
                };
                // grain du bois (déterministe) et reflet du fer
                let grain = if bo > 0.0 {
                    let xd = v.min_x + (x as f64 - v.off) / v.scale;
                    let yd = v.max_y - (y as f64 - v.off) / v.scale;
                    let ph = xd / (0.07 * grain_u)
                        + 0.9 * (yd / (4.0 * grain_u) + xd / (1.7 * grain_u)).sin()
                        + 0.35 * (yd / (1.3 * grain_u)).sin();
                    1.0 + 0.035 * ph.sin() + 0.02 * (2.7 * ph + 1.0).sin()
                } else {
                    1.0
                };
                let spec = if fe > 0.0 {
                    let hl = {
                        let (a, b, c) = (l.0, l.1, l.2 + 1.0);
                        let k = (a * a + b * b + c * c).sqrt();
                        (a / k, b / k, c / k)
                    };
                    let nh = (nn.0 * hl.0 + nn.1 * hl.1 + nn.2 * hl.2).max(0.0);
                    0.45 * nh.powi(28) * sh
                } else {
                    0.0
                };
                for k in 0..3 {
                    let mut base_c = STONE[k] as f64 * (1.0 - wa) + WALL[k] as f64 * wa;
                    base_c = base_c * (1.0 - bo) + WOOD[k] as f64 * grain * bo;
                    base_c = base_c * (1.0 - fe) + IRON[k] as f64 * fe;
                    let c = (base_c * shade + 255.0 * spec * fe).min(255.0);
                    col[k] = col[k] * (1.0 - a_st) + c * a_st;
                }
                alpha = alpha.max(a_st);
            }
            if a_gl > 0.0 {
                let gg = 0.85 + 0.25 * (y as f64 / h as f64);
                for k in 0..3 {
                    col[k] = col[k] * (1.0 - a_gl) + GLASS[k] as f64 * gg * a_gl;
                }
                alpha = alpha.max(a_gl);
            }
            if a_void > 0.0 {
                let gg = 0.8 + 0.4 * (y as f64 / h as f64);
                let sh = 1.0 - 0.35 * shadow[i] as f64;
                for k in 0..3 {
                    col[k] = col[k] * (1.0 - a_void) + DOOR_VOID[k] as f64 * gg * sh * a_void;
                }
                alpha = alpha.max(a_void);
            }
            if transparent {
                for k in 0..3 {
                    px[k] = (col[k] * alpha).clamp(0.0, 255.0) as u8;
                }
            } else {
                for k in 0..3 {
                    px[k] = col[k].clamp(0.0, 255.0) as u8;
                }
            }
            px[3] = (alpha * 255.0).round() as u8;
        }
    });
}

/// Rendu de régions simples (planches de motifs) avec entités de construction.
pub fn render_regions(
    regs: &[Region],
    cons: &[ConstructionEntity],
    bb: &BBox,
    size: u32,
) -> Pixmap {
    let v = View::fit(bb, size, 0.05);
    let mut pm = Pixmap::new(v.w, v.h).unwrap_or_else(|| Pixmap::new(1, 1).expect("pixmap"));
    pm.fill(Color::from_rgba8(BG[0], BG[1], BG[2], 255));
    let id = Transform::identity();
    let r: Vec<&Region> = regs.iter().collect();
    let lw = (size as f32 / 500.0).max(1.0);
    if let Some(p) = region_path(&r, &v) {
        pm.fill_path(&p, &paint(GLASS, 255), FillRule::EvenOdd, id, None);
        pm.stroke_path(&p, &paint(INK, 255), &stroke(lw), id, None);
    }
    draw_construction(&mut pm, cons, &v, lw, &EPURE_CLAIRE);
    pm
}

/// Planche de contact : vignettes et étiquettes.
pub fn contact_sheet(items: &[(Pixmap, Vec<String>)], cols: usize) -> Pixmap {
    let cols = cols.max(1);
    let cw = items.iter().map(|i| i.0.width()).max().unwrap_or(1);
    let ch = items.iter().map(|i| i.0.height()).max().unwrap_or(1);
    let lines = items.iter().map(|i| i.1.len()).max().unwrap_or(0);
    let fs = ((cw as usize) / 160).clamp(1, 3);
    let label_h = (lines * 9 * fs + 6) as u32;
    let rows = items.len().div_ceil(cols);
    let pad = 8u32;
    let w = cols as u32 * (cw + pad) + pad;
    let h = rows as u32 * (ch + label_h + pad) + pad;
    let mut sheet = Pixmap::new(w, h).unwrap_or_else(|| Pixmap::new(1, 1).expect("pixmap"));
    sheet.fill(Color::from_rgba8(255, 255, 255, 255));
    for (k, (pm, labels)) in items.iter().enumerate() {
        let (c, r) = ((k % cols) as u32, (k / cols) as u32);
        let x = pad + c * (cw + pad) + (cw - pm.width()) / 2;
        let y = pad + r * (ch + label_h + pad) + (ch - pm.height()) / 2;
        sheet.draw_pixmap(
            x as i32,
            y as i32,
            pm.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
        let ty = pad + r * (ch + label_h + pad) + ch + 4;
        for (li, t) in labels.iter().enumerate() {
            let tx = (pad + c * (cw + pad)) as usize;
            let (sw, shh) = (sheet.width() as usize, sheet.height() as usize);
            draw_text(
                sheet.data_mut(),
                sw,
                shh,
                tx + 2,
                ty as usize + li * 9 * fs,
                t,
                fs,
                [30, 30, 30],
            );
        }
    }
    sheet
}
