# ADR 0004 — Format de configuration

*Statut: accepte.*

## Decision

**TOML**, dans `$XDG_CONFIG_HOME/sshpass/config.toml`
(surchargeable par la variable d'environnement `SSHPASS_CONFIG`).

## Regle non negociable

**Aucun secret dans ce fichier.** Ni mot de passe, ni cle privee, ni
passphrase. Une connexion ne stocke qu'une *reference* (`ProtonRef`): nom du
coffre, titre de l'item, et eventuellement le champ a lire. Un test unitaire
(`config::tests::no_secret_is_ever_serialized`) verifie que le TOML produit ne
contient aucun de ces mots.

## Structure

```toml
version = 1

[ui]
font_size = 14.0
terminal_font_size = 14.0
sidebar_width = 260.0
scrollback_lines = 10000
pixel_scale = 2

[proton_pass]
binary = "pass-cli"
agent_mode = "own-agent"      # own-agent | load-into-existing | disabled
refresh_interval = 3600
default_vault = "SSH Keys"    # optionnel

[[folders]]
id = "0d3f…"                  # uuid
name = "Production"
# parent = "…"                # dossiers imbricables

[[connections]]
id = "8a12…"
name = "web-01"
host = "10.0.0.4"
user = "root"
port = 22
folder = "0d3f…"              # optionnel: absent = racine
favorite = true
auth = "agent"                # agent | password | key-file
tags = ["prod", "web"]
last_used = 1757260000        # epoch unix, secondes
# key_file = "~/.ssh/id_ed25519"   # seulement pour auth = "key-file"
# command  = "htop"                # sinon, shell interactif
# ssh_options = ["ServerAliveInterval=30"]

[connections.proton]          # optionnel: reference, jamais un secret
vault = "SSH Keys"
item = "web-01"
# field = "password"          # defaut: password
```

## Justification

* **TOML plutot que JSON**: le fichier est destine a etre relu et edite a la
  main; les commentaires et les tableaux de tables (`[[connections]]`) le
  rendent lisible sans outil.
* **Dossiers par identifiant, avec `parent`**: renommer un dossier ne casse
  aucune connexion, et l'imbrication ne demande pas d'encoder une hierarchie
  dans les noms.
* **Favoris et tags portes par la connexion**: un favori est un attribut, pas
  un dossier special; il n'y a donc rien a synchroniser entre deux listes.
* **`last_used` en epoch unix**: comparable et triable sans dependance de date,
  et la section « Recentes » de l'accueil s'en deduit directement.
* **Tous les champs ont une valeur par defaut** (`#[serde(default)]`): un
  fichier partiel, ou ecrit a la main, reste valide. Un fichier illisible
  n'empeche pas le demarrage — l'application repart a vide **sans ecraser** le
  fichier existant.
* **Ecriture atomique** (fichier temporaire puis `rename`): une coupure en
  cours d'ecriture ne laisse pas une configuration tronquee.
