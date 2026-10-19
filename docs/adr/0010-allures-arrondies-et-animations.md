# ADR 0010 — Trois allures: arrondis, rebonds et ondes

*Statut: accepte — complete l'ADR 0001 (angles droits) et l'ADR 0005
(animations).*

## Contexte

La demande: des bords ronds, et une interface « plus vivante, avec des
animations de partout ». Deux choses s'y opposaient:

* l'ADR 0001 avait pose les angles droits comme condition des liseres pixel
  art, et toute l'interface en dependait;
* plusieurs directions etaient possibles, et elles se jugent a l'ecran plutot
  que sur description.

## Decision

**Trois allures, commutables en direct dans Reglages → Allure**, enregistrees
dans `ui.style` du fichier de configuration:

| Allure | Rayons (petit / carte / fenetre) | Panneaux | Arrivees | Onde au clic | Halos | Rythme |
| --- | --- | --- | --- | --- | --- | --- |
| **Vivant** (par defaut) | 8 / 12 / 16 | cartes detachees du fond | rebond (`ease_out_back`) | oui | oui | x 1,6 |
| **Doux** | 5 / 8 / 10 | cartes detachees du fond | freinage simple | non | anneau de focus | x 1,2 |
| **Retro** | 0 / 0 / 0 | colles aux bords | freinage simple | non | non | x 1 |

Les couleurs ne changent pas: la palette dit de quelle couleur sont les choses,
l'allure (`theme::Look`) dit comment elles se posent et bougent. Les sprites
pixel art restent dans les trois allures — ce sont les icones de l'application,
pas un habillage.

### Ou vit l'allure

`theme::apply` range la palette et l'allure dans la memoire d'egui;
`theme::look(ctx)` et `theme::palette(ctx)` les relisent la ou l'on dessine.
L'autre voie — ajouter un parametre a chaque fonction de dessin — aurait
touche des dizaines de signatures, et surtout interdit aux widgets d'implementer
`egui::Widget`: c'est ce trait qui leur permet de passer par `Ui::add_enabled`
et `on_disabled_hover_text`, comme les boutons natifs qu'ils remplacent.

### Les widgets (`ui::widgets`)

Les widgets natifs d'egui changent d'apparence d'une frame a l'autre. Ceux-ci
interpolent tout:

* **Bouton** — trois roles (principal, secondaire, danger); fond, bordure et
  texte glissent au survol, le bouton se resserre d'un point a l'appui (le
  contenu descend d'un pixel en retro), une onde part du point d'appui, un halo
  l'eclaire dans l'allure « Vivant ». Un etat « enclenche » remplace le libelle
  qui changeait (« Coffre » / « Masquer le coffre ») et faisait sauter le
  bouton de largeur.
* **Interrupteur** — remplace les cases a cocher; le bouton glisse et depasse
  sa place avant d'y revenir.
* **Puce** et **selecteur a segments** — methode d'authentification de la
  fiche, choix de l'allure; l'indicateur du selecteur glisse d'une option a
  l'autre.
* **Champ** — halo de focus. Il doit passer *sous* le champ, qu'egui dessine
  lui-meme: on reserve sa place dans la liste des formes (`Shape::Noop`) avant
  le champ, et on la remplit apres, une fois le focus connu.
* **Pastille, chargeur, jauge** — disque au lieu du sprite, points qui sautent
  au lieu du chenillard, piste arrondie; onde radar autour d'une pastille
  pendant une attente reelle (connexion, agent qui demarre, detection).

### L'onde au clic

Un disque qui grandit depuis le point d'appui deborderait des coins arrondis,
et egui ne sait decouper qu'en rectangle. L'onde est donc un **rectangle
arrondi** qui grandit depuis le point d'appui jusqu'a couvrir la surface: du
meme rayon et contenu dans elle, il ne peut pas deborder de ses coins.

### Ce qui glisse d'un endroit a l'autre

Les animateurs d'egui interpolent lineairement, et repartent de zero a chaque
changement de cible. `anim::glide` garde (depart, arrivee, instant) et suit une
courbe au choix: un indicateur peut ainsi depasser sa cible et y revenir, et
un demi-tour repart de la position courante sans saut. Il sert au
soulignement des onglets — un seul, qui glisse de l'onglet quitte a l'onglet
ouvert —, a l'indicateur du selecteur et au bouton de l'interrupteur.

### Le reste

* **Fenetres** — en plus du fondu: montee en echelle de 90 % avec rebond et
  montee de 16 points, et un voile qui assombrit l'ecran. Le voile est peint
  *dans le calque de la fenetre*, avant elle: c'est la seule facon sure de le
  placer sous elle et au-dessus des panneaux, l'ordre des calques qui ne sont
  pas des zones egui n'etant pas garanti. Il est plus grand que l'ecran, parce
  que la montee en echelle reduit tout le calque le temps de l'ouverture.
  **Il ne bloque pas les clics**: comme avant, le panneau Proton Pass reste
  utilisable pendant qu'une fiche est ouverte.
* **Onglets** — un nouvel onglet s'ouvre en s'elargissant, ses voisins
  s'ecartent; la croix de fermeture pousse un disque rouge.
* **Barre laterale** — les lignes arrivent l'une apres l'autre au demarrage et
  a chaque recherche; le lisere d'onglet ouvert grandit depuis le milieu.
* **Accueil** — le logo tombe a sa place et se souleve au survol, le titre et
  le sous-titre montent, les cartes se soulevent avec leur ombre.
* **Notifications** — entree avec rebond, jauge de vie qui se vide.

## Ce qui n'a pas change

La regle de l'ADR 0005 tient: **aucune animation perpetuelle sans attente
reelle, et toutes bornees dans le temps.** « Des animations de partout » se lit
comme « chaque changement d'etat est anime », pas « l'interface bouge au
repos »: une interface qui respire en permanence redemande une frame toutes
les seize millisecondes, pour rien. Les ondes, rebonds et cascades sont des
reponses a une action; l'onde radar d'une pastille ne tourne que pendant une
connexion, un demarrage d'agent ou une detection.

Le terminal ne recoit toujours que le fondu: il est pose dans une carte de
taille fixe, et rien ne fait varier son nombre de colonnes pendant une
transition.

## Consequences

* Aucune dependance ajoutee.
* `pixel::frame` et le chenillard ne servent plus qu'en retro; `ui::widgets`
  choisit entre le trait arrondi et le lisere pixel (`widgets::outline`).
* Les reglages ont gagne une section et ne tenaient plus sur un ecran de 720
  points: leur corps defile, comme celui de la fiche de connexion.
* Les courbes nouvelles (`ease_out_back`, `Glide`) sont pures et testees; les
  widgets le sont sur un vrai `Context` (bascule de l'interrupteur au clic,
  glissement de l'indicateur, elargissement d'un onglet qui s'ouvre).
