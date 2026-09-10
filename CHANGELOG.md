# Journal des versions

## Non publie

### Ce que le serveur ne peut plus vous prendre

Un gestionnaire SSH tient deux choses qu'une machine distante n'a pas a voir :
le presse-papiers du poste, et le mot de passe du coffre. Les deux etaient
atteignables.

**Votre presse-papiers reste chez vous.** Il existe une sequence, OSC 52, qui
sert normalement a copier depuis une session distante — c'est elle qui fait
marcher le `yank` de `vim` a travers `ssh`. Elle a un second sens, moins connu :
la meme sequence permet au serveur de **demander** le contenu du presse-papiers,
et le terminal le lui renvoie. sshpass-gui repondait. Un serveur compromis
recuperait donc ce que vous veniez de copier — un mot de passe, un jeton, une
URL privee — sans que vous ayez colle quoi que ce soit. C'est termine : la
demande est refusee, et une notification vous previent la premiere fois qu'un
serveur essaie. La copie *depuis* le distant, elle, continue de marcher.

**Le mot de passe ne repond plus a n'importe quelle question.** Une connexion en
mode mot de passe negociait aussi `keyboard-interactive`, un mode ou c'est le
**serveur** qui redige les questions. Il lui suffisait d'en poser une pour que le
mot de passe de votre coffre lui soit servi. Ce mode n'est plus negocie
automatiquement, et le pont verifie que la question vient bien de `ssh`.

Les serveurs qui en ont vraiment besoin — PAM, code a usage unique, second
facteur — ont maintenant leur propre mode, « **Interactif (saisie manuelle)** ».
Rien n'y repond a votre place : vous tapez dans le terminal.

**Deux onglets sur la meme machine ne se marchent plus dessus.** Le fichier qui
porte la reference du mot de passe etait nomme d'apres la connexion : ouvrir un
second onglet ecrasait celui du premier, qui pouvait ensuite reclamer le secret
d'un autre serveur. Chaque session a desormais le sien.

**Le mot de passe ne passe plus par la ligne de commande.** Mettre a jour un
mot de passe le rendait brievement lisible dans `/proc`, que tous les comptes
de la machine peuvent consulter. La valeur part maintenant par l'entree
standard, comme a la creation. Si votre version de `pass-cli` ne sait pas faire,
la mise a jour echoue en vous le disant plutot que d'exposer le secret ; le cas
echeant, la case « Mot de passe en ligne de commande » des reglages rend
l'ancien comportement.

### Le terminal se comporte enfin comme un terminal

`Ctrl+C` ne coupait rien. `Tab` ne completait rien : il envoyait le curseur
promener dans les boutons de la barre laterale, apres quoi l'onglet ne recevait
plus une seule touche. Les fleches, `Echap` et `Alt+B` avaient le meme genre de
travers. Rien de tout cela ne venait de l'emulation, qui est celle d'alacritty :
c'est la boite a outils d'interface qui prenait les touches au passage, chacune
pour une bonne raison qui n'a pas cours dans un terminal.

**Le terminal possede maintenant le clavier** au lieu de se contenter des
touches dont aucun bouton ne veut. Il le reclame, et il garde `Tab`, les
fleches et `Echap` — la completion du shell, l'historique, la sortie de `vim`.
Une fenetre modale le lui fait rendre, comme avant.

**`Ctrl+C` interrompt la commande en cours.** La copie, c'est `Ctrl+Maj+C`, et
le collage `Ctrl+Maj+V` : le partage de tous les terminaux, celui que le manuel
decrivait deja. `Ctrl+V` redevient donc le `^V` de readline.

**`Alt+B` ne tape plus de « b ».** La combinaison partait en double, une fois
comme sequence et une fois comme texte.

**La souris va au programme distant quand il la demande.** `htop` change de
tri, `vim` deplace son curseur, `tmux` redimensionne ses volets, `less` defile.
`Maj` enfonce rend la selection locale — la convention habituelle — et `Ctrl`
pendant un glisser selectionne en colonnes. La molette n'ignore plus les
petits crans d'un pave tactile.

**Les programmes qui suivent le focus l'apprennent** : changer d'onglet ou de
fenetre fait recharger a `vim` un fichier modifie sous lui et teint le volet
inactif de `tmux`.

Le detail, et les deux limites qui restent, sont dans
[ADR 0009](docs/adr/0009-entrees-du-terminal.md).

### Un panneau « Securite » dans les reglages

Quatre interrupteurs, tous fermes par defaut sauf l'ecriture du presse-papiers,
chacun avec l'explication de ce qu'il ouvre : lecture et ecriture du
presse-papiers par le distant (avec une taille maximale), repli sur `/tmp`
quand `XDG_RUNTIME_DIR` manque, et le repli de mise a jour ci-dessus.

La fiche de connexion signale aussi les options `ssh` qui font executer un
programme sur **votre** machine (`ProxyCommand`, `LocalCommand`...) ou qui
coupent la verification d'empreinte. Elles restent permises — un bastion en vit
— mais un fichier de configuration recu d'un tiers n'est pas un simple reglage,
et l'interface le dit maintenant.

### Le reste

* Les scripts et les sockets d'agent vivent dans un repertoire verifie prive
  avant chaque usage, et les fichiers sont crees sans jamais suivre un lien
  symbolique depose par quelqu'un d'autre.
* Les messages venus de `pass-cli` sont expurges avant d'etre affiches, et ne
  peuvent plus contenir de sequences qui repeignent le terminal.
* Une reference `pass://` dont le coffre ou l'item contient `/`, `?` ou `#`
  n'est plus ambigue.
* La chaine de construction est verrouillee : actions epinglees par empreinte,
  outils telecharges verifies par SHA256 avant d'etre executes, jeton en
  lecture seule, `cargo audit` et `cargo deny` a chaque construction, et un
  fichier `SHA256SUMS` livre avec les paquets.
* Un fichier [SECURITY.md](SECURITY.md) decrit comment signaler une
  vulnerabilite, et ce que le logiciel promet — ou ne promet pas.

## 1.1.0 — 10 septembre 2026

### L'interface montre ce qu'elle fait

Les fenetres — fiche de connexion, reglages, confirmation — s'ouvrent et se
referment en fondu au lieu d'apparaitre et de disparaitre d'un coup. Les
dossiers de la barre laterale se deplient et se replient sur place, chevron
compris.

Surtout, **une connexion SSH qui s'etablit se voit**: le bandeau violet sous
l'onglet devient une barre de chargement, sa pastille bat, et l'onglet retrouve
son soulignement des que le serveur repond. Pendant qu'un agent Proton Pass
demarre, l'onglet affiche une jauge indiquant le temps restant avant abandon.

Le reste suit le meme principe: les notifications entrent et sortent par le
bord droit, les connexions recentes de l'accueil se posent l'une apres l'autre,
et les boutons s'enfoncent quand on les tient.

Rien de tout cela ne tourne au repos: aucune animation perpetuelle ne demarre
sans une attente reelle, et toutes s'arretent d'elles-memes.

## 1.0.0 — 9 septembre 2026

Premiere version stable.

### Vos mots de passe vont dans Proton Pass

Quand vous creez une connexion, tapez le mot de passe du serveur et cliquez sur
**Enregistrer dans Proton Pass**. Il part dans votre coffre, avec le nom du
serveur et l'utilisateur. Vous n'avez plus a creer l'entree a la main dans
Proton Pass avant.

Si l'entree existe deja, le bouton propose de mettre le mot de passe a jour.

### Vos cles SSH aussi

Vous pouvez **generer** une nouvelle cle directement dans votre coffre, ou y
**importer** une cle que vous avez deja sur votre ordinateur. La connexion s'y
branche toute seule. Il ne reste qu'a deposer la cle publique sur le serveur.

### La connexion a Proton Pass se retablit toute seule

La session Proton Pass se ferme quand vous eteignez votre machine. Avant,
l'application ne fonctionnait plus et il fallait deviner pourquoi.

Maintenant elle s'en apercoit et ouvre la page de connexion dans votre
navigateur. Vous vous identifiez, et tout repart. Si vous preferez le faire
vous-meme, la reconnexion automatique se desactive dans les reglages.

### L'application s'affiche correctement dans les logitheques

Le nom, l'auteur, la description et les captures d'ecran apparaissent
desormais dans Discover, GNOME Logiciels et les autres magasins
d'applications.

---

**Il vous faut** Proton Pass CLI (`pass-cli`) et OpenSSH 8.4 ou plus recent.

**Telechargements** : paquet Debian/Ubuntu (`.deb`), paquet Fedora/openSUSE
(`.rpm`), AppImage portable, ou le binaire seul.
