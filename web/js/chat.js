// Onglet Claude : une conversation par appareil, gardée sur celui-ci.
// Le serveur garde la clé API et fait tourner les outils ; ce qui change
// apparaît dans les autres onglets par le flux d'évènements habituel.

import { api } from './api.js';
import { S } from './store.js';
import { h, clear } from './ui.js';

const KEY = 'menus-chat';
const MAX_SIDE = 1568;

/** Messages au format de l'API : {role, content}. Les photos ne sont gardées
 * que le temps de leur envoi ; l'historique garde « [photo] » à leur place. */
let messages = load();
let pending = false;
let photo = null; // {data, media_type, url}
let root = null;
let list, input, photoPreview, sendBtn;

function load() {
  try { return JSON.parse(localStorage.getItem(KEY) || '[]'); } catch { return []; }
}

function save() {
  const light = messages.map((m) => ({
    ...m,
    content: typeof m.content === 'string'
      ? m.content
      : m.content.map((b) => (b.type === 'image' ? { type: 'text', text: '[photo]' } : b)),
  }));
  messages = light;
  try { localStorage.setItem(KEY, JSON.stringify(light)); } catch { /* mode privé */ }
}

function textOf(message) {
  if (typeof message.content === 'string') return message.content;
  return message.content.filter((b) => b.type === 'text').map((b) => b.text).join('\n');
}

function hasPhoto(message) {
  return Array.isArray(message.content) && message.content.some((b) => b.type === 'image' || b.text === '[photo]');
}

/** Un peu de mise en forme : **gras**, listes « - », paragraphes. */
function rich(text) {
  const box = h('div', { class: 'chat-text' });
  let ul = null;
  for (const line of text.split('\n')) {
    const item = line.match(/^\s*[-•*]\s+(.*)$/);
    if (item) {
      if (!ul) { ul = h('ul'); box.append(ul); }
      ul.append(h('li', {}, inline(item[1])));
      continue;
    }
    ul = null;
    if (line.trim()) box.append(h('p', {}, inline(line)));
  }
  return box;
}

function inline(text) {
  return text.split(/(\*\*[^*]+\*\*)/).map((part) => (part.startsWith('**') && part.endsWith('**') && part.length > 4 ? h('strong', { text: part.slice(2, -2) }) : part));
}

const ACTIONS = {
  plan_week: 'semaine planifiée', set_dinner: 'souper changé', set_brunch: 'brunch changé',
  propose_week: 'semaine proposée', reroll_dinner: 'souper retiré au sort', add_recipes: 'recette ajoutée',
  update_recipe: 'recette modifiée', set_recipe_flags: 'recette marquée', delete_recipe: 'recette supprimée',
  add_to_shopping_list: 'liste de courses complétée', update_settings: 'réglages changés',
};

function bubble(message) {
  const mine = message.role === 'user';
  const node = h('div', { class: `chat-msg ${mine ? 'mine' : 'theirs'}` });
  if (mine && hasPhoto(message)) node.append(h('div', { class: 'chat-photo-tag', text: '📷 photo' }));
  const text = textOf(message).replace(/\[photo\]/g, '').trim();
  if (text) node.append(mine ? h('div', { class: 'chat-text' }, h('p', { text })) : rich(text));
  const done = (message.actions || []).map((a) => ACTIONS[a]).filter(Boolean);
  if (done.length) node.append(h('div', { class: 'chat-actions', text: [...new Set(done)].join(' · ') }));
  return node;
}

function drawMessages() {
  clear(list);
  if (!messages.length) {
    list.append(h('div', { class: 'chat-empty' },
      h('p', { text: 'Demande à Claude :' }),
      ...['Planifie la semaine prochaine', 'Remplace le souper de mardi par du poisson', 'Ajoute une recette de risotto aux champignons', 'Il me reste des courgettes, qu’en faire ?']
        .map((idea) => h('button', { class: 'chat-idea', text: idea, onclick: () => { input.value = idea; input.focus(); } }))));
  }
  for (const m of messages) {
    // Les réponses aux outils n'existent pas côté app : seuls les textes s'affichent.
    if (!textOf(m).trim() && !hasPhoto(m)) continue;
    list.append(bubble(m));
  }
  if (pending) list.append(h('div', { class: 'chat-msg theirs typing' }, h('span'), h('span'), h('span')));
  requestAnimationFrame(() => list.lastElementChild?.scrollIntoView({ block: 'end' }));
}

async function send() {
  const text = input.value.trim();
  if (pending || (!text && !photo)) return;
  const content = [];
  if (photo) content.push({ type: 'image', source: { type: 'base64', media_type: photo.media_type, data: photo.data } });
  content.push({ type: 'text', text: text || 'Voici une recette en photo : transcris-la et ajoute-la.' });
  messages.push({ role: 'user', content: photo ? content : content[0].text });
  input.value = '';
  setPhoto(null);
  pending = true;
  sendBtn.disabled = true;
  drawMessages();
  try {
    // Les messages d'erreur restent à l'écran mais ne partent pas au serveur.
    const wire = messages.filter((m) => !m.error).map(({ role, content }) => ({ role, content }));
    const answer = await api.chat(wire, S.week);
    messages.push({ role: 'assistant', content: answer.reply || '(pas de réponse)', actions: answer.actions });
  } catch (err) {
    messages.push({ role: 'assistant', content: `⚠️ ${err.message}`, error: true });
  } finally {
    pending = false;
    sendBtn.disabled = false;
    save();
    drawMessages();
  }
}

/** Réduit la photo (côté le plus long ≤ 1568 px) avant de l'envoyer. */
async function readPhoto(file) {
  const url = URL.createObjectURL(file);
  const img = await new Promise((resolve, reject) => {
    const el = new Image();
    el.onload = () => resolve(el);
    el.onerror = reject;
    el.src = url;
  });
  const scale = Math.min(1, MAX_SIDE / Math.max(img.width, img.height));
  const canvas = document.createElement('canvas');
  canvas.width = Math.round(img.width * scale);
  canvas.height = Math.round(img.height * scale);
  canvas.getContext('2d').drawImage(img, 0, 0, canvas.width, canvas.height);
  URL.revokeObjectURL(url);
  const dataUrl = canvas.toDataURL('image/jpeg', 0.85);
  return { data: dataUrl.split(',')[1], media_type: 'image/jpeg', url: dataUrl };
}

function setPhoto(value) {
  photo = value;
  clear(photoPreview);
  photoPreview.hidden = !photo;
  if (photo) {
    photoPreview.append(h('img', { src: photo.url, alt: 'Photo jointe' }),
      h('button', { class: 'link-btn', text: 'Retirer', onclick: () => setPhoto(null) }));
  }
}

function build() {
  list = h('div', { class: 'chat-list' });
  input = h('textarea', { class: 'chat-input', rows: 1, placeholder: 'Écrire à Claude…', 'aria-label': 'Message pour Claude' });
  input.addEventListener('input', () => { input.style.height = 'auto'; input.style.height = Math.min(input.scrollHeight, 140) + 'px'; });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey && !matchMedia('(pointer: coarse)').matches) { e.preventDefault(); send(); }
  });
  const file = h('input', { type: 'file', accept: 'image/*', hidden: true });
  file.addEventListener('change', async () => {
    const chosen = file.files?.[0];
    file.value = '';
    if (chosen) setPhoto(await readPhoto(chosen));
  });
  photoPreview = h('div', { class: 'chat-photo', hidden: true });
  sendBtn = h('button', { class: 'chat-send', 'aria-label': 'Envoyer', onclick: send },
    svg('<path d="M4 12l16-8-6 16-2.5-6.5z"/>'));
  const attach = h('button', { class: 'chat-attach', 'aria-label': 'Joindre une photo', onclick: () => file.click() },
    svg('<rect x="3" y="6" width="18" height="14" rx="2.5"/><circle cx="12" cy="13" r="3.5"/><path d="M8.5 6l1.5-2h4l1.5 2"/>'));

  root = h('div', { class: 'chat' },
    h('div', { class: 'chat-head' },
      h('span', { text: 'Claude' }),
      h('button', { class: 'link-btn', text: 'Nouvelle conversation', onclick: () => { if (pending) return; messages = []; save(); drawMessages(); } })),
    list,
    photoPreview,
    h('div', { class: 'chat-bar' }, attach, input, file, sendBtn));
}

function svg(inner) {
  const el = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  el.setAttribute('viewBox', '0 0 24 24');
  el.setAttribute('aria-hidden', 'true');
  el.innerHTML = inner;
  return el;
}

/** Construite une fois : les rechargements de l'état ne touchent pas à la saisie. */
export function renderChat(view) {
  if (!root) { build(); drawMessages(); }
  if (root.parentNode !== view) clear(view, root);
}
