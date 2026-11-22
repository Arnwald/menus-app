// Point d'entrée : navigation entre onglets, en-tête de semaine, mises à jour en direct.

import { api, listen } from './api.js';
import { S, reload, setRenderer } from './store.js';
import { renderWeek } from './week.js';
import { renderShopping } from './shopping.js';
import { renderRecipes } from './recipes.js';
import { renderSettings } from './settings.js';
import { renderChat } from './chat.js';

const VIEWS = {
  semaine: renderWeek,
  courses: renderShopping,
  recettes: renderRecipes,
  reglages: renderSettings,
  claude: renderChat,
};

const $ = (id) => document.getElementById(id);

function weekNumber(id) {
  return Number(id.split('W')[1]);
}

function renderHeader() {
  const d = S.data;
  $('period-title').textContent = `Semaine ${weekNumber(d.week.week)}`;
  $('period-sub').textContent = d.label;
  $('today').hidden = d.week.week === d.current_week;
  const left = d.list.total - d.list.checked;
  const badge = $('badge-courses');
  badge.hidden = left <= 0;
  badge.textContent = left;
}

/** Redessine la vue active. Si l'on est en train de taper dans un champ de la
 * page, on attend qu'il perde le focus pour ne pas effacer la saisie. */
let deferred = false;
function draw() {
  if (!S.data) return;
  const active = document.activeElement;
  const typing = active && $('main').contains(active) && (active.matches('input[type="text"], input[type="search"], textarea'));
  if (typing && active.value) {
    if (!deferred) {
      deferred = true;
      active.addEventListener('blur', () => { deferred = false; draw(); }, { once: true });
    }
    return;
  }
  $('loading').hidden = true;
  renderHeader();
  for (const name of Object.keys(VIEWS)) $(`view-${name}`).hidden = name !== S.view;
  VIEWS[S.view]($(`view-${S.view}`));
  document.querySelectorAll('.tabbar a').forEach((a) => {
    if (a.dataset.view === S.view) a.setAttribute('aria-current', 'page'); else a.removeAttribute('aria-current');
  });
}

function route() {
  const name = location.hash.replace('#', '');
  S.view = VIEWS[name] ? name : 'semaine';
  draw();
  window.scrollTo(0, 0);
}

function go(week) {
  S.week = week;
  reload();
}

setRenderer(draw);
window.addEventListener('hashchange', route);
$('prev').addEventListener('click', () => go(S.data?.prev));
$('next').addEventListener('click', () => go(S.data?.next));
$('today').addEventListener('click', () => go(S.data?.current_week));

// Une modification ailleurs (l'autre téléphone, Claude, un fichier) recharge l'écran.
let timer;
listen(() => {
  clearTimeout(timer);
  timer = setTimeout(reload, 250);
});
// Au retour sur l'app (téléphone sorti de veille), on se resynchronise.
document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') reload(); });

// L'onglet Claude n'apparaît que si le serveur a une clé API.
api.chatInfo().then((info) => {
  if (!info?.enabled) return;
  $('tab-claude').hidden = false;
  document.querySelector('.tabbar').classList.add('five');
}).catch(() => {});

route();
reload();
