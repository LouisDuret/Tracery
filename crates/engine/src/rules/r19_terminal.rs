//! R19 — Terminaux : jour (la cellule entière), plein (pierre) ou jour
//! simple ajouré (cellule légèrement rétrécie).

use super::*;
use crate::genome::Params;
use crate::style::TerminalMode;
use geom::clip::offset_region;

pub struct Terminal;

impl Rule for Terminal {
    fn id(&self) -> &'static str {
        "R19"
    }
    fn name(&self) -> &'static str {
        "Terminal"
    }
    fn terminal(&self) -> bool {
        true
    }
    fn accepts(&self, ctx: &Ctx, a: &Analysis) -> f64 {
        // une grande cellule de premier niveau doit être subdivisée
        if ctx.depth <= 1 && !ctx.terminal_only && a.inscribed.r > 6.0 * ctx.w {
            0.1
        } else {
            1.0
        }
    }
    fn sample(&self, ctx: &Ctx, a: &Analysis, rng: &mut ChaCha8Rng) -> Params {
        let mode = match ctx.style.terminal {
            TerminalMode::Jour => 0.0,
            TerminalMode::Plein => 1.0,
            TerminalMode::Auto => {
                if a.inscribed.r < ctx.min_opening_r() {
                    1.0
                } else if a.inscribed.r < 2.5 * ctx.w {
                    // le plein n'est admis que pour les petites cellules
                    weighted_index(rng, &[0.8, 0.05, 0.15]).unwrap_or(0) as f64
                } else {
                    weighted_index(rng, &[0.85, 0.0, 0.15]).unwrap_or(0) as f64
                }
            }
        };
        Params::new().with("mode", mode)
    }
    fn apply(&self, ctx: &Ctx, a: &Analysis, p: &Params) -> Result<Expansion, RuleError> {
        let mode = p.get_usize("mode", 0);
        let openings = match mode {
            1 => vec![],
            2 => offset_region(&a.region, 0.3 * ctx.w),
            _ => {
                if a.inscribed.r < ctx.min_opening_r() {
                    vec![]
                } else {
                    vec![a.region.clone()]
                }
            }
        };
        Ok(Expansion {
            children: vec![],
            openings,
            construction: vec![],
            key_ratio: None,
        })
    }
}
