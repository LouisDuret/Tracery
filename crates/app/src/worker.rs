//! Tâches d'arrière-plan (§ 6.4) : générations (aperçu puis complète),
//! rendus raster du canevas, vignettes de la galerie. L'interface ne bloque
//! jamais : les tâches tournent dans le pool rayon et les résultats remontent
//! par canal, relevés à chaque image.

use engine::hidden::{DoorDrawing, door_drawing};
use engine::{CancelToken, Design, Genome, generate};
use geom::Region;
use io::raster::{Mode, RenderOpts, View, render, render_view};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

#[allow(clippy::large_enum_variant)] // messages peu fréquents
pub enum Msg {
    Gen {
        shape: u64,
        gen_id: u64,
        preview: bool,
        result: Result<Design, String>,
        /// Traits vus d'une porte, calculés ici plutôt qu'à l'affichage.
        drawing: Option<Arc<DoorDrawing>>,
    },
    Raster {
        id: u64,
        image: egui::ColorImage,
        view: View,
    },
    Thumb {
        batch: u64,
        idx: usize,
        seed: u64,
        design: Arc<Design>,
        image: egui::ColorImage,
    },
}

pub struct Worker {
    tx: Sender<Msg>,
    pub rx: Receiver<Msg>,
    ctx: egui::Context,
}

fn to_image(pm: &tiny_skia::Pixmap) -> egui::ColorImage {
    egui::ColorImage::from_rgba_premultiplied(
        [pm.width() as usize, pm.height() as usize],
        pm.data(),
    )
}

impl Worker {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        Worker { tx, rx, ctx }
    }

    /// Génération (aperçu : profondeur ≤ 2, peu de candidats).
    pub fn generate(
        &self,
        shape: u64,
        gen_id: u64,
        region: Region,
        genome: Genome,
        preview: bool,
        cancel: CancelToken,
    ) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        rayon::spawn(move || {
            let mut g = genome;
            if preview {
                g.style.depth = g.style.depth.min(2);
                g.style.candidates = g.style.candidates.min(4);
            }
            let result = std::panic::catch_unwind(|| generate(&region, &g, &cancel))
                .map_err(|_| "panique pendant la génération".to_string())
                .and_then(|r| r.map_err(|e| e.to_string()));
            let drawing = match &result {
                Ok(d) if d.door && !cancel.is_cancelled() => Some(Arc::new(door_drawing(d))),
                _ => None,
            };
            let _ = tx.send(Msg::Gen {
                shape,
                gen_id,
                preview,
                result,
                drawing,
            });
            ctx.request_repaint();
        });
    }

    /// Rendu raster de plusieurs remplages dans une vue (fond du canevas).
    pub fn raster(&self, id: u64, designs: Vec<Arc<Design>>, mode: Mode, view: View) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        rayon::spawn(move || {
            let refs: Vec<&Design> = designs.iter().map(|d| d.as_ref()).collect();
            let pm = render_view(&refs, mode, true, &view, 1.2);
            let _ = tx.send(Msg::Raster {
                id,
                image: to_image(&pm),
                view,
            });
            ctx.request_repaint();
        });
    }

    /// Vignette de galerie.
    pub fn thumb(&self, batch: u64, idx: usize, region: Region, genome: Genome, size: u32) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        rayon::spawn(move || {
            let seed = genome.seed;
            if let Ok(Ok(d)) =
                std::panic::catch_unwind(|| generate(&region, &genome, &CancelToken::new()))
            {
                let pm = render(
                    &d,
                    &RenderOpts {
                        size,
                        mode: Mode::Pierre,
                        transparent: false,
                        margin: 0.04,
                    },
                );
                let _ = tx.send(Msg::Thumb {
                    batch,
                    idx,
                    seed,
                    design: Arc::new(d),
                    image: to_image(&pm),
                });
                ctx.request_repaint();
            }
        });
    }
}
