# menus

Les soupers de la semaine, les brunchs du week-end et la liste de courses Coop /
Migros qui va avec. Une base JSON en clair sur le disque, une API HTTP, une
interface web pensée pour le téléphone, et un serveur MCP pour planifier avec
Claude (application de bureau ou Claude Code), sans clé API.

## Ce que ça fait

- **Semaine** : un souper par soir, un brunch le samedi et le dimanche. Un bouton
  propose la semaine entière, ↻ change un seul soir. Chaque soir peut aussi être
  « restes de la veille » (le plat de la veille est alors compté double dans la
  liste) ou « libre ».
- **Propositions sans Claude** : tirage pondéré sous contraintes. Plats rapides
  en semaine (35 min par défaut), plat plus long le dimanche, plat qui se garde
  la veille d'un soir « restes », de saison, pas servi ces deux dernières
  semaines, et l'équilibre : 2 poissons, au moins 3 soirs végétariens dont 1 de
  légumineuses, au plus 2 viandes et 2 volailles, féculents variés. Les favoris
  reviennent plus souvent, les recettes exclues jamais.
- **Avec Claude**, dans l'onglet « Claude » de l'app (clé API sur le serveur)
  ou par le serveur MCP `menus mcp` depuis Claude Desktop : proposer une semaine à partir
  d'envies (« il reste des courgettes, jeudi on mange dehors »), en créant au
  besoin quelques recettes ; transcrire une recette collée ou photographiée ;
  modifier une recette (« sans crème », « plus rapide »). Tout ce que Claude
  change apparaît aussitôt sur les téléphones.
- **Courses** : ingrédients additionnés sur la semaine, multipliés pour le foyer
  (2 adultes + 1 enfant à 0,6 portion par défaut), arrondis à des quantités qu'on
  achète, rangés dans l'ordre des rayons. Les fonds de placard (huile, épices…)
  sont à part, « à vérifier ». Ajouts à la main (lait, papier toilette). Les
  cases cochées se propagent en direct d'un téléphone à l'autre. Filtre par
  jours pour deux tournées de courses ; export texte.
- **Recettes** : 44 soupers et 13 éléments de brunch au départ, quantités pour un
  adulte. Recherche par nom ou ingrédient, favoris, exclusion, ajout à la main.

## Démarrer

```bash
cargo run -- serve            # http://localhost:8080
```

Au premier lancement, `data/` reçoit les réglages par défaut, le catalogue
d'ingrédients et les recettes de départ (`seed/*.toml`, embarqués dans le binaire).

## La base

```
data/
  settings.json         foyer, soirs longs / restes / libres, brunchs, ordre des rayons
  ingredients.json      catalogue : rayon, unité de référence, placard
  recipes.json          les recettes (quantités pour UN adulte)
  weeks/2026-W47.json   soupers, brunchs, cases cochées, ajouts
```

Tout est lisible et modifiable à la main ; le serveur surveille le dossier et les
navigateurs ouverts se mettent à jour. Une recette :

```json
{
  "id": "curry-lentilles-corail",
  "name": "Curry de lentilles corail au lait de coco",
  "meal": "dinner",
  "minutes": 30, "active_minutes": 30,
  "protein": "legumes", "starch": "rice", "keeps": true,
  "ingredients": [{ "name": "lentilles corail", "qty": 70.0, "unit": "g" }, …],
  "steps": ["…"]
}
```

La liste de courses additionne par **nom et unité** : une recette doit employer
le nom et l'unité du catalogue (`ingredients.json`). L'API s'en charge : elle
reconnaît `pommes de terre` pour `pomme de terre`, convertit `kg`/`dl`/`l`, et
range un ingrédient inconnu dans le rayon qu'on lui indique (ou « Divers »).

## Ligne de commande

```bash
menus serve [--port 8080] [--host 0.0.0.0]
menus propose [--week prochaine] [--replace] [--dry-run]
menus list [--week 2026-W47] [--days lun,mar,mer]
menus mcp [--url http://localhost:8080] [--token …]   # serveur MCP (stdio)
menus setup-claude [--url …] [--token …]               # le déclare dans Claude Desktop
```

`--data <dossier>` (ou `MENUS_DATA_DIR`) choisit la base. `--week` accepte
`2026-W47`, une date, `courante`, `prochaine`.

## API

Le détail est dans [CLAUDE.md](CLAUDE.md). Quelques exemples :

```bash
curl -X POST localhost:8080/api/weeks/prochaine/propose            # proposition locale
curl -X PUT  localhost:8080/api/weeks/2026-W47/dinners/jeu -d '"leftovers"' -H 'content-type: application/json'
curl "localhost:8080/api/weeks/courante/list?format=text"           # liste à coller
curl -X POST localhost:8080/api/recipes -H 'content-type: application/json' -d '{
  "name": "Gratin de chou-fleur", "protein": "veggie", "minutes": 40,
  "ingredients": ["chou-fleur 250 g", "demi-crème 50 ml", "gruyère râpé 30 g"]
}'
```

## Déploiement (TrueNAS + Dockge)

Même schéma que `sagex-hours-app` : un push sur `main` lance les tests puis
publie `ghcr.io/arnwald/menus-app:latest` ([`image.yml`](.github/workflows/image.yml)).

1. **Dataset** — en créer un pour la base, `Instances/configs/menus`,
   propriétaire `apps` (UID 568).
2. **Paquet GHCR** — dépôt privé, image privée : sur le NAS,
   `docker login ghcr.io -u <compte> -p <PAT avec read:packages>`, ou rendre le
   paquet public.
3. **Pile Dockge** — coller [`deploy/compose.yaml`](deploy/compose.yaml), adapter
   le port, le chemin du dataset et `MENUS_TOKEN`.

Sans registre, depuis le dépôt cloné sur le NAS : `docker compose up -d --build`
avec le [`docker-compose.yml`](docker-compose.yml) de la racine.

### Au magasin

Le NAS est à la maison, les courses non. Pour ouvrir la liste depuis le Coop,
il faut joindre le NAS hors du réseau local : l'app Tailscale de TrueNAS SCALE
et Tailscale sur les deux téléphones suffisent (`http://<nas>:8098`).
`MENUS_TOKEN` devient alors indispensable. Sans accès distant, « Copier la
liste » donne un texte à coller dans une note avant de partir.

Sur iPhone, *Partager → Sur l'écran d'accueil* donne une icône qui ouvre l'app
en plein écran. Le jeton est demandé une fois par appareil.

## Claude (MCP)

`menus mcp` est un serveur MCP en stdio, comme `LinFIR --mcp` : Claude le lance
sur le Mac, et il relaie chaque appel à une instance `menus serve` par son API
HTTP (locale, ou celle du NAS). La base reste au même endroit, l'usage passe par
l'abonnement Claude, et il n'y a pas de clé API à gérer.

Outils : `get_week`, `list_recipes`, `get_recipe`, `list_ingredients`,
`plan_week` (toute la semaine, nouvelles recettes comprises), `set_dinner`,
`set_brunch`, `propose_week`, `reroll_dinner`, `add_recipes`, `update_recipe`,
`set_recipe_flags`, `delete_recipe`, `get_shopping_list`,
`add_to_shopping_list`, `update_settings`. Les consignes du serveur donnent à
Claude les règles d'équilibre et de quantités.

```bash
cargo install --path .        # installe ~/.cargo/bin/menus
```

Claude Desktop : `menus setup-claude` ajoute (ou met à jour) l'entrée `menus` dans
`claude_desktop_config.json`, sans toucher aux autres serveurs, et garde l'ancien
fichier en `.bak`. Relancer ensuite Claude Desktop.

```bash
menus setup-claude                                         # instance locale
menus setup-claude --url http://<nas>:8098 --token …       # instance du NAS
```

En une ligne, depuis l'archive téléchargée (installe, déclare, lance) :

```bash
cd ~/Projects && tar -xzf "$(ls -t ~/Downloads/menus-app*.tar.gz | head -1)" && cd menus-app && cargo install --path . && menus setup-claude && menus serve
```

Claude Code :

```bash
claude mcp add menus -e MENUS_TOKEN=… -- menus mcp --url http://<nas>:8098
```

Pour l'instance du NAS, remplacer l'adresse par `http://<nas>:8098` et
renseigner le jeton. `menus serve` doit tourner : sinon les outils répondent
qu'il ne répond pas, avec l'adresse essayée.
## Claude dans l'app

L'onglet « Claude » apparaît quand le serveur a une clé API (`ANTHROPIC_API_KEY`).
Chacun discute depuis son téléphone, sans compte Claude : le serveur appelle l'API
Messages avec les mêmes outils que `menus mcp`, servis en mémoire et relayés vers sa
propre API, puis renvoie la réponse. La clé ne quitte pas le serveur. On peut joindre
une photo de recette, réduite sur le téléphone avant l'envoi. La conversation est
gardée sur l'appareil (« Nouvelle conversation » la vide).

| Variable | Rôle |
|---|---|
| `ANTHROPIC_API_KEY` | clé de l'organisation Console ; sans elle, pas d'onglet |
| `MENUS_CLAUDE_MODEL` | modèle (défaut `claude-sonnet-5-5`) |

Les abonnements Max et Ultra donnent des crédits API mensuels : sur claude.ai,
Settings > Billing > API credits > « Link organization », puis créer la clé dans
cette organisation sur platform.claude.com. Quand les crédits sont épuisés, l'API
refuse et le chat le dit, sans facturation surprise.

## Développement

```bash
cargo test          # base de départ, quantités, liste, propositions, semaines ISO, outils MCP
cargo run -- serve  # en debug, web/ est relu à chaque requête
```
