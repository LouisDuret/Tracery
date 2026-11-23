# Décisions d'architecture (ADR)

Une entrée par décision structurante : contexte, options, choix. Les écarts
au cahier des charges sont signalés **ÉCART** ; ceux qui touchent un invariant
du § 2 sont signalés **INVARIANT** et doivent être arbitrés par le commanditaire.

---

## ADR-01 — Workspace et périmètre de la première livraison

- **Contexte** : le § 11 impose l'ordre des jalons et interdit de commencer
  l'interface (M6) avant la validation de M4 en ligne de commande.
- **Choix** : livraison des crates `geom`, `motifs`, `engine`, `io` et `cli`
  (M0 à M3, une grande partie de M4 et M5). Les crates `render` (wgpu) et `app`
  (eframe/egui) ne sont pas créées tant que le moteur n'est pas validé sur
  planches.
- Les crates `geom`, `motifs` et `engine` ne dépendent ni de l'interface ni du
  GPU (INV-5) et portent `#![forbid(unsafe_code)]`.

## ADR-02 — Formulation « espace libre » plutôt que lignes d'axe — ÉCART § 4.9

- **Contexte** : le § 4.9 décrit des cellules théoriques sur les lignes d'axe,
  avec un jour égal à « cellule ∖ ⋃ bandes(arêteᵢ, w_k(i)/2) ». Cela suppose de
  suivre l'ordre de chaque arête à travers les booléens, ce que la
  bibliothèque d'offsets ne permet pas directement.
- **Options** : (a) lignes d'axe avec suivi des arêtes ; (b) espace libre : chaque
  nœud reçoit la région encore vide F, et une règle y « taille » de la pierre
  d'épaisseur w_k (bandes de meneaux ou dilatation des jours placés).
- **Choix** : (b). C'est la formulation déjà utilisée par le § 5.5
  (« région ∖ ⋃ D(cⱼ, rⱼ + w) »). Elle reste exacte au compas : un cercle
  inscrit avec écart w est la solution d'Apollonius sur les bords décalés
  (arcs concentriques). Aux points de contact, la pierre vaut w_parent + w_k au
  lieu de max(w_parent, w_k) : le rendu est un peu plus « plein » qu'un
  tracé sur axes.
- **Conséquence** : les meneaux ne sont pas stockés comme lignes d'axe ; la
  pierre est le complémentaire des jours dans la forme. L'extrusion 3D future
  devra reconstruire l'axe médian de la pierre.

## ADR-03 — Dilatation de découpe 1,1·w

Les résidus d'un nœud sont calculés comme F ∖ dilate(jours, 1,1·w) alors que
les jours sont placés à l'écart exact w (ou 0,6·w du bord). Cela évite les
tangences exactes dans les booléens (cas dégénérés de `cavalier_contours`,
risque du § 12.2) : les résidus sont nettement séparés et la pierre entre un
jour et une cellule voisine vaut 1,1·w.

## ADR-04 — Normalisation d'échelle par puissance de deux

La forme est ramenée à L ≈ 100 par un facteur 2ᵏ (multiplication exacte en
virgule flottante), puis le résultat est remis à l'échelle par 2⁻ᵏ. Les
tolérances absolues de `cavalier_contours` (1e-5 / 1e-4) deviennent ainsi
relatives à L, comme le demande le § 4.2.

## ADR-05 — Déterminisme et `cavalier_contours` — INVARIANT INV-3

- Le moteur respecte les règles du § 4.2 : RNG ChaCha8 par nœud,
  trigonométrie `libm`, aucune `HashMap` sur le chemin de génération, registre
  des règles en ordre fixe, candidats évalués dans un ordre fixe. Le
  déterminisme sur une même plateforme est vérifié (test `deterministic`,
  seeds dorées, `validate --determinism`).
- **Risque** : `cavalier_contours` utilise en interne les fonctions
  trigonométriques de `std` (via `num-traits`), pas `libm`. L'identité bit à
  bit **entre plateformes** (Linux / Windows / macOS / WASM) n'est donc pas
  garantie pour les offsets et les booléens. À arbitrer : (1) accepter le
  risque et vérifier les empreintes dorées sur chaque plateforme en CI,
  (2) dupliquer la crate (vendoring) en remplaçant ses appels par `libm`.
  Recommandation : (1) d'abord ; (2) si la CI multi-plateforme diverge.

## ADR-06 — Accrochage des tangences par Apollonius plutôt que Levenberg–Marquardt

Le maximum de la fonction libre (grille, puis recherche de motif
déterministe) est « accroché » en résolvant Apollonius sur les trois
contraintes les plus actives (bords droits, arcs, sommets, cercles déjà
placés). On obtient des tangences exactes (tests à 1e-9) sans optimiseur
continu. Le raffinement LM borné des paramètres continus (§ 5.8) n'est pas
encore implémenté ; ni `nalgebra` ni `spade` ne sont donc utilisés pour
l'instant.

## ADR-07 — Score local et température — ÉCART § 5.8

- Le score local combine couverture, structure (pénalité des grands jours
  laissés simples), hiérarchie (pondérée par l'aire), nombre d'enfants,
  symétrie, proportions canoniques et un terme de **préférence du style**
  (poids de la règle × poids d'application). Sans ce dernier terme, les
  règles fortement pondérées par le style (oculus en rayonnant) n'étaient pas
  préférées au moment du choix, seulement au tirage.
- La sélection utilise softmax(score / (0,08·T)) : les scores sont dans [0, 1]
  et leurs écarts de l'ordre de 0,1 ; avec T = 0,5 sans facteur, le choix
  était quasi uniforme.

## ADR-08 — Jumelage

Les règles déclarent les jumeaux exacts (lancettes par translation, secteurs
de rose et roues par rotation, moitiés de R17 par symétrie). En plus, la
dérivation détecte les sœurs congruentes par symétrie miroir (axe vertical de
la cellule) ou par rotation (cellule circulaire). Une copie jumelle est la
transformation exacte du sous-arbre de la source (§ 5.3). Un nœud portant une
surcharge (verrou, relance) n'est jamais copié.

## ADR-09 — Catalogue de règles livré

| ID | État |
|----|------|
| R01 lancettes, R02 oculus, R03 tête multi-cercles, R04 polylobe, R05 rose radiale (y compris sur couronne), R06 couronne de cercles, R07/R08 trilobe et quadrilobe inscrits, R09 redentage, R10 mouchette, R11 soufflet / dague, R12 roue de mouchettes, R14 panneaux, R15 cercles maximaux, R16 découpe selon l'axe principal, R17 découpe symétrique, R19 terminaux, R20 frise | implémentées |
| R21 tête trilobée de lancette (figure 3) | ajoutée (non listée au § 5.4) |
| R13 réticulation | **non implémentée** |
| R18 vesica | couverte par R08 (quadrilobe dans la lentille) et R11 (soufflet) |

R03 est une variante de R15 limitée au tympan (2 ou 3 cercles). R16 coupe
perpendiculairement à l'axe principal d'inertie et non à l'axe médian (non
calculé à ce stade, voir ADR-10).

## ADR-10 — Analyse de forme simplifiée — ÉCART § 5.2

- Implémenté : SDF exacte, cercle inscrit maximal exact, descripteurs
  (circularité, convexité, allongement), coins, symétrie miroir d'axe
  vertical validée par distance de Hausdorff, détection de baie et de cercle.
- Non implémenté : axe médian (Delaunay/Voronoï), symétries d'axe quelconque
  et ordre de rotation par autocorrélation, cache par empreinte de région.

## ADR-11 — Rendu CPU et export DXF — ÉCART § 3.8

- Le rendu PNG passe par `tiny-skia` (CPU déterministe, prévu comme repli par
  le § 6.5). Le mode Pierre est calculé par transformée de distance exacte
  (Felzenszwalb) sur le masque de pierre, avec un profil en quart de rond et
  un éclairage de Lambert. Le rendu GPU (wgpu, JFA) relève de M7.
- Le DXF est écrit à la main au format AC1009 (R12 : LINE, ARC, CIRCLE,
  calques), lu par LibreCAD et Inkscape. Le cahier des charges demande R2000
  ou plus : à passer par la crate `dxf` en M8 si un logiciel cible l'exige.

## ADR-12 — Styles en RON embarqués

Les sept styles sont décrits dans `assets/styles/*.ron` et embarqués par
`include_str!`. Modifier un poids ne touche aucune règle, mais demande une
recompilation. Un chargement à l'exécution (répertoire de configuration)
pourra être ajouté avec l'application.

## ADR-13 — Performances (à reprendre)

Sur demande du commanditaire, les performances ne sont pas optimisées à ce
stade. Mesures actuelles (release, monothread) : S01 ≈ 0,3 à 0,5 s, S04
≈ 0,25 s, S10 ≈ 1 s par variante, contre 30 à 80 ms visés au § 8 (EP-01,
EP-02). Pistes, par ordre de gain attendu : cache d'analyse par empreinte,
index AABB pour la SDF, cercle inscrit des enfants calculé une seule fois par
candidat, grille adaptative, parallélisme des candidats et des sous-arbres.

## ADR-14 — Grammaire gothique : rôle des cellules (v0.3.0)

Retour du commanditaire : génération trop chaotique, cercles et polylobes
utilisés pour remplir n'importe quoi. Chaque cellule reçoit désormais un
rôle architectural (`engine::grammar`) : baie, lancette, tympan, oculus,
couronne, secteur, panneau, cellule effilée, écoinçon, grande cellule libre.
Un tableau rôle × règle multiplie le poids du style (0 = interdit) :
- une baie se divise en lancettes sous arcs (R01), ou en panneaux en style
  perpendiculaire ;
- une lancette se subdivise, se tréfle ou se redente, et ne reçoit jamais de
  cercles ;
- un polylobe, une rose ou une roue ne garnissent qu'un oculus ou une
  couronne ;
- un écoinçon reçoit un petit motif ajusté (trilobe, mouchette, redents).

La dérivation trace les grandes formes, puis les petites à l'intérieur.
Les cercles en remplissage (R15) passent de 2 à 0,3–0,8 selon les styles.
Le flamboyant passe en symétrie « préférée » et jumelle ses cellules
congruentes 9 fois sur 10.

## ADR-15 — Double mouchette (R22) : barreaux tangents aux grandes formes

Le motif manquait. Un barreau en S partage la cellule en deux mouchettes
tête-bêche. Il naît tangentiellement des formes englobantes :
- **arrondie**, dans un oculus : deux demi-cercles centrés sur un diamètre,
  tangents intérieurement au cercle (`motifs::flamboyant::s_curve_round`) ;
- **brisée**, dans un tympan ou une lentille : biarc G1 entre le pied du
  meneau (ou une pointe) et la clef. Ses tangentes sont prises entre la
  bissectrice du coin et son flanc, si bien que le S prolonge le meneau, dont
  il garde l'épaisseur (1,35·w).

Un candidat dont un jour ou une cellule a un contour pincé ou croisé est
désormais rejeté au profit d'une autre alternative (EQ-05).

## ADR-16 — Redents de mouchette (R23) ; redentage à grands lobes

Le redentage uniforme (R09) produisait une frange de petits lobes, peu
gothique. Deux changements :
- **R09** est limité à 6 lobes au plus ;
- **R23** (nouvelle règle) place 2 ou 3 grands lobes dans la tête d'une
  cellule effilée. Chaque lobe est tangent au contour : son centre est le
  point où la distance au bord vaut ρ. Les redents sont la pierre comprise
  entre deux lobes voisins et le bord, et ils doivent être trapus : rapport
  saillie / base borné, aire ≥ 0,3 du triangle. La queue de la mouchette
  reste ouverte.

## ADR-17 — Soufflet et mouchettes (R24) ; doubles mouchettes sans contenu

Retours du commanditaire : des redents longs se lisaient comme des branches
« qui ne mènent nulle part », la complexité ne remplissait pas les tympans,
et rien ne doit être généré dans une double mouchette.
- **R22** est terminale : les deux moitiés du S sont des jours, sans contenu.
- **R24** (nouvelle règle) donne la composition flamboyante classique du
  tympan. Au sommet du meneau, deux barreaux miroirs en partent
  tangentiellement (déviation ≤ 0,3 rad), s'écartent puis se rejoignent à la
  clef : un soufflet central, flanqué de deux mouchettes que forment les
  résidus contre l'arc.
- **R23** pose ses lobes à la suite sur la parallèle intérieure au contour, à
  distance ρ, autour du point le plus éloigné de la queue. Les rayons ne sont
  plus tracés depuis le centre de tête : dans une cellule étroite, les lobes
  s'y faisaient face. Les paramètres tirés sont essayés d'abord, puis une
  recherche ordonnée. La saillie d'un redent est bornée (≤ 0,5·ρ, ≤ 0,6·base),
  ce qui donne une pointe courte, pas une épine.
- La variable `TRACERY_TRACE` journalise les candidats rejetés et leur motif.

## ADR-18 — Branches tangentes (R25) ; symétrie systématique

Retour du commanditaire : même à forte complexité, aucune branche ni
mouchette ne naissait entre le soufflet et l'arc principal.
- **R25** (nouvelle règle) : un barreau naît tangentiellement d'un bord de la
  cellule (un barreau existant) et en rejoint un autre tangentiellement.
  - Il est construit en biarc G1, avec les tangentes du contour aux deux
    raccords, et partage la cellule en deux mouchettes dont les queues
    naissent aux raccords.
  - Les couples de raccords sont essayés selon la part de contour qu'ils
    isolent.
  - Il garde une épaisseur proche de celle de son barreau d'origine (1,3·w).
  - Il n'est admis que dans une cellule ample, pour éviter les fouillis.
  - Appliquée récursivement selon la complexité, la règle fait émerger les
    branches et mouchettes du réseau flamboyant.
- **R24** élargit son soufflet : sa largeur doit valoir au moins 1/5 de sa
  hauteur, pour ne pas se réduire à une fente.
- **R23** ne pose de redents que dans une cellule assez grande (pas de
  « vrilles » minuscules).
- **Symétrie** : hors style libre, les cellules congruentes (miroir,
  rotation) reçoivent toujours le même dessin. Une baie gothique est
  symétrique.

## ADR-19 — Arcs brisés francs, têtes trilobées brisées, épaisseurs uniformes (v0.4.0)

Retours du commanditaire : les petites baies avaient des arcs presque en plein
cintre, ce qui n'est pas gothique, et les écarts d'épaisseur entre barreaux
étaient disgracieux.
- **R01** : les lancettes ont un rapport de brisure ρ = R/s d'au moins 0,9.
  En deçà, l'arc se lit comme un plein cintre.
- **R21** : la tête trilobée a désormais un lobe supérieur **brisé**, l'arc de
  la lancette rentré de δ (mêmes centres, rayon R − δ), qui garde la pointe.
  Les lobes d'épaule sont tangents au piédroit et à l'arc aux naissances, et
  les redents naissent à leur intersection avec le lobe supérieur.
  L'ancien lobe supérieur circulaire est abandonné.
- **Épaisseurs** :
  - le cadre garde w₀ ;
  - tous les barreaux du remplage partent de 0,55·w₀ et ne décroissent que
    doucement (λ = 0,95 par défaut, jamais sous 80 % du premier ordre) ;
  - les surépaisseurs ad hoc du S, des branches et du soufflet sont
    supprimées ;
  - le curseur λ va désormais jusqu'à 1 (épaisseur strictement uniforme).

## ADR-20 — Têtes de lancette polylobées par redents (R21)

Retour du commanditaire : les ogives des petites baies étaient trop épaisses
et ne portaient pas de polylobes. Le lobe supérieur brisé de l'ADR-19 laissait
sous l'intrados une bande de pierre δ, qui s'ajoutait au barreau, et ses
redents restaient à peine visibles.

R21 construit désormais l'arc redenté à la règle et au compas, en repère
local (naissances à y = 0, centre de l'arc droit C = (a − R, 0)) :
- lobe de clef sur l'axe, tangent aux deux arcs ;
- lobes latéraux tangents à l'arc droit (centre C + (R − ρ)·dir(θₖ)),
  espacés de g·ρ (θₖ par bissection) ;
- redent : la pierre comprise entre l'arc et deux lobes voisins ;
- report à gauche par symétrie exacte ;
- 3 lobes (trilobé) ou 5 (quintilobé).

Une première version, qui réutilisait les redents génériques de R23,
estimait les points de contact par gradient de la distance au bord. Au lobe
de clef, tangent aux deux arcs à la fois, ce gradient est indéfini : la tête
sortait asymétrique, avec des redents informes. Le jour suit l'arc partout
ailleurs, donc la pointe est conservée et l'ogive garde l'épaisseur des
autres barreaux. Le motif `motifs::cusp::trefoil_lancet` reste disponible
(planche des motifs) mais n'est plus employé par la grammaire.

## ADR-21 — Épure sombre ; panneau de hiérarchie des objets

- **Épure sombre** (`Mode::EpureSombre`, touche 5 ; `--mode epure-sombre` en
  ligne de commande) : même contenu que l'épure, sur fond bleu nuit, avec la
  pierre ardoise, les traits clairs et les tracés de construction cyan et
  ambre. Les deux épures partagent le même code de dessin, paramétré par une
  palette (`io::raster::EpurePalette`). L'export PNG de l'application suit le
  mode affiché.
- **Panneau des objets** (à gauche, touche O ou menu Affichage) :
  - la hiérarchie va des formes du document à l'arbre de dérivation de
    chacune (cadre, lancettes, tympan, oculus…) ;
  - une case cochée affiche l'objet, une case décochée le masque ; un clic sur
    un nom sélectionne la forme ou la cellule ;
  - une forme masquée disparaît du canevas et n'est plus cliquable ;
  - un nœud masqué perd son contenu : sa cellule apparaît en jour simple, et
    ses tracés de construction sont retirés ;
  - le masquage ne concerne que l'affichage. Il n'est pas enregistré dans le
    projet, et les exports restent complets.
- Le mode capture (`TRACERY_SMOKE`) accepte `TRACERY_SMOKE_MODE` (mode
  d'affichage) et `TRACERY_SMOKE_HIDE` (chemin d'un nœud à masquer) pour les
  vérifications automatiques.

## ADR-22 — Raccords tangents sans marche (v0.4.1)

Un barreau qui naît tangentiellement d'une forme englobante (S de la double
mouchette arrondie R22, branche tangente R25) avait son axe tangent au
**bord du jour**. Sa bande (± w/2) se terminait alors par un about droit à
moitié dans le jour : la moitié opposée à la cellule-pointe présentait une
marche d'une demi-épaisseur au point de tangence.

L'axe du barreau est désormais tangent à l'**axe du barreau porteur**,
c'est-à-dire au bord du jour reporté de w/2 vers l'extérieur :

- R22 arrondie : le S est construit dans le cercle R + w/2 ;
- R25 : les points de départ et d'arrivée sont reportés de w/2 le long de la
  normale extérieure, tangentes inchangées.

Le bord de la bande affleure alors le bord de la cellule (raccord G1), et
l'about tombe entièrement dans la pierre. Les tests « dans la cellule »
tolèrent w/2 aux extrémités. Seule l'empreinte dorée S10-flamboyant-5 change.

## ADR-23 — Portes : décor extérieur (v0.5.0)

Une forme en arc (case « Porte » du panneau Forme, `--porte` en ligne de
commande, `StyleParams::door` dans le génome) n'est plus une fenêtre à
remplir : elle devient la **baie d'une porte**, et le générateur
(`engine::door`) décore l'**extérieur** de la forme, surtout sa partie
haute, d'après les portails flamboyants de référence :

- **archivolte** : 2 ou 3 moulures concentriques (dilatations exactes de la
  baie, coupées au seuil) séparées par des gorges ; bases à ressaut et,
  parfois, chapiteaux aux naissances ;
- **accolade** : la moulure extérieure de l'archivolte suit l'arc de tête
  (même centre) jusqu'à l'angle φ, pris près de la clé, puis se retourne en
  contre-courbe tangente. Le centre de la contre-courbe est sur le rayon
  prolongé en φ ; son rayon vaut k fois le rayon minimal qui atteint l'axe
  (k ∈ [1,15 ; 2,6]). La pointe est le point de rencontre avec l'axe. Sans
  arc de tête lisible, un biarc G1 sert de repli ;
- **crochets** en feuille de chou frisé (grappes de lobes circulaires) le
  long de l'extrados de l'accolade, et **fleuron** à la pointe (bouquet ou
  croix fleuronnée, bourgeon terminal) ;
- **pinacles** (fût de 12 à 16 % de la portée), enchâssés dans les
  piédroits : le fût mord sur la moulure d'accolade, de 0,6 à 1,2
  fois la largeur de celle-ci, et la moulure passe derrière lui. Tout ce qu'il recouvre (moulures,
  gorges, chapiteaux, encadrement, crochets) en est retranché, sauf les
  socles sur lesquels il repose ; un crochet caché à plus de moitié
  disparaît : fût lisse ou à gorge
  (pas de niches) ; au pied de la flèche, un gâble ouvert — deux rampants
  droits en saillie, sans base, creusés d'une gorge — abrite la tête d'une petite
  lancette (parfois trilobée) placée aussi haut que possible sans toucher
  la moulure ni sa gorge, et qui descend dans le haut du fût ; flèche
  pyramidale à côtés droits (3,5 à 6 fois la largeur du fût), lisse avec
  une arête médiane ou couverte de tuiles à joints décalés ; crochets en
  chou frisé par paires, à pas régulier sur les arêtes, décroissant vers la
  pointe ; épi de faîtage en croix de feuilles (tige prolongeant la flèche, nœud, deux feuilles,
  bourgeon). Fût, face du gâble et flèche forment une seule silhouette, si
  bien qu'aucune base de triangle ne se dessine. La corniche de
  l'encadrement s'arrête contre les fûts ;
- **encadrement** (65 % des seeds) : dalle, corniche à larmier, bandeau
  interrompu par l'accolade, panneau de **lancettes aveugles** (barres
  verticales, têtes brisées ou trilobées) que l'accolade et le fleuron
  masquent ;
- **dans la baie** : baie pleine, tête trilobée redentée, ou linteau à
  coussinets surmonté d'un tympan. Le tympan reçoit un remplage aveugle
  (génération ordinaire dont les jours deviennent des creux, au moins trois
  jours), un écu entre deux feuilles, ou un trilobe. L'écoinçon entre
  archivolte et accolade reçoit le même traitement.

Il n'y a pas de dalle de mur autour du portail : le décor se détache sur le
fond (demande du commanditaire, v0.5.2).

Modèle : `Design::ornaments` (régions `Relief` à quatre niveaux — 0, réservé
au mur, puis encadrement, moulures, sculpture — et `Creux` — gorges, panneaux
aveugles), `Design::door`. Les jours restent dans la forme (EQ-02). Le
rendu pierre des portes est un ombrage par niveaux (profil en quart de rond
par niveau, creux taillés, baie sombre, hauteurs lissées en 3×3) avec
ombres portées, calculées par
un balayage d'horizon ligne par ligne, en O(n) et de façon déterministe. Les
fenêtres gardent leur rendu. SVG : calques `ornements` (aplats) et `traits`
(ADR-24) ; DXF : calques `RELIEF` et `CREUX`. Le panneau des objets liste les
éléments de la porte (baie, archivolte, accolade, crochets, fleuron,
tympan, écoinçon, encadrement, lancettes, pinacles) ; masquer un
élément retire son décor. Cocher « Porte » recadre la vue sur le décor.

Vérification : 256 portes (16 formes × 2 styles × 8 seeds), invariants
respectés. Les empreintes dorées des fenêtres sont inchangées. Planche :
`docs/planches/v05_portes.png`.

## ADR-24 — Parties cachées dans les tracés des portes

**Contexte.** En mode trait et en épure, chaque ornement de porte était
dessiné par son contour complet : on voyait la flèche à travers les
crochets, la base du gâble, les pinacles à travers la corniche, et les
tracés se croisaient.

**Décision.** `engine::hidden::door_drawing` calcule les parties vues.
- Chaque relief a une profondeur (niveau, rang de pose). Un niveau plus haut
  est devant ; à niveau égal, l'ornement posé après passe devant (crochet
  devant la flèche, nœud de l'épi devant sa tige).
- Un creux prend la profondeur de son hôte, le relief le plus en avant sous
  lui (minimum sur des points pris juste en deçà de son contour), et se place
  juste devant lui.
- Un point de contour est caché s'il tombe strictement (à 2·10⁻⁵ de la
  diagonale près) dans un relief plus en avant. Les contours sont
  échantillonnés au pas de 1/2500 de la diagonale, et chaque frontière
  vu/caché est affinée par dichotomie. Les bords communs restent vus.
- La même fonction donne l'ordre de peinture des aplats (épure, vitrail,
  SVG).

Le trait, l'épure et le SVG (calques `ornements` et `traits`) l'utilisent,
ainsi que le canevas de l'application, qui met en cache le résultat par
remplage visible. Le DXF garde les régions fermées (calques `RELIEF` et
`CREUX`), utiles en CAO. Le rendu pierre n'est pas concerné : son champ de
hauteurs règle déjà les recouvrements.

Le pinacle est construit pour se lire sans croisement :
- fût, face du gâble, flèche et tige de l'épi forment une seule silhouette ;
- le gâble en saillie est un chevron ouvert ;
- la lancette est ajustée par dichotomie pour tenir sous les rampants ;
- les fûts n'ont plus de niches (demande du commanditaire).

## ADR-25 — Performances : parallélisme et carte graphique

**Diagnostic** (porte tiers-point, seeds 1, 3, 7 ; fenêtres S01 et S16 ;
mesuré avec `cargo run --release -p cli --example perf`).

| Étape | Avant | Après |
| --- | --- | --- |
| Traits vus d'une porte | 135–210 ms, 70 000–85 000 points | 23–30 ms, 3 400–4 200 points |
| Rendu pierre d'une porte, 1200 px | 135–145 ms | 63–68 ms |
| Rendu pierre d'une porte, 2400 px | 630–670 ms | 230–255 ms |
| Rendu pierre d'une fenêtre, 1200 px | 28–32 ms | 11–14 ms |
| Trait ou épure d'une porte, 1200 px | 160–240 ms | 35–50 ms |
| Génération d'une fenêtre (S01) | 110–250 ms | 60–110 ms |
| Génération d'une fenêtre (S16) | 160–360 ms | 95–175 ms |

Dans l'application, trois défauts s'ajoutaient à ces coûts :
- les traits vus d'une porte étaient calculés sur le fil de l'interface, à
  chaque nouvel aperçu (environ 200 ms bloquées) ;
- 85 000 points étaient retracés à chaque image ;
- le fond pierre ou vitrail était recalculé par le processeur à chaque
  déplacement ou zoom de la vue.

**Décisions.**
- **Traits vus** : la visibilité est testée au pas de 1/1500 de la
  diagonale, et chaque frontière vu/caché est affinée par dichotomie. Un
  segment vu ne garde que ses extrémités ; un arc est découpé à une
  tolérance de corde de 4·10⁻⁵ de la diagonale. Le test d'occultation
  vérifie l'appartenance avant de calculer la distance au bord. Les
  ornements sont traités en parallèle (rayon).
- **Rendus** : la transformée de distance exacte traite colonnes puis
  lignes en parallèle, et les couvertures sont rastérisées par bandes de
  64 lignes en parallèle. Les boucles par pixel (niveaux, creux, lissage,
  éclairage) sont parallélisées. L'horizon des ombres reste séquentiel
  d'une ligne à l'autre, mais chaque ligne est traitée en parallèle. Les
  images produites sont identiques au pixel près.
- **Génération** : à chaque nœud, les K candidats (application, validité,
  score local) sont évalués en parallèle. Les résultats sont repris dans
  l'ordre du tirage, si bien que les empreintes dorées et le déterminisme
  sont inchangés. La validation complète, déjà parallèle par seed, en
  profite peu ; c'est la latence d'une génération dans l'application qui
  diminue.
- **Application** :
  - les traits vus d'une porte sont calculés par la tâche de génération,
    hors du fil de l'interface ;
  - le fond pierre ou vitrail est une texture qui couvre les remplages
    autour de la vue (la vue élargie de moitié, au plus 4096 px), à
    l'échelle de l'écran. Pendant un déplacement ou un zoom, la carte
    graphique la déplace et la met à l'échelle, sans calcul ;
  - la texture n'est recalculée que si le contenu change, si la vue sort de
    la zone couverte ou si le zoom s'écarte de plus d'un facteur 1,6, et
    seulement 120 ms après l'arrêt de la caméra.
- **Carte graphique** : wgpu demande explicitement la carte la plus
  puissante (`HighPerformance`, réglable par `WGPU_POWER_PREF`). Le
  diagnostic `TRACERY_PERF` journalise la carte retenue (ici
  RTX 3070, Vulkan) et le temps de calcul de chaque image, de 0,4 à 1,4 ms
  pour une porte en épure, en trait ou en pierre.

**Non retenu.** Calculer l'ombrage pierre sur la carte graphique (transformée
de distance par *jump flooding*, ombres en shader) : le rendu parallèle
suffit maintenant que le recalcul n'a plus lieu à chaque mouvement de la vue,
et il garde des exports identiques entre la ligne de commande et
l'application. Les mipmaps d'egui n'existent que pour le moteur glow, pas
pour wgpu : au-delà d'un dézoom de 1,6, la texture est recalculée.

## ADR-26 — Ferronneries de portes de bois

**Contexte.** Le commanditaire veut une case « Ferronnerie » sur un
rectangle ou un arc. La forme devient alors une porte de bois ornée de
ferrures gothiques ou médiévales, à la place d'un remplage. Les modèles sont
des pentures à volutes, des rinceaux et des fleurs de lis relevés sur des
portes anciennes.

**Décision.** `engine::iron::generate_iron`, choisie par
`StyleParams::iron`. Elle produit un `Design` avec `door` et `iron` vrais :
le rendu par niveaux, les traits vus (ADR-24) et l'arbre des objets des
portes sont ainsi réutilisés.
- **Vantaux** : deux vantaux si largeur / hauteur > 0,55, un seul sinon,
  ferré à gauche ou à droite. Les vantaux sont en planches verticales,
  séparées par des joints en creux qui passent sous le fer. Une porte à
  deux vantaux a un battement au milieu. Il n'y a pas de jour : la porte
  est fermée.
- **Pentures** : deux ou trois bandes horizontales, de largeur t = 6 à
  7,5 % de la largeur du vantail, placées selon la demi-hauteur de leur
  motif (20 à 25 % du vantail depuis v0.6.2, pour laisser croître les
  rameaux). Elles partent toutes du même bord des
  gonds, avec un gond à cet endroit et des clous tous les 1,7 t. Chacune se
  termine en fourche à deux volutes ou en fleur de lis.
- **Symétries (v0.6.1)** :
  - toutes les pentures portent le **même motif**, **symétrique par
    rapport à leur axe** horizontal. Il est forgé une fois, pour la moitié
    haute, dans un repère local, puis reporté par translation et miroir sur
    chaque penture. Un élément n'est posé que si toutes ses copies tiennent
    dans le vantail : `Leaf::holds` teste chaque isométrie, sans opération
    booléenne ;
  - les tiges (rinceaux entre pentures, tige à fleur de lis de la tête en
    arc) sont symétriques de part et d'autre, et de haut en bas pour les
    rinceaux ;
  - sur une porte double, le second vantail est le **miroir exact** du
    premier, anneau compris ; la serrure reste sur le premier.
- **Volutes, construites au compas** : un grand arc de 190° à 245°, puis
  deux ou trois quarts de cercle dont le rayon diminue d'un rapport de
  0,58 à 0,66 ; un quart de plus (quatre au plus) tant que le dernier
  rayon dépasse 1,6 fois la largeur du fer. Chaque centre est pris sur la normale commune au point de
  raccord (continuité G1). La largeur est constante, sans ressaut d'un arc
  à l'autre.
- **Départs tangents** : une crosse naît d'une bande, d'une tige ou d'une
  grande volute sans talon perpendiculaire. Son tracé commence dans la
  pièce mère, à une demi-largeur près, de sorte que son bord intérieur est
  tangent au bord de la pièce mère ; il y mord de 0,12 fois sa largeur,
  car un contact exactement tangent fait échouer l'union booléenne, qui
  perdait alors la crosse (fleurons orphelins).
- **Hiérarchie (v0.6.2 : fer plus fin, rameaux récursifs)** :
  - grandes crosses espacées le long des bandes (rayon jusqu'à 12 % du
    vantail et au plus le tiers de la demi-hauteur du motif, largeur
    0,24 t) ; fourche 0,3 t ; tiges 0,36 t ;
  - **rameaux récursifs** : chaque crosse porte, à trois places fixes de
    son grand arc (24 %, 55 % et 86 %), des contre-volutes en S tournées
    vers l'extérieur, de rayon 0,36, 0,5 et 0,34 fois le sien et de
    largeur 0,8 fois la sienne (au moins 0,09 t). Chacune en porte à son
    tour, sur quatre générations au plus ; la récursion s'arrête d'elle-même
    quand la place manque ou que le rayon tombe sous 1,6 fois la largeur.
    Un rameau sans place pour une volute devient une feuille en navette ;
  - petites crosses de remplissage (largeur 0,16 t), qui portent aussi
    leurs rameaux (trois générations) ;
  - fleurons dans l'œil des volutes peu enroulées (trois quarts au plus) :
    rosace à cinq lobes ou feuille ; un bouton sinon ;
  - la tige centrale (rinceaux ou tête en arc) a sa place réservée dans le
    motif des pentures : aucune crosse ne la traverse.
- **Serrurerie** : un anneau sur une platine en rosace, près du bord
  libre, entre les bandes les plus écartées, et une entrée de serrure (écu
  percé).
- **Placement** : chaque élément est un disque d'encombrement. Une crosse
  n'est posée que si son disque tient dans le vantail (marge 0,9 t) sans
  toucher les autres ; sinon son rayon est réduit, jusqu'à 14 fois. Un
  garde-fou final
  coupe à la forme tout ce qui en dépasserait (formes quelconques).
- **Union des fers (v0.6.2)** : deux défauts de `geom::clip::union_loop`
  corrigés. Quand la pièce ajoutée relie plusieurs régions, ses trous déjà
  formés étaient comparés à son propre contour et disparaissaient (volutes
  « bouchées » en disque plein) ; une pièce tombant tout entière dans un
  trou d'une autre était absorbée au lieu de rester une île.
- **Niveaux** : bois aux niveaux 0–1 ; fer (bandes fusionnées par union,
  fleurons, platine) au niveau 2 ; clous, gonds et anneau au niveau 3.
- **Rendu pierre** : chêne grisé avec un fil procédural déterministe, et
  fer sombre avec un reflet spéculaire. En épure, le fer est d'un ton plus
  sombre. Dans le SVG, les aplats bois et fer ont leurs couleurs propres.
- **Invariant** : EQ-02 vérifie aussi, pour une ferronnerie, que chaque
  ornement reste dans la forme.

**Interface.** Une case « Ferronnerie » apparaît sous la case « Porte »
(rectangles et arcs) ; les deux cases s'excluent. Côté ligne de commande,
c'est l'option `--ferronnerie`. Les nœuds de l'arbre des objets sont
Vantaux, Planches, Pentures, Fleurons, Clous, Anneau et Serrure.

**Vérification.** 160 générations (S01–S16 et quatre portes, 8 seeds) :
invariants respectés. Tests unitaires : déterminisme et inclusion ;
vantaux en miroir exact.
Planche : `docs/planches/v06_ferronneries.png` ; v0.6.2 :
`docs/planches/v062_ferronneries.png` (quatre portes, seeds 1, 3 et 5).

## ADR-27 — Réglages persistants, multi-sélection et copier-coller (v0.6.3)

**Contexte.** Le mode d'affichage revenait à « Pierre » à chaque
ouverture ; après réouverture, « Enregistrer » redemandait un emplacement,
car la session restaurée par l'autosave n'avait plus de fichier associé.
L'application ne sélectionnait qu'une forme (plus une seconde, par Maj +
clic, pour les booléens) et ne savait pas copier-coller des formes.

**Décision.**
- **Réglages** : fichier JSON `%APPDATA%/Tracery/reglages.json`
  (`crates/app/src/settings.rs`), dans le dossier de l'autosave. La
  persistance d'eframe n'est pas activée : elle ajouterait une dépendance
  (RON) et un second emplacement de stockage. Champs : `mode` (clé lue par
  `Mode::parse`), `interface` (`systeme`, `clair`, `sombre`) et `projet`
  (chemin du fichier ouvert). Tous sont facultatifs (`serde(default)`) : un
  fichier ancien, partiel ou abîmé ne bloque jamais le démarrage. Le
  fichier est réécrit dès qu'un réglage change. L'auto-test
  (`TRACERY_SMOKE`) ne lit ni n'écrit les réglages, pour rester
  reproductible.
- **Fichier ouvert** : au démarrage, si `projet` désigne un fichier
  existant, il redevient le fichier de la session ; le document de
  référence (marque « • » du titre) est relu dans ce fichier.
  « Nouveau » efface l'association.
- **Sélection** (`crates/app/src/selection.rs`) : une forme
  **principale** (panneau de droite, poignées, cellules) et des formes
  **ajoutées**, dans l'ordre d'ajout. Les booléens portent sur une
  sélection d'exactement deux formes (principale, puis seconde), comme
  avant. Règles :
  - Maj ou Ctrl + clic ajoute ou retire une forme ; si la principale est
    retirée, la suivante la remplace ;
  - un appui sur une forme déjà sélectionnée garde la sélection, et glisser
    déplace tout le groupe (une seule étape d'annulation) ; un clic simple
    sur une forme du groupe la sélectionne seule ;
  - glisser dans le vide trace un rectangle de sélection. Il retient les
    formes visibles **entièrement contenues** (boîte englobante), comme
    dans les logiciels de dessin vectoriel. Avec Maj ou Ctrl, elles
    s'ajoutent à la sélection ;
  - en multi-sélection, les poignées sont masquées, pour qu'un appui près
    d'un coin déplace bien le groupe ;
  - suppression, duplication, rotation, symétrie et échelle s'appliquent
    à toute la sélection, autour du centre de sa boîte englobante.
- **Copier-coller** : Ctrl+C et Ctrl+V arrivent dans egui-winit sous la
  forme des événements `Copy` et `Paste` (et non de touches). La copie
  place dans le presse-papiers du système le texte
  `TRACERY-OBJETS 1` suivi du JSON des formes (primitive ou contour libre,
  et génome) : on peut ainsi coller dans une autre fenêtre Tracery.
  `Paste` reconnaît ce texte, ou un génome `RMP1` (Ctrl+Maj+C, inchangé).
  Les copies reçoivent de nouveaux identifiants et le suffixe « (copie) »,
  sans le répéter. Elles gardent leur génome, donc le même remplage. Elles
  sont décalées de 24 points écran vers la droite et le bas, un pas de plus
  à chaque collage du même texte. Le collage passe par `checkpoint` : il
  s'annule par Ctrl+Z. Les copies deviennent la sélection. Le menu Édition
  → Coller reprend le dernier texte copié dans l'application.

**Vérification.** Tests unitaires : sélection (ajout, retrait, promotion,
validation), copier-coller (aller-retour du texte, décalage, génome et
nature conservés, suffixe), déplacement groupé et rectangle de sélection,
réglages (aller-retour, fichier absent ou abîmé, champs inconnus).

## ADR-28 — Nouveau nom : Tracery (v0.6.4)

**Contexte.** Le commanditaire renomme le projet « Tracery » (« remplage »
en anglais) et le publie sur GitHub.

**Décision.** Seul le **nom du produit** change. Le nom commun « remplage »,
qui désigne l'ouvrage généré, reste dans le code, l'interface et la
documentation, qui sont en français.
- Binaires : `tracery` (application) et `tracery-cli` ; structure
  `TraceryApp` ; titres de fenêtre « Tracery ».
- Fichier projet : extension `.tracery`, champ `format` à
  `tracery-project`. Le format est identique et le champ n'est pas
  vérifié à la lecture : les anciens `.remplage` s'ouvrent toujours (filtre
  de la boîte d'ouverture, argument `--shape` de la ligne de commande). Le
  test de lecture garde un exemple à l'ancien format.
- Dossier de l'application : `%APPDATA%/Tracery`. Au premier lancement,
  s'il n'existe pas encore, `reglages.json` et `autosave.remplage` (renommé
  `autosave.tracery`) sont **copiés** depuis `%APPDATA%/Remplage`, laissé
  en place. Le fichier projet associé dans les réglages garde son chemin.
- Variables d'environnement des outils de développement : `TRACERY_PERF`,
  `TRACERY_TRACE`, `TRACERY_SMOKE*`, `TRACERY_DEBUG_UI`, `TRACERY_BLESS`.
  Les anciennes `REMPLAGE_*` ne sont plus lues.
- Presse-papiers : en-tête `TRACERY-OBJETS 1`. Les génomes gardent leur
  préfixe `RMP1` : le changer invaliderait les génomes déjà copiés ou
  enregistrés.
- Inchangés : le dossier de travail local, le cahier des charges
  (`Cahier_des_charges_Remplage.docx`) et les noms des crates.

