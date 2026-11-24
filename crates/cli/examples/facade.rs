//! Planche d'une façade (ADR-32) : décor du mur et toutes les ouvertures
//! du plan, chacune générée par son générateur, rendues ensemble.
//!
//! cargo run --release -p cli --example facade -- <seed> <largeur> <hauteur> <sortie.png> [mode]
use engine::{CancelToken, Genome, StyleParams, generate};
use geom::Region;
use geom::math::*;
use io::raster::{Mode, View, render_view};
use rayon::prelude::*;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed: u64 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let w: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(22000.0);
    let h: f64 = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(14000.0);
    let out = a.get(4).cloned().unwrap_or_else(|| "facade.png".into());
    let mode = a
        .get(5)
        .and_then(|m| Mode::parse(m))
        .unwrap_or(Mode::Pierre);
    let shape = Region::simple(motifs::shapes::rectangle(P2::new(0.0, 0.5 * h), w, h));
    let g = Genome::new(
        seed,
        StyleParams {
            facade: true,
            ..StyleParams::for_family(engine::Family::Flamboyant)
        },
    );
    let c = CancelToken::new();
    let t0 = std::time::Instant::now();
    let d = generate(&shape, &g, &c).expect("façade");
    let p = engine::facade::plan(&shape.bbox(), &g);
    println!(
        "{} étages, {} travées, {} tours, {} échauguettes, {} ouvertures, {} ornements, {:.0} ms",
        p.floors.len() - 1,
        p.columns.len(),
        p.towers.len(),
        p.bartizans.len(),
        p.openings.len(),
        d.ornaments.len(),
        t0.elapsed().as_secs_f64() * 1000.0
    );
    let t1 = std::time::Instant::now();
    let dr = engine::hidden::door_drawing(&d);
    println!(
        "traits vus : {} lignes, {:.0} ms",
        dr.lines.len(),
        t1.elapsed().as_secs_f64() * 1000.0
    );
    let mut ds = vec![d];
    let more: Vec<_> = p
        .openings
        .par_iter()
        .filter_map(|o| generate(&o.region(), &o.genome, &c).ok())
        .collect();
    ds.extend(more);
    let bb = ds.iter().fold(geom::BBox::EMPTY, |b, d| b.union(&d.bbox()));
    let v = View::fit(&bb, 2400, 0.02);
    let refs: Vec<&engine::Design> = ds.iter().collect();
    let t2 = std::time::Instant::now();
    let pm = render_view(&refs, mode, false, &v, 1.2);
    println!("rendu {:.0} ms", t2.elapsed().as_secs_f64() * 1000.0);
    pm.save_png(&out).expect("png");
}
