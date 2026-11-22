// État partagé de l'interface : la réponse de /api/state et la vue courante.

import { api } from './api.js';
import { toast } from './ui.js';

export const S = {
  /** Réponse de /api/state. */
  data: null,
  /** Semaine affichée (`2026-W47`), null = celle que choisit le serveur. */
  week: null,
  view: 'semaine',
  /** Filtres de l'onglet Recettes, gardés le temps de la visite. */
  recipeFilter: { query: '', tags: [] },
};

let renderer = () => {};
export function setRenderer(fn) { renderer = fn; }
export function render() { renderer(); }

// Jours couverts par la liste de courses, mémorisés par semaine sur cet appareil.
const DAYS_KEY = 'menus-days';
export function shoppingDays(week = S.week) {
  try { return JSON.parse(localStorage.getItem(DAYS_KEY) || '{}')[week] || []; } catch { return []; }
}
export function setShoppingDays(days, week = S.week) {
  try {
    const all = JSON.parse(localStorage.getItem(DAYS_KEY) || '{}');
    if (days.length === 0 || days.length === 7) delete all[week]; else all[week] = days;
    localStorage.setItem(DAYS_KEY, JSON.stringify(all));
  } catch { /* mode privé */ }
}

let loading = null;
let again = false;

/** Recharge l'état depuis le serveur, puis redessine. Les appels qui se
 * chevauchent sont fusionnés. */
export async function reload() {
  if (loading) { again = true; return loading; }
  loading = (async () => {
    try {
      const data = await api.state(S.week, shoppingDays().join(','));
      S.data = data;
      S.week = data.week.week;
      render();
    } catch (err) {
      toast(err.message, 'error');
    } finally {
      loading = null;
      if (again) { again = false; reload(); }
    }
  })();
  return loading;
}

export function recipeById(id) {
  return S.data?.recipes.find((r) => r.id === id);
}

export function recipeName(id) {
  return recipeById(id)?.name || id;
}
