# ADR 0005 — Animations et autocompletion, sans dependance externe

*Statut: accepte.*

## Contexte

Trois besoins d'interface: adoucir le survol, faire respirer les changements de
vue, et proposer des completions sur les champs de la fiche de connexion. La
question etait de savoir si egui suffit ou s'il faut une bibliotheque tierce.

Deux besoins s'y sont ajoutes ensuite: **montrer ce qui s'ouvre et ce qui se
ferme** (fenetres, dossiers de la barre laterale), et **montrer ce qui
travaille** — une connexion SSH peut mettre plusieurs secondes a s'etablir, sans
que rien ne bouge a l'ecran.

## Decision

**Tout en egui natif.** Aucune dependance n'a ete ajoutee au `Cargo.toml`.

### Survol

`Context::animate_bool_with_time(id, hovered, 0.12)` renvoie une valeur qui
progresse de 0 a 1 et qui demande elle-meme les rafraichissements tant que
l'animation court. On interpole avec (`ui/anim.rs`):

* la couleur de fond (`Color32::lerp_to_gamma`),
* la couleur des icones et des libelles,
* des grandeurs geometriques — epaisseur du liseré d'onglet ouvert, decalage
  horizontal d'une carte de l'accueil,
* l'opacite (`Color32::gamma_multiply`), qui sert aux fondus croises.

Le cas le plus visible est la ligne de connexion de la barre laterale: le
`user@host` et les actions rapides (modifier, supprimer) occupent la meme
place, et se croisent en fondu au lieu de se remplacer d'un coup.

### Transitions de vue

Deux mecanismes, choisis selon ce qui bouge:

* **Panneau Proton Pass** — `Panel::show_collapsible`, l'animation native
  d'egui: le panneau glisse hors de son bord et la zone centrale suit. Il se
  replie aussi quand on tire la poignee de redimensionnement en deca de sa
  largeur minimale.
* **Zone centrale** — `Context::animate_value_with_time` sur une cle qui
  identifie la vue affichee. Un changement de cle remet l'animation a zero
  (duree nulle) puis la relance vers 1; on applique le resultat avec
  `Ui::multiply_opacity`.

Un point important: **le terminal ne recoit que le fondu, pas le glissement.**
Decaler son rectangle changerait le nombre de colonnes a chaque frame, ce qui
declencherait un `resize` du PTY par frame pendant toute la transition.
L'accueil, lui, glisse de dix pixels vers le haut: rien n'y depend de la
hauteur exacte disponible.

Le fondu croise « ancien et nouveau superposes » a ete ecarte: il faudrait
maintenir deux sessions terminal vivantes et les dessiner toutes les deux, pour
un gain visuel discutable sur du texte.

### Ouverture et fermeture des fenetres

Les quatre fenetres (fiche de connexion, suppression, nouveau dossier,
reglages) passent par `ui::modal::Modal`, qui porte leur habillage commun et
leur animation.

L'ouverture est simple: une `Area` d'egui fait deja son fondu d'entree, on y
ajoute une montee en echelle de 96 % a 100 % avec
`Context::transform_layer_shapes`. Ce transformateur-la ne vaut **que pour la
frame en cours et ne touche pas aux entrees** — contrairement a
`set_transform_layer`, qui est remanent et deplacerait aussi les zones
cliquables.

La fermeture demandait un choix. En mode immediat, une fenetre disparait parce
que l'etat qui la decrivait n'existe plus: `app.editor` repasse a `None`, et son
`Drop` ecrase le mot de passe encore saisi. **Garder la fenetre vivante
quelques frames de plus pour l'animer etait exclu** — cela reviendrait a
retarder l'effacement d'un secret pour une raison decorative.

On garde donc, non pas la fenetre, mais sa **trace**: son rectangle et son
titre. `modal::fade_out_closed`, appele une fois par frame apres toutes les
modales, constate qu'une fenetre suivie n'a pas ete dessinee a cette passe et
efface sa trace en fondu, a sa place, avant de l'oublier. Une fenetre rouverte
pendant son propre fondu reprend simplement sa place.

### Attente et chargement

Ces animations-la ne dependent d'aucun etat: elles tournent tant que quelque
chose travaille. Elles se calculent depuis l'horloge de la frame
(`anim::cycle`, `anim::breathe`) et **reclament elles-memes la frame suivante**,
puisque egui laisse dormir une fenetre inerte.

| Ou | Quoi | Quand |
| --- | --- | --- |
| Bandeau bas de l'onglet | navette qui parcourt le rail violet | connexion en cours |
| Pastille de l'onglet | battement | connexion en cours |
| Ecran d'attente d'un onglet | chenillard pixel + jauge du delai d'abandon | agent Proton Pass qui demarre |
| Barre d'outils, panneau coffre, fiche | chenillard pixel | detection, `pass-cli login`, lecture d'un coffre, ecriture d'un secret |

Le bandeau du bas d'un onglet sert a deux choses, jamais en meme temps:
souligner l'onglet courant, ou montrer que la connexion travaille. Les deux
s'echangent en fondu, pour qu'une connexion etablie ne fasse pas sauter le
bandeau.

Une connexion « en cours » est definie par l'absence de sortie:
`TerminalSession::is_connecting` est vrai tant que le PTY n'a produit aucun
octet (`Event::Wakeup` d'alacritty). **Cette attente est bornee a 45 secondes**,
et celle de l'agent au delai d'abandon deja applique (20 s): passe ce delai
l'animation n'apprend plus rien et ne ferait que reclamer une frame toutes les
seize millisecondes. Meme raison pour la cloche du terminal, qui reste allumee
sans clignoter: elle peut durer des heures.

### Pliage des dossiers

`CollapsingState`, l'etat natif d'egui: c'est lui qui interpole la hauteur du
contenu et coupe ce qui deborde. La verite reste `expanded_folders` dans
l'application; l'etat egui n'anime que le mouvement. Le chevron suit par un
**fondu croise entre deux sprites** (`CHEVRON_RIGHT` et `CHEVRON_DOWN`): faire
tourner un chevron pixel art le rendrait flou a chaque angle intermediaire.

### Notifications, cascade, appui

* **Notifications** — entree et sortie par le bord droit, la sortie prelevee
  sur les dernieres 0,4 s de vie. Chaque notification porte un identifiant
  propre: sans lui, refermer la premiere ferait rejouer l'animation de toutes
  celles qui remontent d'un cran. La colonne a une largeur fixe, sinon le
  glissement elargirait la zone ancree a droite au lieu de faire entrer la
  carte par le bord.
* **Accueil** — les lignes se posent l'une apres l'autre (`anim::stagger`).
  L'opacite y est appliquee couleur par couleur: `Ui::multiply_opacity` vaudrait
  pour tout ce qui suit dans la meme `Ui`, donc pour toutes les lignes
  suivantes, et se cumulerait a chacune.
* **Boutons pixel** — le contenu descend d'un pixel tant que le bouton est
  tenu, sur 0,06 s.

### Autocompletion

Il n'existe pas de crate d'autocompletion mure pour egui: le composant est fait
maison (`ui/autocomplete.rs`), mais uniquement a partir de briques natives —
un `TextEdit` d'identifiant fixe et un `Popup` ancre sous lui
(`RectAlign::BOTTOM_START`).

Le point delicat est le clavier. Un `TextEdit` interprete les fleches comme un
deplacement du curseur et Entree comme une validation: **les touches sont donc
consommees avec `InputState::consume_key` avant que le champ ne soit dessine**,
en s'appuyant sur l'etat d'ouverture de la frame precedente, conserve dans la
memoire d'egui. Fleches pour naviguer, Entree ou Tab pour accepter, Echap pour
fermer la liste sans fermer la fiche.

Les propositions viennent uniquement de ce qui est deja connu — **la frappe ne
declenche aucune requete**:

| Champ | Source |
| --- | --- |
| Hote | hotes des connexions enregistrees, les plus recemment ouvertes en tete, avec leur dossier en precision |
| Utilisateur | utilisateurs deja employes, les plus frequents en tete |
| Coffre | coffres renvoyes par `pass-cli vault list` |
| Item | items du coffre deja charges, avec leur type |

Le filtrage (`autocomplete::filter`) est une fonction pure, testee: recherche
par sous-chaine insensible a la casse, dedoublonnage, exclusion de la valeur
deja saisie a l'identique, plafond a huit propositions.

## Consequences

* Le `Cargo.toml` est inchange: pas de surface de dependance supplementaire.
* Les durees sont centralisees dans `ui::anim` (`HOVER`, `VIEW`, `MODAL`,
  `PULSE`, `SWEEP`); un seul endroit a toucher pour reregler le rythme de toute
  l'interface. `theme::apply` cale `style.animation_time` sur `VIEW`, faute de
  quoi les animations natives d'egui — fondu d'une fenetre, depliage d'un
  dossier — tourneraient a leur valeur par defaut et se decaleraient a l'oeil.
* Choisir un coffre dans les suggestions declenche le chargement de ses items,
  qui alimentent a leur tour les suggestions du champ Item. La liste manuelle
  qui occupait le bas de la fiche a donc disparu.
* Les animations d'etat n'ont pas de cout au repos: egui ne redessine que
  pendant la transition. Les animations perpetuelles, elles, en ont un — elles
  demandent une frame en continu — d'ou la regle: **aucune ne tourne sans une
  attente reelle, et toutes sont bornees dans le temps.**
* Les fonctions de courbe (`ease_out`, `ping_pong`, `phase`, `sweep`, `chase`,
  `stagger`) sont pures et testees. Ce qui depend d'egui est verifie sur un
  vrai `Context` en test: l'animation d'apparition part bien de zero, la trace
  d'une fenetre fermee s'efface puis est oubliee, et la navette d'un onglet en
  connexion avance sans sortir de son rail.
