// Onglet Semaine : soupers et brunchs, propositions, choix d'une recette.

import { api } from './api.js';
import { S, reload, recipeById, recipeName } from './store.js';
import { openRecipe } from './recipes.js';
import {
  h, clear, sheet, toast, attempt, actionButton, dot, recipeMeta, lastSeen, capitalize,
  DAYS, DAY_LABELS, DAY_SHORT, PROTEINS,
} from './ui.js';

/** Pour chaque jour cuisiné, combien de soirs il nourrit ; pour chaque soir
 * « restes », le jour d'origine. */
function leftoverMap(week) {
  const feeds = {};
  const from = {};
  let last = null;
  for (const day of DAYS) {
    const slot = week.dinners[day];
    if (slot?.kind === 'recipe') { last = day; feeds[day] = 1; }
    else if (slot?.kind === 'leftovers' && last) { feeds[last] += 1; from[day] = last; }
  }
  return { feeds, from };
}

function dateOf(monday, index) {
  const d = new Date(`${monday}T12:00:00`);
  d.setDate(d.getDate() + index);
  return d;
}

function isoDate(d) {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

export function renderWeek(el) {
  const d = S.data;
  const week = d.week;
  const hasPlan = Object.keys(week.dinners).length > 0 || Object.keys(week.brunch).length > 0;

  const actions = h('div', { class: 'row' },
    actionButton(hasPlan ? 'Compléter / reproposer' : 'Proposer la semaine', () => (hasPlan ? proposeMenu() : propose(true)), 'btn primary'),
  );

  const { feeds, from } = leftoverMap(week);
  const days = h('div', { class: 'days' }, DAYS.map((day, i) => dayCard(day, i, feeds, from)));

  clear(el,
    balance(week),
    actions,
    week.note ? h('p', { class: 'week-note' }, week.note) : null,
    days,
  );
}

/** Équilibre des soupers : nombre de soirs par catégorie, ce qui manque en safran. */
function balance(week) {
  const counts = { fish: 0, poultry: 0, meat: 0, veggie: 0, legumes: 0 };
  for (const slot of Object.values(week.dinners)) {
    const r = slot.kind === 'recipe' ? recipeById(slot.recipe) : null;
    if (r?.protein) counts[r.protein] += 1;
  }
  const vegetarian = counts.veggie + counts.legumes;
  const item = (cls, n, label, short) => h('span', { class: short ? 'short' : '' }, dot(cls), h('strong', { class: 'num' }, n), ` ${label}`);
  return h('div', { class: 'balance', 'aria-label': 'Équilibre des soupers' },
    item('fish', counts.fish, 'poisson', counts.fish < 2),
    item('veggie', vegetarian, `végé${counts.legumes ? ` (dont ${counts.legumes} légumineuses)` : ''}`, vegetarian < 3 || counts.legumes === 0),
    item('poultry', counts.poultry, 'volaille', false),
    item('meat', counts.meat, 'viande', counts.meat > 2),
  );
}

function dayCard(day, index, feeds, from) {
  const d = S.data;
  const date = dateOf(d.monday, index);
  const today = isoDate(date) === d.today;
  const week = d.week;
  const showBrunch = d.settings.brunch_days.includes(day) || week.brunch[day];

  return h('article', { class: `card day${today ? ' today' : ''}`, 'aria-label': `${DAY_LABELS[day]} ${date.getDate()}` },
    h('div', { class: 'day-label' },
      h('div', { class: 'dname' }, DAY_SHORT[day]),
      h('div', { class: 'dnum num' }, date.getDate()),
    ),
    h('div', { class: 'meals' },
      showBrunch ? brunchBlock(day) : null,
      dinnerBlock(day, feeds, from),
    ),
  );
}

function brunchBlock(day) {
  const items = S.data.week.brunch[day] || [];
  return h('div', { class: 'meal' },
    h('div', { class: 'meal-body' },
      h('div', { class: 'meal-kind' }, 'Brunch'),
      h('div', { class: 'brunch-items' },
        items.map((id) => h('button', {
          class: 'item cap',
          onClick: () => openRecipe(id, [{ label: 'Retirer de ce brunch', cls: 'btn danger', fn: () => setBrunch(day, items.filter((x) => x !== id)) }]),
        }, recipeName(id))),
        h('button', { class: 'add', onClick: () => pickRecipe({ meal: 'brunch', day, onPick: (id) => setBrunch(day, [...items, id]) }) }, '+ Ajouter'),
      ),
    ),
  );
}

function dinnerBlock(day, feeds, from) {
  const slot = S.data.week.dinners[day] || { kind: 'empty' };
  const more = h('button', { class: 'mini', 'aria-label': `Options du ${DAY_LABELS[day]}`, onClick: () => dinnerMenu(day) }, '⋯');

  if (slot.kind === 'empty') {
    return h('div', { class: 'meal' },
      h('div', { class: 'meal-body' },
        h('div', { class: 'meal-kind' }, 'Souper'),
        h('button', { class: 'add-slot', onClick: () => pickDinner(day) }, '+ Choisir un souper'),
      ),
    );
  }

  if (slot.kind !== 'recipe') {
    const text = slot.kind === 'free' ? 'Soir libre' : `Restes : ${from[day] ? recipeName(S.data.week.dinners[from[day]].recipe) : 'rien de cuisiné avant'}`;
    return h('div', { class: 'meal placeholder' },
      h('div', { class: 'meal-body' }, h('div', { class: 'meal-kind' }, 'Souper'), h('div', { class: 'meal-name cap' }, text)),
      h('div', { class: 'meal-actions' }, more),
    );
  }

  const recipe = recipeById(slot.recipe);
  const times = feeds[day] || 1;
  return h('div', { class: 'meal' },
    h('div', { class: 'meal-body' },
      h('div', { class: 'meal-kind' }, 'Souper'),
      h('button', { class: 'meal-name cap', onClick: () => openRecipe(slot.recipe) }, recipe ? recipe.name : `Recette introuvable (${slot.recipe})`),
      recipe ? h('div', { class: 'meal-meta' },
        dot(recipe.protein),
        recipeMeta(recipe),
        recipe.favorite ? h('span', { 'aria-label': 'favori' }, ' · ♥') : null,
        times > 1 ? h('span', { class: 'double' }, `× ${times} : prévoir les restes`) : null,
      ) : null,
    ),
    h('div', { class: 'meal-actions' },
      h('button', { class: 'mini', 'aria-label': 'Proposer autre chose', title: 'Proposer autre chose', onClick: () => reroll(day) }, '↻'),
      more,
    ),
  );
}

// ── Actions ─────────────────────────────────────────────────────────────────

async function setDinner(day, slot) {
  await attempt(() => api.setDinner(S.week, day, slot));
  await reload();
}

async function setBrunch(day, ids) {
  await attempt(() => api.setBrunch(S.week, day, ids));
  await reload();
}

async function reroll(day) {
  await attempt(() => api.reroll(S.week, day));
  await reload();
}

async function propose(keep) {
  const done = await attempt(() => api.propose(S.week, keep));
  if (done) toast(keep ? 'Soirs vides complétés' : 'Nouvelle proposition');
  await reload();
}

function proposeMenu() {
  const s = sheet({
    title: 'Proposer',
    body: h('div', { class: 'menu-list' },
      h('button', { onClick: () => { s.close(); propose(true); } }, 'Compléter les soirs et brunchs vides'),
      h('button', { onClick: () => { s.close(); propose(false); } }, 'Tout reproposer'),
      h('button', { class: 'danger', onClick: async () => {
        if (!confirm('Vider tous les soupers et brunchs de la semaine ? La liste de courses (cases cochées, ajouts) est gardée.')) return;
        s.close();
        await attempt(() => api.clearPlan(S.week), 'Semaine vidée');
        reload();
      } }, 'Vider la semaine'),
    ),
  });
}

function dinnerMenu(day) {
  const slot = S.data.week.dinners[day] || { kind: 'empty' };
  const pick = (fn) => () => { s.close(); fn(); };
  const s = sheet({
    title: capitalize(DAY_LABELS[day]),
    body: h('div', { class: 'menu-list' },
      slot.kind === 'recipe' ? h('button', { onClick: pick(() => openRecipe(slot.recipe)) }, 'Voir la recette') : null,
      h('button', { onClick: pick(() => pickDinner(day)) }, 'Choisir une recette…'),
      h('button', { onClick: pick(() => reroll(day)) }, 'Proposer autre chose'),
      day !== 'mon' && slot.kind !== 'leftovers' ? h('button', { onClick: pick(() => setDinner(day, 'leftovers')) }, 'Restes de la veille') : null,
      slot.kind !== 'free' ? h('button', { onClick: pick(() => setDinner(day, 'free')) }, 'Soir libre (dehors, invités…)') : null,
      slot.kind !== 'empty' ? h('button', { class: 'danger', onClick: pick(() => setDinner(day, 'empty')) }, 'Vider ce soir') : null,
    ),
  });
}

function pickDinner(day) {
  pickRecipe({
    meal: 'dinner',
    day,
    extra: [
      day !== 'mon' ? { label: 'Restes de la veille', fn: () => setDinner(day, 'leftovers') } : null,
      { label: 'Soir libre', fn: () => setDinner(day, 'free') },
    ].filter(Boolean),
    onPick: (id) => setDinner(day, id),
  });
}

/** Choix d'une recette : recherche, filtres, favoris en tête. */
export function pickRecipe({ meal, day, extra = [], onPick }) {
  const filters = meal === 'dinner'
    ? [['quick', 'Rapides'], ['fish', 'Poisson'], ['veg', 'Végé'], ['meat', 'Viande & volaille'], ['fav', 'Favoris']]
    : [['sweet', 'Sucré'], ['savory', 'Salé'], ['fav', 'Favoris']];
  const active = new Set();
  const search = h('input', { type: 'search', placeholder: 'Chercher une recette ou un ingrédient', 'aria-label': 'Chercher' });
  const list = h('div');
  const chips = h('div', { class: 'chips' }, filters.map(([key, label]) => {
    const chip = h('button', { class: 'chip', 'aria-pressed': 'false' }, label);
    chip.addEventListener('click', () => {
      active.has(key) ? active.delete(key) : active.add(key);
      chip.setAttribute('aria-pressed', String(active.has(key)));
      draw();
    });
    return chip;
  }));

  const quick = S.data.settings.quick_minutes;
  const taken = new Set(Object.values(S.data.week.dinners).filter((s) => s.kind === 'recipe').map((s) => s.recipe));
  const fold = (s) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();

  function matches(r) {
    if (r.meal !== meal) return false;
    if (meal === 'brunch' && r.brunch_kind === 'base' && !active.size) return true;
    const q = fold(search.value.trim());
    if (q && !fold(r.name).includes(q) && !r.ingredients.some((i) => fold(i.name).includes(q))) return false;
    if (active.has('quick') && r.minutes > quick) return false;
    if (active.has('fish') && r.protein !== 'fish') return false;
    if (active.has('veg') && !['veggie', 'legumes'].includes(r.protein)) return false;
    if (active.has('meat') && !['meat', 'poultry'].includes(r.protein)) return false;
    if (active.has('fav') && !r.favorite) return false;
    if (active.has('sweet') && r.brunch_kind !== 'sweet') return false;
    if (active.has('savory') && r.brunch_kind !== 'savory') return false;
    return true;
  }

  function draw() {
    const recipes = S.data.recipes.filter(matches).sort((a, b) =>
      (a.excluded - b.excluded) || (b.favorite - a.favorite) || a.name.localeCompare(b.name, 'fr'));
    clear(list, recipes.length ? recipes.map((r) => h('button', { class: 'picker-row', onClick: () => { s.close(); onPick(r.id); } },
      dot(r.protein),
      h('div', { class: 'grow' },
        h('div', { class: 'title cap' }, r.name, r.favorite ? ' ♥' : ''),
        h('div', { class: 'meta' }, [recipeMeta(r), lastSeen(S.data.history, r.id), taken.has(r.id) ? 'déjà cette semaine' : '', r.excluded ? 'exclue' : ''].filter(Boolean).join(' · ')),
      ),
    )) : h('p', { class: 'dim' }, 'Aucune recette ne correspond.'));
  }
  search.addEventListener('input', draw);

  const s = sheet({
    title: meal === 'dinner' ? `Souper du ${DAY_LABELS[day]}` : `Brunch du ${DAY_LABELS[day]}`,
    body: [
      extra.length ? h('div', { class: 'row' }, extra.map((x) => h('button', { class: 'btn small', onClick: () => { s.close(); x.fn(); } }, x.label))) : null,
      search,
      chips,
      list,
    ],
  });
  draw();
}

export { PROTEINS };
