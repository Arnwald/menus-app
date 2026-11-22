// Onglet Courses : liste par rayon, cases partagées en direct, ajouts à la main.

import { api } from './api.js';
import { S, reload, render, shoppingDays, setShoppingDays } from './store.js';
import { h, clear, sheet, toast, attempt, capitalize, DAYS, DAY_SHORT } from './ui.js';

export function renderShopping(el) {
  const d = S.data;
  const list = d.list;
  const selected = shoppingDays();
  const hasPlan = Object.keys(d.week.dinners).length > 0 || Object.keys(d.week.brunch).length > 0;

  const dayChips = h('div', { class: 'chips', role: 'group', 'aria-label': 'Repas couverts' },
    DAYS.map((day) => {
      const on = selected.length === 0 || selected.includes(day);
      return h('button', { class: 'chip', 'aria-pressed': String(on && selected.length > 0), onClick: () => toggleDay(day) }, DAY_SHORT[day]);
    }),
  );

  const extraInput = h('input', { type: 'text', placeholder: 'Ajouter : lait, papier toilette, croquettes…', 'aria-label': 'Article à ajouter', enterkeyhint: 'done' });
  const addExtra = async () => {
    const text = extraInput.value.trim();
    if (!text) return;
    extraInput.value = '';
    await attempt(() => api.addExtras(S.week, text));
    extraInput.blur();
    reload();
  };
  extraInput.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); addExtra(); } });

  const pct = list.total ? Math.round((list.checked / list.total) * 100) : 0;

  clear(el,
    h('div', { class: 'progress' },
      h('div', { class: 'bar', role: 'progressbar', 'aria-valuemin': 0, 'aria-valuemax': list.total, 'aria-valuenow': list.checked }, h('i', { style: `width:${pct}%` })),
      h('span', { class: 'count num' }, `${list.checked} / ${list.total}`),
    ),
    h('div', {},
      h('div', { class: 'dim', style: 'font-size:13px;margin:0 2px 6px' }, selected.length ? 'Repas couverts par cette liste :' : 'Toute la semaine. Pour deux tournées de courses, choisissez les jours :'),
      dayChips,
    ),
    h('div', { class: 'add-extra' }, extraInput, h('button', { class: 'btn primary', onClick: addExtra }, 'Ajouter')),
    !hasPlan ? h('div', { class: 'notice info' }, 'Aucun repas prévu cette semaine : passez par l\'onglet Semaine pour en proposer.') : null,
    list.missing.length ? h('div', { class: 'notice' }, `Recettes introuvables, ignorées : ${list.missing.join(', ')}`) : null,
    extrasBlock(list.extras),
    list.sections.map((section) => aisleBlock(section)),
    list.pantry.length ? pantryBlock(list.pantry) : null,
    h('div', { class: 'row' },
      h('button', { class: 'btn', onClick: copyList }, 'Copier la liste'),
      list.checked ? h('button', { class: 'btn', onClick: uncheckAll }, 'Tout décocher') : null,
    ),
  );
}

function toggleDay(day) {
  let days = shoppingDays();
  if (days.length === 0) days = [day];
  else if (days.includes(day)) days = days.filter((d) => d !== day);
  else days = [...days, day];
  days.sort((a, b) => DAYS.indexOf(a) - DAYS.indexOf(b));
  setShoppingDays(days);
  reload();
}

function itemRow(item) {
  const box = h('input', { type: 'checkbox', checked: item.checked, 'aria-label': item.name });
  const row = h('label', { class: `item-row${item.checked ? ' done' : ''}${item.unknown ? ' unknown' : ''}` },
    box,
    h('div', {},
      h('div', { class: 'name' }, capitalize(item.name)),
      item.used_in.length ? h('div', { class: 'uses' }, item.used_in.join(' · ')) : null,
    ),
    h('span', { class: 'amount' }, item.amount),
  );
  box.addEventListener('change', async () => {
    // Coche immédiate ; le serveur confirme et prévient l'autre téléphone.
    row.classList.toggle('done', box.checked);
    item.checked = box.checked;
    S.data.list.checked += box.checked ? 1 : -1;
    const ok = await attempt(() => api.check(S.week, item.key, box.checked));
    if (!ok) reload();
  });
  return row;
}

function aisleBlock(section) {
  // Les articles cochés descendent en bas de leur rayon.
  const items = [...section.items].sort((a, b) => a.checked - b.checked);
  return h('div', { class: 'card aisle' }, h('h3', {}, section.name), items.map(itemRow));
}

function extrasBlock(extras) {
  if (!extras.length) return null;
  const items = [...extras].sort((a, b) => a.done - b.done);
  return h('div', { class: 'card aisle' },
    h('h3', {}, 'Ajouts'),
    items.map((extra) => {
      const box = h('input', { type: 'checkbox', checked: extra.done, 'aria-label': extra.text });
      const row = h('label', { class: `item-row${extra.done ? ' done' : ''}` },
        box,
        h('div', { class: 'name' }, capitalize(extra.text)),
        h('button', { class: 'del', 'aria-label': `Supprimer ${extra.text}`, onClick: async (e) => {
          e.preventDefault();
          await attempt(() => api.deleteExtra(S.week, extra.id));
          reload();
        } }, '×'),
      );
      box.addEventListener('change', async () => {
        row.classList.toggle('done', box.checked);
        const ok = await attempt(() => api.patchExtra(S.week, extra.id, { done: box.checked }));
        if (!ok) reload();
      });
      return row;
    }),
  );
}

function pantryBlock(items) {
  const left = items.filter((i) => !i.checked).length;
  return h('details', { class: 'card pantry' },
    h('summary', {}, `Placard : à vérifier (${left})`),
    h('div', { class: 'aisle' }, [...items].sort((a, b) => a.checked - b.checked).map(itemRow)),
  );
}

async function copyList() {
  const text = await attempt(() => api.listText(S.week, shoppingDays().join(',')));
  if (typeof text !== 'string') return;
  try {
    await navigator.clipboard.writeText(text);
    toast('Liste copiée');
  } catch {
    // Presse-papiers indisponible (http sur le réseau local) : on montre le texte à sélectionner.
    const area = h('textarea', { readonly: true, style: 'min-height:50vh;font-size:14px' });
    area.value = text;
    sheet({ title: 'Liste à copier', body: [h('p', { class: 'dim' }, 'Sélectionnez tout le texte et copiez-le.'), area] });
    setTimeout(() => area.select(), 100);
  }
}

async function uncheckAll() {
  if (!confirm('Décocher tous les articles de cette semaine ?')) return;
  await attempt(() => api.uncheckAll(S.week));
  reload();
}

export { render };
