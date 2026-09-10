# ADR 0009 — Le terminal possede le clavier et la souris

*Statut: accepte.*

## Contexte

egui est une boite a outils d'interface: elle suppose des boutons, des champs
et un parcours au clavier. Un terminal ne suppose rien de tout cela — il veut
les touches telles quelles. Trois mecanismes d'egui se mettaient en travers, et
le resultat ne se comportait pas comme un terminal.

**Le parcours du focus.** Tant qu'aucun widget n'a le focus, egui garde `Tab`,
les fleches et `Echap` pour promener ce focus d'un bouton a l'autre. Le
terminal se contentait justement des touches dont personne ne voulait
(`memory().focused().is_none()`): la premiere completion au `Tab` envoyait donc
le focus dans la barre laterale, et l'onglet cessait de recevoir quoi que ce
soit jusqu'au clic suivant.

**Le presse-papiers, traduit trop tot.** `egui-winit` convertit `Ctrl+C`,
`Ctrl+X` et `Ctrl+V` en evenements `Copy`, `Cut` et `Paste` **et n'emet pas la
touche** (`is_copy_command`, `lib.rs`). `Ctrl+C` ne pouvait donc pas interrompre
une commande: il copiait, ou ne faisait rien quand il n'y avait pas de
selection.

**Le double envoi d'`Alt`.** winit livre la touche *et* le texte pour `Alt+b`:
le shell reculait d'un mot puis tapait un « b ».

## Decision

### Le terminal prend le focus, il ne l'attend plus

Le widget reclame le focus egui — au clic, et d'office tant qu'aucun champ ne
le demande — puis pose un filtre d'evenements
(`Memory::set_focus_lock_filter`) qui lui reserve `Tab`, les fleches et
`Echap`. Une fenetre modale le lui fait rendre.

### Un partage explicite: `Ctrl` au distant, `Ctrl+Maj` a l'interface

Les evenements de presse-papiers sont relus a la lumiere des modificateurs de
la frame, seul endroit ou retrouver le `Maj` qu'egui a mange:

| Touche | Effet |
| --- | --- |
| `Ctrl+C` | `^C` — interrompt la commande |
| `Ctrl+Maj+C` | copie la selection |
| `Ctrl+X` | `^X` |
| `Ctrl+V` | `^V` — caractere suivant au pied de la lettre |
| `Ctrl+Maj+V` | colle |

C'est le partage de tous les terminaux (xterm, GNOME Terminal, konsole,
alacritty, foot), et celui que le README documentait deja.

Les autres `Ctrl+Maj+…` ne sont plus bloques en bloc: seules les combinaisons
que l'interface utilise vraiment sont retenues, si bien que `Ctrl+Maj+-` (`^_`,
l'annulation de readline) descend a nouveau dans le shell.

### Le texte qui double une sequence `Alt` est ignore

Une combinaison `Alt` encodee arme un drapeau; le texte qui suit
immediatement, s'il y en a un, est jete. Un caractere qui arrive **seul** par
le flux de texte passe toujours: les claviers a troisieme niveau ne perdent
rien.

### La souris va au programme distant quand il la demande

Clics, relachements, deplacements et molette sont encodes (X10, UTF-8 ou SGR
selon ce que le distant a demande), ce qui rend `htop`, `vim`, `tmux` et `less`
pilotables a la souris. `Maj` enfonce rend la selection locale — c'est la
convention de tous les terminaux. `Ctrl` pendant un glisser selectionne en
colonnes.

### Le focus est signale au distant

Prise et perte du clavier — changement d'onglet et changement de fenetre
compris — deviennent `ESC [ I` et `ESC [ O` pour les programmes qui ont demande
ce mode. `vim` recharge alors un fichier modifie sous lui, `tmux` teint le
volet inactif.

## Consequences

* **`Tab` n'atteint plus les boutons** tant qu'un onglet de terminal est
  ouvert. C'est le prix de la completion du shell; l'interface reste
  accessible a la souris et par les `Ctrl+Maj+…`.
* **`Ctrl+V` ne colle plus.** Le collage est `Ctrl+Maj+V`, comme dans un
  terminal. Une limite d'egui subsiste: `egui-winit` n'emet rien du tout quand
  le presse-papiers est vide, donc `Ctrl+V` n'envoie alors pas `^V`.
* **`Maj+Inser` ne colle pas**: egui ne lit le presse-papiers que sur ses
  propres raccourcis, et rien dans son API ne permet de le lire autrement.
* La traduction des evenements est une fonction pure (`render::translate`),
  verifiee par des tests: `Ctrl+C` est precisement le genre de touche qu'on ne
  veut pas voir regresser en silence.

## Alternatives ecartees

**Intercepter avant egui.** Il faudrait doubler la couche `winit` d'`eframe`;
`eframe` n'expose pas les evenements bruts.

**Garder `Ctrl+V` pour coller.** C'est ce que font les navigateurs, pas les
terminaux. Le melange — `Ctrl+C` qui interrompt mais `Ctrl+V` qui colle —
n'est apprenable ni d'un cote ni de l'autre.
