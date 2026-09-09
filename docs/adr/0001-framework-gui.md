# ADR 0001 — Framework GUI: `egui` plutot que `iced`

*Statut: accepte — Decision prise a l'amorce du projet.*

## Contexte

Le cahier des charges retenait `iced` avec `egui` en solution de repli, la
bascule devant se jouer sur « la facilite d'integration du widget terminal
custom ». Les deux repondent a la contrainte principale (100 % Rust, pas de
GTK/Qt, pas de webview), il fallait donc trancher sur le terminal.

## Decision

**`egui` 0.35 via `eframe`, backend `glow` (OpenGL).**

## Justification

Le terminal est la piece la plus contraignante du projet, et c'est elle qui
decide:

1. **Rendu de la grille.** A chaque frame il faut repeindre jusqu'a quelques
   milliers de cellules, chacune avec sa couleur de fond, sa couleur de texte
   et ses attributs. Le mode immediat d'`egui` donne un acces direct au
   `Painter` (`rect_filled`, `text`, `galley`), ce qui permet de regrouper les
   cellules contigues de meme style en segments — c'est exactement ce que fait
   `term/render.rs`. En mode retenu, il aurait fallu maintenir un arbre de
   widgets ou un cache de `canvas::Geometry` invalide a chaque octet recu du
   PTY.
2. **Entrees clavier brutes.** `alacritty_terminal` attend des sequences
   d'echappement, pas des evenements de haut niveau. `egui` expose
   `InputState::events` — la liste brute des `Key`, `Text` et `Paste` — ce dont
   `term/keys.rs` a besoin.
3. **Selection a la souris.** La conversion pixel → cellule et la mise a jour
   continue de la selection demandent le rectangle du widget et la position du
   pointeur a chaque frame; c'est immediat dans `egui`.
4. **Pixel art.** Les sprites sont des rectangles alignes au pixel
   (`ui/pixel.rs`). Le `Painter` d'`egui` suffit, sans passer par une couche de
   dessin vectoriel intermediaire.

Le backend `glow` est prefere a `wgpu`: moins de dependances, binaire plus
leger, et suffisant pour une interface 2D.

## Consequences

* Pas d'arbre de widgets retenu: l'etat vit dans `SshpassApp` et les actions
  de l'interface sont differees dans une file (`app::Action`) appliquee en fin
  de frame. C'est ce qui evite les conflits d'emprunt entre le dessin et la
  mutation de l'etat.
* L'apparence par defaut d'`egui` doit etre entierement retravaillee
  (`theme.rs`): angles droits partout, palette violet/gris sombre, polices
  systeme chargees via `fontdb`.
* `egui` bouge vite entre versions mineures (la 0.35 a deplace les panneaux
  vers `Panel`, ancre dans un `Ui`, et remplace `App::update` par `App::ui`).
  Les versions sont donc epinglees dans `Cargo.toml`.

## Alternatives ecartees

* **`iced`** — le widget terminal aurait demande d'implementer `Widget` avec un
  cache de geometrie et une gestion manuelle des evenements clavier; plus de
  code pour le meme resultat.
* **GTK4 / Qt** — ecartes par le cahier des charges: dependances systeme
  lourdes, packaging AppImage complique.
