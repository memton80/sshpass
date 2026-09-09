# ADR 0006 — Renommage en `sshpass-gui`

*Statut: accepte.*

## Contexte

Debian et Ubuntu distribuent depuis longtemps un paquet nomme **`sshpass`** :
l'utilitaire en ligne de commande qui fournit un mot de passe a `ssh` de
maniere non interactive (`1.09-1` dans `noble/universe`). Il n'a aucun rapport
avec ce projet.

Le conflit s'est manifeste des la premiere construction du paquet Debian,
installe puis interroge sur la machine de developpement :

```
sshpass:
  Installed: 0.1.0-1      <- cette application
  Candidate: 1.09-1       <- l'outil d'Ubuntu
```

Deux collisions, pas une :

1. **Le nom du paquet.** Les deux ne peuvent pas etre installes ensemble.
2. **Le chemin `/usr/bin/sshpass`.** Meme en renommant seulement le paquet,
   `dpkg` refuserait l'installation pour cause de fichier deja possede par un
   autre paquet.

Consequence la plus vicieuse : `1.09 > 0.1.0`, donc un simple `apt upgrade`
remplacerait silencieusement cette application par l'outil en ligne de
commande.

## Decision

**Renommer le projet en `sshpass-gui`**, de la caisse Rust jusqu'aux paquets.

Comme la seconde collision porte sur le chemin du binaire, renommer le seul
paquet n'aurait rien regle : le renommage descend donc jusqu'au binaire.

| Element | Avant | Apres |
| --- | --- | --- |
| Caisse et binaire | `sshpass` | `sshpass-gui` |
| Paquets `.deb` / `.rpm` | `sshpass` | `sshpass-gui` |
| Fichier `.desktop` et icone | `sshpass.*` | `sshpass-gui.*` |
| Repertoire de configuration | `~/.config/sshpass` | `~/.config/sshpass-gui` |
| Repertoire volatil | `$XDG_RUNTIME_DIR/sshpass` | `$XDG_RUNTIME_DIR/sshpass-gui` |
| Variable de surcharge | `SSHPASS_CONFIG` | `SSHPASS_GUI_CONFIG` |
| Titre de fenetre et `app_id` | `sshpass` | `sshpass-gui` |

Le **depot** garde son nom : l'URL reste valide et rien ne s'y accroche.

## Details qui se seraient vus a l'usage

* **La cible des journaux** devient `sshpass_gui` — Rust convertit les tirets
  du nom de caisse en soulignes. Le filtre par defaut de `env_logger` a ete
  ajuste, sans quoi `RUST_LOG` n'aurait plus rien filtre.
* **`app_id` doit rester egal a `StartupWMClass`** du fichier `.desktop`, sans
  quoi le compositeur Wayland ne retrouve pas l'icone de la fenetre.

## Reprise de l'ancienne configuration

Une configuration ecrite avant le renommage serait devenue invisible. Au
demarrage, si `~/.config/sshpass-gui/config.toml` n'existe pas et que
`~/.config/sshpass/config.toml` existe, le fichier est **copie** vers le
nouvel emplacement, et le chemin repris est journalise.

Copie et non deplacement : en cas de retour en arriere, l'original est
toujours la. La reprise est desactivee quand `SSHPASS_GUI_CONFIG` impose un
chemin — un emplacement demande explicitement n'a pas a etre ecrase.

## Consequences

* Les paquets peuvent coexister avec l'outil `sshpass` d'Ubuntu.
* L'AppImage n'etait pas concernee par le conflit, mais suit le renommage
  pour rester coherente.
* Le nom affiche dans l'interface change aussi : une application qui
  s'annonce `sshpass` tout en s'installant sous `sshpass-gui` serait
  deroutante.
