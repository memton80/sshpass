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
└── ui/            panneaux, fenetres et primitives pixel art
```

## Integration continue

`.github/workflows/build.yml` : format, clippy (`-D warnings`), tests, build
release et **test de demarrage headless** sous Xvfb — un binaire qui compile
mais panique au demarrage est detecte. Le binaire est publie en artefact.

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
* Cible principale : Linux (KDE/Wayland et X11). Le packaging AppImage et la
  matrice Windows/macOS sont prevus en V2.

## Licence

GPL-3.0-or-later — voir [LICENSE](LICENSE).
