//! Écrivain SVG maison : vrais arcs (commande A), calques, unités physiques
//! (EF-70). L'axe y du document (vers le haut) est inversé à l'écriture.

use engine::hidden::door_drawing;
use engine::{Design, OrnKind};
use geom::math::*;
use geom::{ConstructionEntity, Loop, Region, Seg};
use std::fmt::Write;

fn f(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s == "-0" { "0".into() } else { s }
}

struct Frame {
    min_x: f64,
    max_y: f64,
}

impl Frame {
    fn p(&self, p: P2) -> (String, String) {
        (f(p.x - self.min_x), f(self.max_y - p.y))
    }
}

fn loop_path(l: &Loop, fr: &Frame, out: &mut String) {
    let Some(first) = l.segs.first() else { return };
    let (x, y) = fr.p(first.start());
    let _ = write!(out, "M{x} {y}");
    for s in &l.segs {
        match *s {
            Seg::Line { b, .. } => {
                let (x, y) = fr.p(b);
                let _ = write!(out, "L{x} {y}");
            }
            Seg::Arc { r, sweep, .. } => {
                // un arc SVG ne peut pas décrire un cercle complet : découpe
                for piece in s.split_arc(PI) {
                    if let Seg::Arc { sweep: sw, .. } = piece {
                        let (x, y) = fr.p(piece.end());
                        let large = if sw.abs() > PI + 1e-12 { 1 } else { 0 };
                        // y inversé : le sens trigonométrique devient horaire
                        let flag = if sw > 0.0 { 0 } else { 1 };
                        let _ = write!(out, "A{} {} 0 {large} {flag} {x} {y}", f(r), f(r));
                    }
                }
                let _ = sweep;
            }
        }
    }
    out.push('Z');
}

fn region_path(r: &Region, fr: &Frame) -> String {
    let mut d = String::new();
    for l in r.loops() {
        loop_path(l, fr, &mut d);
    }
    d
}

/// Options d'export SVG.
pub struct SvgOptions {
    pub construction: bool,
    pub units: &'static str,
    pub margin: f64,
}

impl Default for SvgOptions {
    fn default() -> Self {
        SvgOptions {
            construction: true,
            units: "mm",
            margin: 10.0,
        }
    }
}

/// Export d'un remplage en SVG.
pub fn design_to_svg(d: &Design, opt: &SvgOptions) -> String {
    let bb = d.bbox().grow(opt.margin);
    let fr = Frame {
        min_x: bb.min.x,
        max_y: bb.max.y,
    };
    let (w, h) = (bb.size().x, bb.size().y);
    let mut s = String::new();
    let _ = writeln!(
        s,
        r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" width="{}{u}" height="{}{u}" viewBox="0 0 {} {}">
<title>Remplage {}</title>"##,
        f(w),
        f(h),
        f(w),
        f(h),
        d.version,
        u = opt.units
    );
    // pierre : forme moins jours (règle pair-impair)
    let mut stone = region_path(&d.shape, &fr);
    for o in &d.openings {
        stone.push_str(&region_path(&o.region, &fr));
    }
    let _ = writeln!(
        s,
        r##"<g id="pierre" inkscape:groupmode="layer" inkscape:label="pierre"><path d="{stone}" fill="#e8dfc8" fill-rule="evenodd" stroke="none"/></g>"##
    );
    let _ = writeln!(
        s,
        r##"<g id="jours" inkscape:groupmode="layer" inkscape:label="jours" fill="#2f4660" stroke="#1a1a1a" stroke-width="{}">"##,
        f(0.0015 * d.shape.bbox().diag())
    );
    for o in &d.openings {
        let _ = writeln!(
            s,
            r##"<path d="{}" fill-rule="evenodd" data-path="{}" data-rule="{}"/>"##,
            region_path(&o.region, &fr),
            o.path,
            o.rule
        );
    }
    s.push_str("</g>\n");
    // porte : aplats des ornements du fond vers l'avant, puis traits vus
    // (parties cachées retirées, ADR-24)
    if d.door {
        let dr = door_drawing(d);
        let _ = writeln!(
            s,
            r##"<g id="ornements" inkscape:groupmode="layer" inkscape:label="ornements" stroke="none">"##
        );
        for &i in &dr.order {
            let o = &d.ornaments[i];
            let fill = match (d.iron, o.kind) {
                // ferronnerie : bois des vantaux, fer des pentures
                (true, OrnKind::Relief) if o.level >= 2 => "#3f3833",
                (true, OrnKind::Relief) => "#94765a",
                (true, OrnKind::Creux) => "#6f5640",
                (false, OrnKind::Relief) => {
                    ["#ddd4bc", "#e8dfc8", "#efe7d3", "#f6f0e1"][o.level.min(3) as usize]
                }
                (false, OrnKind::Creux) => "#c9bea3",
            };
            let kind = if o.kind == OrnKind::Relief {
                "relief"
            } else {
                "creux"
            };
            let _ = writeln!(
                s,
                r##"<path d="{}" fill="{fill}" fill-rule="nonzero" data-kind="{kind}" data-level="{}"/>"##,
                region_path(&o.region, &fr),
                o.level
            );
        }
        s.push_str("</g>\n");
        let mut p = String::new();
        for l in &dr.lines {
            for (k, q) in l.iter().enumerate() {
                let (x, y) = fr.p(*q);
                let _ = write!(p, "{}{x} {y}", if k == 0 { "M" } else { "L" });
            }
        }
        p.push_str(&region_path(&d.shape, &fr));
        for o in &d.openings {
            p.push_str(&region_path(&o.region, &fr));
        }
        let _ = writeln!(
            s,
            r##"<g id="traits" inkscape:groupmode="layer" inkscape:label="traits"><path d="{p}" fill="none" stroke="#5a5040" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round"/></g>"##,
            f(0.0008 * d.bbox().diag())
        );
    }
    if opt.construction {
        let sw = f(0.001 * d.shape.bbox().diag());
        let _ = writeln!(
            s,
            r##"<g id="construction" inkscape:groupmode="layer" inkscape:label="construction" fill="none" stroke="#b03a2e" stroke-width="{sw}" stroke-dasharray="{} {}" opacity="0.7">"##,
            f(0.006 * d.shape.bbox().diag()),
            f(0.004 * d.shape.bbox().diag())
        );
        let pr = 0.003 * d.shape.bbox().diag();
        for c in &d.construction {
            match *c {
                ConstructionEntity::Circle { c, r, .. } => {
                    let (x, y) = fr.p(c);
                    let _ = writeln!(s, r##"<circle cx="{x}" cy="{y}" r="{}"/>"##, f(r));
                }
                ConstructionEntity::Point { p, .. } => {
                    let (x, y) = fr.p(p);
                    let _ = writeln!(
                        s,
                        r##"<circle cx="{x}" cy="{y}" r="{}" fill="#b03a2e" stroke="none"/>"##,
                        f(pr)
                    );
                }
                ConstructionEntity::Line { a, b, .. } => {
                    let (x1, y1) = fr.p(a);
                    let (x2, y2) = fr.p(b);
                    let _ = writeln!(s, r##"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}"/>"##);
                }
            }
        }
        s.push_str("</g>\n");
    }
    let _ = writeln!(
        s,
        r##"<g id="contour" inkscape:groupmode="layer" inkscape:label="contour source"><path d="{}" fill="none" stroke="#3a3328" stroke-width="{}"/></g>"##,
        region_path(&d.shape, &fr),
        f(0.002 * d.shape.bbox().diag())
    );
    s.push_str("</svg>\n");
    s
}

/// Export SVG de simples régions (planches de motifs).
pub fn regions_to_svg(regs: &[Region], margin: f64) -> String {
    let mut bb = geom::BBox::EMPTY;
    for r in regs {
        bb = bb.union(&r.bbox());
    }
    let bb = bb.grow(margin);
    let fr = Frame {
        min_x: bb.min.x,
        max_y: bb.max.y,
    };
    let mut s = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} {}">"##,
        f(bb.size().x),
        f(bb.size().y)
    );
    for r in regs {
        let _ = write!(
            s,
            r##"<path d="{}" fill="#2f4660" fill-rule="evenodd"/>"##,
            region_path(r, &fr)
        );
    }
    s.push_str("</svg>\n");
    s
}
