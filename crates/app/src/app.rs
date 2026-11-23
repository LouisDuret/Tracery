//! Application Tracery (§ 7) : barre de menus, outils, canevas, panneau de
//! style et de seed, score, arbre de dérivation, galerie, historique,
//! favoris, barre d'état.

use crate::doc::{
    BoolOp, Category, Document, Handle, Ouvrage, PrimKind, PrimSpec, ShapeItem, boolean,
    group_bbox, objects_from_text, objects_to_text,
};
use crate::selection::Selection;
use crate::settings::{Interface, Settings, mode_key, settings_path};
use crate::worker::{Msg, Worker};
use egui::{Color32, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};
use engine::hidden::{DoorDrawing, door_drawing};
use engine::seed::{parse_seed, splitmix64, to_base32};
use engine::style::{Canon, CrossKind, Family, StyleParams, SymMode, TerminalMode};
use engine::{CancelToken, Design, Genome, NodePath, TreeNode};
use geom::fit::{FitOptions, fit_stroke};
use geom::math::*;
use geom::{ConstructionEntity, Loop, Region, Seg, Xf};
use io::raster::{Mode, View};
use motifs::ArchKind;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Tool {
    Select,
    Freehand,
    Polygon,
    Prim(PrimKind),
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Gallery,
    History,
    Favorites,
}

/// Caméra du canevas : centre (document), échelle (points écran par unité).
#[derive(Clone, Copy)]
struct Cam {
    center: P2,
    scale: f64,
    rect: Rect,
}

impl Cam {
    fn to_screen(self, p: P2) -> Pos2 {
        let c = self.rect.center();
        pos2(
            c.x + ((p.x - self.center.x) * self.scale) as f32,
            c.y - ((p.y - self.center.y) * self.scale) as f32,
        )
    }
    fn to_doc(self, q: Pos2) -> P2 {
        let c = self.rect.center();
        P2::new(
            self.center.x + (q.x - c.x) as f64 / self.scale,
            self.center.y - (q.y - c.y) as f64 / self.scale,
        )
    }
    fn view_rect(&self, v: &View) -> Rect {
        let a = self.to_screen(P2::new(v.min_x, v.max_y));
        let b = self.to_screen(P2::new(
            v.min_x + v.w as f64 / v.scale,
            v.max_y - v.h as f64 / v.scale,
        ));
        Rect::from_min_max(a, b)
    }
}

/// État de génération d'une forme.
#[derive(Default)]
struct GenState {
    sig: u64,
    gen_id: u64,
    full_for: u64,
    design: Option<Arc<Design>>,
    preview_shown: bool,
    pending: Option<Instant>,
    cancel: CancelToken,
    running: u32,
    error: Option<String>,
}

struct Thumb {
    seed: u64,
    score: f64,
    tex: egui::TextureHandle,
}

struct Gallery {
    batch: u64,
    n: usize,
    sort: bool,
    items: Vec<Option<Thumb>>,
    shape: u64,
}

/// Événement de pointeur (bouton principal).
enum PEv {
    Down(Pos2, bool),
    Move(Pos2),
    Up(Pos2),
}

enum Drag {
    None,
    /// Déplacement de toute la sélection.
    Move {
        ids: Vec<u64>,
        last: P2,
        before: Document,
    },
    Handle {
        id: u64,
        h: Handle,
        before: Document,
    },
    Stroke(Vec<P2>),
    Prim {
        start: P2,
        cur: P2,
    },
    /// Rectangle de sélection (ajout à la sélection si `additive`).
    Marquee {
        start: P2,
        cur: P2,
        additive: bool,
    },
}

pub struct TraceryApp {
    doc: Document,
    undo: Vec<Document>,
    redo: Vec<Document>,
    /// Formes sélectionnées (principale et ajoutées, ADR-27).
    sel: Selection,
    sel_node: Option<String>,
    sel_handle: Option<(u64, Handle)>,
    tool: Tool,
    arch_kind: ArchKind,
    poly: Vec<P2>,
    drag: Drag,
    cam: Cam,
    mode: Mode,
    gens: BTreeMap<u64, GenState>,
    worker: Worker,
    raster: Option<(egui::TextureHandle, View)>,
    /// Contenu (mode, remplages) de la texture affichée et de la demande en
    /// cours : la texture ne dépend pas de la caméra, le GPU la déplace et
    /// la met à l'échelle ; on ne la recalcule qu'au besoin.
    raster_key: u64,
    raster_req_key: u64,
    /// Dernière caméra vue et instant de son dernier mouvement.
    cam_seen: (P2, f64, Rect),
    cam_moved: Instant,
    /// Redessiner bientôt (texture à recalculer une fois la caméra immobile).
    repaint_soon: bool,
    /// Diagnostic (TRACERY_PERF) : début de fenêtre, images, temps cumulé
    /// et pire temps d'une image.
    perf: Option<(Instant, u32, Duration, Duration)>,
    raster_inflight: bool,
    raster_counter: u64,
    gen_counter: u64,
    history: Vec<(u64, String)>,
    gallery: Gallery,
    tab: Tab,
    seed_text: String,
    seed_text_src: u64,
    status: String,
    project_path: Option<PathBuf>,
    last_autosave: Instant,
    saved_doc: Document,
    show_help: bool,
    new_style: StyleParams,
    last_panel_push: Instant,
    space_pan: bool,
    smoke: Option<(PathBuf, u32, bool)>,
    /// Forme passée en porte : recadrer sur son décor dès la génération
    /// complète reçue.
    fit_on_door: Option<u64>,
    /// Appui en cours sur le canevas : position, Maj ou Ctrl (ajout), déplacé.
    press: Option<(Pos2, bool, bool)>,
    /// La forme sous le pointeur vient d'être sélectionnée par cet appui
    /// (le clic ne doit alors pas sélectionner aussi une cellule).
    fresh_select: bool,
    /// Panneau de hiérarchie des objets affiché ?
    show_objects: bool,
    /// Formes masquées (affichage seulement).
    hidden_shapes: BTreeSet<u64>,
    /// Nœuds masqués par forme (chemins) : leur contenu n'est pas affiché.
    hidden_nodes: BTreeMap<u64, BTreeSet<String>>,
    /// Remplages filtrés par la visibilité : (source, signature, résultat).
    vis_cache: BTreeMap<u64, (usize, u64, Arc<Design>)>,
    /// Traits vus des portes (parties cachées retirées), par forme.
    door_draw_cache: BTreeMap<u64, (usize, Arc<DoorDrawing>)>,
    /// Réglages tels qu'enregistrés en dernier (None : pas d'enregistrement,
    /// auto-test).
    saved_settings: Option<Settings>,
    /// Dernier texte copié (pour « Coller » du menu) et nombre de collages
    /// successifs de ce texte (chacun décalé un peu plus).
    clip_text: Option<String>,
    paste_count: (u64, u32),
}

fn hash_bytes(b: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for x in b {
        h ^= *x as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn now_seed() -> u64 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    splitmix64(t)
}

fn autosave_path() -> PathBuf {
    crate::settings::config_dir().join("autosave.tracery")
}

fn rule_name(id: &str) -> String {
    match id {
        "R00" | "R00-" => "Cadre".into(),
        "fallback" => "Dernier recours".into(),
        _ => engine::rules::rule_by_id(id)
            .map(|r| r.name().to_string())
            .unwrap_or_else(|| id.to_string()),
    }
}

const INK: Color32 = Color32::from_rgb(40, 36, 30);
const ACCENT: Color32 = Color32::from_rgb(214, 125, 40);
const CONS: Color32 = Color32::from_rgba_premultiplied(150, 50, 40, 170);

/// Couleurs du canevas selon le mode d'affichage.
struct CanvasPalette {
    bg: Color32,
    ink: Color32,
    cons: Color32,
    axis: Color32,
    point: Color32,
}

fn canvas_palette(mode: Mode) -> CanvasPalette {
    if mode == Mode::EpureSombre {
        let p = io::raster::EPURE_SOMBRE;
        let c = |x: [u8; 3], a: u8| Color32::from_rgba_unmultiplied(x[0], x[1], x[2], a);
        CanvasPalette {
            bg: c(p.bg, 255),
            ink: c(p.ink, 255),
            cons: c(p.cons, 170),
            axis: c(p.axis, 170),
            point: c(p.cons, 255),
        }
    } else {
        CanvasPalette {
            bg: Color32::from_rgb(250, 248, 243),
            ink: INK,
            cons: CONS,
            axis: Color32::from_rgba_premultiplied(200, 110, 30, 170),
            point: Color32::from_rgb(176, 58, 46),
        }
    }
}

/// Point représentatif d'une entité de construction (pour le masquage).
fn cons_anchor(c: &ConstructionEntity) -> P2 {
    match *c {
        ConstructionEntity::Point { p, .. } => p,
        ConstructionEntity::Circle { c, .. } => c,
        ConstructionEntity::Line { a, b, .. } => (a + b) * 0.5,
    }
}

impl TraceryApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut doc = Document::default();
        let mut restored = false;
        let mut status =
            "Prêt. Choisissez un outil (A : arc brisé, B : main levée) et tracez une forme."
                .to_string();
        if let Ok(txt) = std::fs::read_to_string(autosave_path())
            && let Ok(p) = serde_json::from_str::<io::Project>(&txt)
        {
            doc = Document::from_project(&p);
            restored = true;
            status = format!(
                "Session précédente restaurée ({} forme(s)).",
                doc.shapes.len()
            );
        }
        let mut sel = Selection::default();
        sel.set(doc.shapes.first().map(|s| s.id));
        #[cfg(debug_assertions)]
        if std::env::var_os("TRACERY_DEBUG_UI").is_some() {
            cc.egui_ctx.all_styles_mut(|st| {
                st.debug.show_interactive_widgets = true;
                st.debug.show_widget_hits = true;
            });
        }
        let mut app = TraceryApp {
            saved_doc: doc.clone(),
            doc,
            undo: vec![],
            redo: vec![],
            sel,
            sel_node: None,
            sel_handle: None,
            tool: Tool::Select,
            arch_kind: ArchKind::equilateral(),
            poly: vec![],
            drag: Drag::None,
            cam: Cam {
                center: P2::new(0.0, 500.0),
                scale: 0.35,
                rect: Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0)),
            },
            mode: Mode::Pierre,
            gens: BTreeMap::new(),
            worker: Worker::new(cc.egui_ctx.clone()),
            raster: None,
            raster_key: 0,
            raster_req_key: 0,
            cam_seen: (P2::ZERO, 0.0, Rect::NOTHING),
            cam_moved: Instant::now(),
            repaint_soon: false,
            perf: std::env::var_os("TRACERY_PERF")
                .map(|_| (Instant::now(), 0, Duration::ZERO, Duration::ZERO)),
            raster_inflight: false,
            raster_counter: 0,
            gen_counter: 0,
            history: vec![],
            gallery: Gallery {
                batch: 0,
                n: 16,
                sort: true,
                items: vec![],
                shape: 0,
            },
            tab: Tab::Gallery,
            seed_text: String::new(),
            seed_text_src: u64::MAX,
            status,
            project_path: None,
            last_autosave: Instant::now(),
            show_help: false,
            new_style: StyleParams::default(),
            last_panel_push: Instant::now(),
            space_pan: false,
            press: None,
            fresh_select: false,
            show_objects: true,
            hidden_shapes: BTreeSet::new(),
            hidden_nodes: BTreeMap::new(),
            vis_cache: BTreeMap::new(),
            door_draw_cache: BTreeMap::new(),
            smoke: std::env::var_os("TRACERY_SMOKE").map(|p| (PathBuf::from(p), 0, false)),
            fit_on_door: None,
            saved_settings: None,
            clip_text: None,
            paste_count: (0, 0),
        };
        if app.smoke.is_some() {
            app.doc = Document::default();
            if std::env::var_os("TRACERY_SMOKE_MENEAUX").is_some() {
                // croisée de pierre (ADR-30)
                let prim = PrimSpec {
                    kind: PrimKind::Rect,
                    center: P2::ZERO,
                    w: 1000.0,
                    h: 1700.0,
                    angle: 0.0,
                };
                let mut st = StyleParams::for_family(Family::Flamboyant);
                st.window = engine::WindowKind::Meneaux;
                app.doc.add("Croisée", Some(prim), None, st, 9);
                app.sel.set(app.doc.shapes.first().map(|s| s.id));
                app.fit_on_door = app.sel.primary;
            } else if std::env::var_os("TRACERY_SMOKE_FER").is_some() {
                // porte de bois à ferrures, deux vantaux (ADR-26)
                let prim = PrimSpec {
                    kind: PrimKind::Rect,
                    center: P2::ZERO,
                    w: 1700.0,
                    h: 2300.0,
                    angle: 0.0,
                };
                let mut st = StyleParams::for_family(Family::Flamboyant);
                st.iron = true;
                app.doc.add("Ferronnerie", Some(prim), None, st, 1);
                app.sel.set(app.doc.shapes.first().map(|s| s.id));
                app.fit_on_door = app.sel.primary;
            } else if std::env::var_os("TRACERY_SMOKE_PORTE").is_some() {
                // porte en arc tiers-point (ADR-23)
                let prim = PrimSpec {
                    kind: PrimKind::Arch(ArchKind::equilateral()),
                    center: P2::ZERO,
                    w: 1000.0,
                    h: 2366.0,
                    angle: 0.0,
                };
                let mut st = StyleParams::for_family(Family::Flamboyant);
                st.door = true;
                app.doc.add("Porte", Some(prim), None, st, 3);
                app.sel.set(app.doc.shapes.first().map(|s| s.id));
                app.fit_on_door = app.sel.primary;
            } else if let Some(r) = engine::refshapes::ref_shape("S01") {
                app.doc
                    .add("S01", None, Some(r.region), StyleParams::default(), 4);
                app.sel.set(app.doc.shapes.first().map(|s| s.id));
            }
            // options de vérification : mode d'affichage, nœud masqué
            if let Some(m) = std::env::var("TRACERY_SMOKE_MODE")
                .ok()
                .and_then(|m| Mode::parse(&m))
            {
                app.mode = m;
            }
            if let (Ok(path), Some(id)) = (std::env::var("TRACERY_SMOKE_HIDE"), app.sel.primary) {
                app.set_node_visible(id, &path, false);
            }
        }
        if app.smoke.is_none() {
            app.load_settings(&cc.egui_ctx, restored);
        }
        app.fit_view_pending();
        // diagnostic : carte graphique retenue par wgpu
        if app.perf.is_some()
            && let Some(rs) = cc.wgpu_render_state.as_ref()
        {
            let i = rs.adapter.get_info();
            perf_log(&format!(
                "[perf] GPU : {} ({:?}, {:?})",
                i.name, i.device_type, i.backend
            ));
        }
        app
    }

    // ------------------------------------------------------------ document

    fn push_undo(&mut self, before: Document) {
        if self.undo.last() != Some(&before) && before != self.doc {
            self.undo.push(before);
            if self.undo.len() > 1000 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }
    fn checkpoint(&mut self) {
        let b = self.doc.clone();
        if self.undo.last() != Some(&b) {
            self.undo.push(b);
            self.redo.clear();
        }
    }
    fn undo(&mut self) {
        if let Some(d) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, d));
            self.validate_selection();
            self.status = "Annulé.".into();
        }
    }
    fn redo(&mut self) {
        if let Some(d) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, d));
            self.validate_selection();
            self.status = "Rétabli.".into();
        }
    }
    fn validate_selection(&mut self) {
        let had = self.sel.primary.is_some();
        let doc = &self.doc;
        self.sel.retain(|id| doc.get(id).is_some());
        if had && self.sel.primary.is_none() {
            self.sel.set(self.doc.shapes.first().map(|s| s.id));
        }
    }
    fn current(&self) -> Option<&ShapeItem> {
        self.sel.primary.and_then(|id| self.doc.get(id))
    }
    fn design_of(&self, id: u64) -> Option<Arc<Design>> {
        self.gens.get(&id).and_then(|g| g.design.clone())
    }

    // ---------------------------------------------------------- visibilité

    /// Remplage tel qu'affiché : None si la forme est masquée ; le contenu
    /// des nœuds masqués est retiré, leur cellule apparaît en jour simple.
    /// Traits vus d'une porte, recalculés quand son remplage visible change.
    fn door_drawing_of(&mut self, id: u64, d: &Arc<Design>) -> Arc<DoorDrawing> {
        let ptr = Arc::as_ptr(d) as usize;
        if let Some((p, v)) = self.door_draw_cache.get(&id)
            && *p == ptr
        {
            return v.clone();
        }
        let v = Arc::new(door_drawing(d));
        self.door_draw_cache.insert(id, (ptr, v.clone()));
        v
    }
    /// Les exports utilisent toujours le remplage complet.
    fn visible_design(&mut self, id: u64) -> Option<Arc<Design>> {
        if self.hidden_shapes.contains(&id) {
            return None;
        }
        let d = self.design_of(id)?;
        let hidden = match self.hidden_nodes.get(&id) {
            Some(h) if !h.is_empty() => h.clone(),
            _ => return Some(d),
        };
        let sig = hash_bytes(
            hidden
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join("|")
                .as_bytes(),
        );
        let ptr = Arc::as_ptr(&d) as usize;
        if let Some((p, s, v)) = self.vis_cache.get(&id)
            && *p == ptr
            && *s == sig
        {
            return Some(v.clone());
        }
        // nœuds masqués les plus hauts
        let tops: Vec<&String> = hidden
            .iter()
            .filter(|h| {
                !hidden
                    .iter()
                    .any(|o| o != *h && h.starts_with(&format!("{o}/")))
            })
            .collect();
        let under = |path: &str| {
            tops.iter()
                .any(|h| path == h.as_str() || path.starts_with(&format!("{h}/")))
        };
        let mut v: Design = (*d).clone();
        v.openings.retain(|o| !under(&o.path));
        v.ornaments.retain(|o| !under(&o.path));
        let mut cells: Vec<Region> = Vec::new();
        // porte : masquer un élément retire son décor, sans créer de jour
        for n in d.tree.iter().filter(|_| !d.door) {
            if tops.contains(&&n.path)
                && let Some(r) = &n.region
            {
                v.openings.push(engine::design::Opening {
                    region: r.clone(),
                    depth: n.depth,
                    path: n.path.clone(),
                    rule: "masqué".into(),
                });
                cells.push(r.clone());
            }
        }
        v.construction
            .retain(|c| !cells.iter().any(|r| r.contains(cons_anchor(c))));
        let arc = Arc::new(v);
        self.vis_cache.insert(id, (ptr, sig, arc.clone()));
        Some(arc)
    }

    fn set_shape_visible(&mut self, id: u64, visible: bool) {
        if visible {
            self.hidden_shapes.remove(&id);
        } else {
            self.hidden_shapes.insert(id);
            if self.sel.primary == Some(id) {
                self.sel_node = None;
                self.sel_handle = None;
            }
        }
    }

    fn set_node_visible(&mut self, id: u64, path: &str, visible: bool) {
        let set = self.hidden_nodes.entry(id).or_default();
        if visible {
            set.remove(path);
        } else {
            set.insert(path.to_string());
        }
    }

    fn add_shape(&mut self, name: &str, prim: Option<PrimSpec>, free: Option<Region>) {
        self.checkpoint();
        let style = self
            .current()
            .map(|s| s.genome.style.clone())
            .unwrap_or_else(|| self.new_style.clone());
        let id = self.doc.add(name, prim, free, style, now_seed() % 100_000);
        self.sel.set(Some(id));
        self.sel_node = None;
        self.record_history(id);
        self.status = format!("{name} ajouté.");
    }

    fn record_history(&mut self, id: u64) {
        if let Some(s) = self.doc.get(id) {
            let g = s.genome.to_compact();
            if self.history.last().map(|h| &h.1) != Some(&g) {
                self.history.push((id, g));
                if self.history.len() > 500 {
                    self.history.remove(0);
                }
            }
        }
    }

    /// Modifie le génome de la forme courante (commande annulable).
    fn edit_genome(&mut self, f: impl FnOnce(&mut Genome)) {
        let Some(id) = self
            .sel
            .primary
            .or_else(|| self.doc.shapes.first().map(|s| s.id))
        else {
            return;
        };
        self.checkpoint();
        if let Some(s) = self.doc.get_mut(id) {
            f(&mut s.genome);
            s.genome.prune();
        }
        self.record_history(id);
    }

    fn set_seed(&mut self, seed: u64) {
        self.edit_genome(|g| g.seed = seed);
        self.status = format!("Seed {}", to_base32(seed));
    }

    fn fit_view_pending(&mut self) {
        let mut bb = geom::BBox::EMPTY;
        for s in &self.doc.shapes {
            if let Some(r) = s.region() {
                bb = bb.union(&r.bbox());
            }
            if let Some(d) = self.design_of(s.id) {
                bb = bb.union(&d.bbox());
            }
        }
        if !bb.is_empty() {
            let r = self.cam.rect;
            let sz = bb.size();
            self.cam.center = bb.center();
            self.cam.scale = (0.88
                * (r.width() as f64 / sz.x.max(1e-9)).min(r.height() as f64 / sz.y.max(1e-9)))
            .clamp(1e-4, 1e4);
        }
    }

    // ---------------------------------------------------------- génération

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.worker.rx.try_recv() {
            match m {
                Msg::Gen {
                    shape,
                    gen_id,
                    preview,
                    result,
                    drawing,
                } => {
                    let Some(g) = self.gens.get_mut(&shape) else {
                        continue;
                    };
                    g.running = g.running.saturating_sub(1);
                    if gen_id != g.gen_id {
                        continue;
                    }
                    match result {
                        Ok(d) => {
                            if preview && g.full_for == gen_id {
                                continue;
                            }
                            if !preview {
                                g.full_for = gen_id;
                                self.status = format!(
                                    "Remplage : {} jours, {} primitives, {:.0} ms, score {:.3}",
                                    d.openings.len(),
                                    d.stats.primitives,
                                    d.stats.millis,
                                    d.score.total
                                );
                            }
                            let door = d.door;
                            g.preview_shown = preview;
                            let d = Arc::new(d);
                            if let Some(dd) = drawing {
                                self.door_draw_cache
                                    .insert(shape, (Arc::as_ptr(&d) as usize, dd));
                            }
                            g.design = Some(d);
                            g.error = None;
                            if !preview && door && self.fit_on_door == Some(shape) {
                                self.fit_on_door = None;
                                self.fit_view_pending();
                            }
                        }
                        Err(e) => {
                            if e != "génération annulée" {
                                g.error = Some(e);
                            }
                        }
                    }
                }
                Msg::Raster { id, image, view } => {
                    self.raster_inflight = false;
                    if id == self.raster_counter {
                        let tex = ctx.load_texture("canevas", image, egui::TextureOptions::LINEAR);
                        self.raster = Some((tex, view));
                        self.raster_key = self.raster_req_key;
                    }
                }
                Msg::Thumb {
                    batch,
                    idx,
                    seed,
                    design,
                    image,
                } => {
                    if batch == self.gallery.batch && idx < self.gallery.items.len() {
                        let tex = ctx.load_texture(
                            format!("vignette-{batch}-{idx}"),
                            image,
                            egui::TextureOptions::LINEAR,
                        );
                        self.gallery.items[idx] = Some(Thumb {
                            seed,
                            score: design.score.total,
                            tex,
                        });
                    }
                }
            }
        }
    }

    /// Détecte les modifications (forme, génome) et lance les générations
    /// après l'anti-rebond de 150 ms (EF-30) ; aperçu pendant les glisser.
    fn schedule(&mut self) {
        let dragging_handle = matches!(self.drag, Drag::Handle { .. } | Drag::Move { .. });
        let ids: Vec<u64> = self.doc.shapes.iter().map(|s| s.id).collect();
        self.gens.retain(|k, _| ids.contains(k));
        for s in &self.doc.shapes {
            let sig =
                hash_bytes(&serde_json::to_vec(&(&s.prim, &s.free, &s.genome)).unwrap_or_default());
            let g = self.gens.entry(s.id).or_default();
            if g.sig != sig {
                g.sig = sig;
                g.pending = Some(Instant::now());
            }
            let Some(t) = g.pending else { continue };
            let Some(region) = s.region() else { continue };
            if dragging_handle {
                // aperçu temps réel (niveaux 1–2), sans attendre
                if g.running == 0 {
                    self.gen_counter += 1;
                    g.gen_id = self.gen_counter;
                    g.cancel.cancel();
                    g.cancel = CancelToken::new();
                    g.running += 1;
                    self.worker.generate(
                        s.id,
                        g.gen_id,
                        region,
                        s.genome.clone(),
                        true,
                        g.cancel.clone(),
                    );
                    g.pending = Some(t);
                }
                continue;
            }
            if t.elapsed() >= Duration::from_millis(150) {
                g.pending = None;
                self.gen_counter += 1;
                g.gen_id = self.gen_counter;
                g.cancel.cancel();
                g.cancel = CancelToken::new();
                g.running += 2;
                self.worker.generate(
                    s.id,
                    g.gen_id,
                    region.clone(),
                    s.genome.clone(),
                    true,
                    g.cancel.clone(),
                );
                self.worker.generate(
                    s.id,
                    g.gen_id,
                    region,
                    s.genome.clone(),
                    false,
                    g.cancel.clone(),
                );
            }
        }
    }

    fn cancel_all(&mut self) {
        for g in self.gens.values_mut() {
            g.cancel.cancel();
            g.pending = None;
        }
        self.status = "Génération annulée.".into();
    }

    fn busy(&self) -> bool {
        self.gens
            .values()
            .any(|g| g.running > 0 || g.pending.is_some())
    }

    /// Fond raster (pierre, vitrail). La texture couvre les remplages autour
    /// de la vue, à l'échelle de l'écran ; pendant un déplacement ou un zoom,
    /// le GPU la déplace et la met à l'échelle sans recalcul. On ne la
    /// recalcule que si le contenu change, si la vue sort de la zone
    /// couverte ou si le zoom s'écarte trop de celui de la texture, et
    /// seulement une fois la caméra immobile.
    fn request_raster(&mut self, ppp: f32) {
        if !matches!(self.mode, Mode::Pierre | Mode::Vitrail) {
            return;
        }
        let cam = self.cam;
        let seen = (cam.center, cam.scale, cam.rect);
        if seen != self.cam_seen {
            self.cam_seen = seen;
            self.cam_moved = Instant::now();
        }
        let ids: Vec<u64> = self.doc.shapes.iter().map(|s| s.id).collect();
        let designs: Vec<Arc<Design>> = ids
            .into_iter()
            .filter_map(|id| self.visible_design(id))
            .collect();
        if designs.is_empty() {
            self.raster = None;
            return;
        }
        let mut key = format!("{:?}", self.mode);
        for d in &designs {
            key.push_str(&format!("{:p}", Arc::as_ptr(d)));
        }
        let key = hash_bytes(key.as_bytes());
        let content = designs
            .iter()
            .fold(geom::BBox::EMPTY, |b, d| b.union(&d.bbox()));
        let content = content.grow(0.02 * content.diag());
        // zone visible (document) et échelle voulue (pixels physiques)
        let hw = cam.rect.width() as f64 / 2.0 / cam.scale;
        let hh = cam.rect.height() as f64 / 2.0 / cam.scale;
        let view_bb = geom::BBox {
            min: P2::new(cam.center.x - hw, cam.center.y - hh),
            max: P2::new(cam.center.x + hw, cam.center.y + hh),
        };
        let Some(vis) = bbox_inter(&view_bb, &content) else {
            return;
        };
        let target = cam.scale * ppp as f64;
        let adequate = self.raster.as_ref().is_some_and(|(_, v)| {
            let covered = geom::BBox {
                min: P2::new(v.min_x, v.max_y - v.h as f64 / v.scale),
                max: P2::new(v.min_x + v.w as f64 / v.scale, v.max_y),
            };
            let r = v.scale / target;
            let c = covered.grow(1e-9 * content.diag());
            c.min.x <= vis.min.x
                && c.min.y <= vis.min.y
                && c.max.x >= vis.max.x
                && c.max.y >= vis.max.y
                && (0.6..=1.6).contains(&r)
        });
        if self.raster_inflight || (self.raster_key == key && adequate) {
            return;
        }
        // caméra en mouvement : la texture actuelle suit sur le GPU, on
        // attend qu'elle s'arrête
        let settle = Duration::from_millis(120);
        if self.raster_key == key && self.raster.is_some() && self.cam_moved.elapsed() < settle {
            self.repaint_soon = true;
            return;
        }
        // zone rendue : la vue élargie de moitié de chaque côté, dans la
        // limite de la taille de texture
        const MAX_PX: f64 = 4096.0;
        let grow = |f: f64| {
            bbox_inter(
                &geom::BBox {
                    min: view_bb.min - (view_bb.max - view_bb.min) * f,
                    max: view_bb.max + (view_bb.max - view_bb.min) * f,
                },
                &content,
            )
            .unwrap_or(vis)
        };
        let mut reg = grow(0.5);
        if reg.size().x.max(reg.size().y) * target > MAX_PX {
            reg = grow(0.1);
        }
        let scale = target.min(MAX_PX / reg.size().x.max(reg.size().y).max(1e-9));
        let view = View {
            scale,
            min_x: reg.min.x,
            max_y: reg.max.y,
            off: 0.0,
            w: (reg.size().x * scale).ceil().max(1.0) as u32,
            h: (reg.size().y * scale).ceil().max(1.0) as u32,
        };
        self.raster_req_key = key;
        self.raster_inflight = true;
        self.raster_counter += 1;
        self.worker
            .raster(self.raster_counter, designs, self.mode, view);
    }

    fn start_gallery(&mut self) {
        let Some(s) = self.current().cloned() else {
            return;
        };
        let Some(region) = s.region() else { return };
        self.gallery.batch += 1;
        self.gallery.shape = s.id;
        self.gallery.items = (0..self.gallery.n).map(|_| None).collect();
        let base = now_seed();
        for i in 0..self.gallery.n {
            let mut g = s.genome.clone();
            g.seed = splitmix64(base ^ i as u64) % 1_000_000_000;
            self.worker
                .thumb(self.gallery.batch, i, region.clone(), g, 180);
        }
        self.tab = Tab::Gallery;
    }

    // ------------------------------------------------------------- fichiers

    fn save_to(&mut self, path: PathBuf) {
        let p = self.doc.to_project();
        match serde_json::to_string_pretty(&p)
            .map_err(|e| e.to_string())
            .and_then(|t| std::fs::write(&path, t).map_err(|e| e.to_string()))
        {
            Ok(()) => {
                self.status = format!("Enregistré : {}", path.display());
                self.project_path = Some(path);
                self.saved_doc = self.doc.clone();
                self.write_autosave();
            }
            Err(e) => self.status = format!("Échec de l'enregistrement : {e}"),
        }
    }
    fn save(&mut self) {
        if let Some(p) = self.project_path.clone() {
            self.save_to(p);
        } else {
            self.save_as();
        }
    }
    fn save_as(&mut self) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Projet Tracery", &["tracery"])
            .set_file_name("projet.tracery")
            .save_file()
        {
            self.save_to(p);
        }
    }
    fn open(&mut self) {
        let Some(p) = rfd::FileDialog::new()
            .add_filter("Projet Tracery", &["tracery", "remplage", "json"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&p)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<io::Project>(&t).map_err(|e| e.to_string()))
        {
            Ok(pr) => {
                self.checkpoint();
                self.doc = Document::from_project(&pr);
                for s in &self.doc.shapes {
                    if s.genome.version != engine::GENERATOR_VERSION {
                        self.status = format!(
                            "Attention : projet créé avec le générateur {} (actuel {}), le résultat peut différer (EF-81).",
                            s.genome.version,
                            engine::GENERATOR_VERSION
                        );
                    }
                }
                self.sel.set(self.doc.shapes.first().map(|s| s.id));
                self.project_path = Some(p);
                self.saved_doc = self.doc.clone();
                self.write_autosave();
                self.fit_view_pending();
            }
            Err(e) => self.status = format!("Ouverture impossible : {e}"),
        }
    }
    fn autosave(&mut self) {
        if self.last_autosave.elapsed() < Duration::from_secs(20)
            || self.doc == self.saved_doc && autosave_path().exists()
        {
            return;
        }
        self.write_autosave();
    }
    /// Sauvegarde automatique immédiate. Appelée aussi à l'ouverture, à
    /// l'enregistrement et à la fermeture : l'autosave doit toujours
    /// correspondre au fichier associé dans les réglages (ADR-27), sans quoi
    /// « Enregistrer » écraserait ce fichier avec une session plus ancienne.
    fn write_autosave(&mut self) {
        if self.smoke.is_some() {
            return;
        }
        self.last_autosave = Instant::now();
        let path = autosave_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(t) = serde_json::to_string(&self.doc.to_project()) {
            let _ = std::fs::write(path, t);
        }
    }
    fn export(&mut self) {
        let Some(id) = self.sel.primary else {
            self.status = "Sélectionnez une forme à exporter.".into();
            return;
        };
        let Some(d) = self.design_of(id) else {
            self.status = "Aucun remplage à exporter.".into();
            return;
        };
        let Some(p) = rfd::FileDialog::new()
            .add_filter("SVG", &["svg"])
            .add_filter("DXF", &["dxf"])
            .add_filter("PNG", &["png"])
            .add_filter("Données de construction (JSON)", &["json"])
            .set_file_name("remplage.svg")
            .save_file()
        else {
            return;
        };
        let ext = p
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let r: Result<(), String> = match ext.as_str() {
            "dxf" => {
                std::fs::write(&p, io::dxf::design_to_dxf(&d, false)).map_err(|e| e.to_string())
            }
            "json" => serde_json::to_string_pretty(d.as_ref())
                .map_err(|e| e.to_string())
                .and_then(|t| std::fs::write(&p, t).map_err(|e| e.to_string())),
            "png" => io::render(
                &d,
                &io::RenderOpts {
                    size: 4096,
                    mode: self.mode,
                    transparent: false,
                    margin: 0.03,
                },
            )
            .save_png(&p)
            .map_err(|e| e.to_string()),
            _ => std::fs::write(&p, io::svg::design_to_svg(&d, &Default::default()))
                .map_err(|e| e.to_string()),
        };
        self.status = match r {
            Ok(()) => format!("Exporté : {}", p.display()),
            Err(e) => format!("Échec de l'export : {e}"),
        };
    }

    fn copy_genome(&mut self, ctx: &egui::Context) {
        if let Some(s) = self.current() {
            let g = s.genome.to_compact();
            ctx.copy_text(g);
            self.status = "Génome copié dans le presse-papiers.".into();
        }
    }
    fn paste_genome(&mut self, text: &str) {
        match Genome::from_compact(text) {
            Ok(g) => {
                if self.doc.shapes.is_empty() {
                    self.status = "Collez le génome après avoir tracé ou ouvert une forme.".into();
                    return;
                }
                self.edit_genome(|x| *x = g);
                self.status = "Génome collé.".into();
            }
            Err(e) => self.status = format!("Génome invalide : {e}"),
        }
    }

    // -------------------------------------------------------------- cellules

    fn node_at(&self, id: u64, p: P2) -> Option<TreeNode> {
        let d = self.design_of(id)?;
        d.tree
            .iter()
            .filter(|n| n.depth > 0 && n.region.as_ref().is_some_and(|r| r.contains(p)))
            .max_by(|a, b| a.depth.cmp(&b.depth).then(b.area.total_cmp(&a.area)))
            .cloned()
    }
    fn selected_node(&self) -> Option<TreeNode> {
        let path = self.sel_node.as_ref()?;
        let d = self.design_of(self.sel.primary?)?;
        d.tree.iter().find(|n| &n.path == path).cloned()
    }
    fn reroll_cell(&mut self) {
        let Some(path) = self.sel_node.clone().and_then(|p| NodePath::parse(&p)) else {
            self.status = "Sélectionnez une cellule (clic dans le remplage).".into();
            return;
        };
        self.edit_genome(|g| g.reroll(&path));
        self.status = format!("Cellule {path} relancée.");
    }
    fn toggle_lock(&mut self) {
        let Some(node) = self.selected_node() else {
            self.status = "Sélectionnez une cellule (clic dans le remplage).".into();
            return;
        };
        let Some(path) = NodePath::parse(&node.path) else {
            return;
        };
        let locked = self
            .current()
            .and_then(|s| s.genome.override_for(&path))
            .is_some_and(|o| o.lock.is_some());
        if locked {
            self.edit_genome(|g| g.unlock(&path));
            self.status = format!("Cellule {path} déverrouillée.");
        } else if node.rule.starts_with('R') && engine::rules::rule_by_id(&node.rule).is_some() {
            let (r, p) = (node.rule.clone(), node.params.clone());
            self.edit_genome(|g| g.lock(&path, &r, p));
            self.status = format!("Cellule {path} verrouillée ({}).", rule_name(&node.rule));
        }
    }

    fn apply_boolean(&mut self, op: BoolOp) {
        let ids = self.sel.ids();
        let &[a, b] = ids.as_slice() else {
            self.status =
                "Sélectionnez exactement deux formes (Maj ou Ctrl + clic pour la seconde).".into();
            return;
        };
        let (Some(sa), Some(sb)) = (self.doc.get(a).cloned(), self.doc.get(b).cloned()) else {
            return;
        };
        let (Some(ra), Some(rb)) = (sa.region(), sb.region()) else {
            return;
        };
        let res = boolean(&ra, &rb, op);
        if res.is_empty() {
            self.status = "Résultat vide.".into();
            return;
        }
        self.checkpoint();
        self.doc.remove(a);
        self.doc.remove(b);
        let mut last = None;
        for r in res {
            self.doc.next_id += 1;
            let id = self.doc.next_id;
            self.doc.shapes.push(ShapeItem {
                id,
                name: format!("Forme {id}"),
                prim: None,
                free: Some(r),
                genome: sa.genome.clone(),
            });
            last = Some(id);
        }
        self.sel.set(last);
        self.status = "Opération booléenne appliquée.".into();
    }

    /// Transforme la sélection autour du centre de sa boîte englobante.
    fn transform_selected(&mut self, x: impl Fn(P2) -> Xf) {
        let ids = self.sel.ids();
        let bb = group_bbox(&self.doc.items(&ids));
        if bb.is_empty() {
            return;
        }
        self.checkpoint();
        let xf = x(bb.center());
        for s in self.doc.shapes.iter_mut().filter(|s| ids.contains(&s.id)) {
            s.transform(&xf);
        }
    }

    /// Sélectionne toutes les formes visibles.
    fn select_all(&mut self) {
        let ids: Vec<u64> = self
            .doc
            .shapes
            .iter()
            .map(|s| s.id)
            .filter(|id| !self.hidden_shapes.contains(id))
            .collect();
        self.sel.set_many(&ids);
        self.sel_node = None;
        self.sel_handle = None;
    }

    /// Supprime les formes sélectionnées.
    fn delete_selected(&mut self) {
        let ids = self.sel.ids();
        if ids.is_empty() {
            return;
        }
        self.checkpoint();
        for id in &ids {
            self.doc.remove(*id);
        }
        self.sel.set(self.doc.shapes.last().map(|s| s.id));
        self.sel_node = None;
        self.sel_handle = None;
        self.status = format!("{} forme(s) supprimée(s).", ids.len());
    }

    // -------------------------------------------------------- copier-coller

    /// Copie la sélection dans le presse-papiers (Ctrl+C).
    fn copy_objects(&mut self, ctx: &egui::Context) {
        let items = self.doc.items(&self.sel.ids());
        if items.is_empty() {
            self.status = "Rien à copier : sélectionnez une ou plusieurs formes.".into();
            return;
        }
        let t = objects_to_text(&items);
        ctx.copy_text(t.clone());
        self.paste_count = (hash_bytes(t.as_bytes()), 0);
        self.clip_text = Some(t);
        self.status = format!("{} forme(s) copiée(s).", items.len());
    }

    /// Colle les formes d'un texte du presse-papiers (Ctrl+V), légèrement
    /// décalées vers la droite et le bas, chaque collage un peu plus. Le
    /// collage est une commande annulable ; les copies deviennent la sélection.
    fn paste_objects(&mut self, text: &str) -> bool {
        let Some(items) = objects_from_text(text) else {
            return false;
        };
        let h = hash_bytes(text.as_bytes());
        let n = if self.paste_count.0 == h {
            self.paste_count.1 + 1
        } else {
            1
        };
        self.paste_count = (h, n);
        let step = 24.0 / self.cam.scale; // 24 points écran
        let off = P2::new(step, -step) * n as f64;
        self.checkpoint();
        let ids = self.doc.paste(&items, off);
        self.sel.set_many(&ids);
        self.sel_node = None;
        self.sel_handle = None;
        for id in &ids {
            self.record_history(*id);
        }
        self.status = format!("{} forme(s) collée(s).", ids.len());
        true
    }

    /// Duplique la sélection à côté d'elle-même.
    fn duplicate_selected(&mut self) {
        let items = self.doc.items(&self.sel.ids());
        if items.is_empty() {
            return;
        }
        let w = group_bbox(&items).size().x;
        self.checkpoint();
        let ids = self.doc.paste(&items, P2::new(w * 1.1, 0.0));
        self.sel.set_many(&ids);
        self.sel_node = None;
    }

    // -------------------------------------------------------------- réglages

    /// `restored` : la session précédente a été relue dans l'autosave ; le
    /// fichier projet n'est réassocié qu'à cette condition.
    fn load_settings(&mut self, ctx: &egui::Context, restored: bool) {
        let s = Settings::load_from(&settings_path());
        if let Some(m) = s.mode() {
            self.mode = m;
        }
        ctx.set_theme(s.interface.to_egui());
        // fichier associé à la session restaurée : « Enregistrer » y écrit
        if let Some(p) = s.projet.clone().filter(|p| restored && p.is_file()) {
            if let Ok(pr) = std::fs::read_to_string(&p)
                .map_err(|e| e.to_string())
                .and_then(|t| serde_json::from_str::<io::Project>(&t).map_err(|e| e.to_string()))
            {
                self.saved_doc = Document::from_project(&pr);
            }
            self.project_path = Some(p);
        }
        self.saved_settings = Some(s);
    }

    fn current_settings(&self, ctx: &egui::Context) -> Settings {
        Settings {
            mode: Some(mode_key(self.mode).into()),
            interface: Interface::from_egui(ctx.options(|o| o.theme_preference)),
            projet: self.project_path.clone(),
        }
    }

    /// Enregistre les réglages dès qu'ils changent.
    fn persist_settings(&mut self, ctx: &egui::Context) {
        if self.saved_settings.is_none() {
            return;
        }
        let s = self.current_settings(ctx);
        if self.saved_settings.as_ref() != Some(&s) {
            let _ = s.save_to(&settings_path());
            self.saved_settings = Some(s);
        }
    }
}

// ================================================================== dessin

fn seg_points(s: &Seg, cam: &Cam, out: &mut Vec<Pos2>) {
    match *s {
        Seg::Line { a, .. } => out.push(cam.to_screen(a)),
        Seg::Arc {
            c,
            r,
            theta0,
            sweep,
        } => {
            let rp = (r * cam.scale).max(0.0);
            let n = ((sweep.abs() * (2.0 * rp).sqrt() / 1.5).ceil() as usize).clamp(1, 1024);
            for k in 0..n {
                out.push(cam.to_screen(c + dir(theta0 + sweep * k as f64 / n as f64) * r));
            }
        }
    }
}

fn loop_points(l: &Loop, cam: &Cam) -> Vec<Pos2> {
    let mut v = Vec::new();
    for s in &l.segs {
        seg_points(s, cam, &mut v);
    }
    v
}

/// Journal du diagnostic de performance : TRACERY_PERF=1 écrit sur la
/// console, toute autre valeur est le chemin d'un fichier journal.
fn perf_log(line: &str) {
    match std::env::var("TRACERY_PERF") {
        Ok(v) if v != "1" => {
            use std::io::Write as _;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(v)
            {
                let _ = writeln!(f, "{line}");
            }
        }
        Ok(_) => eprintln!("{line}"),
        Err(_) => {}
    }
}

/// Intersection de deux boîtes (aucune si elles sont disjointes).
fn bbox_inter(a: &geom::BBox, b: &geom::BBox) -> Option<geom::BBox> {
    let min = P2::new(a.min.x.max(b.min.x), a.min.y.max(b.min.y));
    let max = P2::new(a.max.x.min(b.max.x), a.max.y.min(b.max.y));
    (min.x < max.x && min.y < max.y).then_some(geom::BBox { min, max })
}

/// Forme du canevas : id, contour, remplage visible, traits vus d'une porte.
type CanvasItem = (u64, Region, Option<Arc<Design>>, Option<Arc<DoorDrawing>>);

/// Polylignes (traits vus d'une porte), hors champ écartées.
fn draw_polylines(p: &egui::Painter, lines: &[Vec<P2>], cam: &Cam, stroke: Stroke) {
    let vis = cam.rect.expand(4.0);
    for l in lines {
        let pts: Vec<Pos2> = l.iter().map(|q| cam.to_screen(*q)).collect();
        if pts.len() >= 2 && Rect::from_points(&pts).intersects(vis) {
            p.add(Shape::line(pts, stroke));
        }
    }
}

fn draw_region(p: &egui::Painter, r: &Region, cam: &Cam, stroke: Stroke) {
    let vis = cam.rect.expand(4.0);
    for l in r.loops() {
        let bb = l.bbox();
        let sr = Rect::from_two_pos(cam.to_screen(bb.min), cam.to_screen(bb.max));
        if !sr.intersects(vis) {
            continue;
        }
        let pts = loop_points(l, cam);
        if pts.len() >= 2 {
            p.add(Shape::closed_line(pts, stroke));
        }
    }
}

fn draw_construction(
    p: &egui::Painter,
    cons: &[ConstructionEntity],
    cam: &Cam,
    pal: &CanvasPalette,
) {
    let st = Stroke::new(1.0, pal.cons);
    for c in cons {
        match *c {
            ConstructionEntity::Circle { c, r, .. } => {
                let mut pts = loop_points(&Loop::circle(c, r), cam);
                if let Some(f) = pts.first().copied() {
                    pts.push(f);
                }
                p.extend(Shape::dashed_line(&pts, st, 6.0, 4.0));
            }
            ConstructionEntity::Line { a, b, .. } => {
                p.extend(Shape::dashed_line(
                    &[cam.to_screen(a), cam.to_screen(b)],
                    Stroke::new(1.0, pal.axis),
                    6.0,
                    4.0,
                ));
            }
            ConstructionEntity::Point { p: q, .. } => {
                p.circle_filled(cam.to_screen(q), 2.2, pal.point);
            }
        }
    }
}

// =============================================================== interface

impl eframe::App for TraceryApp {
    /// Fermeture : dernière sauvegarde automatique (les réglages sont déjà
    /// écrits à chaque changement).
    fn on_exit(&mut self) {
        self.write_autosave();
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let t_frame = Instant::now();
        self.frame_ui(ui, frame);
        // diagnostic : temps moyen et pire temps d'une image, par demi-seconde
        if let Some((t0, n, sum, worst)) = &mut self.perf {
            let dt = t_frame.elapsed();
            *n += 1;
            *sum += dt;
            *worst = (*worst).max(dt);
            // auto-test : une ligne par image (la capture arrive vite)
            if t0.elapsed() >= Duration::from_millis(500) || self.smoke.is_some() {
                let line = format!(
                    "[perf] {n} images, moyenne {:.2} ms, pire {:.2} ms",
                    sum.as_secs_f64() * 1e3 / *n as f64,
                    worst.as_secs_f64() * 1e3
                );
                perf_log(&line);
                *t0 = Instant::now();
                (*n, *sum, *worst) = (0, Duration::ZERO, Duration::ZERO);
            }
        }
    }
}

impl TraceryApp {
    fn frame_ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.shortcuts(&ctx);
        self.schedule();
        self.autosave();
        self.persist_settings(&ctx);

        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));
        egui::Panel::bottom("etat").show(ui, |ui| self.status_bar(ui));
        egui::Panel::bottom("galerie")
            .resizable(true)
            .default_size(230.0)
            .size_range(150.0..=520.0)
            .show(ui, |ui| self.bottom_panel(ui));
        egui::Panel::left("outils")
            .resizable(false)
            .exact_size(132.0)
            .show(ui, |ui| self.toolbar(ui));
        if self.show_objects {
            egui::Panel::left("objets")
                .resizable(true)
                .default_size(240.0)
                .size_range(160.0..=480.0)
                .show(ui, |ui| self.objects_panel(ui));
        }
        egui::Panel::right("parametres")
            .resizable(true)
            .default_size(320.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let before = self.doc.clone();
                    self.right_panel(ui);
                    let changed = self.doc != before;
                    if changed && self.last_panel_push.elapsed() > Duration::from_millis(700) {
                        self.push_undo(before);
                        if let Some(id) = self.sel.primary {
                            self.record_history(id);
                        }
                    }
                    if changed {
                        self.last_panel_push = Instant::now();
                    }
                });
            });
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));

        if self.show_help {
            egui::Window::new("Raccourcis clavier")
                .open(&mut self.show_help)
                .show(&ctx, |ui| {
                    egui::Grid::new("aide").striped(true).show(ui, |ui| {
                        for (k, a) in [
                            ("Espace + glisser / bouton du milieu", "Déplacer la vue"),
                            ("Molette", "Zoom centré sur le curseur"),
                            ("F", "Cadrer les formes"),
                            ("V", "Sélection"),
                            ("B", "Main levée"),
                            ("P / R / C", "Polygone / rectangle / cercle"),
                            ("A", "Arc brisé"),
                            ("G", "Générer / régénérer"),
                            ("→ / ←", "Seed suivante / précédente"),
                            ("D", "Seed aléatoire"),
                            ("T", "Relancer la cellule sélectionnée"),
                            ("L", "Verrouiller / déverrouiller la cellule"),
                            ("1 … 5", "Modes Trait, Pierre, Épure, Vitrail, Épure sombre"),
                            ("O", "Afficher / masquer le panneau des objets"),
                            ("Ctrl+Z / Ctrl+Maj+Z", "Annuler / rétablir"),
                            (
                                "Maj / Ctrl + clic",
                                "Ajouter / retirer une forme de la sélection",
                            ),
                            (
                                "Glisser dans le vide",
                                "Rectangle de sélection (Maj / Ctrl : ajouter)",
                            ),
                            ("Ctrl+A", "Tout sélectionner"),
                            (
                                "Ctrl+C / Ctrl+V",
                                "Copier / coller les formes sélectionnées",
                            ),
                            ("Ctrl+Maj+C", "Copier le génome (Ctrl+V le colle)"),
                            ("Ctrl+S / Ctrl+E", "Enregistrer / exporter"),
                            ("Suppr", "Supprimer la forme ou le sommet"),
                            ("Échap", "Annuler la génération ou l'outil"),
                            ("Double-clic sur un contour libre", "Insérer un sommet"),
                        ] {
                            ui.label(k);
                            ui.label(a);
                            ui.end_row();
                        }
                    });
                });
        }
        let title = format!(
            "Tracery — {}{}",
            self.project_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "sans titre".into()),
            if self.doc != self.saved_doc {
                " •"
            } else {
                ""
            }
        );
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
        if std::mem::take(&mut self.repaint_soon) {
            ctx.request_repaint_after(Duration::from_millis(130));
        }
        // auto-test : capture de la fenêtre une fois le remplage rendu
        let ready = self
            .sel
            .primary
            .and_then(|id| self.gens.get(&id))
            .is_some_and(|g| g.full_for == g.gen_id && g.gen_id > 0)
            && (!matches!(self.mode, Mode::Pierre | Mode::Vitrail)
                || (self.raster.is_some() && !self.raster_inflight));
        if let Some((path, frames, requested)) = &mut self.smoke {
            *frames += 1;
            ctx.request_repaint_after(Duration::from_millis(100));
            if ready && *frames > 20 && !*requested {
                *requested = true;
                let path = path.clone();
                ctx.request_screenshot(move |img| {
                    let mut data = Vec::with_capacity(img.pixels.len() * 4);
                    for c in &img.pixels {
                        data.extend_from_slice(&c.to_array());
                    }
                    if let Some(pm) =
                        tiny_skia::IntSize::from_wh(img.size[0] as u32, img.size[1] as u32)
                            .and_then(|sz| tiny_skia::Pixmap::from_vec(data, sz))
                    {
                        let _ = pm.save_png(&path);
                    }
                    std::process::exit(0);
                });
            }
        }
    }
}

impl TraceryApp {
    fn shortcuts(&mut self, ctx: &egui::Context) {
        // Ctrl+C et Ctrl+V arrivent en événements Copy et Paste (egui-winit) ;
        // un champ de texte actif les garde pour lui.
        if ctx.text_edit_focused() {
            return;
        }
        let (copy, pasted): (bool, Vec<String>) = ctx.input(|i| {
            (
                i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::Paste(t) => Some(t.clone()),
                        _ => None,
                    })
                    .collect(),
            )
        });
        if copy {
            self.copy_objects(ctx);
        }
        for t in pasted {
            if t.trim_start().starts_with("RMP1") {
                self.paste_genome(&t);
            } else if !self.paste_objects(&t) {
                self.status = "Le presse-papiers ne contient ni formes ni génome Tracery.".into();
            }
        }
        let cs = Modifiers::COMMAND | Modifiers::SHIFT;
        let k = |m: Modifiers, key: Key| ctx.input_mut(|i| i.consume_key(m, key));
        if k(cs, Key::Z) || k(Modifiers::COMMAND, Key::Y) {
            self.redo();
        }
        if k(Modifiers::COMMAND, Key::Z) {
            self.undo();
        }
        if k(cs, Key::C) {
            self.copy_genome(ctx);
        }
        if k(Modifiers::COMMAND, Key::S) {
            self.save();
        }
        if k(Modifiers::COMMAND, Key::E) {
            self.export();
        }
        if k(Modifiers::COMMAND, Key::O) {
            self.open();
        }
        if k(Modifiers::COMMAND, Key::A) {
            self.select_all();
        }
        let plain = |key: Key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        if plain(Key::Escape) {
            self.cancel_all();
            self.poly.clear();
            self.drag = Drag::None;
            self.tool = Tool::Select;
        }
        if plain(Key::V) {
            self.tool = Tool::Select;
        }
        if plain(Key::B) {
            self.tool = Tool::Freehand;
        }
        if plain(Key::P) {
            self.tool = Tool::Polygon;
        }
        if plain(Key::R) {
            self.tool = Tool::Prim(PrimKind::Rect);
        }
        if plain(Key::C) {
            self.tool = Tool::Prim(PrimKind::Circle);
        }
        if plain(Key::A) {
            self.tool = Tool::Prim(PrimKind::Arch(self.arch_kind));
        }
        if plain(Key::F) {
            self.fit_view_pending();
        }
        if plain(Key::G) {
            for g in self.gens.values_mut() {
                g.pending = Some(Instant::now() - Duration::from_secs(1));
            }
        }
        if plain(Key::ArrowRight)
            && let Some(s) = self.current().map(|s| s.genome.seed)
        {
            self.set_seed(s.wrapping_add(1));
        }
        if plain(Key::ArrowLeft)
            && let Some(s) = self.current().map(|s| s.genome.seed)
        {
            self.set_seed(s.wrapping_sub(1));
        }
        if plain(Key::D) {
            self.set_seed(now_seed() % 1_000_000_000);
        }
        if plain(Key::T) {
            self.reroll_cell();
        }
        if plain(Key::L) {
            self.toggle_lock();
        }
        if plain(Key::O) {
            self.show_objects = !self.show_objects;
        }
        for (key, m) in [
            (Key::Num1, Mode::Trait),
            (Key::Num2, Mode::Pierre),
            (Key::Num3, Mode::Epure),
            (Key::Num4, Mode::Vitrail),
            (Key::Num5, Mode::EpureSombre),
        ] {
            if plain(key) {
                self.mode = m;
            }
        }
        if plain(Key::Delete) || plain(Key::Backspace) {
            if let Some((id, Handle::Vertex(li, i))) = self.sel_handle {
                self.checkpoint();
                if let Some(s) = self.doc.get_mut(id) {
                    s.delete_vertex(li, i);
                }
                self.sel_handle = None;
            } else {
                self.delete_selected();
            }
        }
        if !self.poly.is_empty() && plain(Key::Enter) {
            self.close_polygon();
        }
        self.space_pan = ctx.input(|i| i.key_down(Key::Space));
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Fichier", |ui| {
                if ui.button("Nouveau").clicked() {
                    self.checkpoint();
                    self.doc = Document::default();
                    self.sel.set(None);
                    self.project_path = None;
                    self.sel_node = None;
                    self.new_style = StyleParams::default();
                }
                if ui.button("Ouvrir…  (Ctrl+O)").clicked() {
                    self.open();
                }
                if ui.button("Enregistrer  (Ctrl+S)").clicked() {
                    self.save();
                }
                if ui.button("Enregistrer sous…").clicked() {
                    self.save_as();
                }
                ui.separator();
                if ui.button("Exporter…  (Ctrl+E)").clicked() {
                    self.export();
                }
                ui.separator();
                ui.menu_button("Formes de référence", |ui| {
                    for id in engine::refshapes::REF_IDS {
                        if let Some(r) = engine::refshapes::ref_shape(id)
                            && ui.button(format!("{id} — {}", r.name)).clicked()
                        {
                            self.add_shape(id, None, Some(r.region));
                            self.fit_view_pending();
                        }
                    }
                });
                ui.separator();
                if ui.button("Quitter").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Édition", |ui| {
                if ui.button("Annuler  (Ctrl+Z)").clicked() {
                    self.undo();
                }
                if ui.button("Rétablir  (Ctrl+Maj+Z)").clicked() {
                    self.redo();
                }
                ui.separator();
                if ui.button("Copier  (Ctrl+C)").clicked() {
                    let c = ui.ctx().clone();
                    self.copy_objects(&c);
                }
                if ui
                    .add_enabled(
                        self.clip_text.is_some(),
                        egui::Button::new("Coller  (Ctrl+V)"),
                    )
                    .clicked()
                    && let Some(t) = self.clip_text.clone()
                {
                    self.paste_objects(&t);
                }
                if ui.button("Dupliquer la sélection").clicked() {
                    self.duplicate_selected();
                }
                if ui.button("Supprimer la sélection  (Suppr)").clicked() {
                    self.delete_selected();
                }
                if ui.button("Tout sélectionner  (Ctrl+A)").clicked() {
                    self.select_all();
                }
                ui.separator();
                for (label, op) in [
                    ("Union", BoolOp::Union),
                    ("Différence", BoolOp::Difference),
                    ("Intersection", BoolOp::Intersection),
                    ("Ou exclusif", BoolOp::Xor),
                ] {
                    if ui.button(label).clicked() {
                        self.apply_boolean(op);
                    }
                }
                ui.separator();
                if ui.button("Rotation +15°").clicked() {
                    self.transform_selected(|c| Xf::rotate_about(c, PI / 12.0));
                }
                if ui.button("Rotation −15°").clicked() {
                    self.transform_selected(|c| Xf::rotate_about(c, -PI / 12.0));
                }
                if ui.button("Symétrie horizontale").clicked() {
                    self.transform_selected(|c| Xf::mirror_x(c.x));
                }
                if ui.button("Agrandir ×1,1").clicked() {
                    self.transform_selected(|c| {
                        Xf::translate(c).then_after(&Xf::scale(1.1).then_after(&Xf::translate(-c)))
                    });
                }
                if ui.button("Réduire ÷1,1").clicked() {
                    self.transform_selected(|c| {
                        Xf::translate(c)
                            .then_after(&Xf::scale(1.0 / 1.1).then_after(&Xf::translate(-c)))
                    });
                }
                if ui.button("Convertir en contour libre").clicked()
                    && let Some(id) = self.sel.primary
                {
                    self.checkpoint();
                    if let Some(s) = self.doc.get_mut(id) {
                        s.make_free();
                    }
                }
                ui.separator();
                if ui.button("Copier le génome  (Ctrl+Maj+C)").clicked() {
                    let c = ui.ctx().clone();
                    self.copy_genome(&c);
                }
            });
            ui.menu_button("Affichage", |ui| {
                for (m, l) in [
                    (Mode::Trait, "Trait  (1)"),
                    (Mode::Pierre, "Pierre  (2)"),
                    (Mode::Epure, "Épure  (3)"),
                    (Mode::Vitrail, "Vitrail  (4)"),
                    (Mode::EpureSombre, "Épure sombre  (5)"),
                ] {
                    ui.radio_value(&mut self.mode, m, l);
                }
                ui.separator();
                ui.menu_button("Thème de l'interface", |ui| {
                    let cur = Interface::from_egui(ui.ctx().options(|o| o.theme_preference));
                    for (t, l) in [
                        (Interface::Systeme, "Celui du système"),
                        (Interface::Clair, "Clair"),
                        (Interface::Sombre, "Sombre"),
                    ] {
                        if ui.radio(cur == t, l).clicked() {
                            ui.ctx().set_theme(t.to_egui());
                        }
                    }
                });
                ui.separator();
                ui.checkbox(&mut self.show_objects, "Panneau des objets  (O)");
                ui.separator();
                if ui.button("Cadrer  (F)").clicked() {
                    self.fit_view_pending();
                }
            });
            ui.menu_button("Génération", |ui| {
                if ui.button("Générer  (G)").clicked() {
                    for g in self.gens.values_mut() {
                        g.pending = Some(Instant::now() - Duration::from_secs(1));
                    }
                }
                if ui.button("Seed suivante  (→)").clicked()
                    && let Some(s) = self.current().map(|s| s.genome.seed)
                {
                    self.set_seed(s.wrapping_add(1));
                }
                if ui.button("Seed précédente  (←)").clicked()
                    && let Some(s) = self.current().map(|s| s.genome.seed)
                {
                    self.set_seed(s.wrapping_sub(1));
                }
                if ui.button("Seed aléatoire  (D)").clicked() {
                    self.set_seed(now_seed() % 1_000_000_000);
                }
                ui.separator();
                if ui.button("Relancer la cellule  (T)").clicked() {
                    self.reroll_cell();
                }
                if ui.button("Verrouiller la cellule  (L)").clicked() {
                    self.toggle_lock();
                }
                if ui.button("Effacer toutes les surcharges").clicked() {
                    self.edit_genome(|g| g.overrides.clear());
                }
                if ui.button("Annuler la génération  (Échap)").clicked() {
                    self.cancel_all();
                }
            });
            ui.menu_button("Aide", |ui| {
                if ui.button("Raccourcis clavier").clicked() {
                    self.show_help = true;
                }
                ui.label(format!("Tracery, générateur {}", engine::GENERATOR_VERSION));
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let mut tool_button = |ui: &mut egui::Ui, t: Tool, label: &str, tip: &str| {
            if ui
                .add_sized(
                    [120.0, 26.0],
                    egui::Button::selectable(self.tool == t, label),
                )
                .on_hover_text(tip)
                .clicked()
            {
                self.tool = t;
                self.poly.clear();
            }
        };
        tool_button(
            ui,
            Tool::Select,
            "Sélection",
            "Sélection, déplacement, poignées, cellules (V)",
        );
        tool_button(
            ui,
            Tool::Freehand,
            "Main levée",
            "Tracé à main levée ajusté en arcs (B)",
        );
        tool_button(
            ui,
            Tool::Polygon,
            "Polygone",
            "Clics successifs, Entrée ou clic sur le premier point pour fermer (P)",
        );
        tool_button(ui, Tool::Prim(PrimKind::Rect), "Rectangle", "Glisser (R)");
        tool_button(ui, Tool::Prim(PrimKind::Circle), "Cercle", "Glisser (C)");
        tool_button(
            ui,
            Tool::Prim(PrimKind::Arch(self.arch_kind)),
            "Arc brisé",
            "Glisser : portée × hauteur (A)",
        );
        ui.label("Type d'arc :");
        let before = self.arch_kind;
        egui::ComboBox::from_id_salt("arc")
            .width(116.0)
            .selected_text(self.arch_kind.name())
            .show_ui(ui, |ui| {
                for (k, l) in [
                    (ArchKind::Round, "plein cintre"),
                    (ArchKind::equilateral(), "tiers-point"),
                    (ArchKind::Pointed { rho: 1.25 }, "lancette"),
                    (ArchKind::Pointed { rho: 0.75 }, "surbaissé"),
                    (ArchKind::tudor(), "Tudor"),
                    (ArchKind::ogee(), "accolade"),
                ] {
                    ui.selectable_value(&mut self.arch_kind, k, l);
                }
            });
        if before != self.arch_kind {
            self.tool = Tool::Prim(PrimKind::Arch(self.arch_kind));
        }
        ui.separator();
        ui.label("Primitives :");
        for (k, l) in [
            (PrimKind::Vesica, "Vesica"),
            (PrimKind::Reuleaux, "Triangle curv."),
            (PrimKind::Oval, "Ovale"),
            (PrimKind::Annulus, "Couronne"),
            (PrimKind::Teardrop, "Larme"),
            (PrimKind::Foil(4), "Quadrilobe"),
            (PrimKind::Polygon(6), "Hexagone"),
        ] {
            if ui
                .add_sized(
                    [120.0, 22.0],
                    egui::Button::selectable(self.tool == Tool::Prim(k), l),
                )
                .clicked()
            {
                self.tool = Tool::Prim(k);
            }
        }
        ui.separator();
        ui.label("Booléens :");
        ui.horizontal_wrapped(|ui| {
            for (l, op, tip) in [
                ("∪", BoolOp::Union, "Union"),
                ("−", BoolOp::Difference, "Différence"),
                ("∩", BoolOp::Intersection, "Intersection"),
                ("⊕", BoolOp::Xor, "Ou exclusif"),
            ] {
                if ui.button(l).on_hover_text(tip).clicked() {
                    self.apply_boolean(op);
                }
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(p) = ui.ctx().pointer_hover_pos()
                && self.cam.rect.contains(p)
            {
                let q = self.cam.to_doc(p);
                ui.monospace(format!("x {:>8.1}  y {:>8.1} mm", q.x, q.y));
                ui.separator();
            }
            ui.monospace(format!("zoom {:.0} %", self.cam.scale * 100.0));
            ui.separator();
            if let Some(d) = self.sel.primary.and_then(|id| self.design_of(id)) {
                ui.label(format!(
                    "{:.0} ms · {} primitives · {} nœuds",
                    d.stats.millis, d.stats.primitives, d.stats.nodes
                ));
                if !d.warnings.is_empty() {
                    ui.separator();
                    ui.colored_label(ACCENT, format!("⚠ {}", d.warnings.len()))
                        .on_hover_text(d.warnings.join("\n"));
                }
                ui.separator();
            }
            if self.busy() {
                ui.spinner();
            }
            ui.label(&self.status);
        });
    }

    /// Choix de l'ouvrage (ADR-29) : catégorie (fenêtre, porte), puis type.
    fn ouvrage_ui(&mut self, ui: &mut egui::Ui, id: u64) {
        let Some(s) = self.doc.get_mut(id) else {
            return;
        };
        let prim = s.prim;
        let cur = Ouvrage::of(&s.genome.style);
        let mut want = cur;
        egui::Grid::new("ouvrage").num_columns(2).show(ui, |ui| {
            ui.label("Catégorie");
            egui::ComboBox::from_id_salt("categorie")
                .selected_text(cur.category().label())
                .show_ui(ui, |ui| {
                    for c in Category::ALL {
                        let fit = Ouvrage::first_fitting(c, &s.genome.style, prim.as_ref());
                        let r = ui.add_enabled(
                            fit.is_some(),
                            egui::Button::selectable(cur.category() == c, c.label()),
                        );
                        let r = r.on_disabled_hover_text(
                            "Les portes demandent un arc (encadrement, porte en bois) ou un rectangle (porte en bois).",
                        );
                        if r.clicked()
                            && c != cur.category()
                            && let Some(o) = fit
                        {
                            want = o;
                        }
                    }
                });
            ui.end_row();
            ui.label("Type");
            egui::ComboBox::from_id_salt("type_ouvrage")
                .selected_text(cur.label())
                .width(200.0)
                .show_ui(ui, |ui| {
                    for &o in cur.category().types() {
                        let r = ui
                            .add_enabled(
                                o.fits(prim.as_ref()) || o == cur,
                                egui::Button::selectable(o == cur, o.label()),
                            )
                            .on_hover_text(o.hint())
                            .on_disabled_hover_text(
                                "Ne convient pas à cette forme : l'encadrement demande un arc, la porte en bois un arc ou un rectangle.",
                            );
                        if r.clicked() {
                            want = o;
                        }
                    }
                });
            ui.end_row();
        });
        if !cur.has_generator() {
            ui.small("Générateur à venir : la forme est remplie d'un remplage en attendant.");
        }
        if cur == Ouvrage::Meneaux {
            // réglages de la croisée (ADR-30)
            let st = &mut s.genome.style;
            egui::Grid::new("croisee").num_columns(2).show(ui, |ui| {
                ui.label("Croisée");
                egui::ComboBox::from_id_salt("croisee_kind")
                    .selected_text(match st.cross {
                        CrossKind::Auto => "selon les proportions",
                        CrossKind::Simple => "simple",
                        CrossKind::Double => "double",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut st.cross,
                            CrossKind::Auto,
                            "selon les proportions",
                        );
                        ui.selectable_value(&mut st.cross, CrossKind::Simple, "simple")
                            .on_hover_text("Un meneau et une traverse : quatre jours.");
                        ui.selectable_value(&mut st.cross, CrossKind::Double, "double")
                            .on_hover_text("Deux meneaux et une traverse : six jours.");
                    });
                ui.end_row();
                ui.label("Angles hauts");
                egui::ComboBox::from_id_salt("croisee_angles")
                    .selected_text(match st.round_top {
                        None => "selon la seed",
                        Some(false) => "droits",
                        Some(true) => "arrondis",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut st.round_top, None, "selon la seed");
                        ui.selectable_value(&mut st.round_top, Some(false), "droits");
                        ui.selectable_value(&mut st.round_top, Some(true), "arrondis");
                    });
                ui.end_row();
            });
            if !prim.is_some_and(|p| p.kind == PrimKind::Rect) {
                ui.small("Angles arrondis, bases, appui et larmier : sur un rectangle seulement.");
            }
        }
        if want != cur {
            want.apply(&mut s.genome.style);
            if want.category() == Category::Porte {
                // recadrer sur le décor de la porte dès la génération reçue
                self.fit_on_door = Some(id);
            }
        }
    }

    fn right_panel(&mut self, ui: &mut egui::Ui) {
        if self.doc.shapes.is_empty() {
            ui.heading("Aucune forme");
            ui.label("Tracez une forme avec un outil de la barre de gauche, ou ouvrez une forme de référence (Fichier → Formes de référence).");
            return;
        }
        let Some(id) = self.sel.primary.filter(|id| self.doc.get(*id).is_some()) else {
            ui.heading("Aucune sélection");
            ui.label("Cliquez sur une forme du canevas, ou sur son nom dans le panneau des objets, pour régler sa forme, sa seed et son style.");
            return;
        };
        let design = self.design_of(id);
        // -------------------------------------------------------- ouvrage
        egui::CollapsingHeader::new("Ouvrage")
            .default_open(true)
            .show(ui, |ui| self.ouvrage_ui(ui, id));
        // ---------------------------------------------------------- forme
        egui::CollapsingHeader::new("Forme").default_open(true).show(ui, |ui| {
            let Some(s) = self.doc.get_mut(id) else { return };
            ui.text_edit_singleline(&mut s.name);
            if let Some(p) = &mut s.prim {
                ui.label(p.kind.label());
                egui::Grid::new("prim").num_columns(2).show(ui, |ui| {
                    ui.label("Centre x");
                    ui.add(egui::DragValue::new(&mut p.center.x).speed(1.0).suffix(" mm"));
                    ui.end_row();
                    ui.label("Centre y");
                    ui.add(egui::DragValue::new(&mut p.center.y).speed(1.0).suffix(" mm"));
                    ui.end_row();
                    ui.label("Largeur");
                    ui.add(egui::DragValue::new(&mut p.w).speed(1.0).range(1.0..=1e6).suffix(" mm"));
                    ui.end_row();
                    ui.label("Hauteur");
                    ui.add(egui::DragValue::new(&mut p.h).speed(1.0).range(1.0..=1e6).suffix(" mm"));
                    ui.end_row();
                    let mut deg = p.angle.to_degrees();
                    ui.label("Rotation");
                    if ui.add(egui::DragValue::new(&mut deg).speed(0.5).suffix(" °")).changed() {
                        p.angle = deg.to_radians();
                    }
                    ui.end_row();
                    if let PrimKind::Arch(ArchKind::Pointed { rho }) = &mut p.kind {
                        ui.label("Brisure ρ");
                        ui.add(egui::Slider::new(rho, 0.5..=1.6));
                        ui.end_row();
                    }
                });
                if ui.button("Convertir en contour libre").clicked() {
                    s.make_free();
                }
            } else if let Some(r) = &s.free {
                ui.label(format!(
                    "Contour libre : {} segments, {} trou(s)",
                    r.segs().count(),
                    r.holes.len()
                ));
                ui.small("Glisser un sommet ou un milieu (courbure) ; double-clic : insérer un sommet ; Suppr : supprimer le sommet sélectionné.");
                if let Some((sid, Handle::Mid(li, i))) = self.sel_handle
                    && sid == id && ui.button("Segment ↔ arc").clicked() {
                        s.toggle_arc(li, i);
                    }
            }
            if let Some(r) = s.region() {
                let bb = r.bbox();
                ui.small(format!("{:.0} × {:.0} mm, aire {:.0} mm²", bb.size().x, bb.size().y, r.area()));
            }
        });
        // ---------------------------------------------------------- seed
        egui::CollapsingHeader::new("Seed")
            .default_open(true)
            .show(ui, |ui| {
                let Some(s) = self.doc.get(id) else { return };
                let seed = s.genome.seed;
                if self.seed_text_src != seed {
                    self.seed_text = to_base32(seed);
                    self.seed_text_src = seed;
                }
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.seed_text)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(170.0),
                );
                if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    let v = parse_seed(&self.seed_text);
                    if let Some(s) = self.doc.get_mut(id) {
                        s.genome.seed = v;
                    }
                }
                ui.horizontal(|ui| {
                    if ui
                        .button("◀")
                        .on_hover_text("Seed précédente (←)")
                        .clicked()
                        && let Some(s) = self.doc.get_mut(id)
                    {
                        s.genome.seed = s.genome.seed.wrapping_sub(1);
                    }
                    if ui.button("▶").on_hover_text("Seed suivante (→)").clicked()
                        && let Some(s) = self.doc.get_mut(id)
                    {
                        s.genome.seed = s.genome.seed.wrapping_add(1);
                    }
                    if ui.button("Aléatoire").on_hover_text("D").clicked()
                        && let Some(s) = self.doc.get_mut(id)
                    {
                        s.genome.seed = now_seed() % 1_000_000_000;
                    }
                    if ui
                        .button("Copier")
                        .on_hover_text("Copier le génome complet (Ctrl+Maj+C)")
                        .clicked()
                    {
                        let c = ui.ctx().clone();
                        self.copy_genome(&c);
                    }
                });
                ui.small("Collez un génome RMP1… avec Ctrl+V sur le canevas.");
                if ui.button("Ajouter aux favoris").clicked()
                    && let Some(s) = self.doc.get(id)
                {
                    let f = io::project::Favorite {
                        shape: s.name.clone(),
                        genome: s.genome.to_compact(),
                        label: format!(
                            "{} #{}",
                            s.genome.style.family.key(),
                            to_base32(s.genome.seed)
                        ),
                    };
                    self.doc.favorites.push(f);
                }
            });
        // ---------------------------------------------------------- style
        egui::CollapsingHeader::new("Style")
            .default_open(true)
            .show(ui, |ui| {
                let Some(s) = self.doc.get_mut(id) else {
                    return;
                };
                let st = &mut s.genome.style;
                let fam_before = st.family;
                egui::ComboBox::from_label("Famille")
                    .selected_text(st.family.key())
                    .show_ui(ui, |ui| {
                        for f in Family::ALL {
                            ui.selectable_value(&mut st.family, f, f.key());
                        }
                    });
                if st.family != fam_before {
                    let keep = st.clone();
                    *st = StyleParams {
                        depth: keep.depth,
                        density: keep.density,
                        w0: keep.w0,
                        lambda: keep.lambda,
                        w_min: keep.w_min,
                        frame: keep.frame,
                        door: keep.door,
                        iron: keep.iron,
                        window: keep.window,
                        cross: keep.cross,
                        round_top: keep.round_top,
                        candidates: keep.candidates,
                        temperature: keep.temperature,
                        ..StyleParams::for_family(st.family)
                    };
                }
                ui.add(egui::Slider::new(&mut st.depth, 1..=7).text("Complexité"));
                ui.add(egui::Slider::new(&mut st.density, 0.35..=0.80).text("Densité"));
                ui.checkbox(&mut st.frame, "Cadre (bordure d'ordre 0)");
                let mut w0 = st.w0 * 100.0;
                if ui
                    .add(egui::Slider::new(&mut w0, 0.0..=8.0).text("Cadre w₀ (% √aire)"))
                    .changed()
                {
                    st.w0 = w0 / 100.0;
                }
                ui.add(egui::Slider::new(&mut st.lambda, 0.5..=1.0).text("Rapport λ"));
                ui.add(egui::Slider::new(&mut st.w_min, 0.0..=20.0).text("Largeur min (mm)"));
                ui.horizontal(|ui| {
                    ui.label("Lobes :");
                    for n in 3u8..=8 {
                        let mut on = st.foils.contains(&n);
                        if ui.checkbox(&mut on, n.to_string()).changed() {
                            if on {
                                st.foils.push(n);
                                st.foils.sort_unstable();
                            } else if st.foils.len() > 1 {
                                st.foils.retain(|x| *x != n);
                            }
                        }
                    }
                });
                ui.add(egui::Slider::new(&mut st.k, 1.0..=1.5).text("Chevauchement k"));
                ui.add(egui::Slider::new(&mut st.rho, 0.5..=1.5).text("Brisure ρ"));
                let sym = st.sym_mode();
                egui::ComboBox::from_label("Symétrie")
                    .selected_text(match sym {
                        SymMode::Strict => "stricte",
                        SymMode::Preferred => "préférée",
                        SymMode::Free => "libre",
                    })
                    .show_ui(ui, |ui| {
                        for (m, l) in [
                            (SymMode::Strict, "stricte"),
                            (SymMode::Preferred, "préférée"),
                            (SymMode::Free, "libre"),
                        ] {
                            if ui.selectable_label(sym == m, l).clicked() {
                                st.symmetry = Some(m);
                            }
                        }
                    });
                ui.add(egui::Slider::new(&mut st.cusping, 0.0..=1.0).text("Redentage"));
                ui.add(egui::Slider::new(&mut st.temperature, 0.0..=2.0).text("Température"));
                egui::ComboBox::from_label("Canon")
                    .selected_text(match st.canon {
                        Canon::Off => "off",
                        Canon::Soft => "doux",
                        Canon::Strict => "strict",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut st.canon, Canon::Off, "off");
                        ui.selectable_value(&mut st.canon, Canon::Soft, "doux");
                        ui.selectable_value(&mut st.canon, Canon::Strict, "strict");
                    });
                ui.add(egui::Slider::new(&mut st.candidates, 1..=64).text("Candidats K"));
                egui::ComboBox::from_label("Cellules terminales")
                    .selected_text(match st.terminal {
                        TerminalMode::Jour => "jour",
                        TerminalMode::Plein => "plein",
                        TerminalMode::Auto => "auto",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut st.terminal, TerminalMode::Jour, "jour");
                        ui.selectable_value(&mut st.terminal, TerminalMode::Plein, "plein");
                        ui.selectable_value(&mut st.terminal, TerminalMode::Auto, "auto");
                    });
                self.new_style = st.clone();
            });
        // ---------------------------------------------------------- score
        egui::CollapsingHeader::new("Score")
            .default_open(true)
            .show(ui, |ui| {
                if let Some(d) = &design {
                    ui.label(format!("Total : {:.3}", d.score.total));
                    for (k, v) in &d.score.terms {
                        ui.add(
                            egui::ProgressBar::new(*v as f32)
                                .desired_width(260.0)
                                .text(format!("{k}  {v:.2}")),
                        );
                    }
                } else if let Some(e) = self.gens.get(&id).and_then(|g| g.error.clone()) {
                    ui.colored_label(Color32::RED, e);
                } else {
                    ui.label("En attente…");
                }
            });
        // ---------------------------------------------------- cellule
        egui::CollapsingHeader::new("Cellule sélectionnée")
            .default_open(true)
            .show(ui, |ui| {
                let Some(node) = self.selected_node() else {
                    ui.small("Cliquez dans le remplage pour sélectionner une cellule.");
                    return;
                };
                let path = NodePath::parse(&node.path);
                let ov = path
                    .as_ref()
                    .and_then(|p| {
                        self.doc
                            .get(id)
                            .and_then(|s| s.genome.override_for(p).cloned())
                    })
                    .unwrap_or_default();
                ui.label(format!("{} — {}", node.path, rule_name(&node.rule)));
                ui.small(format!(
                    "types : {} · rayon inscrit {:.1} mm · score local {:.3}{}",
                    node.types.join(", "),
                    node.inscribed,
                    node.local_score,
                    node.twin_of
                        .as_ref()
                        .map(|t| format!(" · jumeau de {t}"))
                        .unwrap_or_default()
                ));
                if !node.params.0.is_empty() {
                    let ps: Vec<String> = node
                        .params
                        .0
                        .iter()
                        .map(|(k, v)| format!("{k}={v:.3}"))
                        .collect();
                    ui.small(ps.join("  "));
                }
                ui.horizontal(|ui| {
                    if ui.button("Relancer (T)").clicked() {
                        self.reroll_cell();
                    }
                    let l = if ov.lock.is_some() {
                        "Déverrouiller (L)"
                    } else {
                        "Verrouiller (L)"
                    };
                    if ui.button(l).clicked() {
                        self.toggle_lock();
                    }
                });
                if let Some(p) = path {
                    let current = ov.force_rule.clone().unwrap_or_else(|| "auto".into());
                    let mut chosen = current.clone();
                    egui::ComboBox::from_label("Règle imposée")
                        .selected_text(if current == "auto" {
                            "auto".to_string()
                        } else {
                            format!("{current} {}", rule_name(&current))
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut chosen, "auto".to_string(), "auto");
                            for r in engine::rules::registry() {
                                ui.selectable_value(
                                    &mut chosen,
                                    r.id().to_string(),
                                    format!("{} {}", r.id(), r.name()),
                                );
                            }
                        });
                    if chosen != current {
                        let f = if chosen == "auto" {
                            None
                        } else {
                            Some(chosen.clone())
                        };
                        if let Some(s) = self.doc.get_mut(id) {
                            s.genome.force_rule(&p, f.as_deref());
                            s.genome.prune();
                        }
                    }
                }
            });
        // ---------------------------------------------------------- arbre
        egui::CollapsingHeader::new("Arbre de dérivation")
            .default_open(false)
            .show(ui, |ui| {
                let Some(d) = &design else { return };
                let ovs = self
                    .doc
                    .get(id)
                    .map(|s| s.genome.overrides.clone())
                    .unwrap_or_default();
                for n in &d.tree {
                    let mut label = format!(
                        "{}{} {}",
                        "  ".repeat(n.depth as usize),
                        n.path,
                        rule_name(&n.rule)
                    );
                    if ovs.get(&n.path).is_some_and(|o| o.lock.is_some()) {
                        label.push_str(" 🔒");
                    }
                    if n.twin_of.is_some() {
                        label.push_str(" ⇔");
                    }
                    if ui
                        .selectable_label(self.sel_node.as_deref() == Some(&n.path), label)
                        .clicked()
                    {
                        self.sel_node = Some(n.path.clone());
                    }
                }
            });
    }

    /// Hiérarchie des objets : formes, puis arbre de dérivation de chacune.
    /// Case cochée : affiché ; décochée : masqué (affichage seulement, les
    /// exports restent complets). Un clic sur un nom sélectionne.
    fn objects_panel(&mut self, ui: &mut egui::Ui) {
        ui.strong("Objets");
        ui.horizontal_wrapped(|ui| {
            if ui
                .small_button("Tout masquer")
                .on_hover_text("Masque toutes les formes")
                .clicked()
            {
                self.hidden_shapes = self.doc.shapes.iter().map(|s| s.id).collect();
                self.sel_node = None;
            }
            if ui
                .small_button("Tout afficher")
                .on_hover_text("Affiche toutes les formes et tous les nœuds")
                .clicked()
            {
                self.hidden_shapes.clear();
                self.hidden_nodes.clear();
            }
        });
        ui.separator();
        let shapes: Vec<(u64, String)> = self
            .doc
            .shapes
            .iter()
            .map(|s| (s.id, s.name.clone()))
            .collect();
        if shapes.is_empty() {
            ui.weak("Aucune forme. Tracez-en une sur le canevas.");
            return;
        }
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (id, name) in shapes {
                    let design = self.design_of(id);
                    let visible = !self.hidden_shapes.contains(&id);
                    let cid = ui.make_persistent_id(("objet", id));
                    let state = egui::collapsing_header::CollapsingState::load_with_default_open(
                        ui.ctx(),
                        cid,
                        self.sel.primary == Some(id),
                    );
                    state
                        .show_header(ui, |ui| {
                            let mut v = visible;
                            if ui
                                .checkbox(&mut v, "")
                                .on_hover_text("Afficher / masquer la forme")
                                .changed()
                            {
                                self.set_shape_visible(id, v);
                            }
                            let text = if visible {
                                egui::RichText::new(&name).strong()
                            } else {
                                egui::RichText::new(&name).weak()
                            };
                            let on = self.sel.contains(id)
                                && (self.sel.primary != Some(id) || self.sel_node.is_none());
                            let r = ui.selectable_label(on, text);
                            if r.clicked() {
                                // Maj / Ctrl + clic : ajouter ou retirer
                                let m = ui.input(|i| i.modifiers);
                                if m.shift || m.command {
                                    self.sel.toggle(id);
                                } else {
                                    self.sel.set(Some(id));
                                }
                                self.sel_node = None;
                                self.sel_handle = None;
                            }
                        })
                        .body(|ui| match &design {
                            Some(d) => {
                                let mut kids: BTreeMap<String, Vec<usize>> = BTreeMap::new();
                                let mut roots = Vec::new();
                                for (i, n) in d.tree.iter().enumerate() {
                                    match n.path.rfind('/') {
                                        Some(k) => {
                                            kids.entry(n.path[..k].to_string()).or_default().push(i)
                                        }
                                        None => roots.push(i),
                                    }
                                }
                                for i in roots {
                                    self.node_row(ui, id, d, &kids, i, !visible);
                                }
                            }
                            None => {
                                ui.weak("Pas encore de remplage.");
                            }
                        });
                }
            });
    }

    /// Ligne d'un nœud de l'arbre (dépliable s'il a des enfants).
    fn node_row(
        &mut self,
        ui: &mut egui::Ui,
        id: u64,
        d: &Design,
        kids: &BTreeMap<String, Vec<usize>>,
        i: usize,
        parent_hidden: bool,
    ) {
        let n = &d.tree[i];
        let own_hidden = self
            .hidden_nodes
            .get(&id)
            .is_some_and(|s| s.contains(&n.path));
        let hidden = parent_hidden || own_hidden;
        let mut label = format!("{}  {}", rule_name(&n.rule), n.path);
        if n.twin_of.is_some() {
            label.push_str("  ⇔");
        }
        let text = if hidden {
            egui::RichText::new(label).weak()
        } else {
            egui::RichText::new(label)
        };
        let selected =
            self.sel.primary == Some(id) && self.sel_node.as_deref() == Some(n.path.as_str());
        let path = n.path.clone();
        let header = |ui: &mut egui::Ui, this: &mut Self| {
            let mut v = !own_hidden;
            if ui
                .add_enabled(!parent_hidden, egui::Checkbox::without_text(&mut v))
                .on_hover_text("Afficher / masquer ce nœud et son contenu")
                .changed()
            {
                this.set_node_visible(id, &path, v);
            }
            if ui.selectable_label(selected, text.clone()).clicked() {
                this.sel.set(Some(id));
                this.sel_node = Some(path.clone());
            }
        };
        match kids.get(&n.path) {
            Some(children) if !children.is_empty() => {
                let cid = ui.make_persistent_id(("noeud", id, n.path.as_str()));
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    cid,
                    n.depth < 2,
                )
                .show_header(ui, |ui| header(ui, self))
                .body(|ui| {
                    for &c in children {
                        self.node_row(ui, id, d, kids, c, hidden);
                    }
                });
            }
            _ => {
                ui.horizontal(|ui| {
                    // aligne les feuilles sur les nœuds dépliables
                    ui.add_space(ui.spacing().indent);
                    header(ui, self);
                });
            }
        }
    }

    fn bottom_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Gallery, "Galerie");
            ui.selectable_value(&mut self.tab, Tab::History, "Historique");
            ui.selectable_value(&mut self.tab, Tab::Favorites, "Favoris");
            ui.separator();
            if self.tab == Tab::Gallery {
                ui.add(egui::Slider::new(&mut self.gallery.n, 8..=64).text("vignettes"));
                ui.checkbox(&mut self.gallery.sort, "trier par score");
                if ui.button("Générer la galerie").clicked() {
                    self.start_gallery();
                }
            }
        });
        ui.separator();
        match self.tab {
            Tab::Gallery => {
                let mut order: Vec<usize> = (0..self.gallery.items.len()).collect();
                if self.gallery.sort {
                    order.sort_by(|a, b| {
                        let sa = self.gallery.items[*a]
                            .as_ref()
                            .map(|t| t.score)
                            .unwrap_or(-1.0);
                        let sb = self.gallery.items[*b]
                            .as_ref()
                            .map(|t| t.score)
                            .unwrap_or(-1.0);
                        sb.total_cmp(&sa)
                    });
                }
                let mut adopt = None;
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for i in order {
                            ui.vertical(|ui| match &self.gallery.items[i] {
                                Some(t) => {
                                    let size =
                                        t.tex.size_vec2() * (150.0 / t.tex.size_vec2().max_elem());
                                    if ui
                                        .add(egui::Button::image(egui::Image::new((
                                            t.tex.id(),
                                            size,
                                        ))))
                                        .on_hover_text("Cliquer pour adopter cette variante")
                                        .clicked()
                                    {
                                        adopt = Some(t.seed);
                                    }
                                    ui.small(format!("{:.3}", t.score));
                                }
                                None => {
                                    ui.add_sized([150.0, 150.0], egui::Spinner::new());
                                }
                            });
                        }
                    });
                });
                if let Some(seed) = adopt {
                    self.sel.set(Some(self.gallery.shape));
                    self.set_seed(seed);
                }
            }
            Tab::History => {
                let mut apply = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (k, (sid, g)) in self.history.iter().enumerate().rev() {
                        let name = self
                            .doc
                            .get(*sid)
                            .map(|s| s.name.clone())
                            .unwrap_or_else(|| "?".into());
                        let short: String = g.chars().take(48).collect();
                        if ui
                            .selectable_label(false, format!("{k:>3}  {name}  {short}…"))
                            .clicked()
                        {
                            apply = Some((*sid, g.clone()));
                        }
                    }
                });
                if let Some((sid, g)) = apply
                    && self.doc.get(sid).is_some()
                {
                    self.sel.set(Some(sid));
                    self.paste_genome(&g);
                }
            }
            Tab::Favorites => {
                let mut apply = None;
                let mut remove = None;
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (k, f) in self.doc.favorites.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut f.label);
                            ui.small(&f.shape);
                            if ui.button("Appliquer").clicked() {
                                apply = Some(f.genome.clone());
                            }
                            if ui.button("✕").clicked() {
                                remove = Some(k);
                            }
                        });
                    }
                    if self.doc.favorites.is_empty() {
                        ui.small("Aucun favori : utilisez « ★ Ajouter aux favoris » dans le panneau Seed.");
                    }
                });
                if let Some(g) = apply {
                    self.paste_genome(&g);
                }
                if let Some(k) = remove {
                    self.checkpoint();
                    self.doc.favorites.remove(k);
                }
            }
        }
    }

    fn close_polygon(&mut self) {
        if self.poly.len() >= 3 {
            let mut l = Loop::polygon(&self.poly);
            if !l.is_ccw() {
                l = l.reversed();
            }
            if geom::clip::self_intersects(&l) {
                self.status = "Polygone auto-intersectant : tracé refusé.".into();
            } else {
                self.add_shape("Polygone", None, Some(Region::simple(l)));
            }
        }
        self.poly.clear();
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let first = self.cam.rect.width() < 1.0
            || self.cam.rect == Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0));
        self.cam.rect = resp.rect;
        if first {
            self.fit_view_pending();
        }
        let ppp = ui.ctx().pixels_per_point();
        let ctx = ui.ctx().clone();
        let pointer = resp.hover_pos().or(resp.interact_pointer_pos());
        let doc_p = pointer.map(|p| self.cam.to_doc(p));
        let px = 1.0 / self.cam.scale; // une unité écran en mm

        // ------------------------------------------------------------ vue
        if resp.hovered() {
            let (scroll, zoom) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let f = exp(scroll as f64 * 0.0018) * zoom as f64;
            if (f - 1.0).abs() > 1e-6
                && let Some(p) = pointer
            {
                let before = self.cam.to_doc(p);
                self.cam.scale = (self.cam.scale * f).clamp(0.01, 100.0);
                let after = self.cam.to_doc(p);
                self.cam.center += before - after;
            }
        }
        let panning = resp.dragged_by(PointerButton::Middle)
            || (self.space_pan && resp.dragged_by(PointerButton::Primary));
        if panning {
            let d = resp.drag_delta();
            self.cam.center -= P2::new(d.x as f64, -d.y as f64) / self.cam.scale;
        }

        // ------------------------------------------------------ outils
        // Gestion pilotée par les événements (appui, déplacements,
        // relâchement) : le point d'appui est conservé et tous les points
        // sont pris en compte, même quand plusieurs événements arrivent dans
        // une seule image.
        if !panning {
            let evs: Vec<PEv> = ctx.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::PointerButton {
                            pos,
                            button: PointerButton::Primary,
                            pressed,
                            modifiers,
                        } => Some(if *pressed {
                            PEv::Down(*pos, modifiers.shift || modifiers.command)
                        } else {
                            PEv::Up(*pos)
                        }),
                        egui::Event::PointerMoved(p) => Some(PEv::Move(*p)),
                        _ => None,
                    })
                    .collect()
            });
            for ev in evs {
                match ev {
                    PEv::Down(pos, shift) => {
                        let on_canvas =
                            resp.rect.contains(pos) && ctx.layer_id_at(pos) == Some(resp.layer_id);
                        if on_canvas && !self.space_pan {
                            self.press = Some((pos, shift, false));
                            self.on_down(self.cam.to_doc(pos), shift, px);
                        }
                    }
                    PEv::Move(pos) => {
                        if let Some((p0, _, moved)) = &mut self.press {
                            if p0.distance(pos) > 3.0 {
                                *moved = true;
                            }
                            let moved = *moved;
                            self.on_move(self.cam.to_doc(pos), moved, px);
                        }
                    }
                    PEv::Up(pos) => {
                        if let Some((p0, shift, moved)) = self.press.take() {
                            let moved = moved || p0.distance(pos) > 3.0;
                            self.on_up(self.cam.to_doc(pos), shift, moved, px);
                        }
                    }
                }
            }
            if resp.double_clicked() {
                match self.tool {
                    Tool::Polygon => self.close_polygon(),
                    Tool::Select => {
                        if let (Some(id), Some(p)) = (self.sel.primary, doc_p) {
                            self.checkpoint();
                            if let Some(s) = self.doc.get_mut(id)
                                && s.insert_vertex(p)
                            {
                                self.status = "Sommet inséré.".into();
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // -------------------------------------------------------- rendu
        self.request_raster(ppp);
        let pal = canvas_palette(self.mode);
        painter.rect_filled(resp.rect, 0.0, pal.bg);
        let cam = self.cam;
        let raster_mode = matches!(self.mode, Mode::Pierre | Mode::Vitrail);
        if raster_mode && let Some((tex, v)) = &self.raster {
            painter.image(
                tex.id(),
                cam.view_rect(v),
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let epure = matches!(self.mode, Mode::Epure | Mode::EpureSombre);
        let visible: Vec<(u64, Region)> = self
            .doc
            .shapes
            .iter()
            .filter(|s| !self.hidden_shapes.contains(&s.id))
            .filter_map(|s| s.region().map(|r| (s.id, r)))
            .collect();
        let mut items: Vec<CanvasItem> = Vec::new();
        for (id, r) in visible {
            let d = self.visible_design(id);
            let dd = match &d {
                Some(d) if d.door && !raster_mode => Some(self.door_drawing_of(id, d)),
                _ => None,
            };
            items.push((id, r, d, dd));
        }
        for (s_id, region, design, door_dr) in &items {
            let s_id = *s_id;
            match (design, raster_mode) {
                (Some(d), false) => {
                    let w = if epure { 1.0 } else { 1.3 };
                    if let Some(dd) = door_dr {
                        draw_polylines(&painter, &dd.lines, &cam, Stroke::new(0.8, pal.ink));
                    }
                    draw_region(&painter, &d.shape, &cam, Stroke::new(1.6, pal.ink));
                    for o in &d.openings {
                        draw_region(&painter, &o.region, &cam, Stroke::new(w, pal.ink));
                    }
                    if epure {
                        draw_construction(&painter, &d.construction, &cam, &pal);
                    }
                }
                (Some(_), true) => {}
                (None, _) => draw_region(&painter, region, &cam, Stroke::new(1.5, pal.ink)),
            }
            // aperçu en cours : contour pointillé discret
            if self
                .gens
                .get(&s_id)
                .is_some_and(|g| g.preview_shown && g.running > 0)
            {
                painter.text(
                    cam.to_screen(region.bbox().max),
                    egui::Align2::RIGHT_BOTTOM,
                    "aperçu",
                    egui::FontId::proportional(11.0),
                    ACCENT,
                );
            }
        }
        // sélection
        // (principale en orange, autres formes en bleu)
        for (k, id) in self.sel.ids().into_iter().enumerate() {
            let col = if k == 0 {
                ACCENT
            } else {
                Color32::from_rgb(60, 120, 200)
            };
            let Some(s) = self.doc.get(id) else {
                continue;
            };
            if self.hidden_shapes.contains(&s.id) {
                continue;
            }
            let Some(r) = s.region() else { continue };
            let bb = r.bbox();
            let rect = Rect::from_two_pos(cam.to_screen(bb.min), cam.to_screen(bb.max)).expand(6.0);
            painter.add(Shape::dashed_line(
                &[
                    rect.left_top(),
                    rect.right_top(),
                    rect.right_bottom(),
                    rect.left_bottom(),
                    rect.left_top(),
                ],
                Stroke::new(1.0, col),
                5.0,
                4.0,
            ));
        }
        if let Some(node) = self.selected_node()
            && let Some(r) = &node.region
        {
            draw_region(&painter, r, &cam, Stroke::new(2.5, ACCENT));
        }
        // poignées : seulement pour une forme seule (en multi-sélection, un
        // appui sur une forme déplace tout le groupe)
        if self.tool == Tool::Select
            && !self.sel.is_multi()
            && let Some(s) = self.current()
            && !self.hidden_shapes.contains(&s.id)
        {
            for (h, p) in s.handles() {
                let q = cam.to_screen(p);
                let selected = self
                    .sel_handle
                    .is_some_and(|(id, sh)| id == s.id && sh == h);
                match h {
                    Handle::Mid(..) => {
                        painter.circle(
                            q,
                            3.5,
                            if selected { ACCENT } else { Color32::WHITE },
                            Stroke::new(1.0, ACCENT),
                        );
                    }
                    _ => {
                        painter.rect(
                            Rect::from_center_size(q, vec2(8.0, 8.0)),
                            0.0,
                            if selected { ACCENT } else { Color32::WHITE },
                            Stroke::new(1.2, ACCENT),
                            egui::StrokeKind::Middle,
                        );
                    }
                }
            }
        }
        // tracés en cours
        match &self.drag {
            Drag::Stroke(pts) => {
                let v: Vec<Pos2> = pts.iter().map(|p| cam.to_screen(*p)).collect();
                painter.add(Shape::line(v, Stroke::new(1.5, ACCENT)));
            }
            Drag::Prim { start, cur } => {
                if let Tool::Prim(kind) = self.tool {
                    let spec = PrimSpec {
                        kind,
                        center: (*start + *cur) * 0.5,
                        w: (cur.x - start.x).abs(),
                        h: (cur.y - start.y).abs(),
                        angle: 0.0,
                    };
                    if let Some(r) = spec.build() {
                        draw_region(&painter, &r, &cam, Stroke::new(1.5, ACCENT));
                    }
                }
            }
            Drag::Marquee { start, cur, .. } if self.press.is_some_and(|p| p.2) => {
                let r = Rect::from_two_pos(cam.to_screen(*start), cam.to_screen(*cur));
                painter.rect(
                    r,
                    0.0,
                    ACCENT.gamma_multiply(0.08),
                    Stroke::new(1.0, ACCENT),
                    egui::StrokeKind::Middle,
                );
            }
            _ => {}
        }
        if !self.poly.is_empty() {
            let mut v: Vec<Pos2> = self.poly.iter().map(|p| cam.to_screen(*p)).collect();
            if let Some(p) = pointer {
                v.push(p);
            }
            painter.add(Shape::line(v, Stroke::new(1.5, ACCENT)));
        }
        if self.doc.shapes.is_empty() {
            painter.text(
                resp.rect.center(),
                egui::Align2::CENTER_CENTER,
                "Choisissez un outil (A : arc brisé, B : main levée…) et tracez une forme",
                egui::FontId::proportional(16.0),
                Color32::GRAY,
            );
        }
        // infobulle de cellule (EF-63)
        if self.tool == Tool::Select
            && !panning
            && let (Some(id), Some(p)) = (self.sel.primary, doc_p)
            && let Some(n) = self.node_at(id, p)
        {
            resp.on_hover_text(format!(
                "{} — {}\nprofondeur {} · rayon inscrit {:.1} mm\nscore local {:.3}",
                n.path,
                rule_name(&n.rule),
                n.depth,
                n.inscribed,
                n.local_score
            ));
        }
    }

    fn handle_at(&self, p: P2, px: f64) -> Option<(u64, Handle)> {
        if self.sel.is_multi() {
            return None;
        }
        self.current().and_then(|s| {
            s.handles()
                .into_iter()
                .filter(|(_, q)| dist(*q, p) < 9.0 * px)
                .min_by(|a, b| dist(a.1, p).total_cmp(&dist(b.1, p)))
                .map(|(h, _)| (s.id, h))
        })
    }

    /// Appui du bouton principal. `additive` : Maj ou Ctrl enfoncée.
    fn on_down(&mut self, p: P2, additive: bool, px: f64) {
        self.fresh_select = false;
        self.drag = match self.tool {
            Tool::Select => {
                if let Some((id, h)) = self.handle_at(p, px).filter(|_| !additive) {
                    self.sel_handle = Some((id, h));
                    Drag::Handle {
                        id,
                        h,
                        before: self.doc.clone(),
                    }
                } else if let Some(id) = self.shape_at(p) {
                    // Une forme déjà sélectionnée garde la sélection (le
                    // groupe se déplace) ; sinon elle la remplace, ou s'y
                    // ajoute avec Maj / Ctrl.
                    if !self.sel.contains(id) {
                        if additive {
                            self.sel.add(id);
                        } else {
                            self.sel.set(Some(id));
                        }
                        self.sel_node = None;
                        self.sel_handle = None;
                        self.fresh_select = true;
                    }
                    Drag::Move {
                        ids: self.sel.ids(),
                        last: p,
                        before: self.doc.clone(),
                    }
                } else {
                    Drag::Marquee {
                        start: p,
                        cur: p,
                        additive,
                    }
                }
            }
            Tool::Freehand => Drag::Stroke(vec![p]),
            Tool::Prim(_) => Drag::Prim { start: p, cur: p },
            Tool::Polygon => Drag::None,
        };
    }

    fn on_move(&mut self, p: P2, moved: bool, px: f64) {
        match &mut self.drag {
            Drag::Handle { id, h, .. } if moved => {
                let (id, h) = (*id, *h);
                if let Some(s) = self.doc.get_mut(id) {
                    s.drag_handle(h, p);
                }
            }
            Drag::Move { ids, last, .. } if moved => {
                let d = p - *last;
                *last = p;
                let ids = ids.clone();
                self.doc.translate(&ids, d);
            }
            Drag::Stroke(pts) => {
                if pts.last().is_none_or(|q| dist(*q, p) > px) {
                    pts.push(p);
                }
            }
            Drag::Prim { cur, .. } | Drag::Marquee { cur, .. } => *cur = p,
            _ => {}
        }
    }

    fn on_up(&mut self, p: P2, additive: bool, moved: bool, px: f64) {
        self.on_move(p, moved, px);
        match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::Handle { before, .. } | Drag::Move { before, .. } if moved => {
                self.push_undo(before);
                // génération complète au relâchement
                for g in self.gens.values_mut() {
                    if g.pending.is_some() {
                        g.pending = Some(Instant::now() - Duration::from_secs(1));
                    }
                }
            }
            Drag::Marquee {
                start,
                cur,
                additive,
            } if moved => {
                let b = geom::BBox::from_pts(start, cur);
                let ids = self.doc.inside(&b, |id| self.hidden_shapes.contains(&id));
                if additive {
                    for id in &ids {
                        self.sel.add(*id);
                    }
                } else {
                    self.sel.set_many(&ids);
                }
                self.sel_node = None;
                self.sel_handle = None;
                self.status = format!("{} forme(s) sélectionnée(s).", self.sel.len());
            }
            Drag::Stroke(pts) => match fit_stroke(&pts, &FitOptions::from_pixel(px)) {
                Some(l) => {
                    let n = l.segs.len();
                    self.add_shape("Tracé", None, Some(Region::simple(l)));
                    self.status = format!("Tracé ajusté en {n} arcs et segments.");
                }
                None => self.status = "Tracé trop court ou auto-intersectant : refusé.".into(),
            },
            Drag::Prim { start, cur } => {
                let (w, h) = ((cur.x - start.x).abs(), (cur.y - start.y).abs());
                if let Tool::Prim(kind) = self.tool
                    && w * self.cam.scale > 6.0
                    && h * self.cam.scale > 6.0
                {
                    let spec = PrimSpec {
                        kind,
                        center: (start + cur) * 0.5,
                        w,
                        h,
                        angle: 0.0,
                    };
                    self.add_shape(&kind.label(), Some(spec), None);
                    self.tool = Tool::Select;
                }
            }
            _ => {
                if !moved {
                    self.click(p, additive, px);
                }
            }
        }
    }

    /// Clic simple (sans déplacement).
    fn click(&mut self, p: P2, additive: bool, px: f64) {
        match self.tool {
            Tool::Polygon => {
                let near_first = self
                    .poly
                    .first()
                    .is_some_and(|f| dist(*f, p) * self.cam.scale < 12.0);
                if near_first && self.poly.len() >= 3 {
                    self.close_polygon();
                } else if self
                    .poly
                    .last()
                    .is_none_or(|q| dist(*q, p) * self.cam.scale > 2.0)
                {
                    self.poly.push(p);
                }
            }
            Tool::Select => match self.shape_at(p) {
                // Maj / Ctrl + clic : ajouter (fait à l'appui) ou retirer
                Some(id) if additive => {
                    if !self.fresh_select {
                        self.sel.toggle(id);
                    }
                    self.sel_node = None;
                    self.sel_handle = None;
                }
                // clic simple sur une forme d'un groupe : elle seule reste
                Some(id) if self.sel.is_multi() => {
                    self.sel.set(Some(id));
                    self.sel_node = None;
                    self.sel_handle = None;
                }
                Some(id) => {
                    // premier clic : la forme ; clic suivant : la cellule
                    if self.sel.primary != Some(id) || self.fresh_select {
                        self.sel.set(Some(id));
                        self.sel_node = None;
                    } else {
                        self.sel_node = self.node_at(id, p).map(|n| n.path);
                    }
                    self.sel_handle = self.handle_at(p, px);
                }
                // clic sur une poignée posée sur le contour : la sélection reste
                None if self.handle_at(p, px).is_some() => {
                    self.sel_handle = self.handle_at(p, px);
                }
                // Maj / Ctrl + clic dans le vide : la sélection reste
                None if additive => {}
                // clic dans le vide : plus rien n'est sélectionné
                None => {
                    self.sel.set(None);
                    self.sel_node = None;
                    self.sel_handle = None;
                }
            },
            _ => {}
        }
    }

    fn shape_at(&self, p: P2) -> Option<u64> {
        self.doc
            .shapes
            .iter()
            .rev()
            .filter(|s| !self.hidden_shapes.contains(&s.id))
            .find(|s| s.region().is_some_and(|r| r.outer.contains(p)))
            .map(|s| s.id)
    }
}
