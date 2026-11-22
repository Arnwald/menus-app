// Onglet Recettes : recherche, fiche détaillée, ajout et modification.

import { api } from './api.js';
import { S, reload, recipeById } from './store.js';
import {
  h, clear, sheet, spinner, toast, attempt, actionButton, dot, recipeMeta, lastSeen, capitalize,
  PROTEINS, STARCHES, BRUNCH_KINDS, DAYS, DAY_LABELS,
} from './ui.js';

const FILTERS = [
  ['dinner', 'Soupers'], ['brunch', 'Brunch'], ['quick', 'Rapides'], ['fish', 'Poisson'], ['veg', 'Végé'],
  ['legumes', 'Légumineuses'], ['meat', 'Viande & volaille'], ['keeps', 'Se garde'], ['fav', 'Favoris'], ['excluded', 'Exclues'],
];

const fold = (s) => s.normalize('NFD').replace(/[̀-ͯ]/g, '').toLowerCase();

export function renderRecipes(el) {
  const f = S.recipeFilter;
  const search = h('input', { type: 'search', placeholder: 'Chercher une recette ou un ingrédient', value: f.query, 'aria-label': 'Chercher' });
  const list = h('div', { class: 'card recipe-list' });
  const count = h('div', { class: 'dim', style: 'font-size:13px;padding:0 2px' });

  const chips = h('div', { class: 'chips' }, FILTERS.map(([key, label]) => {
    const chip = h('button', { class: 'chip', 'aria-pressed': String(f.tags.includes(key)) }, label);
    chip.addEventListener('click', () => {
      f.tags = f.tags.includes(key) ? f.tags.filter((t) => t !== key) : [...f.tags, key];
      chip.setAttribute('aria-pressed', String(f.tags.includes(key)));
      draw();
    });
    return chip;
  }));

  function draw() {
    const q = fold(f.query.trim());
    const quick = S.data.settings.quick_minutes;
    const t = new Set(f.tags);
    const shown = S.data.recipes.filter((r) => {
      if (q && !fold(r.name).includes(q) && !r.ingredients.some((i) => fold(i.name).includes(q))) return false;
      if (t.has('dinner') && r.meal !== 'dinner') return false;
      if (t.has('brunch') && r.meal !== 'brunch') return false;
      if (t.has('quick') && r.minutes > quick) return false;
      if (t.has('fish') && r.protein !== 'fish') return false;
      if (t.has('veg') && !['veggie', 'legumes'].includes(r.protein)) return false;
      if (t.has('legumes') && r.protein !== 'legumes') return false;
      if (t.has('meat') && !['meat', 'poultry'].includes(r.protein)) return false;
      if (t.has('keeps') && !r.keeps) return false;
      if (t.has('fav') && !r.favorite) return false;
      // Les recettes exclues n'apparaissent qu'avec le filtre « Exclues ».
      if (t.has('excluded') !== r.excluded) return false;
      return true;
    }).sort((a, b) => a.name.localeCompare(b.name, 'fr'));

    count.textContent = `${shown.length} recette${shown.length > 1 ? 's' : ''}`;
    clear(list, shown.length ? shown.map((r) => h('button', { class: `recipe-row${r.excluded ? ' excluded' : ''}`, onClick: () => openRecipe(r.id) },
      dot(r.protein),
      h('div', { class: 'grow' },
        h('div', { class: 'title cap' }, r.name),
        h('div', { class: 'meta' }, [recipeMeta(r), lastSeen(S.data.history, r.id)].filter(Boolean).join(' · ')),
      ),
      r.favorite ? h('span', { class: 'fav', 'aria-label': 'favori' }, '♥') : null,
    )) : h('p', { class: 'dim', style: 'padding:14px' }, 'Aucune recette ne correspond.'));
  }
  search.addEventListener('input', () => { f.query = search.value; draw(); });

  clear(el,
    h('div', { class: 'row' }, h('button', { class: 'btn primary', onClick: () => recipeForm() }, 'Ajouter une recette')),
    search,
    chips,
    count,
    list,
  );
  draw();
}

// ── Fiche ───────────────────────────────────────────────────────────────────

/** Ouvre la fiche d'une recette ; `extra` : actions propres au contexte. */
export async function openRecipe(id, extra = []) {
  const s = sheet({ title: recipeById(id)?.name || 'Recette', body: spinner('Chargement…') });
  const data = await attempt(() => api.recipe(id));
  if (!data) { s.close(); return; }
  const r = data.recipe;
  const people = `${S.data.settings.adults} adulte${S.data.settings.adults > 1 ? 's' : ''}${S.data.settings.children ? ` + ${S.data.settings.children} enfant${S.data.settings.children > 1 ? 's' : ''}` : ''}`;

  const toggle = (field, label) => actionButton(label, async () => {
    await attempt(() => api.patchRecipe(r.id, { [field]: !r[field] }));
    s.close();
    await reload();
    openRecipe(id, extra);
  }, 'btn small');

  s.setTitle(capitalize(r.name));
  s.setBody(
    h('div', { class: 'meal-meta' }, dot(r.protein), recipeMeta(r), r.starch ? ` · ${STARCHES[r.starch] || r.starch}` : '', lastSeen(S.data.history, r.id) ? ` · ${lastSeen(S.data.history, r.id)}` : ''),
    r.excluded ? h('div', { class: 'notice' }, 'Exclue des propositions.') : null,
    h('div', {},
      h('h3', { class: 'section-title' }, `Ingrédients pour ${people}`),
      h('ul', { class: 'ingredients' }, data.lines.map((l) => h('li', { class: l.pantry ? 'pantry' : '' }, h('span', { class: 'label cap' }, l.name), h('span', { class: 'amount' }, l.amount)))),
    ),
    r.steps.length ? h('div', {}, h('h3', { class: 'section-title' }, 'Préparation'), h('ol', { class: 'steps' }, r.steps.map((st) => h('li', {}, st)))) : null,
    r.notes ? h('div', { class: 'notice info' }, r.notes) : null,
    h('div', { class: 'row' },
      toggle('favorite', r.favorite ? '♥ Favori' : '♡ Favori'),
      toggle('excluded', r.excluded ? 'Réintégrer aux propositions' : 'Exclure des propositions'),
    ),
  );
  s.setFoot(
    extra.map((x) => h('button', { class: x.cls || 'btn', onClick: () => { s.close(); x.fn(); } }, x.label)),
    h('button', { class: 'btn', onClick: () => { s.close(); scheduleRecipe(r); } }, 'Mettre au menu'),
    h('button', { class: 'btn', onClick: () => { s.close(); editMenu(r); } }, 'Modifier'),
  );
}

/** Place une recette sur un soir (souper) ou un brunch de la semaine affichée. */
function scheduleRecipe(r) {
  const week = S.data.week;
  const brunch = r.meal === 'brunch';
  const s = sheet({
    title: `Mettre au menu : semaine ${week.week.split('W')[1]}`,
    body: h('div', { class: 'menu-list' }, DAYS.map((day) => {
      const slot = week.dinners[day];
      const current = brunch
        ? (week.brunch[day] || []).length ? `${(week.brunch[day] || []).length} élément(s)` : 'vide'
        : slot?.kind === 'recipe' ? recipeById(slot.recipe)?.name || slot.recipe : slot?.kind === 'leftovers' ? 'restes' : slot?.kind === 'free' ? 'libre' : 'vide';
      return h('button', { onClick: async () => {
        s.close();
        if (brunch) await attempt(() => api.setBrunch(week.week, day, [...(week.brunch[day] || []), r.id]), 'Ajouté au brunch');
        else await attempt(() => api.setDinner(week.week, day, r.id), 'Mis au menu');
        reload();
      } }, h('strong', {}, capitalize(DAY_LABELS[day])), h('span', { class: 'dim' }, ` — ${current}`));
    })),
  });
}

function editMenu(r) {
  const s = sheet({
    title: capitalize(r.name),
    body: h('div', { class: 'menu-list' },
      h('button', { onClick: () => { s.close(); recipeForm(r); } }, 'Modifier'),
      h('button', { class: 'danger', onClick: async () => {
        if (!confirm(`Supprimer « ${r.name} » ? Elle sera retirée des semaines à venir.`)) return;
        s.close();
        await attempt(() => api.deleteRecipe(r.id), 'Recette supprimée');
        reload();
      } }, 'Supprimer la recette'),
    ),
  });
}

// ── Formulaire manuel ───────────────────────────────────────────────────────

/** Création (`r` absent) ou modification d'une recette à la main. */
function recipeForm(r) {
  const select = (options, value) => h('select', {}, Object.entries(options).map(([k, label]) => h('option', { value: k, selected: k === value }, label)));
  const name = h('input', { type: 'text', value: r?.name || '', placeholder: 'Gratin de chou-fleur' });
  const meal = select({ dinner: 'Souper', brunch: 'Brunch' }, r?.meal || 'dinner');
  const protein = select(PROTEINS, r?.protein || 'veggie');
  const kind = select({ sweet: 'Sucré', savory: 'Salé', base: 'Base (pain, beurre…)' }, r?.brunch_kind || 'savory');
  const starch = select({ '': '—', ...STARCHES }, r?.starch || '');
  const minutes = h('input', { type: 'number', min: 1, value: r?.minutes || 30, inputmode: 'numeric' });
  const keeps = h('input', { type: 'checkbox', checked: r?.keeps });
  const ingredients = h('textarea', { placeholder: 'Une ligne par ingrédient, quantité pour UN adulte :\nchou-fleur 250 g\ndemi-crème 50 ml\ngruyère râpé 30 g\nœufs 1 pce' });
  ingredients.value = r ? r.ingredients.map((l) => `${l.name} ${l.qty} ${l.unit}`).join('\n') : '';
  const steps = h('textarea', { placeholder: 'Une étape par ligne' });
  steps.value = r ? r.steps.join('\n') : '';

  const proteinField = h('label', { class: 'field' }, 'Catégorie', protein);
  const kindField = h('label', { class: 'field' }, 'Type de brunch', kind);
  const sync = () => { proteinField.hidden = meal.value !== 'dinner'; kindField.hidden = meal.value !== 'brunch'; };
  meal.addEventListener('change', sync);
  sync();

  const s = sheet({
    title: r ? 'Modifier la recette' : 'Nouvelle recette',
    body: [
      h('label', { class: 'field' }, 'Nom', name),
      h('div', { class: 'row' }, h('label', { class: 'field grow' }, 'Repas', meal), h('label', { class: 'field grow' }, 'Durée totale (min)', minutes)),
      proteinField,
      kindField,
      h('label', { class: 'field' }, 'Féculent', starch),
      h('label', { class: 'check' }, keeps, 'Se réchauffe bien (peut servir de restes)'),
      h('label', { class: 'field' }, 'Ingrédients', h('span', { class: 'hint' }, 'Unités : g, ml, dl, kg, pce, gousse, botte, tranche, cs, cc, pincée. Un ingrédient inconnu rejoint le rayon « Divers ».'), ingredients),
      h('label', { class: 'field' }, 'Préparation', steps),
    ],
    foot: [
      h('button', { class: 'btn', onClick: () => s.close() }, 'Annuler'),
      actionButton('Enregistrer', async () => {
        const draft = {
          name: name.value.trim(),
          meal: meal.value,
          protein: meal.value === 'dinner' ? protein.value : undefined,
          brunch_kind: meal.value === 'brunch' ? kind.value : undefined,
          starch: starch.value || undefined,
          minutes: Number(minutes.value) || 30,
          active_minutes: r && Number(minutes.value) === r.minutes ? r.active_minutes : undefined,
          keeps: keeps.checked,
          months: r?.months,
          ingredients: ingredients.value.split('\n').map((l) => l.trim()).filter(Boolean),
          steps: steps.value.split('\n').map((l) => l.trim()).filter(Boolean),
          notes: r?.notes,
        };
        const out = await attempt(() => (r ? api.replaceRecipe(r.id, draft) : api.createRecipes([draft])));
        if (!out) return;
        s.close();
        if (out.warnings?.length) toast(out.warnings.join(' · '));
        else toast(r ? 'Recette modifiée' : 'Recette ajoutée');
        await reload();
        openRecipe(r ? r.id : out.created[0].id);
      }, 'btn primary'),
    ],
  });
}

export { BRUNCH_KINDS };
