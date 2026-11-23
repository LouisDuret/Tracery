//! tracery-cli : génération, planches de contact, validation des
//! invariants et bancs d'essai (EF-90).

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Parser, Subcommand};
use engine::refshapes::{REF_IDS, ref_shape};
use engine::seed::{parse_seed, to_base32};
use engine::style::{CrossKind, Family, StyleParams, SymMode, WindowKind};
use engine::validate::validate;
use engine::{CancelToken, Design, Genome, generate};
use io::project::{Project, ProjectShape, RegionJson};
use io::raster::{Mode, RenderOpts, contact_sheet, render, render_regions};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Parser)]
#[command(
    name = "tracery-cli",
    version,
    about = "Générateur procédural de remplages gothiques"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Génère un remplage (SVG, PNG, DXF, JSON).
    Generate(GenArgs),
    /// Planche de contact de N seeds en PNG.
    Gallery(GalleryArgs),
    /// Vérifie les invariants sur une plage de seeds.
    Validate(ValidateArgs),
    /// Banc d'essai (temps de génération).
    Bench(BenchArgs),
    /// Écrit les formes de référence S01–S16 en JSON (et une planche).
    Shapes {
        #[arg(long, default_value = "tests/shapes")]
        out: PathBuf,
    },
    /// Planche de démonstration des motifs (jalon M2).
    Motifs {
        #[arg(long, default_value = "motifs.png")]
        out: PathBuf,
        #[arg(long, default_value_t = 360)]
        size: u32,
    },
}

#[derive(Args, Clone)]
struct StyleArgs {
    /// Famille de style (rayonnant, geometrique, flamboyant, curvilineaire,
    /// perpendiculaire, plaque, libre).
    #[arg(long, default_value = "rayonnant")]
    style: String,
    /// Complexité (profondeur maximale 1–7).
    #[arg(long)]
    depth: Option<u8>,
    #[arg(long)]
    density: Option<f64>,
    /// Largeur du cadre en fraction de √aire.
    #[arg(long)]
    w0: Option<f64>,
    #[arg(long)]
    lambda: Option<f64>,
    /// Largeur minimale de meneau (unités du document).
    #[arg(long)]
    w_min: Option<f64>,
    #[arg(long)]
    temperature: Option<f64>,
    #[arg(long)]
    candidates: Option<u8>,
    /// strict | preferred | free
    #[arg(long)]
    symmetry: Option<String>,
    /// Lobes autorisés, ex. 3,4,5
    #[arg(long, value_delimiter = ',')]
    foils: Option<Vec<u8>>,
    /// Pas de cadre (remplage à fleur).
    #[arg(long)]
    no_frame: bool,
    /// Porte : la forme devient une baie de porte décorée à l'extérieur
    /// (archivolte, accolade, crochets, fleuron, pinacles, lancettes).
    #[arg(long, alias = "door")]
    porte: bool,
    /// Ferronnerie : porte de bois à un ou deux vantaux (selon les
    /// proportions), ornée de pentures à volutes, fleurons, clous et anneau.
    #[arg(long, aliases = ["ironwork", "ferronneries"])]
    ferronnerie: bool,
    /// Fenêtre à meneaux et croisées (ADR-30) : chambranle mouluré, croix
    /// de pierre, vitrerie, appui et larmier.
    #[arg(long, alias = "mullion")]
    meneaux: bool,
    /// Croisée d'une fenêtre à meneaux : auto, simple ou double.
    #[arg(long)]
    croisee: Option<String>,
    /// Fenêtre à meneaux : angles hauts arrondis (oui / non ; sinon seed).
    #[arg(long)]
    arrondi: Option<String>,
}

impl StyleArgs {
    fn params(&self) -> Result<StyleParams> {
        let fam =
            Family::parse(&self.style).ok_or_else(|| anyhow!("style inconnu : {}", self.style))?;
        let mut p = StyleParams::for_family(fam);
        if let Some(v) = self.depth {
            p.depth = v;
        }
        if let Some(v) = self.density {
            p.density = v;
        }
        if let Some(v) = self.w0 {
            p.w0 = v;
        }
        if let Some(v) = self.lambda {
            p.lambda = v;
        }
        if let Some(v) = self.w_min {
            p.w_min = v;
        }
        if let Some(v) = self.temperature {
            p.temperature = v;
        }
        if let Some(v) = self.candidates {
            p.candidates = v;
        }
        if let Some(f) = &self.foils {
            p.foils = f.clone();
        }
        if let Some(s) = &self.symmetry {
            p.symmetry = Some(match s.as_str() {
                "strict" | "stricte" => SymMode::Strict,
                "preferred" | "preferee" | "préférée" => SymMode::Preferred,
                "free" | "libre" => SymMode::Free,
                _ => bail!("symétrie inconnue : {s}"),
            });
        }
        if self.no_frame {
            p.frame = false;
        }
        if self.porte {
            p.door = true;
        }
        if self.ferronnerie {
            p.iron = true;
        }
        if self.meneaux {
            p.window = WindowKind::Meneaux;
        }
        if let Some(c) = &self.croisee {
            p.cross = match c.as_str() {
                "auto" => CrossKind::Auto,
                "simple" => CrossKind::Simple,
                "double" => CrossKind::Double,
                _ => bail!("croisée inconnue : {c} (auto, simple, double)"),
            };
        }
        if let Some(a) = &self.arrondi {
            p.round_top = match a.as_str() {
                "oui" | "yes" => Some(true),
                "non" | "no" => Some(false),
                "auto" => None,
                _ => bail!("arrondi : oui, non ou auto"),
            };
        }
        Ok(p)
    }
}

#[derive(Args)]
struct GenArgs {
    /// Forme : S01…S16, fichier JSON (région, primitive) ou projet .tracery (.remplage accepté).
    #[arg(long)]
    shape: String,
    #[command(flatten)]
    style: StyleArgs,
    /// Seed (décimal, base32 ou texte).
    #[arg(long, default_value = "0")]
    seed: String,
    /// Génome compact RMP1… (remplace style et seed).
    #[arg(long)]
    genome: Option<String>,
    /// Fichier de sortie (.svg, .png, .dxf, .json).
    #[arg(long)]
    out: PathBuf,
    /// Taille du PNG (plus grand côté) ; écrit aussi <out>.png si --out n'est pas un PNG.
    #[arg(long)]
    png: Option<u32>,
    /// Mode de rendu PNG : trait, pierre, epure, vitrail, epure-sombre.
    #[arg(long, default_value = "pierre")]
    mode: String,
    #[arg(long)]
    transparent: bool,
    /// Écrit aussi un DXF.
    #[arg(long)]
    dxf: Option<PathBuf>,
    /// DXF : contours de découpe seulement.
    #[arg(long)]
    cut_profiles: bool,
    /// Écrit aussi les données de construction (JSON).
    #[arg(long)]
    json: Option<PathBuf>,
    /// Écrit aussi un fichier projet .tracery.
    #[arg(long)]
    project: Option<PathBuf>,
}

#[derive(Args)]
struct GalleryArgs {
    #[arg(long)]
    shape: String,
    #[command(flatten)]
    style: StyleArgs,
    /// Plage de seeds : 0..63 (bornes incluses), liste 1,5,9 ou valeur.
    #[arg(long, default_value = "0..15")]
    seeds: String,
    #[arg(long)]
    out: PathBuf,
    #[arg(long, default_value_t = 8)]
    cols: usize,
    /// Taille d'une vignette.
    #[arg(long, default_value_t = 280)]
    size: u32,
    #[arg(long, default_value = "pierre")]
    mode: String,
    /// Trie les vignettes par score décroissant.
    #[arg(long)]
    sort_score: bool,
}

#[derive(Args)]
struct ValidateArgs {
    /// Formes (S01…, fichiers, motifs *.json, ou « all »).
    #[arg(long, num_args = 1.., default_value = "all")]
    shapes: Vec<String>,
    #[arg(long, default_value = "0..99")]
    seeds: String,
    /// Styles (liste séparée par des virgules, ou « all »).
    #[arg(long, default_value = "rayonnant")]
    styles: String,
    #[arg(long)]
    report: Option<PathBuf>,
    /// Vérifie aussi le déterminisme (deux exécutions).
    #[arg(long)]
    determinism: bool,
    #[arg(long, default_value_t = 0.0)]
    w_min: f64,
}

#[derive(Args)]
struct BenchArgs {
    #[arg(long, num_args = 1.., default_value = "S01")]
    shapes: Vec<String>,
    #[arg(long, default_value = "0..19")]
    seeds: String,
    #[arg(long, default_value = "rayonnant")]
    style: String,
}

fn parse_seeds(s: &str) -> Result<Vec<u64>> {
    if let Some((a, b)) = s.split_once("..") {
        let b = b.trim_start_matches('=');
        let (a, b): (u64, u64) = (a.trim().parse()?, b.trim().parse()?);
        if b < a {
            bail!("plage vide : {s}");
        }
        return Ok((a..=b).collect());
    }
    Ok(s.split(',').map(parse_seed).collect())
}

fn expand_shapes(list: &[String]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for s in list {
        if s == "all" {
            out.extend(REF_IDS.iter().map(|x| x.to_string()));
        } else if s.contains('*') {
            let p = Path::new(s);
            let dir = p
                .parent()
                .filter(|d| !d.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let pat = p
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            let (pre, suf) = pat.split_once('*').unwrap_or((&pat, ""));
            let mut found: Vec<String> = std::fs::read_dir(dir)
                .with_context(|| format!("{}", dir.display()))?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with(pre) && n.ends_with(suf))
                .map(|n| dir.join(n).to_string_lossy().to_string())
                .collect();
            found.sort();
            out.extend(found);
        } else {
            out.push(s.clone());
        }
    }
    Ok(out)
}

fn fingerprint(d: &Design) -> String {
    let j = serde_json::to_vec(&d.openings).unwrap_or_default();
    blake3::hash(&j).to_hex()[..16].to_string()
}

fn mode(s: &str) -> Result<Mode> {
    Mode::parse(s).ok_or_else(|| anyhow!("mode de rendu inconnu : {s}"))
}

fn cmd_generate(a: GenArgs) -> Result<()> {
    let shape = io::load_shape(&a.shape).map_err(|e| anyhow!(e))?;
    let genome = if let Some(g) = &a.genome {
        Genome::from_compact(g).map_err(|e| anyhow!(e))?
    } else if let Some(g) = shape.genome.clone() {
        g
    } else {
        Genome::new(parse_seed(&a.seed), a.style.params()?)
    };
    let d = generate(&shape.region, &genome, &CancelToken::new())?;
    let ext = a
        .out
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let render_png = |path: &Path, size: u32| -> Result<()> {
        let pm = render(
            &d,
            &RenderOpts {
                size,
                mode: mode(&a.mode)?,
                transparent: a.transparent,
                ..Default::default()
            },
        );
        pm.save_png(path)
            .with_context(|| format!("{}", path.display()))
    };
    match ext.as_str() {
        "svg" => std::fs::write(&a.out, io::svg::design_to_svg(&d, &Default::default()))?,
        "dxf" => std::fs::write(&a.out, io::dxf::design_to_dxf(&d, a.cut_profiles))?,
        "json" => std::fs::write(&a.out, serde_json::to_string_pretty(&d)?)?,
        "png" => render_png(&a.out, a.png.unwrap_or(2048).min(16384))?,
        _ => bail!("extension de sortie non gérée : {}", a.out.display()),
    }
    if let (Some(size), false) = (a.png, ext == "png") {
        render_png(&a.out.with_extension("png"), size.min(16384))?;
    }
    if let Some(p) = &a.dxf {
        std::fs::write(p, io::dxf::design_to_dxf(&d, a.cut_profiles))?;
    }
    if let Some(p) = &a.json {
        std::fs::write(p, serde_json::to_string_pretty(&d)?)?;
    }
    if let Some(p) = &a.project {
        let mut pr = Project::new("mm");
        pr.shapes.push(ProjectShape {
            id: shape.name.clone(),
            region: RegionJson::from_region(&shape.region),
            genome: genome.clone(),
            extra: None,
        });
        std::fs::write(p, serde_json::to_string_pretty(&pr)?)?;
    }
    let v = validate(&d, genome.style.w_min, genome.style.w_min);
    println!("forme      : {}", shape.name);
    println!("seed       : {}", to_base32(genome.seed));
    println!("génome     : {}", genome.to_compact());
    println!(
        "résultat   : {} jours, {} primitives, {} nœuds, {:.1} ms",
        d.openings.len(),
        d.stats.primitives,
        d.stats.nodes,
        d.stats.millis
    );
    let terms: Vec<String> = d
        .score
        .terms
        .iter()
        .map(|(k, v)| format!("{k}={v:.2}"))
        .collect();
    println!("score      : {:.3} ({})", d.score.total, terms.join(" "));
    println!("empreinte  : {}", fingerprint(&d));
    if v.is_empty() {
        println!("invariants : OK");
    } else {
        println!("invariants : {} violation(s)", v.len());
        for x in v.iter().take(10) {
            println!("  {} {}", x.code, x.detail);
        }
    }
    Ok(())
}

fn cmd_gallery(a: GalleryArgs) -> Result<()> {
    let shape_specs = expand_shapes(std::slice::from_ref(&a.shape))?;
    let shapes: Vec<io::LoadedShape> = shape_specs
        .iter()
        .map(|s| io::load_shape(s).map_err(|e| anyhow!(e)))
        .collect::<Result<_>>()?;
    let multi = shapes.len() > 1;
    let params = a.style.params()?;
    let seeds = parse_seeds(&a.seeds)?;
    let m = mode(&a.mode)?;
    let jobs: Vec<(usize, u64)> = (0..shapes.len())
        .flat_map(|i| seeds.iter().map(move |&s| (i, s)))
        .collect();
    let t0 = Instant::now();
    let mut results: Vec<(usize, u64, Design)> = jobs
        .par_iter()
        .map(|&(i, s)| {
            let g = Genome::new(s, params.clone());
            generate(&shapes[i].region, &g, &CancelToken::new()).map(|d| (i, s, d))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let gen_ms = t0.elapsed().as_secs_f64() * 1000.0;
    if a.sort_score {
        results.sort_by(|x, y| y.2.score.total.total_cmp(&x.2.score.total));
    }
    let items: Vec<_> = results
        .par_iter()
        .map(|(i, s, d)| {
            let pm = render(
                d,
                &RenderOpts {
                    size: a.size,
                    mode: m,
                    ..Default::default()
                },
            );
            let bad = validate(d, 0.0, 0.0).len();
            let mut l2 = format!("S={:.3}", d.score.total);
            if bad > 0 {
                l2.push_str(&format!(" #{bad}"));
            }
            let l1 = if multi {
                format!("{} #{s}", shapes[*i].name)
            } else {
                format!("#{s}")
            };
            (pm, vec![l1, l2])
        })
        .collect();
    let sheet = contact_sheet(&items, a.cols);
    sheet
        .save_png(&a.out)
        .with_context(|| format!("{}", a.out.display()))?;
    let mean = results.iter().map(|r| r.2.score.total).sum::<f64>() / results.len().max(1) as f64;
    println!(
        "{} variantes ({}) en {:.0} ms ; score moyen {:.3} → {}",
        results.len(),
        params.family.key(),
        gen_ms,
        mean,
        a.out.display()
    );
    Ok(())
}

#[derive(serde::Serialize)]
struct ReportEntry {
    shape: String,
    style: String,
    seed: u64,
    violations: Vec<engine::validate::Violation>,
    millis: f64,
    score: f64,
    fingerprint: String,
}

fn cmd_validate(a: ValidateArgs) -> Result<()> {
    let shapes = expand_shapes(&a.shapes)?;
    let seeds = parse_seeds(&a.seeds)?;
    let fams: Vec<Family> = if a.styles == "all" {
        Family::ALL.to_vec()
    } else {
        a.styles
            .split(',')
            .map(|s| Family::parse(s).ok_or_else(|| anyhow!("style inconnu : {s}")))
            .collect::<Result<_>>()?
    };
    let t0 = Instant::now();
    let mut all: Vec<ReportEntry> = Vec::new();
    let mut total_bad = 0usize;
    for sh in &shapes {
        let shape = io::load_shape(sh).map_err(|e| anyhow!(e))?;
        for fam in &fams {
            let mut params = StyleParams::for_family(*fam);
            params.w_min = a.w_min;
            let entries: Vec<ReportEntry> = seeds
                .par_iter()
                .map(|&s| {
                    let g = Genome::new(s, params.clone());
                    let res = std::panic::catch_unwind(|| {
                        generate(&shape.region, &g, &CancelToken::new())
                    });
                    match res {
                        Ok(Ok(d)) => {
                            let mut v = validate(&d, a.w_min, 0.0);
                            let fp = fingerprint(&d);
                            if a.determinism
                                && let Ok(d2) = generate(&shape.region, &g, &CancelToken::new())
                                && fingerprint(&d2) != fp
                            {
                                v.push(engine::validate::Violation {
                                    code: "EQ-06".into(),
                                    detail: "empreintes différentes".into(),
                                });
                            }
                            ReportEntry {
                                shape: shape.name.clone(),
                                style: fam.key().into(),
                                seed: s,
                                violations: v,
                                millis: d.stats.millis,
                                score: d.score.total,
                                fingerprint: fp,
                            }
                        }
                        Ok(Err(e)) => ReportEntry {
                            shape: shape.name.clone(),
                            style: fam.key().into(),
                            seed: s,
                            violations: vec![engine::validate::Violation {
                                code: "GEN".into(),
                                detail: e.to_string(),
                            }],
                            millis: 0.0,
                            score: 0.0,
                            fingerprint: String::new(),
                        },
                        Err(_) => ReportEntry {
                            shape: shape.name.clone(),
                            style: fam.key().into(),
                            seed: s,
                            violations: vec![engine::validate::Violation {
                                code: "PANIC".into(),
                                detail: "panique pendant la génération".into(),
                            }],
                            millis: 0.0,
                            score: 0.0,
                            fingerprint: String::new(),
                        },
                    }
                })
                .collect();
            let bad: Vec<&ReportEntry> = entries
                .iter()
                .filter(|e| !e.violations.is_empty())
                .collect();
            let mut ms: Vec<f64> = entries.iter().map(|e| e.millis).collect();
            ms.sort_by(|x, y| x.total_cmp(y));
            let med = ms.get(ms.len() / 2).copied().unwrap_or(0.0);
            let mean_score =
                entries.iter().map(|e| e.score).sum::<f64>() / entries.len().max(1) as f64;
            println!(
                "{:<5} {:<16} {:>5} seeds  {:>4} en échec  médiane {:>7.1} ms  score {:.3}",
                shape.name,
                fam.key(),
                entries.len(),
                bad.len(),
                med,
                mean_score
            );
            for b in bad.iter().take(3) {
                println!(
                    "      seed {} : {} {}",
                    b.seed, b.violations[0].code, b.violations[0].detail
                );
            }
            total_bad += bad.len();
            all.extend(entries);
        }
    }
    println!(
        "total : {} générations, {} en échec, {:.1} s",
        all.len(),
        total_bad,
        t0.elapsed().as_secs_f64()
    );
    if let Some(r) = &a.report {
        std::fs::write(r, serde_json::to_string_pretty(&all)?)?;
    }
    if total_bad > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn cmd_bench(a: BenchArgs) -> Result<()> {
    let shapes = expand_shapes(&a.shapes)?;
    let seeds = parse_seeds(&a.seeds)?;
    let fam = Family::parse(&a.style).ok_or_else(|| anyhow!("style inconnu"))?;
    let params = StyleParams::for_family(fam);
    for sh in &shapes {
        let shape = io::load_shape(sh).map_err(|e| anyhow!(e))?;
        let mut ms = Vec::new();
        for &s in &seeds {
            let g = Genome::new(s, params.clone());
            let t = Instant::now();
            let d = generate(&shape.region, &g, &CancelToken::new())?;
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
            let _ = d;
        }
        ms.sort_by(|x, y| x.total_cmp(y));
        let t = Instant::now();
        let _: Vec<_> = seeds
            .par_iter()
            .map(|&s| {
                generate(
                    &shape.region,
                    &Genome::new(s, params.clone()),
                    &CancelToken::new(),
                )
            })
            .collect();
        let par = t.elapsed().as_secs_f64() * 1000.0;
        println!(
            "{:<5} monothread : médiane {:.1} ms, p90 {:.1} ms, max {:.1} ms ; {} variantes en parallèle : {:.0} ms",
            shape.name,
            ms[ms.len() / 2],
            ms[(ms.len() * 9) / 10],
            ms[ms.len() - 1],
            seeds.len(),
            par
        );
    }
    Ok(())
}

fn cmd_shapes(out: &Path) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let mut items = Vec::new();
    for id in REF_IDS {
        let s = ref_shape(id).ok_or_else(|| anyhow!("{id}"))?;
        let j = RegionJson::from_region(&s.region);
        std::fs::write(
            out.join(format!("{id}.json")),
            serde_json::to_string_pretty(&j)?,
        )?;
        let pm = render_regions(std::slice::from_ref(&s.region), &[], &s.region.bbox(), 220);
        items.push((pm, vec![id.to_string()]));
    }
    contact_sheet(&items, 8).save_png(out.join("formes.png"))?;
    println!("{} formes écrites dans {}", REF_IDS.len(), out.display());
    Ok(())
}

fn cmd_motifs(out: &Path, size: u32) -> Result<()> {
    use geom::math::*;
    use geom::{ConstructionEntity, ConstructionOp, Region};
    use motifs::foils::Foil;
    use motifs::*;
    let mut items = Vec::new();
    let mut push = |regs: Vec<Region>, cons: Vec<ConstructionEntity>, label: &str| {
        let mut bb = geom::BBox::EMPTY;
        for r in &regs {
            bb = bb.union(&r.bbox());
        }
        for c in &cons {
            if let ConstructionEntity::Circle { c, r, .. } = c {
                bb = bb.union(&geom::BBox::from_pts(
                    *c - P2::splat(*r),
                    *c + P2::splat(*r),
                ));
            }
        }
        items.push((
            render_regions(&regs, &cons, &bb, size),
            vec![label.to_string()],
        ));
    };
    // Figure 1 : familles d'arcs (portée s = 2)
    for (k, name) in [
        (ArchKind::Round, "PLEIN CINTRE"),
        (ArchKind::equilateral(), "TIERS-POINT"),
        (ArchKind::Pointed { rho: 1.2 }, "LANCETTE"),
        (ArchKind::Pointed { rho: 0.7 }, "SURBAISSE"),
        (ArchKind::tudor(), "TUDOR"),
        (ArchKind::ogee(), "ACCOLADE"),
    ] {
        let (l, c) = arch_window(k, 2.0, 0.6);
        push(vec![Region::simple(l)], c.construction, name);
    }
    // Figure 2 : polylobes n = 3…6 (k = 1,15), effet de k
    for n in 3..=6 {
        let f = Foil {
            c: P2::ZERO,
            ri: 1.0,
            n,
            k: 1.15,
            phase: FRAC_PI_2,
        };
        let mut cons = f.construction();
        cons.push(ConstructionEntity::Circle {
            c: P2::ZERO,
            r: 1.0,
            op: ConstructionOp::FoilLobe,
        });
        push(
            vec![Region::simple(f.to_loop())],
            cons,
            &format!("POLYLOBE N={n}"),
        );
    }
    for k in [1.0, 1.4] {
        let f = Foil {
            c: P2::ZERO,
            ri: 1.0,
            n: 4,
            k,
            phase: FRAC_PI_2 + PI / 4.0,
        };
        push(
            vec![Region::simple(f.to_loop())],
            f.construction(),
            &format!("K={k}"),
        );
    }
    // Figure 4 : mouchette, soufflets
    if let Some((l, c)) = flamboyant::mouchette(P2::ZERO, 3.0, P2::new(1.0, 0.8), 2.4, 0.95) {
        push(vec![Region::simple(l)], c, "MOUCHETTE");
    }
    if let Some((l, c)) =
        flamboyant::soufflet_heart(P2::new(0.0, -3.0), P2::new(0.0, 1.0), 1.0, 4.0)
    {
        push(vec![Region::simple(l)], c, "SOUFFLET");
    }
    if let Some((l, c)) = flamboyant::soufflet_two_points(0.6, 1.2, 2.0) {
        push(vec![Region::simple(l)], c, "SOUFFLET 2 POINTES");
    }
    // § 4.8 : autres motifs
    push(
        vec![Region::simple(shapes::curved_triangle(P2::ZERO, 2.0, 1.0))],
        vec![],
        "REULEAUX",
    );
    push(
        vec![Region::simple(shapes::vesica(P2::ZERO, 2.0))],
        vec![],
        "VESICA",
    );
    push(
        vec![Region::simple(shapes::oval4(P2::ZERO, 3.0, 2.0, 1.0))],
        vec![],
        "OVALE 4 CENTRES",
    );
    let (circles, r0) = foils::ring_of_circles(P2::ZERO, 1.0, 6, 0.06, FRAC_PI_2);
    let mut regs: Vec<Region> = circles.iter().map(|c| Region::circle(c.c, c.r)).collect();
    regs.push(Region::circle(P2::ZERO, r0));
    push(
        regs,
        vec![ConstructionEntity::Circle {
            c: P2::ZERO,
            r: 1.0,
            op: ConstructionOp::Rose,
        }],
        "COURONNE",
    );
    // Redentage générique et tête trilobée
    let sq = Region::simple(shapes::rectangle(P2::ZERO, 2.0, 2.0));
    if let Some(c) = cusp::cusp_region(&sq, 0.28, 1.15, Some(0.0)) {
        push(c.regions.clone(), cusp::cusp_construction(&c), "REDENTAGE");
    }
    if let Some((r, c)) = cusp::trefoil_lancet(1.0, 1.0, -1.2, 0.25, 0.1) {
        push(r, c, "TETE TRILOBEE");
    }
    // Figure 3 : baie canonique (r = s/4, y0 = s·√5/4)
    let s = 2.0;
    let (r, y0) = two_light_oculus(s, s, s / 2.0);
    let (win, cw) = arch_window(ArchKind::equilateral(), s, 3.0);
    let mut cons = cw.construction;
    cons.push(ConstructionEntity::Circle {
        c: P2::new(0.0, y0),
        r,
        op: ConstructionOp::Apollonius,
    });
    cons.push(ConstructionEntity::point(
        P2::new(0.0, y0),
        ConstructionOp::Apollonius,
    ));
    for x in [-0.5, 0.5] {
        let (l, c) = arch_window(ArchKind::equilateral(), 1.0, 3.0);
        let l = l.transform(&geom::Xf::translate(P2::new(x, 0.0)));
        cons.extend(
            c.construction
                .iter()
                .map(|e| e.transform(&geom::Xf::translate(P2::new(x, 0.0)))),
        );
        let _ = l;
    }
    push(vec![Region::simple(win)], cons, "BAIE CANONIQUE");
    contact_sheet(&items, 6).save_png(out)?;
    println!("{} motifs → {}", items.len(), out.display());
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Generate(a) => cmd_generate(a),
        Cmd::Gallery(a) => cmd_gallery(a),
        Cmd::Validate(a) => cmd_validate(a),
        Cmd::Bench(a) => cmd_bench(a),
        Cmd::Shapes { out } => cmd_shapes(&out),
        Cmd::Motifs { out, size } => cmd_motifs(&out, size),
    }
}
