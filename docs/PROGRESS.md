# Avancement

Compte rendu des jalons (§ 10). Les planches sont dans [planches/](planches/) ;
elles sont produites par `tracery-cli` (voir le README).

## Synthèse

| Jalon | État | Commentaire |
|-------|------|-------------|
| M0 — Fondations | **fait** | Workspace, CLI, écrivain SVG, rendu PNG tiny-skia, planches de contact, CI (fmt, clippy `-D warnings`, tests, validate) |
| M1 — Géométrie | **fait**, partiel sur la couverture | Types, intersections, tangentes, cercle par 3 points, Apollonius (cercles, droites, points), biarcs, adaptateur offsets/booléens, SDF exacte, aires, cercle inscrit exact. Pas de proptest ni de mesure de couverture |
| M2 — Motifs | **fait** | Arcs (plein cintre, tiers-point, lancette, surbaissé, Tudor, accolade), polylobes, couronne, mouchette, soufflets, triangle curviligne, vesica, lunule, ovale à quatre centres, redentage générique, tête trilobée, baie canonique (r = s/4, y₀ = s√5/4 vérifiés) |
| M3 — Grammaire et seeds | **fait** | Analyse et classification, dérivation, seeds hiérarchiques, génome (JSON et RMP1), verrous et relances, jumelage, règles R01, R02, R04, R07, R09, R17, R19 et suivantes |
| M4 — Évaluation | **partiel** | Score global à 10 termes, K candidats + softmax, accrochage des tangences par Apollonius. Manquent : raffinement LM, recherche en faisceau / MCTS, mémoïsation, objectif EP-01 |
| M5 — Formes quelconques | **largement fait** | R03, R05, R06, R08, R10, R11, R12, R14, R15, R16, R20, R21 et styles en RON. Manquent : R13 (réticulation), axe médian, symétries d'axe quelconque |
| M6 — Application | **en cours** | Application egui (`remplage`) : outils, édition, multi-sélection, copier-coller, booléens, génération asynchrone, seeds, galerie, verrous, historique, favoris, annuler/rétablir, projet, exports, réglages persistants. Scénario § 7.3 non encore déroulé à la main |
| M7 — GPU | non commencé | Rendu CPU seulement (modes trait, pierre, épure, vitrail) |
| M8 — Exports | partiel | SVG (arcs vrais, calques), DXF R12, PNG, JSON de construction et projet faits. Manquent : PDF, import SVG, mutation, croisement, favoris |

## Validation (EQ-01 à EQ-06)

`tracery-cli validate --shapes all --styles all --seeds 0..19 --determinism` :
16 formes × 7 styles × 20 seeds = 2 240 générations, chacune générée deux fois.
Résultat : voir la fin de ce fichier.

Seeds dorées : `tests/golden/seeds.json` (test `engine/tests/golden.rs`).

## Version 0.3.0 du générateur (grammaire gothique)

- Rôles architecturaux et tableau règle × rôle (ADR-14).
- Double mouchette arrondie et brisée R22 (ADR-15).
- Redents de mouchette R23, et redentage R09 limité à 6 lobes (ADR-16).
- Validation : 2240 générations (16 formes × 7 styles × 20 seeds, déterminisme
  compris), 0 échec. 44 tests passent, clippy `-D warnings` est propre, et les
  seeds dorées sont re-bénies.
- Planches : `docs/planches/v03_s01_rayonnant.png`,
  `v03_s01_flamboyant.png` et `v03_oculus_double_mouchette.png`.
- Soufflet et mouchettes R24, doubles mouchettes sans contenu, redents
  courts posés le long du contour (ADR-17). Après ce lot, la validation donne
  toujours 2240 générations sans échec.
- Branches tangentes R25, soufflet élargi, symétrie systématique hors style
  libre (ADR-18). La validation donne toujours 2240 générations sans échec ;
  44 tests passent et clippy est propre.
- v0.4.0 : arcs brisés francs (ρ ≥ 0,9), têtes trilobées brisées,
  épaisseurs presque uniformes (ADR-19). La validation donne 2240
  générations sans échec ; 44 tests passent et clippy est propre.
- Têtes de lancette polylobées par redents, sans surépaisseur (ADR-20). La
  validation donne 2240 générations sans échec ; 44 tests passent et clippy
  est propre.
- Application : mode Épure sombre et panneau de hiérarchie des objets
  (afficher / masquer), vérifiés par captures (ADR-21).
- v0.4.1 : raccords tangents sans marche pour le S des doubles mouchettes
  arrondies (R22) et les branches tangentes (R25) (ADR-22).
- v0.5.0 : portes (case « Porte » sur un arc, `--porte`) : archivolte,
  accolade à crochets en chou frisé, fleuron, pinacles, encadrement à
  lancettes aveugles, linteau et tympan, rendu pierre par niveaux avec
  ombres portées (ADR-23, planche `v05_portes.png`).
- v0.5.1 : tracés des portes sans parties cachées (trait, épure, SVG,
  canevas) ; pinacles sans niches, gâble ouvert, lancette ajustée sous le
  gâble, épi prolongeant la flèche, pinacles enchâssés dans les piédroits
  (ADR-23, ADR-24).
- Performances (ADR-25) :
  - traits vus 7 fois plus rapides, avec 20 fois moins de points ;
  - rendu pierre 2 à 3 fois plus rapide, génération 2 fois plus rapide
    (parallélisme) ;
  - fond pierre déplacé et mis à l'échelle par la carte graphique ;
  - traits des portes calculés hors du fil de l'interface ;
  - diagnostic `TRACERY_PERF`.
- v0.5.2 : plus de dalle de mur rectangulaire autour des portes.
- v0.6.0 : ferronneries (case « Ferronnerie », `--ferronnerie`) : porte de bois
  à un ou deux vantaux selon les proportions, pentures à volutes au
  compas, fourches et fleurs de lis, rinceaux, fleurons, clous, anneau et
  serrure ; rendu bois et fer (ADR-26, planche `v06_ferronneries.png`).
- v0.6.1 : ferronneries symétriques :
  - chaque penture symétrique par rapport à son axe, toutes identiques ;
  - vantaux en miroir exact ;
  - volutes plus fines, plus petites et plus nombreuses, de largeur
    constante ;
  - départs tangents aux bandes, aux tiges et aux grandes volutes.
- v0.6.2 : ferronneries plus fines et ramifiées :
  - fer des volutes presque deux fois plus fin (0,24 t au lieu de 0,42 t,
    remplissage 0,16 t), enroulement plus serré quand la place le permet ;
  - rameaux récursifs structurés : trois contre-volutes en S à places fixes
    sur chaque grande volute, sur quatre générations ;
  - motif des pentures plus haut (20 à 25 % du vantail) ;
  - corrections de l'union booléenne (volutes bouchées, volutes perdues
    laissant des fleurons orphelins) et place réservée à la tige centrale.
- v0.6.3 : application (ADR-27), sans changement du générateur :
  - réglages conservés d'une session à l'autre (`reglages.json`, à côté de
    l'autosave) : mode d'affichage, thème de l'interface (nouveau choix
    Affichage → Thème de l'interface) et fichier projet ouvert ;
  - « Enregistrer » écrit dans le fichier ouvert, même après réouverture de
    l'application ; seul « Enregistrer sous… » demande un emplacement ;
  - multi-sélection : Maj ou Ctrl + clic (canevas et panneau des objets),
    rectangle de sélection en glissant dans le vide, Ctrl+A ; la sélection
    se déplace d'un bloc, se supprime, se duplique et se transforme
    (rotations, symétrie, échelle) autour de son centre ;
  - copier-coller des formes : Ctrl+C / Ctrl+V (et menu Édition), copies
    décalées de 24 points écran, un peu plus à chaque collage, annulables
    par Ctrl+Z ; le génome reste copié par Ctrl+Maj+C.
- v0.6.4 : le projet s'appelle désormais **Tracery** (ADR-28) : binaires
  `tracery` (application) et `tracery-cli`, fichiers projet `.tracery` (les
  `.remplage` restent lisibles), dossier `%APPDATA%/Tracery` (réglages et
  session repris de `%APPDATA%/Remplage` au premier lancement), variables
  `TRACERY_*`. Publication sur GitHub (`LouisDuret/Tracery`).
- v0.6.5 : choix de l'ouvrage dans le panneau de droite (ADR-29) : menu
  « Catégorie » (fenêtre, porte), puis menu « Type » (fenêtre : remplage,
  fenêtre à meneaux et croisées, vitrail ; porte : encadrement de porte,
  porte en bois). Il remplace les cases « Porte » et « Ferronnerie ». Les
  générateurs des fenêtres à meneaux et des vitraux restent à écrire : ces
  types sont remplis d'un remplage en attendant.
- v0.7.0 : générateur de fenêtres à meneaux et croisées (ADR-30) :
  - chambranle mouluré (listel et gorge sur l'arête, baguette et gorge
    autour de chaque jour), croisée simple (quatre jours) ou double (six),
    traverse au-dessus du milieu ;
  - angles hauts droits ou arrondis en quart de cercle ;
  - bases au pied des meneaux, vitrerie en losanges ou à petits carreaux,
    appui saillant, larmier à retours et culots ;
  - réglages « Croisée » et « Angles hauts » dans la section Ouvrage ;
    options `--meneaux`, `--croisee`, `--arrondi` de la ligne de commande ;
  - le rendu pierre et les épures montrent les plombs devant le vide des
    jours (planche `v07_croisees.png`).
- v0.7.1 : croisées corrigées (ADR-30) : meneaux et traverse en relief
  positif (croix et baguette en saillie, filet central), vitres et plombs
  au fond de la baie, plombs gris ; arrondis des angles hauts plus
  discrets (0,14 à 0,24 largeur de jour).
- v0.7.2 : « Encadrement de porte » (porte à pinacles) se choisit sur toute
  forme, rectangle compris, et plus seulement sur un arc (ADR-29).
- v0.7.3 : croisées arrondies : l'arête extérieure du chambranle est
  arrondie elle aussi (arc concentrique à la baie) ; au lieu du larmier
  droit, une accolade surmontée d'un épi, comme sur les encadrements de
  porte (ADR-30).
- v0.7.4 : croisées à accolade (ADR-30) : accolade en biarc (arcs de
  cercle) comme sur les portes, crochets en chou frisé, fleuron, pinacles
  latéraux ; favorisée sur les baies étroites, évitée sur les larges.
- v0.7.5 : appui des croisées mouluré (ADR-30) : tablette à boudin aux
  becs arrondis (parfois renflés en bossage), gorge, face en retrait,
  listel et plaque de fond ; joints de pose sous les meneaux d'une baie
  large.
- v0.7.6 : appui des croisées repris d'après le profil Blender du
  commanditaire (ADR-30) : tablette à nez ovale (arcs de tête et de
  flanc) marquée de deux méridiens, rebord, congé creux, face, baguette et
  filet ; lisible en épure sombre (planche `v07_appuis.png`).
- Mis de côté à la demande du commanditaire : le remplissage des formes
  libres (tracé à main levée).

## Planches

| Fichier | Contenu |
|---------|---------|
| `m2_motifs.png` | Figures 1, 2, 4 et la baie canonique de la figure 3, avec centres et cercles de construction |
| `formes_<style>.png` | Une variante par forme de référence S01–S16, pour quatre styles |
| `s01_rayonnant.png` | 16 variantes de la baie S01 triées par score |
| `s01_epure.png` | Mode épure (centres, cercles de construction, axes) |
| `s04_vitrail.png` | Mode vitrail |
| `v05_portes.png` | Portes : quatre seeds en mode pierre et une épure |
| `v06_ferronneries.png` | Ferronneries : vantaux simples et doubles, arcs et rectangles, une épure |
| `v062_ferronneries.png` | Ferronneries v0.6.2 : fer fin, rameaux récursifs (quatre portes, seeds 1, 3, 5) |
| `v07_appuis.png` | Appuis v0.7.6 : baie carrée 1600 × 1500, seeds 0 à 3, en rendu pierre et en épure sombre |
| `v07_croisees.png` | Fenêtres à meneaux : baie 1000 × 1700 (seeds 0 à 7), baie étroite 800 × 1500 (seeds 0 à 3) et baie carrée 1500 × 1500 (seeds 0 à 3) |

## Écarts et points à arbitrer

Détail dans [DECISIONS.md](DECISIONS.md) :
1. **INV-3 (déterminisme multi-plateforme)** : `cavalier_contours` utilise la
   trigonométrie de `std`. L'identité bit à bit entre plateformes est à
   confirmer par la CI (ADR-05).
2. Formulation « espace libre » au lieu des lignes d'axe du § 4.9 (ADR-02).
3. Performances très au-dessus des cibles du § 8 (ADR-13), mises de côté à
   la demande du commanditaire.
4. DXF R12 au lieu de R2000 (ADR-11).
5. Questions ouvertes du § 12.3 non tranchées : les hypothèses H1 à H6 ont
   été retenues telles quelles.

## Résultat de la dernière validation

21 novembre 2026, `validate --shapes all --styles all --seeds 0..19 --determinism` :
**2 240 générations, 0 violation d'invariant, 0 divergence de déterminisme**
(362 s sur la machine de développement, en parallèle). Les 40 tests unitaires
et d'intégration passent, et `cargo clippy -D warnings` ne signale rien.
