# Politique de securite

sshpass-gui manipule ce qu'un poste a de plus sensible : des mots de passe de
serveurs, des cles SSH privees et une session Proton Pass. Un defaut ici ne
coute pas un affichage casse, il coute un acces. Ce fichier dit comment le
signaler, ce que le logiciel promet, et ce qu'il ne promet pas.

## Signaler une vulnerabilite

**N'ouvrez pas d'issue publique.** Une issue est indexee avant meme qu'un
correctif existe.

Passez par l'onglet **Security → Report a vulnerability** du depot
([GitHub Security Advisories](https://github.com/memton80/sshpass/security/advisories/new)),
qui ouvre un fil prive avec les mainteneurs.

Ce qui aide, dans l'ordre d'utilite :

* ce qu'un attaquant obtient, et depuis quelle position (serveur SSH distant,
  autre compte du meme poste, fichier de configuration recu d'un tiers) ;
* les etapes pour le reproduire, meme approximatives ;
* la version de sshpass-gui, celle de `pass-cli` et la distribution ;
* le correctif que vous voyez, si vous en voyez un.

Un identifiant CVE ou un avis GHSA peut etre demande pour vous ; dites-le si
vous souhaitez y etre credite, ou rester anonyme.

### Delais

| Etape | Delai vise |
| --- | --- |
| Accuse de reception | 72 heures |
| Premiere evaluation (severite, perimetre) | 7 jours |
| Correctif publie, ou plan date | 90 jours |

Ce sont des **objectifs**, tenus par un projet a un mainteneur, pas un
engagement contractuel. Si un delai derape, le fil prive le dira plutot que de
rester muet.

### Divulgation coordonnee

Le correctif est publie d'abord, l'avis ensuite. Passe le correctif, la
publication est la bienvenue et le credit est donne. Si les 90 jours passent
sans correctif ni nouvelles, publiez : un projet silencieux ne doit pas
proteger plus longtemps un defaut que ses utilisateurs subissent.

## Versions supportees

| Version | Correctifs de securite |
| --- | --- |
| Derniere version publiee | oui |
| Versions anterieures | non |

Le projet n'a pas de branche de maintenance : un correctif sort dans une
nouvelle version, et la mise a jour est le chemin.

## Ce que le logiciel promet

* **Aucun secret dans la configuration.** Le TOML ne contient que des
  references `pass://coffre/item[/champ]`. Un test le verifie a chaque
  execution de la suite.
* **Aucun secret dans la memoire de sshpass-gui a la lecture.** Un mot de passe
  servi a `ssh` ne transite ni par le processus ni par le PTY : `ssh` execute
  lui-meme le script `SSH_ASKPASS`, qui appelle `pass-cli`. Le script ne
  contient qu'une reference.
* **Le mot de passe n'est detenu qu'a l'ecriture**, dans un `Secret` non
  clonable, masque au `Debug`, dont le tampon est mis a zero a la destruction.
* **Rien ne passe par un shell.** `ssh` et `pass-cli` sont lances par
  `Command::new(...).args(...)`. Une valeur biscornue reste un argument.
* **Le presse-papiers ne part pas vers le serveur.** Les requetes de lecture
  OSC 52 sont refusees par defaut, sans reponse.
* **Le pont askpass ne repond qu'a `ssh`.** L'authentification par mot de passe
  negocie `password` seul, jamais `keyboard-interactive`, dont les questions
  sont redigees par le serveur ; le script verifie en plus la forme de l'invite.
* **Les fichiers volatils sont prives.** Scripts askpass et sockets d'agent
  vivent dans `$XDG_RUNTIME_DIR/sshpass-gui`, cree en 0700, verifie a chaque
  usage, et les scripts sont crees avec `O_EXCL` en 0700.

## Ce que le logiciel ne promet pas

Ces points sont connus et assumes. Les signaler n'apporte rien de neuf ; les
contourner ou les reduire, si.

* **La configuration est une politique d'execution, pas un simple reglage.**
  Une connexion porte des options `ssh -o` libres, et `ssh` sait executer des
  programmes locaux pour le compte de sa configuration (`ProxyCommand`,
  `LocalCommand` avec `PermitLocalCommand`, `KnownHostsCommand`, `Match exec`).
  **N'ouvrez jamais un `config.toml` recu d'un tiers.** L'interface signale
  ces directives, mais elle ne les interdit pas : les interdire retirerait un
  usage legitime (bastion, tunnel) sans rien empecher a qui peut deja ecrire
  dans le fichier.
* **Un compte local qui vous a compromis a deja gagne.** Le modele de menace
  couvre les *autres* comptes de la machine, pas le votre : qui execute du code
  sous votre identite lit votre configuration et parle a vos agents.
* **La memoire n'est pas verrouillee.** Le tampon d'un `Secret` est efface,
  mais l'allocateur, la pagination ou une copie faite par `serde_json`
  echappent a ce controle. Rien n'est `mlock`.
* **`pass-cli` est fait confiance.** Le binaire designe dans les reglages est
  execute avec vos droits, et c'est lui qui detient les secrets. Le champ
  attend un chemin ou un nom trouve dans le `PATH` : les deux se choisissent.
* **La mise a jour d'un mot de passe peut echouer plutot que de fuiter.** Le
  gabarit part par l'entree standard. Si la version de `pass-cli` installee ne
  connait pas cette forme, le seul autre chemin ecrit le mot de passe dans
  `/proc/<pid>/cmdline`, lisible par tous les comptes de la machine : c'est
  refuse, sauf a cocher « Mot de passe en ligne de commande » dans les
  reglages.
* **L'ecriture OSC 52 reste permise** (bornee, et desactivable) : un serveur
  peut donc remplacer le contenu de votre presse-papiers. Relisez ce que vous
  collez apres une session sur une machine dont vous n'etes pas sur.

## Chaine de construction

* Les actions GitHub sont **epinglees par SHA**, pas par etiquette ; le
  commentaire garde la version lisible.
* Les outils AppImage telecharges puis executes sur le runner sont figes a une
  version precise et leur **SHA256 est verifie avant** le `chmod +x`.
* Le jeton du workflow est en `contents: read`.
* `cargo audit` et `cargo deny` tournent a chaque construction ; la politique
  de dependances est dans [`deny.toml`](deny.toml).
* Les paquets publies sont accompagnes d'un fichier `SHA256SUMS`.

## Perimetre

Sont dans le perimetre : le code de ce depot, ses fichiers de construction,
et la maniere dont il appelle `ssh` et `pass-cli`.

Sont hors perimetre : les failles de `pass-cli`, d'OpenSSH, de Proton Pass ou
des caisses tierces — signalez-les a leurs auteurs, et dites-le nous si
sshpass-gui peut en attenuer l'effet.
