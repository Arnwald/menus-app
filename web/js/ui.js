// Petits outils d'interface : construction du DOM, feuilles, messages.

/** Crée un élément. `props` : class, text, on<Évènement>, aria-*, data-*, et attributs. */
export function h(tag, props = {}, ...children) {
  const el = document.createElement(tag);
  for (const [key, value] of Object.entries(props || {})) {
    if (value === undefined || value === null || value === false) continue;
    if (key === 'class') el.className = value;
    else if (key === 'text') el.textContent = value;
    else if (key.startsWith('on') && typeof value === 'function') el.addEventListener(key.slice(2).toLowerCase(), value);
    else if (key === 'value') el.value = value;
    else if (key === 'checked' || key === 'disabled' || key === 'selected' || key === 'open') el[key] = !!value;
    else el.setAttribute(key, value === true ? '' : String(value));
  }
  append(el, children);
  return el;
}

function append(el, children) {
  for (const child of children.flat(Infinity)) {
    if (child === null || child === undefined || child === false) continue;
    el.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
}

export function clear(el, ...children) {
  el.replaceChildren();
  append(el, children);
  return el;
}

export const PROTEINS = {
  fish: 'poisson',
  poultry: 'volaille',
  meat: 'viande',
  veggie: 'végétarien',
  legumes: 'légumineuses',
};

export const STARCHES = {
  rice: 'riz', pasta: 'pâtes', potato: 'pommes de terre', bread: 'pain', couscous: 'semoule',
  quinoa: 'quinoa', polenta: 'polenta', noodles: 'nouilles', tortilla: 'tortillas', gnocchi: 'gnocchis', none: 'sans',
};

export const BRUNCH_KINDS = { sweet: 'sucré', savory: 'salé', base: 'base' };

export const DAYS = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];
export const DAY_LABELS = { mon: 'lundi', tue: 'mardi', wed: 'mercredi', thu: 'jeudi', fri: 'vendredi', sat: 'samedi', sun: 'dimanche' };
export const DAY_SHORT = { mon: 'lun', tue: 'mar', wed: 'mer', thu: 'jeu', fri: 'ven', sat: 'sam', sun: 'dim' };

export function capitalize(s) {
  return s ? s.charAt(0).toUpperCase() + s.slice(1) : s;
}

export function dot(protein) {
  return h('span', { class: `dot ${protein || ''}`, 'aria-hidden': 'true' });
}

/** « 25 min », « 1 h 15 dont 20 min actives ». */
export function duration(recipe) {
  const fmt = (m) => (m >= 60 ? `${Math.floor(m / 60)} h${m % 60 ? ' ' + String(m % 60).padStart(2, '0') : ''}` : `${m} min`);
  const total = fmt(recipe.minutes);
  return recipe.active_minutes && recipe.active_minutes < recipe.minutes - 5 ? `${total} dont ${recipe.active_minutes} actives` : total;
}

/** Ligne de méta d'une recette : durée · catégorie · se garde. */
export function recipeMeta(recipe) {
  const parts = [duration(recipe)];
  if (recipe.meal === 'brunch') parts.push(`brunch ${BRUNCH_KINDS[recipe.brunch_kind] || ''}`.trim());
  else if (recipe.protein) parts.push(PROTEINS[recipe.protein]);
  if (recipe.keeps) parts.push('se garde');
  return parts.join(' · ');
}

/** Il y a combien de temps une recette est passée au menu. */
export function lastSeen(history, id) {
  const gap = history?.[id];
  if (gap === undefined) return '';
  if (gap === 0) return 'cette semaine';
  if (gap === 1) return 'à 1 semaine';
  return `à ${gap} semaines`;
}

// ── Feuilles ───────────────────────────────────────────────────────────────

let openSheets = [];

/**
 * Ouvre une feuille modale. `body` et `foot` : nœuds ou listes de nœuds.
 * Renvoie { close, body, foot, setBusy }.
 */
export function sheet({ title, body = [], foot = [], onClose } = {}) {
  const root = document.getElementById('sheet-root');
  const bodyEl = h('div', { class: 'sheet-body' }, body);
  const footEl = h('div', { class: 'sheet-foot' }, foot);
  const titleEl = h('h2', { text: title });
  const panel = h('div', { class: 'sheet', role: 'dialog', 'aria-modal': 'true', 'aria-label': title },
    h('div', { class: 'sheet-head' }, titleEl, h('button', { class: 'mini', 'aria-label': 'Fermer', onClick: () => close() }, '×')),
    bodyEl,
    footEl,
  );
  if (!foot || (Array.isArray(foot) && foot.length === 0)) footEl.hidden = true;
  const backdrop = h('div', { class: 'sheet-backdrop', onClick: (e) => { if (e.target === backdrop) close(); } }, panel);
  root.append(backdrop);
  document.body.style.overflow = 'hidden';

  const onKey = (e) => { if (e.key === 'Escape' && openSheets[openSheets.length - 1] === handle) close(); };
  document.addEventListener('keydown', onKey);

  function close() {
    if (!backdrop.isConnected) return;
    backdrop.remove();
    document.removeEventListener('keydown', onKey);
    openSheets = openSheets.filter((s) => s !== handle);
    if (!openSheets.length) document.body.style.overflow = '';
    onClose?.();
  }

  const handle = {
    close,
    body: bodyEl,
    foot: footEl,
    setTitle: (t) => { titleEl.textContent = t; },
    setBody: (...nodes) => clear(bodyEl, nodes),
    setFoot: (...nodes) => { clear(footEl, nodes); footEl.hidden = nodes.flat().length === 0; },
  };
  openSheets.push(handle);
  setTimeout(() => panel.querySelector('input, textarea, select')?.focus({ preventScroll: true }), 50);
  return handle;
}

export function closeAllSheets() {
  [...openSheets].forEach((s) => s.close());
}

export function spinner(text) {
  return h('div', { class: 'spinner' }, text);
}

// ── Messages ───────────────────────────────────────────────────────────────

let toastTimer;
export function toast(message, kind = '') {
  const el = document.getElementById('toast');
  el.textContent = message;
  el.className = `toast ${kind}`;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { el.hidden = true; }, kind === 'error' ? 6000 : 2800);
}

/** Exécute une action et affiche l'erreur éventuelle. */
export async function attempt(fn, success) {
  try {
    const out = await fn();
    if (success) toast(success);
    return out;
  } catch (err) {
    toast(err.message || String(err), 'error');
    return undefined;
  }
}

/** Bouton qui se désactive pendant son action. */
export function actionButton(label, fn, cls = 'btn') {
  const btn = h('button', { class: cls, type: 'button' }, label);
  btn.addEventListener('click', async () => {
    btn.disabled = true;
    try { await fn(); } finally { btn.disabled = false; }
  });
  return btn;
}
