//! Mesures de performance (diagnostic) : génération, traits vus, rendus.
use engine::{CancelToken, Genome, generate};
use io::raster::{Mode, RenderOpts, render};
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let shape_path = args
        .get(1)
        .cloned()
        .unwrap_or("tests/shapes/porte_tiers_point.json".into());
    let door = args.get(2).map(|s| s == "porte").unwrap_or(true);
    let region = io::project::load_shape(&shape_path).expect("forme").region;
    for seed in [1u64, 3, 7] {
        let mut g = Genome::new(
            seed,
            engine::StyleParams::for_family(engine::Family::Flamboyant),
        );
        g.style.door = door;
        let t = Instant::now();
        let d = generate(&region, &g, &CancelToken::new()).unwrap();
        let tg = ms(t);
        let t = Instant::now();
        let dr = engine::hidden::door_drawing(&d);
        let th = ms(t);
        let npts: usize = dr.lines.iter().map(|l| l.len()).sum();
        let mut line = format!(
            "seed {seed}: gen {tg:7.1} ms | traits vus {th:6.1} ms ({} polylignes, {npts} pts, {} ornements)",
            dr.lines.len(),
            d.ornaments.len()
        );
        for (mode, size) in [
            (Mode::Pierre, 1200u32),
            (Mode::Pierre, 2400),
            (Mode::Trait, 1200),
            (Mode::Epure, 1200),
            (Mode::Vitrail, 1200),
        ] {
            let t = Instant::now();
            let _ = render(
                &d,
                &RenderOpts {
                    size,
                    mode,
                    transparent: false,
                    margin: 0.04,
                },
            );
            line += &format!(" | {mode:?}@{size} {:6.1}", ms(t));
        }
        println!("{line}");
    }
}
