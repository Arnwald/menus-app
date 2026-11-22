// Onglet Réglages : foyer, soirs particuliers, brunch, rayons, accès.

import { api, token, setToken } from './api.js';
import { S, reload } from './store.js';
import { h, clear, attempt, capitalize, DAYS, DAY_LABELS } from './ui.js';

async function save(patch) {
  const ok = await attempt(() => api.patchSettings(patch));
  if (ok) reload();
}

function stepper(label, value, min, max, step, onChange, format = String) {
  const out = h('output', {}, format(value));
  const set = (v) => { const n = Math.min(max, Math.max(min, Math.round(v * 100) / 100)); if (n !== value) onChange(n); };
  return h('div', { class: 'stepper' },
    h('span', {}, label),
    h('div', { class: 'ctrl' },
      h('button', { class: 'mini', 'aria-label': `${label} : moins`, onClick: () => set(value - step), disabled: value <= min }, '−'),
      out,
      h('button', { class: 'mini', 'aria-label': `${label} : plus`, onClick: () => set(value + step), disabled: value >= max }, '+'),
    ),
  );
}

export function renderSettings(el) {
  const s = S.data.settings;
  const comma = (x) => String(x).replace('.', ',');

  // Un mode par soir : normal, long, restes, libre.
  const mode = (day) => (s.free_days.includes(day) ? 'free' : s.leftover_days.includes(day) ? 'leftovers' : s.long_days.includes(day) ? 'long' : 'normal');
  const setMode = (day, m) => {
    const without = (list) => list.filter((d) => d !== day);
    const patch = { long_days: without(s.long_days), leftover_days: without(s.leftover_days), free_days: without(s.free_days) };
    if (m === 'long') patch.long_days.push(day);
    if (m === 'leftovers') patch.leftover_days.push(day);
    if (m === 'free') patch.free_days.push(day);
    save(patch);
  };
  const modes = [['normal', 'Normal'], ['long', 'Long'], ['leftovers', 'Restes'], ['free', 'Libre']];

  const moveAisle = (i, delta) => {
    const aisles = [...s.aisles];
    const j = i + delta;
    if (j < 0 || j >= aisles.length) return;
    [aisles[i], aisles[j]] = [aisles[j], aisles[i]];
    save({ aisles });
  };

  clear(el,
    h('div', { class: 'card settings-block' },
      h('h2', {}, 'À table'),
      stepper('Adultes', s.adults, 0, 12, 1, (v) => save({ adults: v })),
      stepper('Enfants', s.children, 0, 12, 1, (v) => save({ children: v })),
      stepper('Portion d\'un enfant (× adulte)', s.child_factor, 0.25, 1.5, 0.05, (v) => save({ child_factor: v }), comma),
      h('p', { class: 'dim', style: 'margin:0;font-size:14px' }, `Les quantités sont calculées pour ${comma(S.data.portions)} portions adultes.`),
    ),

    h('div', { class: 'card settings-block' },
      h('h2', {}, 'Les soirs'),
      h('p', { class: 'dim', style: 'margin:0;font-size:14px' }, `Normal : plat de ${s.quick_minutes} min au plus. Long : on a le temps de cuisiner. Restes : on mange ce qui a été préparé en double la veille. Libre : pas de souper prévu.`),
      DAYS.map((day) => h('div', { class: 'day-mode' },
        h('span', {}, capitalize(DAY_LABELS[day])),
        h('div', { class: 'segmented', role: 'group', 'aria-label': DAY_LABELS[day] },
          modes.map(([key, label]) => h('button', {
            'aria-pressed': String(mode(day) === key),
            disabled: key === 'leftovers' && day === 'mon',
            title: key === 'leftovers' && day === 'mon' ? 'Le lundi n\'a pas de veille dans la semaine' : null,
            onClick: () => mode(day) !== key && setMode(day, key),
          }, label)),
        ),
      )),
      stepper('Plat rapide : minutes au plus', s.quick_minutes, 10, 90, 5, (v) => save({ quick_minutes: v })),
      stepper('Ne pas reproposer un plat avant (semaines)', s.avoid_weeks, 0, 8, 1, (v) => save({ avoid_weeks: v })),
    ),

    h('div', { class: 'card settings-block' },
      h('h2', {}, 'Brunch'),
      h('div', { class: 'chips' }, DAYS.map((day) => h('button', {
        class: 'chip',
        'aria-pressed': String(s.brunch_days.includes(day)),
        onClick: () => save({ brunch_days: s.brunch_days.includes(day) ? s.brunch_days.filter((d) => d !== day) : DAYS.filter((d) => d === day || s.brunch_days.includes(d)) }),
      }, capitalize(DAY_LABELS[day])))),
      h('label', { class: 'check' },
        h('input', { type: 'checkbox', checked: s.brunch_base, onChange: (e) => save({ brunch_base: e.target.checked }) }),
        'Toujours prévoir la base (tresse, beurre, confiture, jus)',
      ),
    ),

    h('div', { class: 'card settings-block aisle-order' },
      h('h2', {}, 'Ordre des rayons'),
      h('label', { class: 'field' }, 'Magasin',
        h('select', { onChange: (e) => save({ store_name: e.target.value }) },
          ['Coop', 'Migros', 'Coop ou Migros'].map((name) => h('option', { value: name, selected: name === s.store_name }, name)),
        ),
      ),
      h('p', { class: 'dim', style: 'margin:0;font-size:14px' }, 'La liste suit cet ordre : rangez les rayons dans l\'ordre où vous les parcourez.'),
      h('ol', {}, s.aisles.map((aisle, i) => h('li', {},
        h('span', { class: 'label' }, aisle.name),
        h('button', { class: 'mini', 'aria-label': `Monter ${aisle.name}`, disabled: i === 0, onClick: () => moveAisle(i, -1) }, '↑'),
        h('button', { class: 'mini', 'aria-label': `Descendre ${aisle.name}`, disabled: i === s.aisles.length - 1, onClick: () => moveAisle(i, 1) }, '↓'),
      ))),
    ),

    h('div', { class: 'card settings-block' },
      h('h2', {}, 'Avec Claude'),
      h('p', { style: 'margin:0' }, 'Dans Claude (application de bureau ou Claude Code), le serveur MCP « menus » donne accès à cette app : demander une semaine selon vos envies, ajouter une recette collée ou photographiée, la modifier. Les changements apparaissent ici aussitôt.'),
      h('button', { class: 'btn small', onClick: () => { setToken(''); location.reload(); } }, 'Changer le jeton d\'accès sur cet appareil'),
    ),
  );
}
