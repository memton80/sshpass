# Journal des versions

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
