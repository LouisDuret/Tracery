use engine::{CancelToken, Genome, StyleParams, WindowKind, generate};
use geom::Region;
use geom::math::*;
use motifs::{ArchKind, arch_window};

fn main() {
    let c = CancelToken::new();
    for rho in [1.0, 1.15] {
        let k = ArchKind::Pointed { rho };
        let s = 1000.0;
        let ha = motifs::arch_curve(k, s).height;
        for jamb in [800.0, 1300.0] {
            let (l, _) = arch_window(k, s, jamb);
            let r = Region::simple(l);
            let bb0 = r.bbox();
            let (mut up, mut side, mut down) = (0f64, 0f64, 0f64);
            let mut t = 0.0;
            for seed in 0..80 {
                let st = StyleParams {
                    door: true,
                    ..Default::default()
                };
                let g = Genome::new(seed, st);
                let t0 = std::time::Instant::now();
                let d = generate(&r, &g, &c).unwrap();
                t += t0.elapsed().as_secs_f64();
                let bb = d.bbox();
                up = up.max((bb.max.y - bb0.max.y) / s);
                side = side.max((bb.max.x - bb0.max.x) / s);
                down = down.max((bb0.min.y - bb.min.y) / s);
            }
            println!(
                "porte rho {rho} jamb {jamb} ha {ha:.0}: haut +{up:.2}s, côté +{side:.2}s, bas -{down:.2}s, {:.1} ms",
                t * 1000.0 / 80.0
            );
        }
    }
    for (w, h) in [
        (1000.0, 1700.0),
        (600.0, 1100.0),
        (900.0, 1000.0),
        (500.0, 900.0),
    ] {
        let r = Region::simple(motifs::shapes::rectangle(P2::ZERO, w, h));
        let bb0 = r.bbox();
        let (mut up, mut side, mut down) = (0f64, 0f64, 0f64);
        for seed in 0..80 {
            let st = StyleParams {
                window: WindowKind::Meneaux,
                ..Default::default()
            };
            let g = Genome::new(seed, st);
            let d = generate(&r, &g, &c).unwrap();
            let bb = d.bbox();
            up = up.max((bb.max.y - bb0.max.y) / w);
            side = side.max((bb.max.x - bb0.max.x) / w);
            down = down.max((bb0.min.y - bb.min.y) / w);
        }
        println!("croisée {w}x{h}: haut +{up:.2}w, côté +{side:.2}w, bas -{down:.2}w");
    }
}
