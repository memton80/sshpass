# ADR 0007 — Ecrire les secrets dans Proton Pass

*Statut: accepte.*

## Contexte

Jusqu'ici sshpass-gui savait **lire** Proton Pass et rien d'autre : lister les
coffres, lister les items, et pointer une connexion vers un item existant. La
liaison fonctionnait — les coffres s'affichaient bien — mais creer une
connexion ne remplissait rien dans le coffre. Concretement :

* choisir « Mot de passe (Proton Pass) » obligeait a **d'abord** creer l'item a
  la main dans l'application Proton Pass, puis a revenir en recopier le titre ;
* une connexion par cle etait dans le meme cas : la cle devait deja se trouver
  dans le coffre, importee par un autre outil.

La fiche de connexion refusait meme d'etre enregistree tant qu'aucun item
n'etait designe, sans offrir le moindre moyen d'en creer un. Le chemin
naturel — « je saisis le mot de passe du serveur, il part dans mon coffre » —
n'existait pas.

## Ce que `pass-cli` permet

| Besoin | Commande | Ou passe le secret |
| --- | --- | --- |
| Creer un identifiant | `item create login --vault-name V --from-template -` | **entree standard** |
| Changer un mot de passe | `item update --vault-name V --item-title T --field password=…` | ligne de commande |
| Importer une cle | `item create ssh-key import --from-private-key <chemin>` | un chemin, pas la cle |
| Generer une cle | `item create ssh-key generate --key-type ed25519` | rien ne sort du coffre |

Source : [`docs/commands/item.md`](https://github.com/protonpass/pass-cli/blob/main/docs/public/docs/commands/item.md).
Le gabarit accepte par `--from-template` est `{"title", "username", "email",
"password", "urls"}`.

## Decision

**Ecrire depuis la fiche de connexion, en faisant passer le secret par
l'entree standard chaque fois que `pass-cli` le permet.**

### Le contrat sur les secrets

La regle de lecture est inchangee : **sshpass-gui ne lit jamais un secret.**
Un mot de passe est servi a `ssh` par le script `SSH_ASKPASS`, qui n'embarque
que l'URI ; une cle privee reste dans le coffre et n'est vue que par l'agent.

L'ecriture ajoute une regle, forcement plus faible puisqu'il faut bien tenir la
valeur pour la transmettre :

1. **Jamais sur disque.** Aucun fichier temporaire : ni gabarit JSON, ni copie
   de cle.
2. **Jamais sur la ligne de commande** — une exception, ci-dessous. Le gabarit
   est pousse dans `stdin`, hors de portee de `/proc/<pid>/cmdline`, que
   n'importe quel processus de la machine peut lire.
3. **Jamais affiche.** Le type [`pass::Secret`] n'est ni clonable ni
   `Display` ; son `Debug` ne montre qu'une longueur. Les arguments cites dans
   un message d'erreur passent par `secret::redact`, qui masque la valeur des
   champs sensibles.
4. **Efface des que possible.** `Secret` remet son tampon a zero a la
   destruction ; le champ de saisie de la fiche est vide au clic
   (`Secret::take`) et de nouveau ecrase a la fermeture de la fenetre.
5. **Jamais en retour.** Aucune variante de `PassResponse` ne rapporte de
   secret : seulement le coffre, le titre de l'item et un message.

Ces garanties sont celles d'un programme qui coopere, pas celles d'une enclave :
l'allocateur peut recopier un tampon, le noyau peut le pager sur disque, et
`serde_json` fabrique une chaine intermediaire lors de la serialisation. C'est
le maximum atteignable sans dependance supplementaire ni allocateur dedie.

### L'exception : la mise a jour

`pass-cli item update` n'accepte les valeurs que par `--field cle=valeur`. Il
n'existe ni forme `--from-template`, ni lecture sur l'entree standard. Mettre a
jour un mot de passe expose donc la valeur dans `/proc/<pid>/cmdline` pendant
la duree de l'appel.

Trois options etaient possibles :

* **ne pas offrir la mise a jour** — une rotation de mot de passe obligerait a
  sortir de l'application, alors que c'est l'operation la plus courante apres
  la creation ;
* **supprimer puis recreer l'item** — la valeur ne passerait plus par la ligne
  de commande, mais l'item changerait d'identifiant et perdrait son historique,
  et un echec entre les deux appels detruirait le secret ;
* **assumer `--field`, et le dire.**

C'est la troisieme qui est retenue. L'interface affiche l'avertissement sous le
bouton des que l'item vise existe deja, et le bouton lui-meme change de libelle
(« Mettre a jour » plutot que « Enregistrer ») : l'utilisateur sait laquelle
des deux commandes il declenche.

### Creer ou mettre a jour

Proton Pass accepte deux items de meme titre dans un meme coffre. Un doublon
rendrait l'URI `pass://coffre/titre` ambigue et la connexion imprevisible. La
fiche lit donc le coffre avant d'ecrire — automatiquement a l'ouverture — et
choisit selon ce qu'elle y trouve. Tant que la liste n'est pas connue, le
bouton reste inactif et dit pourquoi : ecrire a l'aveugle creerait le doublon
que l'on cherche a eviter.

Une cle SSH n'a pas d'equivalent de `update` cote `pass-cli` : un titre deja
pris est refuse, avec le message qui invite a en choisir un autre.

### Ce qui est ecrit dans l'item

Un item reduit a un titre et un mot de passe serait inexploitable depuis
l'application Proton Pass. Le gabarit reprend donc ce que la fiche connait
deja :

```json
{
  "title": "web-01",
  "username": "root",
  "password": "…",
  "urls": ["ssh://root@10.0.0.4:2222"]
}
```

Les champs vides sont omis plutot qu'envoyes vides : un `username` vide
afficherait une ligne inutile. L'URL `ssh://` fait le lien entre l'item et la
machine — c'est elle qui rend l'item reconnaissable dans une liste.

### Apres l'ecriture

La connexion est rattachee a l'item cree, dans la fiche ouverte **et** dans la
configuration deja enregistree : abandonner la fiche ne doit pas perdre le lien
vers un item pourtant bien cree. Une cle SSH bascule en plus la connexion en
`auth = "agent"` — la cle vit desormais dans le coffre, c'est l'agent Proton
Pass qui la sert, plus un fichier local.

## Consequences

* Le parcours complet — creer une connexion, saisir son mot de passe, se
  connecter — tient dans l'application, sans passer par l'application Proton
  Pass.
* La configuration TOML reste sans aucun secret : la propriete est inchangee,
  et toujours verifiee par un test.
* La surface de contact avec `pass-cli` s'elargit : quatre commandes d'ecriture
  s'ajoutent aux quatre de lecture. Leurs arguments exacts et le passage par
  `stdin` sont verifies par des tests qui pilotent un faux `pass-cli`,
  lequel journalise ce qu'il recoit — y compris l'absence du mot de passe dans
  ses arguments.
* La cle publique d'une cle generee n'est pas affichee par sshpass-gui : la
  lire supposerait de recuperer l'item entier, donc aussi la partie privee, ce
  que le contrat de lecture interdit. Elle se recupere depuis l'application
  Proton Pass, ou avec `ssh-add -L` sur la socket de l'agent du coffre, que le
  panneau lateral affiche.

[`pass::Secret`]: ../../src/pass/secret.rs
