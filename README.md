# Tracery

Générateur procédural de remplages gothiques construits à la règle et au
compas. Chaque trait est un segment ou un arc de cercle à centre et rayon
explicites ; chaque variante est entièrement déterminée par une seed.

État : moteur et ligne de commande (jalons M0 à M3 et une partie de M4–M5).
Voir [docs/PROGRESS.md](docs/PROGRESS.md) et [docs/DECISIONS.md](docs/DECISIONS.md).

## Construction

```powershell
cargo build --release
```

Le binaire est `target/release/tracery-cli.exe`.

## Exemples

```powershell
# Baie tiers-point de référence → SVG (calques), PNG en mode pierre, DXF
tracery-cli generate --shape S01 --style rayonnant --seed 7QK3-M2XD-9A1P-F --depth 4 --out fenetre.svg --png 2048 --dxf fenetre.dxf

# Porte : l'arc devient une baie décorée à l'extérieur (accolade, pinacles…)
tracery-cli generate --shape tests/shapes/porte_tiers_point.json --style flamboyant --porte --seed 3 --out porte.png --png 2048

# Ferronnerie : porte de bois à un ou deux vantaux (selon les proportions)
tracery-cli generate --shape tests/shapes/porte_double_rect.json --style flamboyant --ferronnerie --seed 1 --out vantaux.png --png 2048

# Fenêtre à meneaux : croisée double, angles hauts arrondis, vitrerie
tracery-cli generate --shape tests/shapes/porte_simple_rect.json --meneaux --croisee double --arrondi oui --seed 4 --out croisee.png --png 2048

# Planche de contact de 64 variantes, triées par score
tracery-cli gallery --shape S10 --style flamboyant --seeds 0..63 --sort-score --out planche.png

# Une vignette par forme de référence
tracery-cli gallery --shape all --seeds 3 --out formes.png

# Vérification des invariants (EQ-01 à EQ-06)
tracery-cli validate --shapes all --styles all --seeds 0..999 --determinism --report rapport.json

# Banc d'essai
tracery-cli bench --shapes S01 S04 S10 --seeds 0..19

# Formes de référence S01–S16 en JSON, planche des motifs (M2)
tracery-cli shapes --out tests/shapes
tracery-cli motifs --out motifs.png
```

Une forme peut être :
- un identifiant de référence `S01` … `S16` ;
- un fichier JSON de région (format de l'annexe B : `{"outer": [{"line": …}, {"arc": …}], "holes": []}`) ;
- un fichier JSON de primitive, par exemple `{"primitive": "arch", "kind": "lancette", "rho": 1.2, "span": 1000, "jambs": 1500}` ;
- un fichier projet `.tracery` (forme et génome ; les anciens `.remplage` restent lisibles).

Autres options utiles :
- `--genome RMP1.…` rejoue exactement une variante ;
- `--mode trait|pierre|epure|vitrail|epure-sombre` choisit le rendu PNG ;
- `--json` exporte les données de construction (centres, rayons, provenance, arbre de dérivation) ;
- `--project` écrit un fichier projet.

### Performances

Mesure de chaque étape (génération, traits vus d'une porte, rendus) :

```powershell
cargo run --release -p cli --example perf -- tests/shapes/porte_tiers_point.json porte
cargo run --release -p cli --example perf -- S01 fenetre
```

Dans l'application, `TRACERY_PERF` journalise le temps de chaque image
(moyenne et pire, par demi-seconde) et la carte graphique utilisée. La
valeur `1` écrit sur la console ; toute autre valeur est le chemin d'un
fichier journal, car la version optimisée n'a pas de console. Par défaut,
wgpu choisit la carte graphique la plus puissante ; `WGPU_POWER_PREF=low`
impose la carte intégrée.

## Organisation

```
crates/geom     types, constructions, Apollonius, offsets/booléens, SDF
crates/motifs   arcs gothiques, polylobes, mouchettes, soufflets, redentage
crates/engine   analyse, grammaire, règles R01–R21, score, seeds, génome, styles
crates/io       SVG, DXF, PNG (tiny-skia), fichier projet
crates/cli      tracery-cli
assets/styles   styles (RON)
tests/shapes    formes de référence (JSON)
tests/golden    empreintes des seeds dorées
docs            DECISIONS.md, PROGRESS.md, planches
```
