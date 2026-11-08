// my-calendar PWA: vanilla JS, no build step.
const TZ = Intl.DateTimeFormat().resolvedOptions().timeZone;
const PALETTE = ['#a78bfa', '#4fd1c5', '#f6ad55', '#f687b3', '#63b3ed', '#68d391', '#fc8181', '#ecc94b'];
const $ = (s, r = document) => r.querySelector(s);
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const tint = (c, p = 16) => `color-mix(in srgb, ${c} ${p}%, transparent)`;

// ---------- dates ----------
const sod = (d) => new Date(d.getFullYear(), d.getMonth(), d.getDate());
const addDays = (d, n) => new Date(d.getFullYear(), d.getMonth(), d.getDate() + n);
const sameDay = (a, b) => a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
const sow = (d) => addDays(sod(d), -d.getDay()); // week starts Sunday
const pad = (n) => String(n).padStart(2, '0');
const ymd = (d) => `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
const hm = (d) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;
const parseYmd = (s) => { const [y, m, d] = s.split('-').map(Number); return new Date(y, m - 1, d); };
const fmt = (d, o) => d.toLocaleDateString([], o);
const fmtT = (d) => d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
const today = () => sod(new Date());
function rel(d) {
  const n = Math.round((sod(d) - today()) / 864e5);
  if (n === 0) return 'today';
  if (n === 1) return 'tomorrow';
  if (n === -1) return 'yesterday';
  return n > 0 ? `in ${n} days` : `${-n} days ago`;
}
const courseName = (c) => (c || '').replace('-', ' ');

// ---------- state ----------
const S = {
  view: localStorage.getItem('view') || 'agenda',
  sel: today(),
  agendaDays: 45,
  loaded: null, // {from, to, events}
  loading: false,
  calendars: null,
  courses: null,
  scrolled: false,
};

// ---------- api ----------
async function api(path, opts = {}) {
  const init = { method: opts.method || 'GET', headers: {} };
  if (opts.body !== undefined) { init.body = JSON.stringify(opts.body); init.headers['content-type'] = 'application/json'; }
  let r;
  try { r = await fetch('/api' + path, init); } catch { throw new Error(navigator.onLine ? 'Server unreachable' : 'You are offline'); }
  if (!r.ok) throw new Error((await r.text()) || r.statusText);
  return r.status === 204 ? null : r.json();
}
function toast(msg) {
  const t = $('#toast'); t.textContent = msg; t.classList.add('show');
  clearTimeout(toast.t); toast.t = setTimeout(() => t.classList.remove('show'), 2400);
}
const attempt = (fn) => async (...a) => { try { return await fn(...a); } catch (e) { toast(e.message); } };

function norm(e) {
  if (e.all_day) { e._s = parseYmd(e.start); e._e = parseYmd(e.end); }
  else { e._s = new Date(e.start); e._e = new Date(e.end); }
  return e;
}
function need(from, to) {
  const L = S.loaded;
  if (L && L.from <= from && L.to >= to) return L.events;
  if (!S.loading) {
    S.loading = true;
    const f = addDays(from, -35), t = addDays(to, 35);
    api(`/events?from=${f.toISOString()}&to=${t.toISOString()}&tz=${encodeURIComponent(TZ)}`)
      .then((ev) => { S.loaded = { from: f, to: t, events: ev.map(norm) }; })
      .catch((e) => { toast(e.message); S.loaded = { from: f, to: t, events: L?.events || [] }; })
      .finally(() => { S.loading = false; render(); });
  }
  return L ? L.events : [];
}
function invalidate() { S.loaded = null; S.calendars = null; S.courses = null; }

function eventsOn(events, day) {
  const a = sod(day), b = addDays(a, 1);
  return events.filter((e) => e.all_day ? e._s <= a && e._e > a
    : (e._s < b && e._e > a) || (+e._s === +e._e && e._s >= a && e._s < b));
}
const dayDots = (list) => list.slice(0, 3).map((e) => `<i style="background:${e.color}"></i>`).join('');

// ---------- rendering ----------
function render() {
  document.querySelectorAll('#nav a').forEach((a) => a.classList.toggle('on', a.dataset.v === S.view));
  $('#fab').classList.toggle('hide', S.view === 'settings');
  document.body.className = 'view-' + S.view;
  ({ agenda, month, week, settings })[S.view]();
}

function header(title, btns = '') { $('#hdr').innerHTML = `<div class="hdr"><h1>${title}</h1><div class="hbtns">${btns}</div></div>`; }
const monthTitle = (d) => `${fmt(d, { month: 'long' })} <small>${d.getFullYear()}</small>`;
const todayBtn = `<button class="pill" data-act="today">Today</button>`;

function evRow(e, day) {
  const course = e.course ? `<span class="chip" style="background:${tint(e.color)};color:${e.color}">${esc(courseName(e.course))}</span>` : '';
  const past = e._e < new Date() && !e.all_day;
  if (e.kind === 'due') {
    return `<div class="ev due ${e.done ? 'done' : ''}" data-id="${e.id}" data-s="${e.start}">
      <button class="cb ${e.done ? 'on' : ''}" style="--c:${e.color}" data-done="${e.id}" aria-label="Done"><svg><use href="#i-check"/></svg></button>
      <div class="bd"><div class="tt">${esc(e.title)}</div><div class="sb">${course}Due ${e.all_day ? '' : fmtT(e._s)}</div></div></div>`;
  }
  let tm;
  if (e.all_day) tm = 'All day';
  else {
    const startsToday = sameDay(e._s, day), endsToday = sameDay(e._e, day) || +e._e === +addDays(day, 1);
    tm = `${startsToday ? hm(e._s) : '…'}${+e._s !== +e._e ? `<em>${endsToday ? hm(e._e) : '…'}</em>` : ''}`;
  }
  const sub = [course, esc(e.course ? '' : e.location), e.rrule ? 'Repeats' : '', e.kind === 'available' ? 'Available' : ''].filter(Boolean).join(' ');
  return `<div class="ev ${past ? 'past' : ''}" data-id="${e.id}" data-s="${e.start}"><span class="bar" style="background:${e.color}"></span>
    <div class="tm">${tm}</div><div class="bd"><div class="tt">${esc(e.title)}</div>${sub ? `<div class="sb">${sub}</div>` : ''}</div></div>`;
}

function strip(events, start) {
  let h = '<div class="strip" id="swipe">';
  for (let i = 0; i < 7; i++) {
    const d = addDays(start, i);
    h += `<button class="d ${sameDay(d, today()) ? 'today' : ''} ${sameDay(d, S.sel) ? 'sel' : ''}" data-day="${ymd(d)}">
      <span class="w">${fmt(d, { weekday: 'short' })}</span><span class="n">${d.getDate()}</span><span class="dots">${dayDots(eventsOn(events, d))}</span></button>`;
  }
  return h + '</div>';
}

function dayHead(d) {
  const t = sameDay(d, today());
  return `<div class="dayh ${t ? 't' : ''}"><b>${t ? 'Today' : fmt(d, { weekday: 'long' })}</b><span>${fmt(d, t ? { weekday: 'long', month: 'short', day: 'numeric' } : { month: 'short', day: 'numeric' })}</span></div>`;
}

function agenda() {
  const ws = sow(S.sel);
  const end = addDays(S.sel, S.agendaDays);
  const events = need(ws, end);
  header(monthTitle(S.sel), todayBtn);
  let h = strip(events, ws) + '<div class="pad">';
  let any = false;
  for (let d = S.sel; d < end; d = addDays(d, 1)) {
    const list = eventsOn(events, d);
    if (!list.length && !sameDay(d, S.sel)) continue;
    any = any || list.length > 0;
    h += dayHead(d) + (list.length ? list.map((e) => evRow(e, d)).join('') : '<div class="empty">Nothing scheduled</div>');
  }
  if (!any && !S.loading && S.loaded) h += '<div class="blank">Nothing coming up.<br>Tap + to add an event.</div>';
  h += `<button class="more" data-act="more">Show more</button></div>`;
  $('#main').innerHTML = h;
}

function month() {
  const first = new Date(S.sel.getFullYear(), S.sel.getMonth(), 1);
  const gs = sow(first);
  const last = new Date(first.getFullYear(), first.getMonth() + 1, 0);
  const weeks = Math.ceil((last - gs) / 864e5 / 7 + 1e-9) || 5;
  const ge = addDays(gs, weeks * 7);
  const events = need(gs, ge);
  header(monthTitle(first), `<button class="ib" data-act="prev"><svg><use href="#i-left"/></svg></button><button class="ib" data-act="next"><svg><use href="#i-right"/></svg></button>`);
  let h = '<div class="mgrid" id="swipe" style="padding:0 6px">' + ['S', 'M', 'T', 'W', 'T', 'F', 'S'].map((w) => `<div class="wd">${w}</div>`).join('');
  for (let d = gs; d < ge; d = addDays(d, 1)) {
    const cls = [d.getMonth() !== first.getMonth() && 'o', sameDay(d, today()) && 'today', sameDay(d, S.sel) && 'sel'].filter(Boolean).join(' ');
    const dl = eventsOn(events, d);
    const pills = dl.slice(0, 3).map((e) => `<button class="mp ${e.done ? 'done' : ''}" data-id="${e.id}" data-s="${e.start}" style="--c:${e.color}">${e.all_day ? '' : `<i>${fmtT(e._s).replace(':00', '')}</i>`}${esc(e.title)}</button>`).join('')
      + (dl.length > 3 ? `<span class="mmore">+${dl.length - 3} more</span>` : '');
    h += `<div class="mc ${cls}" data-day="${ymd(d)}"><span class="n">${d.getDate()}</span><span class="dots">${dayDots(dl)}</span><span class="mt">${pills}</span></div>`;
  }
  const list = eventsOn(events, S.sel);
  h += `</div><div class="selday pad">${dayHead(S.sel).replace('</span></div>', ` · ${list.length} item${list.length === 1 ? '' : 's'}</span></div>`)}
    ${list.length ? list.map((e) => evRow(e, S.sel)).join('') : '<div class="empty">Nothing scheduled</div>'}</div>`;
  $('#main').innerHTML = h;
}

const weekDays = () => (innerWidth >= 760 ? 7 : 3);
function week() {
  const n = weekDays();
  const start = n === 7 ? sow(S.sel) : S.sel;
  const end = addDays(start, n);
  const events = need(start, end);
  const last = addDays(end, -1);
  const title = start.getMonth() === last.getMonth()
    ? `${fmt(start, { month: 'short' })} ${start.getDate()}–${last.getDate()}`
    : `${fmt(start, { month: 'short', day: 'numeric' })} – ${fmt(last, { month: 'short', day: 'numeric' })}`;
  header(title, todayBtn);
  const days = [...Array(n)].map((_, i) => addDays(start, i));
  let head = `<div class="tlh" style="--days:${n}"><div></div>` + days.map((d) =>
    `<div class="${sameDay(d, today()) ? 'today' : ''}" data-day="${ymd(d)}" data-go="agenda"><div class="w">${fmt(d, { weekday: 'short' })}</div><div class="n">${d.getDate()}</div></div>`).join('') + '</div>';
  let ad = `<div class="allday" style="--days:${n}"><span>all day</span>` + days.map((d) =>
    `<div class="cell">${eventsOn(events, d).filter((e) => e.all_day).map((e) =>
      `<button class="a" data-id="${e.id}" data-s="${e.start}" style="background:${tint(e.color)};color:${e.color}">${esc(e.title)}</button>`).join('')}</div>`).join('') + '</div>';
  $('#hdr').insertAdjacentHTML('beforeend', head + ad);
  const H = 52;
  let h = `<div class="tl" id="swipe" style="--days:${n}"><div class="hrs">${[...Array(24)].map((_, i) => `<div>${i ? fmtT(new Date(2000, 0, 1, i)).replace(':00', '') : ''}</div>`).join('')}</div>`;
  for (const d of days) {
    const list = eventsOn(events, d).filter((e) => !e.all_day);
    h += `<div class="col" data-day="${ymd(d)}">${layout(list, d).map(({ e, top, ht, col, cols }) =>
      `<button class="blk ${e.kind === 'due' ? 'due' : ''}" data-id="${e.id}" data-s="${e.start}" style="top:${top * H}px;height:${Math.max(ht * H - 2, 20)}px;left:calc(${(col / cols) * 100}% + 2px);width:calc(${100 / cols}% - 4px);background:${tint(e.color, 18)};color:${e.color}">
        <b>${e.done ? '✓ ' : ''}${esc(e.title)}</b><span>${e.kind === 'due' ? 'Due ' + fmtT(e._s) : fmtT(e._s)}</span></button>`).join('')}
      ${sameDay(d, today()) ? `<div class="now" style="top:${((Date.now() - today()) / 36e5) * H}px"></div>` : ''}</div>`;
  }
  $('#main').innerHTML = h + '</div>';
  if (!S.scrolled) {
    const hr = days.some((d) => sameDay(d, today())) ? Math.max(new Date().getHours() - 2, 0) : 7;
    $('#main').scrollTop = hr * H;
    S.scrolled = true;
  }
}

// Overlap layout: hours from day start, column assignment within clusters.
function layout(list, d) {
  const day0 = sod(d);
  const items = list.map((e) => {
    const s = Math.max((e._s - day0) / 36e5, 0);
    const en = Math.min((e._e - day0) / 36e5, 24);
    return { e, top: s, ht: Math.max(en - s, 0.5), end: Math.max(en, s + 0.5) };
  }).sort((a, b) => a.top - b.top || b.end - a.end);
  let cluster = [], cols = [], clusterEnd = -1;
  const flush = () => { cluster.forEach((it) => (it.cols = cols.length)); cluster = []; cols = []; };
  for (const it of items) {
    if (it.top >= clusterEnd) { flush(); clusterEnd = -1; }
    let c = cols.findIndex((end) => end <= it.top);
    if (c < 0) { c = cols.length; cols.push(0); }
    cols[c] = it.end; it.col = c; cluster.push(it);
    clusterEnd = Math.max(clusterEnd, it.end);
  }
  flush();
  return items;
}

// ---------- settings ----------
async function loadSettings() {
  [S.calendars, S.courses] = await Promise.all([api('/calendars'), api('/courses')]);
}
function ago(iso) {
  if (!iso) return 'never';
  const m = Math.round((Date.now() - new Date(iso)) / 6e4);
  if (m < 1) return 'just now';
  if (m < 60) return `${m} min ago`;
  if (m < 1440) return `${Math.round(m / 60)} h ago`;
  return `${Math.round(m / 1440)} d ago`;
}
function settings() {
  header('Settings');
  if (!S.calendars) {
    $('#main').innerHTML = '';
    loadSettings().then(render, (e) => toast(e.message));
    return;
  }
  const subs = S.calendars.filter((c) => c.kind === 'ics');
  const locals = S.calendars.filter((c) => c.kind === 'local');
  let h = '<div class="pad"><div class="sec">Subscriptions</div><div class="card">';
  for (const c of subs) {
    h += `<div class="sr" data-sub="${c.id}"><span class="ic" style="background:${tint(c.color, 14)};color:${c.color}"><svg><use href="#i-book"/></svg></span>
      <div class="m">${esc(c.name)}<small><span class="stat ${c.last_sync_ok === false ? 'bad' : ''}"></span>${c.last_sync_ok === false ? esc(c.last_sync_status) : `Synced ${ago(c.last_sync_at)} · ${c.event_count} events`}</small></div>
      <button class="syncbtn" data-sync="${c.id}">Sync</button></div>`;
  }
  h += `<div class="sr" data-sub="new"><span class="ic" style="background:var(--s3);color:var(--tx2)"><svg><use href="#i-plus"/></svg></span><div class="m" style="color:var(--tx2)">Add .ics URL</div></div></div>`;
  h += '<div class="sec">Calendars &amp; courses</div><div class="card">';
  for (const c of locals) h += `<div class="sr" data-local="${c.id}"><span class="dot" style="background:${c.color}"></span><div class="m">${esc(c.name)}</div><span class="r">Local ›</span></div>`;
  for (const c of S.courses) {
    h += `<div class="sr" data-course="${esc(c.code)}"><span class="dot" style="background:${c.color};${c.hidden ? 'opacity:.35' : ''}"></span>
      <div class="m" style="${c.hidden ? 'color:var(--tx3)' : ''}">${esc(courseName(c.code))}<small>${esc(c.label)}</small></div>
      <span class="r">${c.hidden ? '<svg><use href="#i-eyeoff"/></svg>' : '›'}</span></div>`;
  }
  h += `</div><div class="foot">Week starts Sunday · Times in ${esc(TZ)}</div></div>`;
  $('#main').innerHTML = h;
}

// ---------- sheets ----------
function sheet(html, onMount) {
  const root = $('#sheet-root');
  root.innerHTML = `<div class="dim"></div><div class="sheet" role="dialog"><div class="grab"></div>${html}</div>`;
  $('.dim', root).onclick = closeSheet;
  onMount?.($('.sheet', root));
  history.pushState({ sheet: 1 }, '');
}
function closeSheet(fromPop) {
  const root = $('#sheet-root');
  if (!root.innerHTML) return;
  root.innerHTML = '';
  if (fromPop !== true && history.state?.sheet) history.back();
}
addEventListener('popstate', () => closeSheet(true));

function findEvent(id, start) {
  return (S.loaded?.events || []).find((e) => e.id === +id && (!start || e.start === start)) ||
    (S.loaded?.events || []).find((e) => e.id === +id);
}

function whenText(e) {
  const dOpt = { weekday: 'short', month: 'short', day: 'numeric' };
  if (e.all_day) {
    const last = addDays(e._e, -1);
    return sameDay(e._s, last) ? fmt(e._s, { weekday: 'long', month: 'short', day: 'numeric' }) : `${fmt(e._s, dOpt)} – ${fmt(last, dOpt)}`;
  }
  if (e.kind === 'due') return `Due ${fmt(e._s, { weekday: 'long', month: 'short', day: 'numeric' })} · ${fmtT(e._s)}`;
  if (+e._s === +e._e) return `${fmt(e._s, dOpt)} · ${fmtT(e._s)}`;
  return sameDay(e._s, e._e) ? `${fmt(e._s, dOpt)} · ${fmtT(e._s)} – ${fmtT(e._e)}` : `${fmt(e._s, dOpt)} ${fmtT(e._s)} – ${fmt(e._e, dOpt)} ${fmtT(e._e)}`;
}
const REPEATS = [
  ['', 'Never'], ['FREQ=DAILY', 'Every day'], ['WEEKLY', 'Every week'],
  ['FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR', 'Every weekday'], ['FREQ=MONTHLY', 'Every month'], ['FREQ=YEARLY', 'Every year'],
];
const DAYS = ['SU', 'MO', 'TU', 'WE', 'TH', 'FR', 'SA'];
function repeatLabel(r) {
  if (!r) return 'Never';
  if (/^FREQ=WEEKLY;BYDAY=[A-Z]{2}$/.test(r) || r === 'FREQ=WEEKLY') return 'Every week';
  const known = REPEATS.find(([v]) => v === r);
  if (known) return known[1];
  const m = /^FREQ=WEEKLY;BYDAY=([A-Z,]+)$/.exec(r);
  const names = { SU: 'Sun', MO: 'Mon', TU: 'Tue', WE: 'Wed', TH: 'Thu', FR: 'Fri', SA: 'Sat' };
  if (m) return 'Weekly on ' + m[1].split(',').map((d) => names[d]).join(', ');
  return 'Custom';
}

function swatches(cur, extra = '') {
  return `<div class="sw">${PALETTE.map((c) => `<button type="button" data-color="${c}" class="${c === cur ? 'on' : ''}" style="background:${c}"></button>`).join('')}${extra}</div>`;
}
function bindSwatches(el, cb) {
  el.querySelectorAll('.sw button[data-color]').forEach((b) => (b.onclick = () => {
    el.querySelectorAll('.sw button').forEach((x) => x.classList.remove('on'));
    b.classList.add('on'); cb(b.dataset.color);
  }));
}

function detail(e) {
  const tag = e.course ? courseName(e.course) : e.calendar;
  const url = e.url;
  let h = `<span class="chip" style="background:${tint(e.color)};color:${e.color}">${esc(tag)}</span><h3>${esc(e.title)}</h3>`;
  if (e.readonly) h += `<span class="src"><svg><use href="#i-sync"/></svg>From ${esc(e.calendar)} · read-only</span>`;
  h += `<div class="row"><svg><use href="#i-clock"/></svg><div>${whenText(e)}<small>${rel(e._s)}</small></div></div>`;
  if (e.rrule) h += `<div class="row"><svg><use href="#i-repeat"/></svg><div>${repeatLabel(e.rrule)}</div></div>`;
  if (e.course) h += `<div class="row"><svg><use href="#i-book"/></svg><div>${esc(e.location.split(' - ')[0])}<small>${esc(e.course_label || '')}</small></div></div>`;
  else if (e.location) h += `<div class="row"><svg><use href="#i-pin"/></svg><div>${esc(e.location)}</div></div>`;
  const desc = (e.description || '').replace(/\n*View event - \S+\s*$/, '').trim();
  if (desc) h += `<div class="row"><svg><use href="#i-text"/></svg><div class="desc">${esc(desc)}</div></div>`;
  h += `<div class="row"><svg><use href="#i-edit"/></svg><textarea id="note" rows="1" placeholder="Add a private note…">${esc(e.note || '')}</textarea></div>`;
  if (e.readonly) h += `<div class="row" style="align-items:center"><span style="flex:1;color:var(--tx2)">Colour</span>${swatches(e.color_override, '<button type="button" data-reset style="background:var(--s3);font-size:11px;color:var(--tx2)" title="Default">↺</button>')}</div>`;
  h += '<div class="btns">';
  if (url) h += `<a class="btn" href="${esc(url)}" target="_blank" rel="noopener"><svg><use href="#i-link"/></svg>Open${e.readonly ? ' in D2L' : ''}</a>`;
  if (e.kind === 'due') h += `<button class="btn pri" data-done><svg><use href="#i-check"/></svg>${e.done ? 'Mark not done' : 'Mark done'}</button>`;
  if (!e.readonly) h += `<button class="btn icon" data-del aria-label="Delete"><svg><use href="#i-trash"/></svg></button><button class="btn pri" data-edit><svg><use href="#i-edit"/></svg>Edit</button>`;
  h += '</div>';
  if (e.readonly) h += `<button class="link" data-hide>Hide this event</button>`;
  sheet(h, (el) => {
    const meta = attempt(async (m) => { await api(`/events/${e.id}/meta`, { method: 'PUT', body: m }); invalidate(); render(); });
    const note = $('#note', el);
    note.oninput = () => { note.style.height = 'auto'; note.style.height = note.scrollHeight + 'px'; };
    note.oninput();
    note.onblur = () => { if (note.value !== (e.note || '')) { e.note = note.value; meta({ note: note.value }); } };
    bindSwatches(el, (c) => meta({ color_override: c }));
    const reset = $('[data-reset]', el); if (reset) reset.onclick = () => { meta({ color_override: null }); closeSheet(); };
    const done = $('[data-done]', el); if (done) done.onclick = () => { meta({ done: !e.done }); closeSheet(); };
    const hide = $('[data-hide]', el); if (hide) hide.onclick = () => { meta({ hidden: true }); closeSheet(); toast('Event hidden'); };
    const del = $('[data-del]', el); if (del) del.onclick = attempt(async () => {
      if (!confirm(e.rrule ? 'Delete all occurrences of this event?' : 'Delete this event?')) return;
      await api(`/events/${e.id}`, { method: 'DELETE' }); closeSheet(); invalidate(); render(); toast('Deleted');
    });
    const ed = $('[data-edit]', el); if (ed) ed.onclick = () => { closeSheet(); setTimeout(() => editor(e), 30); };
  });
}

function editor(e, preset) {
  const isNew = !e;
  const locals = (S.calendars || []).filter((c) => c.kind === 'local');
  let s, en;
  if (e) {
    // recurring: edit the whole series from its first occurrence
    const ss = e.series_start || e.start, se = e.series_end || e.end;
    s = e.all_day ? parseYmd(ss) : new Date(ss);
    en = e.all_day ? addDays(parseYmd(se), -1) : new Date(se);
  } else {
    const base = preset || (() => { const n = new Date(); const d = new Date(S.sel); d.setHours(sameDay(d, n) ? n.getHours() + 1 : 9, 0, 0, 0); return d; })();
    s = base; en = new Date(+base + 36e5);
  }
  let allDay = e?.all_day || false;
  let color = e?.color_override || null;
  const rr = e?.rrule || '';
  const repOpts = REPEATS.map(([v, l]) => {
    const val = v === 'WEEKLY' ? '__weekly' : v;
    const sel = (v === 'WEEKLY' && /^FREQ=WEEKLY(;BYDAY=[A-Z]{2})?$/.test(rr)) || (v !== 'WEEKLY' && v === rr);
    return `<option value="${val}" ${sel ? 'selected' : ''}>${l}</option>`;
  }).join('') + (rr && !REPEATS.some(([v]) => v === rr) && !/^FREQ=WEEKLY(;BYDAY=[A-Z]{2})?$/.test(rr) ? `<option value="${esc(rr)}" selected>${esc(repeatLabel(rr))}</option>` : '');
  const h = `<form id="ef"><div class="ftop"><button type="button" data-cancel>Cancel</button><b>${isNew ? 'New event' : 'Edit event'}</b><button class="save" type="submit">Save</button></div>
    <input class="title-in" name="title" placeholder="Title" value="${esc(e?.title || '')}" autocomplete="off" required>
    <div class="grp">
      <div class="fr"><span>All-day</span><button type="button" class="tog ${allDay ? 'on' : ''}" data-allday></button></div>
      <div class="fr"><span>Starts</span><span class="v"><input type="date" name="sd" value="${ymd(s)}" required><input type="time" name="st" value="${hm(s)}"></span></div>
      <div class="fr"><span>Ends</span><span class="v"><input type="date" name="ed" value="${ymd(en)}" required><input type="time" name="et" value="${hm(en)}"></span></div>
    </div>
    <div class="grp"><div class="fr"><span>Repeat</span><select name="rrule">${repOpts}</select></div>
      ${locals.length > 1 ? `<div class="fr"><span>Calendar</span><select name="cal">${locals.map((c) => `<option value="${c.id}" ${c.id === e?.calendar_id ? 'selected' : ''}>${esc(c.name)}</option>`).join('')}</select></div>` : ''}
    </div>
    <div class="grp">
      <div class="fr"><span>Colour</span>${swatches(color)}</div>
      <div class="fr"><span>Location</span><input class="plain" name="location" placeholder="Add" value="${esc(e?.location || '')}"></div>
      <div class="fr stack"><textarea name="description" placeholder="Notes">${esc(e?.description || '')}</textarea></div>
    </div></form>`;
  sheet(h, (el) => {
    const f = $('#ef', el);
    const times = () => f.querySelectorAll('input[type=time]').forEach((i) => (i.style.display = allDay ? 'none' : ''));
    times();
    $('[data-allday]', el).onclick = (ev) => { allDay = !allDay; ev.currentTarget.classList.toggle('on', allDay); times(); };
    $('[data-cancel]', el).onclick = closeSheet;
    bindSwatches(el, (c) => (color = c));
    f.sd.onchange = () => { if (f.ed.value < f.sd.value) f.ed.value = f.sd.value; };
    f.st.onchange = () => {
      if (f.ed.value === f.sd.value && f.et.value <= f.st.value) {
        const d = new Date(`${f.sd.value}T${f.st.value}`); d.setHours(d.getHours() + 1); f.ed.value = ymd(d); f.et.value = hm(d);
      }
    };
    if (isNew) setTimeout(() => f.title.focus(), 250);
    f.onsubmit = attempt(async (ev) => {
      ev.preventDefault();
      let start, end;
      if (allDay) { start = f.sd.value; end = ymd(addDays(parseYmd(f.ed.value), 1)); }
      else {
        start = new Date(`${f.sd.value}T${f.st.value || '00:00'}`).toISOString();
        end = new Date(`${f.ed.value}T${f.et.value || '00:00'}`).toISOString();
        if (end < start) throw new Error('End is before start');
      }
      let rrule = f.rrule.value || null;
      if (rrule === '__weekly') rrule = `FREQ=WEEKLY;BYDAY=${DAYS[parseYmd(f.sd.value).getDay()]}`;
      const body = { title: f.title.value, all_day: allDay, start, end, rrule, location: f.location.value, description: f.description.value,
        color, calendar_id: f.cal ? +f.cal.value : e?.calendar_id };
      await api(isNew ? '/events' : `/events/${e.id}`, { method: isNew ? 'POST' : 'PUT', body });
      closeSheet(); invalidate(); S.sel = sod(parseYmd(f.sd.value)); render(); toast(isNew ? 'Event added' : 'Saved');
    });
  });
}

function subSheet(c) {
  const isNew = !c;
  let color = c?.color || PALETTE[1], byCourse = c ? c.color_by_course : true;
  const h = `<form id="sf"><div class="ftop"><button type="button" data-cancel>Cancel</button><b>Subscription</b><button class="save" type="submit">Save</button></div>
    <label class="lbl">Name</label><input class="inp" name="name" value="${esc(c?.name || '')}" placeholder="e.g. Langara D2L" required>
    <label class="lbl">Feed URL (.ics)</label><textarea class="inp mono" name="url" rows="3" placeholder="https://…/feed.ics?token=…" ${isNew ? 'required' : ''}>${esc(c?.source_url || '')}</textarea>
    <div id="test"></div>
    <div class="grp">
      <div class="fr"><span>Sync every</span><select name="interval">${[15, 30, 60, 180, 720].map((m) => `<option value="${m}" ${m === (c?.sync_interval_min || 30) ? 'selected' : ''}>${m < 60 ? m + ' min' : m / 60 + ' h'}</option>`).join('')}</select></div>
      <div class="fr"><span>Colour</span>${swatches(color)}</div>
      <div class="fr"><span>Colour by course</span><button type="button" class="tog ${byCourse ? 'on' : ''}" data-bc></button></div>
    </div>
    <div class="btns"><button type="button" class="btn" data-test><svg><use href="#i-sync"/></svg>Test URL</button></div>
    ${isNew ? '' : '<button type="button" class="link danger" data-rm>Remove subscription</button>'}</form>`;
  sheet(h, (el) => {
    const f = $('#sf', el);
    $('[data-cancel]', el).onclick = closeSheet;
    bindSwatches(el, (x) => (color = x));
    $('[data-bc]', el).onclick = (ev) => { byCourse = !byCourse; ev.currentTarget.classList.toggle('on', byCourse); };
    const urlChanged = () => f.url.value.trim() && f.url.value !== (c?.source_url || '');
    $('[data-test]', el).onclick = attempt(async (ev) => {
      ev.currentTarget.classList.add('busy');
      const r = await api('/calendars/test', { method: 'POST', body: urlChanged() ? { url: f.url.value } : { id: c?.id } }).finally(() => ev.currentTarget?.classList.remove('busy'));
      $('#test', el).innerHTML = r.ok
        ? `<div class="note-box ok"><svg><use href="#i-check"/></svg><div>Feed OK${r.name ? `: “${esc(r.name)}”` : ''}<small>${r.events} events · ${r.courses} courses detected</small></div></div>`
        : `<div class="note-box err"><div>Couldn't load feed<small>${esc(r.error)}</small></div></div>`;
      if (r.ok && r.name && !f.name.value) f.name.value = r.name;
    });
    if (!isNew) $('[data-rm]', el).onclick = attempt(async () => {
      if (!confirm(`Remove “${c.name}” and its events?`)) return;
      await api(`/calendars/${c.id}`, { method: 'DELETE' }); closeSheet(); invalidate(); render();
    });
    f.onsubmit = attempt(async (ev) => {
      ev.preventDefault();
      const body = { name: f.name.value, color, color_by_course: byCourse, sync_interval_min: +f.interval.value };
      if (urlChanged()) body.source_url = f.url.value;
      const btn = $('.save', el); btn.textContent = 'Saving…';
      await api(isNew ? '/calendars' : `/calendars/${c.id}`, { method: isNew ? 'POST' : 'PATCH', body: isNew ? { ...body, kind: 'ics' } : body })
        .finally(() => (btn.textContent = 'Save'));
      closeSheet(); invalidate(); render(); toast(isNew ? 'Subscribed' : 'Saved');
    });
  });
}

function courseSheet(c) {
  let color = c.color, hidden = c.hidden;
  sheet(`<form id="cf"><div class="ftop"><button type="button" data-cancel>Cancel</button><b>${esc(courseName(c.code))}</b><button class="save" type="submit">Save</button></div>
    <label class="lbl">Name</label><input class="inp" name="label" value="${esc(c.label)}">
    <div class="grp"><div class="fr"><span>Colour</span>${swatches(color)}</div>
    <div class="fr"><span>Show in calendar</span><button type="button" class="tog ${hidden ? '' : 'on'}" data-show></button></div></div></form>`, (el) => {
    const f = $('#cf', el);
    $('[data-cancel]', el).onclick = closeSheet;
    bindSwatches(el, (x) => (color = x));
    $('[data-show]', el).onclick = (ev) => { hidden = !hidden; ev.currentTarget.classList.toggle('on', !hidden); };
    f.onsubmit = attempt(async (ev) => {
      ev.preventDefault();
      await api(`/courses/${encodeURIComponent(c.code)}`, { method: 'PATCH', body: { label: f.label.value, color, hidden } });
      closeSheet(); invalidate(); render();
    });
  });
}

function localSheet(c) {
  let color = c.color;
  sheet(`<form id="lf"><div class="ftop"><button type="button" data-cancel>Cancel</button><b>Calendar</b><button class="save" type="submit">Save</button></div>
    <label class="lbl">Name</label><input class="inp" name="name" value="${esc(c.name)}" required>
    <div class="grp"><div class="fr"><span>Colour</span>${swatches(color)}</div></div></form>`, (el) => {
    const f = $('#lf', el);
    $('[data-cancel]', el).onclick = closeSheet;
    bindSwatches(el, (x) => (color = x));
    f.onsubmit = attempt(async (ev) => {
      ev.preventDefault();
      await api(`/calendars/${c.id}`, { method: 'PATCH', body: { name: f.name.value, color } });
      closeSheet(); invalidate(); render();
    });
  });
}

// ---------- events / input ----------
function setView(v) {
  S.view = v; S.scrolled = false;
  localStorage.setItem('view', v);
  $('#main').scrollTop = 0;
  render();
}
function shift(dir) {
  if (S.view === 'agenda') S.sel = addDays(S.sel, 7 * dir);
  else if (S.view === 'month') {
    const m = new Date(S.sel.getFullYear(), S.sel.getMonth() + dir, 1);
    const t = today();
    S.sel = m.getMonth() === t.getMonth() && m.getFullYear() === t.getFullYear() ? t : m;
  } else if (S.view === 'week') S.sel = addDays(S.sel, weekDays() * dir);
  render();
}

document.addEventListener('click', (ev) => {
  const t = ev.target.closest('[data-v],[data-act],[data-done],[data-id],[data-day],[data-sync],[data-sub],[data-course],[data-local],.col');
  if (!t || t.closest('.sheet')) return;
  const d = t.dataset;
  if (d.v) return setView(d.v);
  if (d.act === 'today') { S.sel = today(); S.scrolled = false; return render(); }
  if (d.act === 'prev') return shift(-1);
  if (d.act === 'next') return shift(1);
  if (d.act === 'more') { S.agendaDays += 60; return render(); }
  if (d.done) {
    ev.stopPropagation();
    const e = findEvent(d.done, t.closest('[data-s]')?.dataset.s);
    if (!e) return;
    e.done = !e.done; render();
    if (navigator.vibrate) navigator.vibrate(10);
    api(`/events/${e.id}/meta`, { method: 'PUT', body: { done: e.done } }).catch((er) => { e.done = !e.done; render(); toast(er.message); });
    return;
  }
  if (d.id) { const e = findEvent(d.id, d.s); if (e) detail(e); return; }
  if (d.sync) {
    ev.stopPropagation();
    t.classList.add('busy'); t.textContent = 'Syncing…';
    api(`/calendars/${d.sync}/sync`, { method: 'POST' }).then((r) => { toast(r.ok ? r.status : 'Sync failed: ' + r.status); invalidate(); render(); }, (e) => { toast(e.message); render(); });
    return;
  }
  if (d.sub) return subSheet(d.sub === 'new' ? null : S.calendars.find((c) => c.id === +d.sub));
  if (d.course) return courseSheet(S.courses.find((c) => c.code === d.course));
  if (d.local) return localSheet(S.calendars.find((c) => c.id === +d.local));
  if (t.classList.contains('col')) {
    // tap empty space in timeline: new event at that hour
    const r = t.getBoundingClientRect();
    const hr = Math.floor((ev.clientY - r.top) / 52 * 2) / 2;
    const s = parseYmd(t.dataset.day); s.setHours(Math.floor(hr), (hr % 1) * 60);
    return openEditor(s);
  }
  if (d.day) { S.sel = parseYmd(d.day); if (d.go) return setView(d.go); return render(); }
});
const openEditor = async (preset) => {
  if (!S.calendars) await loadSettings().catch(() => {});
  editor(null, preset);
};
$('#fab').onclick = () => openEditor();

// horizontal swipe on strip / month grid / timeline
let sx = 0, sy = 0, sEl = null;
document.addEventListener('touchstart', (e) => { sEl = e.target.closest('#swipe'); sx = e.touches[0].clientX; sy = e.touches[0].clientY; }, { passive: true });
document.addEventListener('touchend', (e) => {
  if (!sEl) return;
  const dx = e.changedTouches[0].clientX - sx, dy = e.changedTouches[0].clientY - sy;
  if (Math.abs(dx) > 60 && Math.abs(dx) > Math.abs(dy) * 1.5) shift(dx < 0 ? 1 : -1);
  sEl = null;
}, { passive: true });
document.addEventListener('keydown', (e) => {
  if (e.target.closest('input,textarea,select')) return;
  if (e.key === 'Escape') closeSheet();
  if (e.key === 'ArrowLeft') shift(-1);
  if (e.key === 'ArrowRight') shift(1);
  if (e.key === 't') { S.sel = today(); render(); }
});

let lastDay = ymd(today());
function refresh() {
  if (document.visibilityState !== 'visible' || $('#sheet-root').innerHTML) return;
  if (ymd(today()) !== lastDay) { lastDay = ymd(today()); S.sel = today(); }
  invalidate(); render();
}
document.addEventListener('visibilitychange', refresh);
setInterval(refresh, 5 * 60e3);
let lastW = weekDays();
addEventListener('resize', () => { if (weekDays() !== lastW) { lastW = weekDays(); if (S.view === 'week') render(); } });

if ('serviceWorker' in navigator) navigator.serviceWorker.register('/sw.js');
render();
