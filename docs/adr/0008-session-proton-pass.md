# ADR 0008 — Surveiller la session Proton Pass et la rouvrir

*Statut: accepte.*

## Contexte

Toutes les commandes `pass-cli` supposent une session ouverte. Cette session
ne dure pas : elle expire, et l'arret de la machine y met fin. Or sshpass-gui
ne sondait que la **presence** du binaire (`pass-cli --version`) — un test qui
reussit parfaitement alors que la session est fermee.

Consequence observee : au redemarrage de la machine, l'application affichait
une pastille verte, puis chaque operation echouait. Les messages venaient de
`pass-cli` et parlaient d'autorisation, sans jamais nommer le remede. Il
fallait deviner qu'il fallait taper `pass-cli login` dans un terminal.

## Ce que `pass-cli` permet

| Besoin | Commande | Remarque |
| --- | --- | --- |
| Verifier la session | `pass-cli info` | echoue si la session est fermee |
| Rouvrir la session | `pass-cli login` | imprime une URL, attend le navigateur |
| Deverrouiller | `pass-cli session unlock` | demande un code a l'utilisateur |

Sources : [`docs/commands/info.md`](https://github.com/protonpass/pass-cli/blob/main/docs/public/docs/commands/info.md),
[`docs/commands/login.md`](https://github.com/protonpass/pass-cli/blob/main/docs/public/docs/commands/login.md),
[`docs/commands/session.md`](https://github.com/protonpass/pass-cli/blob/main/docs/public/docs/commands/session.md).

## Decision

**Sonder la session, et la rouvrir toute seule quand elle est fermee.**

### Trois etats, trois remedes

L'ancien `PassStatus` melangeait tout dans « disponible / absent ». Il en
distingue maintenant quatre, parce que les remedes n'ont rien a voir :

| Etat | Cause | Remede |
| --- | --- | --- |
| `Ready` | session ouverte | — |
| `LoggedOut` | expiree, ou machine redemarree | `pass-cli login`, **automatique** |
| `Locked` | verrouillee par un code | `pass-cli session unlock`, **manuel** |
| `Missing` | binaire introuvable | installer Proton Pass CLI |

Le verrouillage est explicitement exclu de l'automatisation : le code n'est
connu que de l'utilisateur, et `unlock` le demande sur un terminal. Relancer
`login` n'y changerait rien — l'interface dit donc quoi taper, et s'arrete la.

### Reconnaitre une session fermee

`pass-cli` ne reserve pas de code de sortie a ce motif : seul le texte de la
sortie d'erreur permet de trancher. L'analyse suit le meme principe tolerant
que celle du JSON ([ADR 0002](0002-integration-pass-cli.md)) : une liste de
tournures connues (`not logged in`, `session expired`, `unauthorized`…),
comparee en minuscules.

Deux precautions :

* **le verrouillage est teste en premier**, car son message parle lui aussi
  d'autorisation ;
* **seule la sortie d'erreur est examinee, jamais la commande.** `pass-cli
  item create login` contient « login » sans rien dire de la session ; le
  confondre lancerait une reconnexion a chaque creation d'identifiant ratee.

Un `info` qui echoue **sans** tournure reconnue est quand meme traite comme une
session fermee : c'est de loin la cause la plus probable, et une tentative de
reconnexion inutile coute moins cher qu'une application bloquee sur un message
que personne ne sait interpreter.

### Quand la verification a lieu

Trois moments, parce qu'aucun ne suffit seul :

1. **Au demarrage.** C'est le cas du redemarrage de machine, celui qui a motive
   cette ADR.
2. **Toutes les cinq minutes.** Une session peut expirer pendant que
   l'application tourne. egui ne redessine pas une fenetre inerte : la ronde
   programme donc son propre reveil (`request_repaint_after`), sans quoi une
   session expirant la nuit ne serait vue qu'au matin.
3. **A chaque appel qui echoue.** C'est souvent une operation ordinaire qui
   revele la coupure avant la ronde. Les reponses du thread Proton Pass portent
   pour cela un `PassFailure` — le message **et** le verdict sur la session —
   plutot qu'une simple chaine : sans quoi chaque appelant devrait refaire
   l'analyse du texte.

S'y ajoute un garde-fou a l'ouverture d'une connexion : si elle depend du
coffre et que la session est fermee, l'onglet n'est pas ouvert du tout. Il
echouerait de toute facon — l'agent ne demarrerait pas, ou le script askpass ne
trouverait rien — et un onglet en erreur explique moins bien la situation
qu'un message qui nomme la cause et lance la reconnexion.

### Rouvrir sans bloquer l'interface

`pass-cli login` ne peut pas passer par l'executeur des autres commandes : il
attend que l'authentification web aboutisse, c'est-a-dire le temps qu'il faut a
un humain. Le delai de 45 secondes des autres appels le tuerait.

Il est donc supervise comme un agent (`pass::login`) : processus a part, sortie
lue ligne a ligne pendant qu'il tourne, et **premiere URL imprimee ouverte dans
le navigateur**. Une borne de cinq minutes evite qu'un flux oublie ne tourne
pour la duree de la session.

Deux precautions sur l'URL, qui vient d'une sortie de processus et part vers un
ouvreur de liens :

* **seul `https://` est accepte.** Un `file://` ou un `javascript:` issu d'une
  ligne mal analysee ne sera jamais ouvert.
* **elle est passee en argument unique, sans shell.** Rien de ce qu'elle
  contient ne peut etre interprete comme une commande.

Aucun identifiant ne transite par sshpass-gui : l'authentification se joue
entierement entre le navigateur et Proton. L'application ne voit qu'une adresse
publique.

### Ne jamais insister

Une reconnexion automatique qui se repete est pire que pas de reconnexion du
tout : un poste laisse seul finirait avec une pile d'onglets de navigateur.
L'autorisation est donc a usage unique — consommee au lancement d'une
tentative, et rendue seulement quand une session est confirmee ouverte.

Apres un echec, plus rien ne part tout seul. Le panneau lateral montre un
bouton « Se reconnecter », et le clic vaut autorisation explicite. Le
comportement est verifie de bout en bout : un `login` qui echoue n'est tente
qu'une fois, meme au bout de vingt secondes de rondes.

L'ouverture automatique du navigateur reste par ailleurs desactivable
(`auto_login` dans `[proton_pass]`, case dans les reglages) : sur un poste ou
elle n'est pas souhaitable, la detection continue et seul le bouton subsiste.

## Consequences

* Le cas qui a motive l'ADR — redemarrer la machine, rouvrir sshpass-gui — se
  resout sans que l'utilisateur ait a savoir que `pass-cli login` existe.
* Une pastille verte signifie desormais **utilisable**, pas seulement
  « binaire present ». Les fonctions d'ecriture, qui exigent `is_available()`,
  ne s'activent donc plus sur une session morte.
* Le contrat de securite est inchange : sshpass-gui ne voit ni identifiant, ni
  mot de passe Proton, ni jeton de session — seulement une URL publique.
* La detection repose sur des tournures anglaises. Un `pass-cli` traduit ne
  serait pas reconnu mot pour mot, mais le repli — traiter un `info` en echec
  comme une session fermee — garde le comportement correct.
* Si aucun ouvreur d'URL n'existe (`xdg-open`, `open`), rien n'est bloque :
  l'interface affiche le lien avec un bouton « Copier le lien ».
