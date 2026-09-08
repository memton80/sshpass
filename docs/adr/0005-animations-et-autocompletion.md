# ADR 0005 — Animations et autocompletion, sans dependance externe

*Statut: accepte.*

## Contexte

Trois besoins d'interface: adoucir le survol, faire respirer les changements de
vue, et proposer des completions sur les champs de la fiche de connexion. La
question etait de savoir si egui suffit ou s'il faut une bibliotheque tierce.

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
* Les durees sont centralisees dans `ui::anim` (`HOVER`, `VIEW`); un seul
  endroit a toucher pour reregler le rythme de toute l'interface.
* Choisir un coffre dans les suggestions declenche le chargement de ses items,
  qui alimentent a leur tour les suggestions du champ Item. La liste manuelle
  qui occupait le bas de la fiche a donc disparu.
* Les animations n'ont pas de cout au repos: egui ne redessine que pendant la
  transition.
