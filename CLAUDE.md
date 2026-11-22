# menus — mémo de pilotage

App de menus familiaux et de liste de courses. Base JSON en clair dans `data/`,
API HTTP, UI web (modules ES, sans build).

## Trois façons d'agir sur la base

0. **Les outils MCP** (`menus mcp`, voir le README) quand ils sont branchés : ils
   relaient vers l'API et renvoient des résumés lisibles. À préférer à curl.
1. **L'API** (`http://localhost:8080`, ou l'adresse du NAS) — la seule qui marche
   à distance, et celle à privilégier : elle valide les recettes contre le
   catalogue, résout les noms, convertit les unités.
2. **Les fichiers** `data/*.json` en édition directe — le serveur surveille le
   dossier et l'interface se met à jour. Rien n'est validé : un nom d'ingrédient
   ou une unité qui ne correspond pas au catalogue fera une ligne à part dans la
   liste de courses.

Si l'instance exige un jeton : `-H "X-Auth-Token: $MENUS_TOKEN"`.

**Avant toute chose**, vérifier que le serveur répond :
`curl -s -m 2 localhost:8080/api/settings`. Sinon, depuis la racine du projet :
`cargo run --quiet -- serve` (au premier plan — le lancer en tâche de fond).

## Le modèle

- **Recette** : `{id, name, meal: dinner|brunch, brunch_kind: sweet|savory|base,
  minutes, active_minutes, protein: fish|poultry|meat|veggie|legumes, starch,
  keeps, months, ingredients: [{name, qty, unit}], steps, favorite, excluded}`.
  Quantités pour **un adulte**. Unités : `g ml pce gousse botte tranche cs cc pincée`.
- **Semaine** `2026-W47` : `dinners.{mon..sun}` = `{"kind":"recipe","recipe":id}`,
  `{"kind":"leftovers"}` (restes du dernier plat cuisiné avant), `{"kind":"free"}` ;
  `brunch.{sat,sun}` = liste d'ids ; `checked` (clés cochées) ; `extras`.
- **Réglages** : foyer (`adults`, `children`, `child_factor`), `long_days`,
  `leftover_days`, `free_days`, `brunch_days`, `quick_minutes`, `avoid_weeks`,
  `aisles` (ordre du magasin), `store_name`.

Partout, une semaine s'écrit `2026-W47`, `2026-11-16`, `courante` ou `prochaine` ;
un jour `mon`/`lun`/`lundi`.

## Planifier

```bash
curl -s "localhost:8080/api/state?week=prochaine" | jq '.week, .history'   # tout l'état d'une semaine
curl -X POST localhost:8080/api/weeks/prochaine/propose -H 'content-type: application/json' -d '{"keep":true}'
curl -X POST localhost:8080/api/weeks/2026-W48/reroll/tue

# Un soir : un id, "leftovers", "free" ou "empty"
curl -X PUT localhost:8080/api/weeks/2026-W48/dinners/mer -H 'content-type: application/json' -d '"chili-sin-carne"'
curl -X PUT localhost:8080/api/weeks/2026-W48/dinners/jeu -H 'content-type: application/json' -d '"leftovers"'
curl -X PUT localhost:8080/api/weeks/2026-W48/brunch/sam -H 'content-type: application/json' \
  -d '{"recipes":["base-brunch","crepes","shakshuka"]}'
curl -X DELETE localhost:8080/api/weeks/2026-W48/plan        # vide soupers et brunchs
```

Pour proposer une semaine soi-même (plutôt que `propose`), écrire tous les soirs
d'un coup avec `apply`, qui peut aussi créer des recettes nouvelles (ids
provisoires `new-1`, remplacés à la création) :

```bash
curl -X POST localhost:8080/api/weeks/2026-W48/apply -H 'content-type: application/json' -d '{
  "dinners": {"mon": {"kind":"recipe","recipe":"pates-thon-tomate"}, "tue": {"kind":"recipe","recipe":"new-1"},
              "wed": {"kind":"recipe","recipe":"chili-sin-carne"}, "thu": {"kind":"leftovers"}, "fri": {"kind":"free"}},
  "brunch": {"sat": ["base-brunch","crepes","shakshuka"]},
  "new_recipes": [{"id":"new-1","name":"Gratin de courgettes à la feta","protein":"veggie","minutes":30,
                   "ingredients":["courgette 250 g","feta 50 g","pomme de terre 150 g"],"steps":["…"]}],
  "note": "Courgettes à finir mardi."
}'
```

`apply` **remplace** tous les soupers et brunchs de la semaine. Règles d'équilibre
à respecter : 2 poissons, au moins 3 soirs végétariens dont 1 de légumineuses,
au plus 2 viandes et 2 volailles, plats ≤ `quick_minutes` sauf soirs longs, plat
qui se garde la veille d'un soir « restes », de saison, pas servi dans les
`avoid_weeks` semaines (voir `history` dans `/api/state` : écart en semaines).

## Recettes

```bash
curl localhost:8080/api/recipes
curl localhost:8080/api/recipes/curry-lentilles-corail       # + quantités pour le foyer
curl -X POST localhost:8080/api/recipes -H 'content-type: application/json' -d '[{…}, {…}]'
curl -X PUT localhost:8080/api/recipes/<id> -H 'content-type: application/json' -d '{…recette complète…}'
curl -X PATCH localhost:8080/api/recipes/<id> -H 'content-type: application/json' -d '{"favorite":true}'
curl -X DELETE localhost:8080/api/recipes/<id>               # la retire aussi des semaines à venir
```

Une ligne d'ingrédient s'écrit en objet `{name, qty, unit, aisle?}` ou en texte :
`"oignon 0.5 pce"`, `"200 g de farine"`, `"2 oignons"`, `"1,5 dl lait"`. Le nom
est rapproché du catalogue (casse, accents, pluriel) ; un ingrédient inconnu est
ajouté au catalogue dans `aisle` (id de rayon) ou « Divers ». La réponse liste
les remarques (`warnings`) : les lire.

```bash
curl localhost:8080/api/ingredients                           # avec le nombre de recettes qui l'emploient
curl -X PUT "localhost:8080/api/ingredients/menthe fraîche" -H 'content-type: application/json' \
  -d '{"aisle":"fl","unit":"botte"}'
```

## Courses

```bash
curl "localhost:8080/api/weeks/courante/list?format=text"
curl "localhost:8080/api/weeks/courante/list?days=lun,mar,mer"   # JSON, une partie de la semaine
curl -X POST localhost:8080/api/weeks/courante/extras -H 'content-type: application/json' -d '{"text":"lait, papier toilette"}'
curl -X POST localhost:8080/api/weeks/courante/check -H 'content-type: application/json' -d '{"key":"oignon-pce","done":true}'
curl -X POST localhost:8080/api/weeks/courante/uncheck-all
```

## Réglages

```bash
curl -X PATCH localhost:8080/api/settings -H 'content-type: application/json' \
  -d '{"leftover_days":["thu"],"long_days":["sun"],"free_days":["fri"]}'
```

## Code

`src/model.rs` types · `src/store.rs` lecture/écriture atomique · `src/recipes.rs`
brouillons, lignes en texte libre, validation · `src/shopping.rs` liste,
arrondis, export texte · `src/planner.rs` propositions locales · `src/mcp.rs`
serveur MCP (relais vers l'API) · `src/seed.rs` + `seed/*.toml` base de départ · `src/api/`
routes · `web/` interface.

`cargo test` couvre la base de départ (chaque ingrédient au catalogue, mêmes
unités), les quantités, la liste, 1000 propositions tirées au hasard, les
semaines ISO et les outils MCP.
