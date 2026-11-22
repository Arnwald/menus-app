// Accès à l'API. Le jeton n'est demandé que si le serveur en exige un.

const TOKEN_KEY = 'menus-token';

export function token() {
  try { return (localStorage.getItem(TOKEN_KEY) || '').replace(/\s+/g, ''); } catch { return ''; }
}

export function setToken(value) {
  value = (value || '').replace(/\s+/g, '');
  try { value ? localStorage.setItem(TOKEN_KEY, value) : localStorage.removeItem(TOKEN_KEY); } catch { /* mode privé */ }
}

function headers(extra = {}) {
  const head = { ...extra };
  const value = token();
  if (value) head['X-Auth-Token'] = value;
  return head;
}

async function send(method, path, { json, form } = {}) {
  const options = { method, headers: headers(json !== undefined ? { 'Content-Type': 'application/json' } : {}) };
  if (json !== undefined) options.body = JSON.stringify(json);
  if (form) options.body = form;

  const response = await fetch(path, options);
  if (response.status === 401) {
    const given = window.prompt("Cette instance demande un jeton d'accès :");
    if (given) { setToken(given.trim()); return send(method, path, { json, form }); }
    throw new Error("jeton d'accès requis");
  }
  const text = await response.text();
  let payload = null;
  try { payload = text ? JSON.parse(text) : null; } catch { payload = text; }
  if (!response.ok) throw new Error(payload?.error || `${response.status} ${response.statusText}`);
  return payload;
}

const enc = encodeURIComponent;

export const api = {
  state: (week, days) => send('GET', `/api/state?week=${enc(week || '')}&days=${enc(days || '')}`),
  recipe: (id) => send('GET', `/api/recipes/${enc(id)}`),
  createRecipes: (recipes, source) => send('POST', '/api/recipes', { json: { recipes, source } }),
  replaceRecipe: (id, draft) => send('PUT', `/api/recipes/${enc(id)}`, { json: draft }),
  patchRecipe: (id, patch) => send('PATCH', `/api/recipes/${enc(id)}`, { json: patch }),
  deleteRecipe: (id) => send('DELETE', `/api/recipes/${enc(id)}`),
  setDinner: (week, day, slot) => send('PUT', `/api/weeks/${week}/dinners/${day}`, { json: slot }),
  setBrunch: (week, day, recipes) => send('PUT', `/api/weeks/${week}/brunch/${day}`, { json: { recipes } }),
  propose: (week, keep) => send('POST', `/api/weeks/${week}/propose`, { json: { keep } }),
  reroll: (week, day) => send('POST', `/api/weeks/${week}/reroll/${day}`),
  apply: (week, proposal) => send('POST', `/api/weeks/${week}/apply`, { json: proposal }),
  clearPlan: (week) => send('DELETE', `/api/weeks/${week}/plan`),
  listText: (week, days) => send('GET', `/api/weeks/${week}/list?format=text&days=${enc(days || '')}`),
  check: (week, key, done) => send('POST', `/api/weeks/${week}/check`, { json: { key, done } }),
  uncheckAll: (week) => send('POST', `/api/weeks/${week}/uncheck-all`),
  addExtras: (week, text) => send('POST', `/api/weeks/${week}/extras`, { json: { text } }),
  patchExtra: (week, id, patch) => send('PATCH', `/api/weeks/${week}/extras/${enc(id)}`, { json: patch }),
  deleteExtra: (week, id) => send('DELETE', `/api/weeks/${week}/extras/${enc(id)}`),
  patchSettings: (patch) => send('PATCH', '/api/settings', { json: patch }),
};

/** URL avec le jeton, pour EventSource qui n'envoie pas d'en-têtes. */
export function withToken(path) {
  const value = token();
  if (!value) return path;
  return path + (path.includes('?') ? '&' : '?') + 'token=' + encodeURIComponent(value);
}

/** Flux d'évènements : rappelle `onChange` à chaque modification de la base. */
export function listen(onChange) {
  let source;
  const connect = () => {
    source = new EventSource(withToken('/api/stream'));
    source.addEventListener('change', (event) => {
      try { onChange(JSON.parse(event.data)); } catch { onChange({ scope: 'all' }); }
    });
    source.onerror = () => { source.close(); setTimeout(connect, 3000); };
  };
  connect();
  return () => source && source.close();
}
