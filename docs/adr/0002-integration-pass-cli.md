# ADR 0002 — Integration `pass-cli`: sortie JSON et analyse tolerante

*Statut: accepte.*

## Contexte

Le cahier des charges demandait de verifier si `pass-cli` expose une sortie
JSON exploitable, faute de quoi il aurait fallu analyser du texte libre.

## Verification

La documentation officielle de Proton Pass CLI le confirme: `--output FORMAT`
accepte `human` ou `json` sur les commandes que nous utilisons.

| Besoin | Commande |
| --- | --- |
| Lister les coffres | `pass-cli vault list --output json` |
| Lister les items d'un coffre | `pass-cli item list --vault-name <coffre> --output json` |
| Lire un champ | `pass-cli item view "pass://<coffre>/<item>/<champ>"` |
| Demarrer un agent | `pass-cli ssh-agent start --vault-name <coffre> --socket-path <sock>` |
| Charger dans l'agent existant | `pass-cli ssh-agent load --vault-name <coffre>` |

Les commandes d'**ecriture** (`item create login`, `item create ssh-key`,
`item update`) sont venues plus tard, avec leurs propres contraintes de
transmission des secrets : voir
[ADR 0007](0007-ecriture-dans-proton-pass.md).

Sources: [`docs/commands/item.md`](https://github.com/protonpass/pass-cli/blob/main/docs/public/docs/commands/item.md),
[`docs/commands/vault.md`](https://github.com/protonpass/pass-cli/blob/main/docs/public/docs/commands/vault.md),
[`docs/commands/ssh-agent.md`](https://github.com/protonpass/pass-cli/blob/main/docs/public/docs/commands/ssh-agent.md).

## Decision

**Utiliser `--output json`, avec des analyseurs volontairement tolerants.**

La documentation publique decrit les commandes et les options, mais ne fige pas
le schema des objets renvoyes. Plutot que de parier sur des noms de champs
exacts, `pass/cli.rs`:

* **normalise les cles** avant comparaison (minuscules, sans separateurs), donc
  `shareId`, `share_id` et `Share-ID` sont equivalents;
* **accepte plusieurs alias** par valeur (`shareId` | `id` | `vaultId`, etc.);
* **accepte un tableau nu ou enveloppe**: `[...]`, `{"vaults": [...]}` ou
  `{"data": [...]}`;
* **ignore les entrees inexploitables** (sans titre) au lieu d'echouer sur le
  lot entier.

Ce comportement est couvert par des tests unitaires qui n'exigent pas
l'installation de `pass-cli`.

## Consequences

* Une evolution du schema JSON de `pass-cli` a de bonnes chances de passer sans
  changement de code; sinon, la correction se limite a ajouter un alias.
* Les tests ne valident pas le schema **reel**: c'est le prix de l'absence de
  specification publiee. A confronter a une sortie reelle des la premiere
  installation de `pass-cli` sur un poste de developpement.
* Tous les appels sont bloquants (deverrouillage de session, reseau): ils
  s'executent sur un thread dedie (`pass::PassWorker`) avec un delai maximal
  au-dela duquel le processus est tue, pour qu'un `pass-cli` bloque ne gele
  jamais l'interface.
