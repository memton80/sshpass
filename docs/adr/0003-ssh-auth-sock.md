# ADR 0003 — Strategie `SSH_AUTH_SOCK`: un agent par coffre

*Statut: accepte.*

## Contexte

`pass-cli ssh-agent start` est un processus au premier plan qui expose une
socket Unix. Chaque onglet terminal lance un `ssh` qui lit `SSH_AUTH_SOCK` **au
demarrage** et ne la relit jamais. Il fallait decider de la granularite: un
agent par onglet, par coffre, ou un seul global.

## Decision

**Un agent par coffre, partage par tous les onglets qui l'utilisent**, avec
trois modes selectionnables dans les reglages (`config::AgentMode`):

| Mode | Comportement |
| --- | --- |
| `own-agent` *(defaut)* | sshpass-gui supervise un `pass-cli ssh-agent start` par coffre, sur `$XDG_RUNTIME_DIR/sshpass-gui/agent-<coffre>-<hash>.sock`. Chaque onglet recoit dans son environnement le `SSH_AUTH_SOCK` de l'agent du coffre associe a sa connexion. |
| `load-into-existing` | `pass-cli ssh-agent load` pousse les cles dans l'agent deja reference par `SSH_AUTH_SOCK`; sshpass-gui ne surcharge rien. |
| `disabled` | Les onglets heritent simplement de l'environnement. |

## Justification

* **Pas un agent par onglet.** Chaque demarrage d'agent peut demander un
  deverrouillage de session; ouvrir trois onglets sur le meme serveur en
  demanderait trois. Un agent par coffre mutualise ce cout.
* **Pas un agent global unique.** `pass-cli` filtre les cles par coffre
  (`--vault-name`). Un agent unique obligerait a charger tous les coffres, donc
  a exposer a `ssh` des cles sans rapport avec la connexion en cours.
* **Le nom de la socket est tronque et hache.** Les sockets Unix sont limitees a
  ~108 octets; un nom de coffre long ferait echouer le `bind`. Le hachage
  garantit l'unicite malgre la troncature.
* **C'est l'existence de la socket qui fait foi**, pas le texte affiche par
  `pass-cli`: `AgentManager::poll` attend le fichier avant de declarer l'agent
  utilisable. Un onglet ouvert avant que l'agent soit pret reste en attente
  (avec un bouton « Connecter sans attendre l'agent ») et demarre des que la
  socket apparait, ou abandonne au bout de 20 secondes avec un message clair.

## Mots de passe: `SSH_ASKPASS`, jamais le PTY

Pour `auth = "password"`, sshpass-gui **ne lit pas** le secret. Il ecrit un script
`SSH_ASKPASS` (mode 0700, dans le repertoire volatil) qui contient seulement
l'URI `pass://...`, puis pose `SSH_ASKPASS_REQUIRE=force`. C'est `ssh` qui
execute le script et lit sa sortie.

Consequence: le mot de passe ne transite ni par la memoire de sshpass-gui, ni par le
PTY, ni par les journaux. Aucune methode du module `pass` ne lit de secret.
`SSH_ASKPASS_REQUIRE=force` demande OpenSSH 8.4 ou plus recent.

## Consequences

* Les agents sont tues a la fermeture de l'application (`Drop` de
  `AgentManager`) et leurs sockets supprimees.
* Une socket orpheline (agent tue sans nettoyage) est supprimee avant un
  nouveau demarrage, sans quoi le `bind` echouerait.
* Changer de binaire `pass-cli` ou de mode dans les reglages arrete les agents
  en cours.
