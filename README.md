# sshpass

Gestionnaire de connexions SSH avec terminal integre, **100 % Rust natif**,
adosse a **Proton Pass** pour les cles et les secrets.

Ni Electron, ni webview, ni GTK, ni Qt : une fenetre `winit`, un rendu OpenGL,
et une emulation de terminal en Rust pur.

![Ecran d'accueil : barre laterale, dossiers, favoris et connexions recentes](docs/captures/accueil.png)

![Terminal integre : couleurs ANSI, gras, souligne et historique](docs/captures/terminal.png)

## Ce que ca fait

* **Barre laterale** : connexions, dossiers imbricables, favoris, recherche
  incrementale (`Ctrl+Maj+F`), actions rapides au survol.
* **Ecran d'accueil** : recherche et section « Recentes » (`nom` + `user@host`).
* **Terminal par onglet** : emulation xterm-256color, historique, selection
  souris (simple / mot / ligne), copier-coller, redimensionnement dynamique,
  couleurs 24 bits, gras et souligne.
* **Proton Pass dans l'interface** : parcourir les coffres et leurs items,
  associer un item a une connexion, voir et piloter l'etat des agents SSH.
* **CRUD des connexions** : creation, modification, suppression, deplacement
  entre dossiers, favoris et tags.
* **Autocompletion** sur l'hote, l'utilisateur, le coffre et l'item, alimentee
  par les connexions deja enregistrees et le coffre deja charge — navigation
  aux fleches, `Entree` ou `Tab` pour accepter, `Echap` pour fermer.
* **Interface animee** : survols en fondu, transitions de vue et glissement du
  panneau Proton Pass, le tout avec les animateurs natifs d'egui (aucune
  dependance ajoutee) — voir [ADR 0005](docs/adr/0005-animations-et-autocompletion.md).
* **Theme sombre violet/gris**, habillage pixel art dessine (icones, bordures,
  pastilles, curseur), texte en **police systeme**.

## Pile technique

| Role | Choix | Pourquoi |
| --- | --- | --- |
| Interface | [`egui`](https://github.com/emilk/egui) / `eframe` (backend `glow`) | mode immediat : acces direct au `Painter`, ideal pour la grille du terminal et le pixel art — voir [ADR 0001](docs/adr/0001-framework-gui.md) |
| Terminal | [`alacritty_terminal`](https://crates.io/crates/alacritty_terminal) | emulation VT100/xterm en Rust pur, sans `libvte` |
| Polices | [`fontdb`](https://crates.io/crates/fontdb) | lecture des polices systeme en Rust pur, sans `freetype` ni `font-kit` |
| Secrets | `pass-cli` en sous-processus | sortie `--output json` — voir [ADR 0002](docs/adr/0002-integration-pass-cli.md) |
| Config | TOML | lisible et editable a la main — voir [ADR 0004](docs/adr/0004-format-de-configuration.md) |
| Animations | `Context::animate_*` d'egui | interpolation par identifiant, rafraichissement demande automatiquement — voir [ADR 0005](docs/adr/0005-animations-et-autocompletion.md) |

Le pixel art se limite aux elements graphiques : **tout le texte** (libelles,
champs, terminal) utilise la police par defaut du systeme.

## Compilation

### Dependances systeme (Debian / Ubuntu)

```bash
sudo apt-get install -y \
  libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libx11-dev libx11-xcb-dev libxcb-xkb-dev \
  libgl1-mesa-dev libfontconfig1-dev pkg-config
```

Sur une machine qui ne fait qu'**executer** le binaire, les paquets runtime
correspondants suffisent (`libxkbcommon-x11-0`, `libwayland-client0`, `libgl1`,
`libxcb-xkb1`).

### Construire et lancer

```bash
cargo build --release
./target/release/sshpass
```

Rust 1.92 ou plus recent.

## Paquets

Chaque push produit les quatre formats en artefacts de la CI
(onglet **Actions** → dernier run → *Artifacts*) :

| Format | Fichier | Construit par |
| --- | --- | --- |
| Binaire Linux | `sshpass-x86_64-linux` | `cargo build --release` |
| Debian / Ubuntu | `sshpass_<version>-1_amd64.deb` | [`cargo-deb`](https://github.com/kornelski/cargo-deb) |
| Fedora / openSUSE | `sshpass-<version>-1.x86_64.rpm` | [`cargo-generate-rpm`](https://github.com/cat-in-136/cargo-generate-rpm) |
| Portable | `sshpass-<version>-x86_64.AppImage` | [`linuxdeploy`](https://github.com/linuxdeploy/linuxdeploy) |
| Windows | `sshpass.exe` | `cargo build --release` sur `windows-latest` |

Les reconstruire en local :

```bash
cargo install cargo-deb cargo-generate-rpm
cargo build --release
cargo deb --no-build       # -> target/debian/
cargo generate-rpm         # -> target/generate-rpm/
```

### Dependances declarees

`winit`, `glutin` et `xkbcommon-dl` ouvrent leurs bibliotheques par `dlopen`,
pas par edition de liens : le binaire ne declare que `libc`, `libm` et
`libgcc`. Ni `dpkg-shlibdeps` ni l'analyse ELF du RPM ne voient donc les
bibliotheques graphiques, et un paquet reduit a ses dependances automatiques
paniquerait au premier lancement sur une machine sans `libxkbcommon-x11`.

Elles sont donc declarees a la main dans `Cargo.toml` — par nom de paquet pour
le `.deb`, par soname pour le `.rpm` (les noms de paquets different entre
Fedora et openSUSE). Pour verifier la liste apres une montee de version d'egui
ou de winit :

```bash
strings -a target/release/sshpass | grep -oE 'lib[A-Za-z0-9_-]+\.so(\.[0-9]+)+' | sort -u
```

L'AppImage n'embarque aucune de ces bibliotheques : elles figurent toutes sur
la liste d'exclusion AppImage (pilotes graphiques et bibliotheques systeme, qui
doivent venir de l'hote).

### Conflit de nom a connaitre

> Debian et Ubuntu distribuent deja un paquet **`sshpass`** : l'outil en ligne
> de commande qui fournit un mot de passe a `ssh` de maniere non interactive
> (version 1.09 dans `noble/universe`). Il n'a aucun rapport avec ce projet,
> mais il porte le meme nom **et** installe le meme chemin `/usr/bin/sshpass`.
>
> Consequences concretes : les deux paquets ne peuvent pas coexister, et comme
> `1.09 > 0.1.0`, un `apt upgrade` remplacerait cette application par l'outil
> en ligne de commande.
>
> Si vous comptez distribuer le `.deb`, renommez le paquet. Une seule ligne
> dans `[package.metadata.deb]` suffit pour le nom du paquet :
>
> ```toml
> name = "sshpass-gui"
> ```
>
> Le nom du binaire, lui, se change avec une section `[[bin]]` :
>
> ```toml
> [[bin]]
> name = "sshpass-gui"
> path = "src/main.rs"
> ```
>
> (il faudra alors ajuster `Exec=` dans `packaging/sshpass.desktop` et les
> chemins des `assets`). L'AppImage et le `.exe` ne sont pas concernes.

### Icone et fichier `.desktop`

L'icone n'est pas dessinee a la main : elle est rasterisee depuis la meme
grille 8x8 que `pixel::TERMINAL`, avec la palette de `theme.rs`. Apres toute
modification du sprite :

```bash
python3 packaging/generate-icon.py
```

## Utilisation

### Premier lancement

Sans configuration, l'accueil propose de creer une connexion. Le fichier est
ecrit dans `~/.config/sshpass/config.toml` a la premiere sauvegarde.

Pour travailler sur une configuration de test :

```bash
SSHPASS_CONFIG=/tmp/essai.toml ./target/release/sshpass
```

### Proton Pass

sshpass appelle le binaire `pass-cli` (configurable dans les reglages). La
pastille de la barre d'outils indique s'il est detecte ; un clic relance la
detection.

Trois modes d'agent, au choix dans les reglages :

* **Agent dedie** *(defaut)* — sshpass demarre un `pass-cli ssh-agent start` par
  coffre et injecte le `SSH_AUTH_SOCK` correspondant dans chaque onglet.
* **Agent existant** — `pass-cli ssh-agent load` pousse les cles dans l'agent
  deja en place ; sshpass ne surcharge rien.
* **Desactive** — les onglets heritent de l'environnement.

Detail de la strategie : [ADR 0003](docs/adr/0003-ssh-auth-sock.md).

### Mots de passe

Pour une connexion en `auth = "password"`, sshpass **ne lit jamais le secret**.
Il ecrit un script `SSH_ASKPASS` (mode 0700) qui ne contient que l'URI
`pass://coffre/item/champ`, et laisse `ssh` l'executer lui-meme. Le mot de passe
ne passe donc ni par la memoire de sshpass, ni par le PTY, ni par les journaux.

Necessite OpenSSH 8.4 ou plus recent (pour `SSH_ASKPASS_REQUIRE=force`).

### Raccourcis

| Raccourci | Effet |
| --- | --- |
| `Ctrl+Maj+F` | Recherche dans la barre laterale |
| `Ctrl+Maj+T` | Terminal local |
| `Ctrl+Maj+P` | Panneau Proton Pass |
| `Ctrl+Maj+Tab` | Onglet suivant |
| `Ctrl+Maj+C` / `Ctrl+Maj+V` | Copier / coller dans le terminal |
| `Ctrl+Maj+A` | Tout selectionner dans le terminal |
| `Ctrl+Maj+W` | Fermer l'onglet |
| `Maj+PagePrec` / `Maj+PageSuiv` | Defiler l'historique |
| `Haut` / `Bas`, `Entree`, `Echap` | Naviguer, accepter, fermer une liste de suggestions |

Les combinaisons `Ctrl+Maj+…` sont reservees a l'interface : tout le reste
descend dans le terminal, y compris `Ctrl+C`, `Ctrl+D` et `Ctrl+F`.

## Configuration

Le format complet est decrit dans
[ADR 0004](docs/adr/0004-format-de-configuration.md). L'essentiel :

```toml
version = 1

[proton_pass]
binary = "pass-cli"
agent_mode = "own-agent"

[[connections]]
id = "8a12…"
name = "web-01"
host = "10.0.0.4"
user = "root"
port = 22
favorite = true
auth = "agent"
tags = ["prod"]

[connections.proton]
vault = "SSH Keys"
item = "web-01"
```

**Aucun secret n'est ecrit dans ce fichier** — uniquement des references vers
les items du coffre. Un test unitaire le verifie.

## Developpement

```bash
cargo test                          # tests unitaires (sans pass-cli ni serveur X)
cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
```

Essayer l'interface sans serveur d'affichage :

```bash
xvfb-run -a --server-args="-screen 0 1280x800x24" ./target/debug/sshpass
```

### Organisation

```
src/
├── main.rs        point d'entree, chargement de la configuration
├── app.rs         etat global, boucle eframe, file d'actions
├── theme.rs       palette sombre et polices systeme
├── config/        modele de donnees et persistance TOML
├── pass/          pass-cli (cli.rs), agents SSH (agent.rs), pont askpass
├── term/          session PTY, encodage clavier, couleurs, rendu egui
└── ui/            panneaux, fenetres, animations, autocompletion, pixel art
```

## Integration continue

`.github/workflows/build.yml`, trois jobs :

* **checks** — `cargo fmt --check`, `clippy -D warnings`, `cargo test`.
* **linux** — build release, puis `.deb`, `.rpm` et AppImage.
* **windows** — `cargo test` et build release sur `windows-latest`.

Les binaires ne sont pas seulement compiles : `.github/scripts/smoke-test.sh`
les **lance vraiment** sur un serveur X virtuel et echoue s'ils s'arretent
dans les quinze secondes. Un binaire qui compile mais panique au demarrage —
DLL absente, police introuvable, contexte OpenGL refuse — est ainsi detecte.
Le binaire nu, l'AppImage et le `.exe` passent chacun ce test.

## Limites connues

* Le rapport de souris (`MOUSE_REPORT_CLICK`) n'est pas transmis aux
  applications distantes : la souris pilote la selection locale. La molette est
  bien traduite en fleches sur l'ecran alternatif (`less`, `vim`).
* L'italique du terminal est rendu avec la fonte normale (le gras a bien sa
  propre famille).
* Le schema JSON exact de `pass-cli` n'etant pas publie, les analyseurs sont
  tolerants et testes sur plusieurs conventions de nommage
  ([ADR 0002](docs/adr/0002-integration-pass-cli.md)) ; a confronter a une
  sortie reelle.
* Sous Windows, l'agent SSH de Proton Pass n'est pas pilote : `pass-cli`
  y expose un tube nomme la ou sshpass attend une socket Unix. Les modes
  « agent existant » et « desactive » restent utilisables.
* Pas de paquet macOS ni de `.dmg` pour l'instant, et pas de build ARM64.
* Le `.exe` n'embarque pas encore d'icone de ressource Windows (il faudrait
  une dependance de build `winresource` et un `.ico`).

## Licence

GPL-3.0-or-later — voir [LICENSE](LICENSE).
