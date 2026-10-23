/* PhotoCraft Studio — помощник внутри редактора.
 *
 * Панель в браузере разговаривает с моделью через api/assistant.php, а действия
 * выполняет в самом редакторе через мост window.__photocraftNative
 * (apps/photocraft-web/src/control_bridge.rs): те же команды, что у десктопного
 * канала управления (docs/control-protocol.md).
 *
 * Без сборщиков и зависимостей: один файл, подключается из index.html.
 */
(() => {
  'use strict';

  const CFG = Object.assign({
    api: 'api/assistant.php',
    labels: 'studio/ru-labels.json',
    language: 'ru',            // язык интерфейса редактора; '' — не трогать
    defaultTheme: 'neuLight',  // тема при первом запуске
    maxSteps: 10,              // сколько шагов подряд помощник может сделать за один запрос
    screenshotMax: 1280,       // длинная сторона снимка экрана для модели, px
    openOnStart: false,
  }, window.STUDIO_CONFIG || {});

  const DARK_THEMES = new Set(['pro', 'proMedium', 'studio', 'solarizedDark', 'adwaitaDark', 'breezeDark', 'softDark', 'neuDark']);
  const READ_TOOLS = new Set(['find_commands', 'get_state', 'look_at_screen']);
  // Никогда: выход, внешние ссылки, служебное, смена темы (у панели своя кнопка).
  const DENY = /^(app\.|help\.|ui\.gpu|window\.theme\.|file\.(quit|exit|print))/;
  // Только после подтверждения пользователя.
  const CONFIRM = /(^file\.(close|revert|save)|delete|remove|clear|purge|flatten|merge|discard|reset)/i;

  // ------------------------------------------------------------------ мост к редактору
  const PC = {
    ready: () => !!(window.__photocraftNative && typeof window.__photocraftNative.call === 'function'),
    async raw(method, params) {
      if (!PC.ready()) throw new Error('Редактор ещё загружается');
      const text = await window.__photocraftNative.call(method, JSON.stringify(params ?? {}));
      return JSON.parse(text);
    },
    async call(method, params) {
      const r = await PC.raw(method, params);
      if (r && r.ok === false) throw new Error(r.error || 'команда не выполнена');
      return r && Object.prototype.hasOwnProperty.call(r, 'result') ? r.result : r;
    },
    exec(id, params) {
      const p = params && typeof params === 'object' && !Array.isArray(params) && Object.keys(params).length ? params : null;
      return PC.call('engine.execute', { command: id, params: p });
    },
  };
  // Для своих скриптов и отладки из консоли: await photocraft.call('ui.inspect')
  window.photocraft = { control: PC.raw, call: PC.call, exec: PC.exec };

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const withTimeout = (p, ms, what) => Promise.race([p, sleep(ms).then(() => { throw new Error(what + ': нет ответа'); })]);
  const clip = (s, n) => (s.length > n ? s.slice(0, n) + '…[обрезано]' : s);
  const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
  const store = {
    get(k, d) { try { const v = localStorage.getItem('studio.' + k); return v === null ? d : JSON.parse(v); } catch { return d; } },
    set(k, v) { try { localStorage.setItem('studio.' + k, JSON.stringify(v)); } catch { /* приватный режим */ } },
  };

  // ------------------------------------------------------------------ русские названия
  let L = { text: {}, id: {} };
  const labelsLoaded = fetch(CFG.labels).then((r) => (r.ok ? r.json() : L)).then((j) => { L = j; }).catch(() => {});
  function ru(label) {
    if (!label) return '';
    if (L.text[label]) return L.text[label];
    const bare = label.replace(/…$/, '');
    if (bare !== label && L.text[bare]) return L.text[bare] + '…';
    return label;
  }
  const ruName = (c) => L.id[c.id] || ru(c.label);
  const ruPath = (c) => [...(c.menu || []).map(ru), ruName(c)].join(' › ');

  // ------------------------------------------------------------------ поиск команд
  let catalog = null;
  let catalogAt = 0;
  async function commands() {
    if (!catalog || Date.now() - catalogAt > 3000) {
      catalog = await PC.call('engine.commands');
      catalogAt = Date.now();
    }
    return catalog;
  }
  // Частые просьбы → слова, которыми такие команды называются.
  const SYN = [
    [/фон|задн/, 'background subject remove select'], [/ярч|светл|темн|ярк|экспоз/, 'brightness contrast levels curves exposure shadows highlights'],
    [/контраст/, 'contrast curves levels'], [/обрез|кадр/, 'crop trim'], [/поверн|разверн|наклон/, 'rotate'],
    [/отраз|зеркал/, 'flip'], [/размер|уменьш|увелич|разрешен/, 'image size canvas size'], [/ч\/?б|черно|чёрно|серы|обесцвет/, 'black white desaturate grayscale'],
    [/резк/, 'sharpen unsharp smart'], [/размы/, 'blur gaussian'], [/шум|зерн/, 'noise'], [/текст|надпис|шрифт/, 'type text character'],
    [/слой|сло[ия]/, 'layer'], [/маск/, 'mask'], [/выдел/, 'select selection'], [/инверт|негатив/, 'invert'],
    [/оттен|насыщ|цвет/, 'hue saturation color balance vibrance'], [/сохран/, 'save'], [/экспорт/, 'export'], [/откр/, 'open'],
    [/нов|созд/, 'new'], [/отмен/, 'undo'], [/тен/, 'shadow'], [/обвод/, 'stroke'], [/залив/, 'fill'], [/градиент/, 'gradient'],
    [/виньет/, 'vignette lens correction'], [/ретуш|кож|прыщ|морщ/, 'healing spot surface blur'], [/красн.*глаз/, 'red eye'],
    [/объедин|свест|слить/, 'merge flatten'], [/дубл|копи/, 'duplicate copy'], [/холст/, 'canvas'], [/автомат|авто/, 'auto'],
    [/уровн/, 'levels'], [/кривы/, 'curves'], [/фильтр/, 'filter'], [/стил|эффект/, 'layer style effects'],
  ];
  const norm = (s) => String(s || '').toLowerCase().replace(/ё/g, 'е');
  async function findCommands(query) {
    const all = await commands();
    await labelsLoaded;
    const q = norm(query).trim();
    if (!q) return { results: [], hint: 'Пустой запрос' };
    const terms = new Set(q.split(/[^\p{L}\p{N}.]+/u).filter((t) => t.length > 1));
    for (const [re, words] of SYN) if (re.test(q)) words.split(' ').forEach((w) => terms.add(w));
    const scored = [];
    for (const c of all) {
      if (DENY.test(c.id)) continue;
      const name = norm(ruName(c));
      const en = norm(c.label);
      const path = norm((c.menu || []).map(ru).join(' ') + ' ' + (c.menu || []).join(' '));
      const id = norm(c.id);
      const kw = norm(c.keywords);
      let s = 0;
      if (name === q || en === q) s += 20;
      else if (name.includes(q) || en.includes(q)) s += 10;
      for (const t of terms) {
        if (name.includes(t)) s += name.startsWith(t) ? 7 : 5;
        if (en.includes(t)) s += en.startsWith(t) ? 6 : 4;
        if (id.includes(t)) s += 3;
        if (kw.includes(t)) s += 3;
        if (path.includes(t)) s += 1;
      }
      if (s > 0) scored.push([s + (c.enabled ? 1 : 0) + ((c.menu || []).length ? 1 : 0), c]);
    }
    scored.sort((a, b) => b[0] - a[0]);
    const results = scored.slice(0, 10).map(([, c]) => ({
      id: c.id,
      name: ruName(c),
      menu: ruPath(c),
      shortcut: c.shortcut || undefined,
      enabled: !!c.enabled,
      why_disabled: c.enabled ? undefined : (c.disabledReason || undefined),
      needs_document: c.needsDocument || undefined,
      params: c.params ? clip(String(c.params), 500) : undefined,
    }));
    return results.length ? { results } : { results: [], hint: 'Ничего не найдено. Попробуй другие слова, можно по-английски (например: curves, crop, gaussian blur).' };
  }

  // ------------------------------------------------------------------ состояние и снимок экрана
  async function snapshot() {
    const ui = await PC.call('ui.inspect');
    const d = ui.document;
    const state = { tool: ui.tool };
    if (d) {
      state.document = {
        name: d.name, width: d.width, height: d.height, mode: d.mode, depth: d.depth, resolution: d.resolution,
        hasSelection: d.hasSelection, selectionBounds: d.selectionBounds || undefined,
        activeLayer: d.activeLayer, canUndo: d.canUndo, canRedo: d.canRedo,
        lastSteps: Array.isArray(d.history) ? d.history.slice(-5).map((h) => (h && (h.name || h.label)) || h) : undefined,
      };
      try {
        const layers = await PC.exec('layer.list', { flat: true });
        const rows = Array.isArray(layers) ? layers : (layers && layers.layers) || [];
        state.layers = rows.slice(0, 40).map((l) => ({
          id: l.id, name: l.name, kind: l.kind, visible: l.visible, active: l.active || undefined, selected: l.selected || undefined, depth: l.depth || undefined,
        }));
        if (rows.length > 40) state.layersMore = rows.length - 40;
      } catch { /* слоёв нет — ничего */ }
    } else {
      state.document = null;
      state.note = 'Документ не открыт';
    }
    if (Array.isArray(ui.dialogs) && ui.dialogs.length) {
      state.dialogs = ui.dialogs.map((g) => ({ id: g.id, kind: g.kind, title: g.title, fields: g.fields }));
    }
    if (ui.statusError || ui.status) state.status = ui.statusError || ui.status;
    return state;
  }

  function downscale(src) {
    return new Promise((resolve, reject) => {
      const img = new Image();
      img.onload = () => {
        const k = Math.min(1, CFG.screenshotMax / Math.max(img.width, img.height));
        const c = document.createElement('canvas');
        c.width = Math.max(1, Math.round(img.width * k));
        c.height = Math.max(1, Math.round(img.height * k));
        c.getContext('2d').drawImage(img, 0, 0, c.width, c.height);
        resolve(c.toDataURL('image/jpeg', 0.84).split(',')[1]);
      };
      img.onerror = () => reject(new Error('не удалось прочитать снимок'));
      img.src = src;
    });
  }
  // Снимок экрана. В браузере ui.screenshot не поддерживается (eframe не умеет), а холст
  // WebGL/WebGPU очищается после показа кадра. Поэтому читаем его сразу после того, как
  // редактор нарисовал кадр: оборачиваем requestAnimationFrame и снимаем в том же такте.
  let frameWaiters = [];
  const origRAF = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (cb) => origRAF((t) => {
    cb(t);
    if (!frameWaiters.length) return;
    const waiters = frameWaiters;
    frameWaiters = [];
    let url = null;
    try { url = document.getElementById('photocraft_canvas').toDataURL('image/png'); } catch { /* холст недоступен */ }
    waiters.forEach((w) => w(url));
  });
  const nextFrame = () => new Promise((resolve) => { frameWaiters.push(resolve); });
  // Однотонный снимок — значит, попали между кадрами.
  function looksBlank(src) {
    return new Promise((resolve) => {
      const img = new Image();
      img.onload = () => {
        const c = document.createElement('canvas');
        c.width = 48; c.height = 32;
        const g = c.getContext('2d');
        g.drawImage(img, 0, 0, 48, 32);
        const d = g.getImageData(0, 0, 48, 32).data;
        const seen = new Set();
        for (let i = 0; i < d.length && seen.size < 6; i += 4 * 5) seen.add(d[i] + ',' + d[i + 1] + ',' + d[i + 2] + ',' + d[i + 3]);
        resolve(seen.size < 4);
      };
      img.onerror = () => resolve(true);
      img.src = src;
    });
  }
  async function screenshot() {
    if (!PC.ready()) return null;
    for (let attempt = 0; attempt < 4; attempt++) {
      // Любой запрос к редактору заставляет его нарисовать новый кадр.
      PC.raw('jobs.list').catch(() => {});
      const url = await Promise.race([nextFrame(), sleep(1500).then(() => null)]);
      if (url && !(await looksBlank(url))) {
        try { return await downscale(url); } catch { return null; }
      }
    }
    return null;
  }

  // ------------------------------------------------------------------ интерфейс
  const ICON = {
    spark: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l1.9 4.6L18.5 9.5l-4.6 1.9L12 16l-1.9-4.6L5.5 9.5l4.6-1.9z"/><path d="M19 15l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z"/></svg>',
    bulb: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9c.6.5 1 1.2 1 2.1h5c0-.9.4-1.6 1-2.1A6 6 0 0 0 12 3z"/></svg>',
    hand: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 4l6 6-9 9H5v-6z"/><path d="M12.5 5.5l6 6"/></svg>',
    sun: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>',
    moon: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5z"/></svg>',
    close: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M13 5l7 7-7 7M5 5l7 7-7 7"/></svg>',
    eye: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/></svg>',
    send: '<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h13M13 6l6 6-6 6"/></svg>',
    stop: '<svg viewBox="0 0 24 24" fill="currentColor"><rect x="7" y="7" width="10" height="10" rx="2"/></svg>',
  };

  const root = document.createElement('div');
  root.id = 'studio-root';
  root.dataset.theme = 'light';
  root.innerHTML = `
    <div class="st-veil" aria-hidden="true"></div>
    <div class="st-drop" aria-hidden="true"><span>Отпустите, чтобы открыть в редакторе</span></div>
    <aside class="st-panel" aria-label="Помощник">
      <header class="st-head">
        <div class="st-logo st-raised-sm" aria-hidden="true"><span></span></div>
        <div class="st-title"><b>Помощник</b><small data-ref="sub">Загрузка редактора…</small></div>
        <button class="st-icon-btn" data-ref="theme" title="Светлая / тёмная тема" aria-label="Светлая или тёмная тема">${ICON.moon}</button>
        <button class="st-icon-btn" data-ref="hide" title="Свернуть панель" aria-label="Свернуть панель">${ICON.close}</button>
      </header>
      <div class="st-seg st-pressed" role="group" aria-label="Режим помощника">
        <button data-mode="advice" aria-pressed="true">${ICON.bulb}Совет</button>
        <button data-mode="act" aria-pressed="false">${ICON.hand}Сделай</button>
      </div>
      <div class="st-mode-hint" data-ref="hint"></div>
      <div class="st-log" data-ref="log" role="log" aria-live="polite"></div>
      <div class="st-composer">
        <div class="st-input st-pressed">
          <button class="st-icon-btn" data-ref="eye" aria-pressed="true" title="Показывать помощнику экран" aria-label="Показывать помощнику экран">${ICON.eye}</button>
          <textarea data-ref="input" rows="1" placeholder="Спросите или попросите…" aria-label="Сообщение помощнику"></textarea>
          <button class="st-send" data-ref="send" title="Отправить (Enter)" aria-label="Отправить">${ICON.send}</button>
        </div>
        <div class="st-foot"><span>Ctrl+Z отменяет действия помощника</span><button data-ref="clear">Новый разговор</button></div>
      </div>
    </aside>
    <button class="st-fab" data-ref="fab" title="Помощник" aria-label="Открыть помощника">${ICON.spark}</button>`;
  const $ = (name) => root.querySelector(`[data-ref="${name}"]`);
  const log = $('log');
  const input = $('input');

  // Клавиши, набранные в панели, не должны доходить до редактора (иначе «B» выберет кисть).
  for (const type of ['keydown', 'keyup', 'keypress', 'paste', 'copy', 'cut']) {
    root.addEventListener(type, (e) => e.stopPropagation());
  }

  let mode = store.get('mode', 'advice');
  let attachScreen = store.get('eye', true);
  let busy = false;
  let stopRequested = false;
  let convo = [];

  const HINTS = {
    advice: 'Объясняю и подсказываю, ничего не меняю в документе.',
    act: 'Делаю сам: нажимаю команды и настраиваю диалоги. Всё отменяется Ctrl+Z.',
  };
  function setMode(m) {
    mode = m === 'act' ? 'act' : 'advice';
    store.set('mode', mode);
    root.querySelectorAll('.st-seg button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.mode === mode)));
    $('hint').textContent = HINTS[mode];
  }
  function setEye(on) {
    attachScreen = !!on;
    store.set('eye', attachScreen);
    $('eye').setAttribute('aria-pressed', String(attachScreen));
    $('eye').title = attachScreen ? 'Помощник видит экран' : 'Помощник не видит экран';
  }
  function setOpen(open) {
    root.classList.toggle('open', open);
    document.body.classList.toggle('studio-open', open);
    store.set('open', open);
    // eframe подхватывает новый размер canvas на следующем кадре
    window.dispatchEvent(new Event('resize'));
    if (open) setTimeout(() => input.focus(), 200);
  }
  function setBusy(on) {
    busy = on;
    root.classList.toggle('busy', on);
    $('send').innerHTML = on ? ICON.stop : ICON.send;
    $('send').title = on ? 'Остановить' : 'Отправить (Enter)';
  }
  function scroll() { log.scrollTop = log.scrollHeight; }

  function format(text) {
    let h = esc(text.trim());
    h = h.replace(/`([^`\n]+)`/g, '<code>$1</code>');
    h = h.replace(/\*\*([^*\n]+)\*\*/g, '<b>$1</b>');
    h = h.replace(/\((Ctrl|Shift|Alt|Cmd|⌘|⇧|⌥)([^)\n]{0,24})\)/g, '(<kbd>$1$2</kbd>)');
    // «Изображение › Коррекция › Кривые» — путь в меню
    // Сегменты — слова через одиночные пробелы: конечный пробел не должен «съедать» разделитель.
    const C = '[^\\s›«»"():.,!?;]';
    const word = `${C}+`;
    const path = new RegExp(`([А-ЯЁA-Z]${C}*(?: ${word}){0,2}(?: › ${word}(?: ${word}){0,3}){1,4})`, 'g');
    h = h.replace(path, '<span class="st-path">$1</span>');
    return h;
  }
  function add(el) { log.querySelector('.st-empty')?.remove(); log.appendChild(el); scroll(); return el; }
  function msg(cls, html) { const d = document.createElement('div'); d.className = 'st-msg ' + cls; d.innerHTML = html; return add(d); }
  function typing(on) {
    log.querySelector('.st-typing')?.remove();
    if (on) { const t = document.createElement('div'); t.className = 'st-typing st-raised-sm'; t.innerHTML = '<i></i><i></i><i></i>'; add(t); }
  }

  const ACTION_TEXT = {
    find_commands: (a) => ['Ищу команду', a.query],
    get_state: () => ['Смотрю состояние документа', ''],
    look_at_screen: () => ['Смотрю на экран', ''],
    select_tool: (a) => ['Инструмент', a.tool],
    set_dialog_field: (a) => ['Поле диалога', `${a.field} = ${JSON.stringify(a.value)}`],
    confirm_dialog: () => ['Нажимаю OK', ''],
    cancel_dialog: () => ['Отменяю диалог', ''],
    undo: (a) => ['Отменяю', `${a.steps || 1} шаг(а)`],
  };
  function card(name, args, label) {
    const d = document.createElement('div');
    const [title, detail] = label || (ACTION_TEXT[name] ? ACTION_TEXT[name](args) : [name, '']);
    d.className = 'st-act' + (READ_TOOLS.has(name) ? ' read' : '');
    d.innerHTML = `<span class="st-dot"></span><div class="st-act-text"><b>${esc(title)}</b>${detail ? `<small>${esc(detail)}</small>` : ''}</div>`;
    return add(d);
  }
  function cardResult(d, ok, error, undoable) {
    if (!ok) {
      d.classList.add('fail');
      const s = d.querySelector('small') || d.querySelector('.st-act-text').appendChild(document.createElement('small'));
      s.textContent = error;
      return;
    }
    if (undoable) {
      const b = document.createElement('button');
      b.className = 'st-mini';
      b.textContent = 'Отменить';
      b.onclick = async () => {
        b.disabled = true;
        try { await PC.exec('edit.undo'); b.textContent = 'Отменено'; } catch (e) { b.textContent = 'Не вышло'; b.title = e.message; }
      };
      d.appendChild(b);
    }
  }
  function confirmAction(text) {
    return new Promise((resolve) => {
      const d = document.createElement('div');
      d.className = 'st-confirm st-raised';
      d.innerHTML = `<p>${esc(text)}</p><div class="st-row"><button class="st-mini danger" data-a="no">Не надо</button><button class="st-mini" data-a="yes">Разрешить</button></div>`;
      add(d);
      d.querySelectorAll('button').forEach((b) => {
        b.onclick = () => { d.remove(); resolve(b.dataset.a === 'yes'); };
      });
    });
  }

  function showEmpty() {
    log.innerHTML = `<div class="st-empty">
      <h3>Чем помочь?</h3>
      <p>Спросите, где найти функцию, или попросите сделать что-то за вас.</p>
      <div class="st-chips">
        <button class="st-chip">Как вырезать объект из фона?</button>
        <button class="st-chip" data-act="1">Сделай фото ярче и контрастнее</button>
        <button class="st-chip" data-act="1">Обрежь до квадрата</button>
        <button class="st-chip">Что делают «Кривые»?</button>
      </div></div>`;
    log.querySelectorAll('.st-chip').forEach((c) => {
      c.onclick = () => { if (c.dataset.act) setMode('act'); input.value = c.textContent; send(); };
    });
  }

  // ------------------------------------------------------------------ инструменты для модели
  async function topDialog() {
    const ui = await PC.call('ui.inspect');
    const list = ui.dialogs || [];
    if (!list.length) throw new Error('Нет открытого диалога');
    return list[list.length - 1];
  }
  async function commandById(id) {
    const c = (await commands()).find((x) => x.id === id);
    if (!c) throw new Error(`Нет команды «${id}». Найди нужную через find_commands.`);
    if (DENY.test(id)) throw new Error('Эту команду помощнику выполнять нельзя.');
    return c;
  }
  const short = (v) => (v === undefined || v === null ? { ok: true } : { ok: true, result: v });

  async function runTool(name, args) {
    if (mode !== 'act' && !READ_TOOLS.has(name)) {
      return { ok: false, error: 'Сейчас режим «Совет»: действия выключены. Объясни пользователю, как сделать это самому, или предложи переключиться в «Сделай».' };
    }
    switch (name) {
      case 'find_commands': {
        const d = card(name, args);
        const r = await findCommands(String(args.query || ''));
        cardResult(d, true);
        return r;
      }
      case 'get_state': {
        card(name, args);
        return await snapshot();
      }
      case 'look_at_screen': {
        const d = card(name, args);
        const img = await screenshot();
        cardResult(d, !!img, 'Снимок не получился');
        return img ? { ok: true, note: 'Снимок приложен следующим сообщением.', __image: img } : { ok: false, error: 'Снимок экрана не получился' };
      }
      case 'run_command':
      case 'open_command': {
        const c = await commandById(String(args.id || ''));
        const label = [name === 'open_command' ? 'Открываю' : 'Выполняю', ruPath(c)];
        if (CONFIRM.test(c.id) && !(await confirmAction(`Помощник хочет выполнить «${ruPath(c)}». Разрешить?`))) {
          card(name, args, ['Пропущено пользователем', ruPath(c)]).classList.add('fail');
          return { ok: false, error: 'Пользователь не разрешил это действие. Не повторяй его, спроси, как поступить.' };
        }
        const d = card(name, args, label);
        root.classList.add('acting');
        try {
          const r = name === 'open_command'
            ? await PC.call('ui.menu.invoke', { id: c.id })
            : await PC.exec(c.id, args.params && typeof args.params === 'object' ? args.params : null);
          cardResult(d, true, '', name === 'run_command');
          return short(r && typeof r === 'object' ? JSON.parse(clip(JSON.stringify(r), 3000).replace(/…\[обрезано\]$/, '') || 'null') : r);
        } catch (e) {
          cardResult(d, false, e.message);
          return { ok: false, error: e.message, params_doc: c.params ? clip(String(c.params), 500) : undefined };
        } finally {
          root.classList.remove('acting');
        }
      }
      case 'set_dialog_field': {
        const d = card(name, args);
        try {
          const g = await topDialog();
          const r = await PC.call('ui.dialog.set', { dialog: g.id, field: String(args.field), value: args.value });
          cardResult(d, true);
          return short(r);
        } catch (e) { cardResult(d, false, e.message); return { ok: false, error: e.message }; }
      }
      case 'confirm_dialog':
      case 'cancel_dialog': {
        const d = card(name, args);
        try {
          const g = await topDialog();
          const r = await PC.call(name === 'confirm_dialog' ? 'ui.dialog.confirm' : 'ui.dialog.cancel', { dialog: g.id });
          cardResult(d, true, '', name === 'confirm_dialog');
          return short(r);
        } catch (e) { cardResult(d, false, e.message); return { ok: false, error: e.message }; }
      }
      case 'select_tool': {
        const d = card(name, args);
        try { await PC.call('ui.set', { tool: String(args.tool) }); cardResult(d, true); return { ok: true }; } catch (e) { cardResult(d, false, e.message); return { ok: false, error: e.message }; }
      }
      case 'undo': {
        const n = Math.max(1, Math.min(20, parseInt(args.steps, 10) || 1));
        const d = card(name, { steps: n });
        try { for (let i = 0; i < n; i++) await PC.exec('edit.undo'); cardResult(d, true); return { ok: true }; } catch (e) { cardResult(d, false, e.message); return { ok: false, error: e.message }; }
      }
      default:
        return { ok: false, error: `Нет инструмента ${name}` };
    }
  }

  // Модель иногда пишет вызов инструмента текстом вместо tool_calls — подхватываем его.
  const TOOL_NAMES = ['find_commands', 'get_state', 'look_at_screen', 'run_command', 'open_command', 'set_dialog_field', 'confirm_dialog', 'cancel_dialog', 'select_tool', 'undo'];
  function inlineCalls(text) {
    if (!text || !/[{[]/.test(text)) return [];
    const m = text.match(/\{[\s\S]*\}/);
    if (!m) return [];
    try {
      const o = JSON.parse(m[0]);
      const list = Array.isArray(o) ? o : [o];
      return list
        .filter((x) => x && TOOL_NAMES.includes(x.name))
        .map((x) => ({ function: { name: x.name, arguments: x.arguments || x.parameters || {} } }));
    } catch { return []; }
  }

  // ------------------------------------------------------------------ разговор с моделью
  function outgoing() {
    // Картинки занимают много контекста: оставляем только две последние.
    const out = convo.slice(-60).map((m) => ({ ...m }));
    let images = 0;
    for (let i = out.length - 1; i >= 0; i--) {
      if (!out[i].images) continue;
      if (images >= 2) delete out[i].images; else images += out[i].images.length;
    }
    return out;
  }
  async function ask() {
    const r = await fetch(CFG.api, {
      method: 'POST',
      credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ mode, messages: outgoing() }),
    });
    let j;
    try { j = await r.json(); } catch { throw new Error(`Сервер помощника ответил не JSON (HTTP ${r.status}). Проверьте, что PHP работает в папке api/.`); }
    if (!j.ok) {
      const e = new Error(j.error || 'Ошибка помощника');
      e.noVision = !!j.noVision;
      throw e;
    }
    return j;
  }

  async function send() {
    if (busy) { stopRequested = true; return; }
    const text = input.value.trim();
    if (!text) return;
    input.value = '';
    autosize();
    setBusy(true);
    stopRequested = false;
    msg('user', esc(text));
    try {
      let state = null;
      let image = null;
      if (PC.ready()) {
        state = await snapshot().catch((e) => ({ error: e.message }));
        if (attachScreen) image = await screenshot();
      }
      convo.push({
        role: 'user',
        content: text + '\n\n[Состояние редактора — автоматически]\n' + clip(JSON.stringify(state ?? { note: 'Редактор ещё загружается' }), 4000),
        ...(image ? { images: [image] } : {}),
      });
      for (let step = 0; step < CFG.maxSteps; step++) {
        if (stopRequested) { msg('bot', 'Остановлено.'); break; }
        typing(true);
        const j = await ask();
        typing(false);
        const m = j.message || {};
        let calls = Array.isArray(m.tool_calls) ? m.tool_calls : [];
        let content = m.content || '';
        if (!calls.length) {
          calls = inlineCalls(content);
          if (calls.length) content = content.replace(/\{[\s\S]*\}/, '').trim();
        }
        convo.push({ role: 'assistant', content: m.content || '', ...(calls.length ? { tool_calls: calls } : {}) });
        if (content) msg('bot', format(content));
        if (!calls.length) break;
        let pendingImage = null;
        for (const c of calls) {
          if (stopRequested) break;
          const fname = c.function && c.function.name;
          let a = (c.function && c.function.arguments) || {};
          if (typeof a === 'string') { try { a = JSON.parse(a || '{}'); } catch { a = {}; } }
          let result;
          try { result = await runTool(fname, a || {}); } catch (e) { result = { ok: false, error: e.message || String(e) }; }
          if (result && result.__image) { pendingImage = result.__image; delete result.__image; }
          convo.push({ role: 'tool', tool_name: fname, content: clip(JSON.stringify(result ?? { ok: true }), 6000) });
        }
        if (pendingImage) convo.push({ role: 'user', content: '[Снимок экрана после действий — прислан автоматически]', images: [pendingImage] });
        if (step === CFG.maxSteps - 1) msg('bot', `Сделал ${CFG.maxSteps} шагов подряд и остановился. Напишите «продолжай», если нужно дальше.`);
      }
    } catch (e) {
      typing(false);
      if (e.noVision) setEye(false);
      msg('err', esc(e.message || String(e)));
    } finally {
      typing(false);
      setBusy(false);
      root.classList.remove('acting');
    }
  }

  // ------------------------------------------------------------------ буфер обмена и перетаскивание
  // Браузер отдаёт картинку из буфера обмена только странице (в событии paste), поэтому
  // Ctrl+V перехватывается здесь: картинка уходит в редактор (setClipboardImage), а затем
  // выполняется его «Вставить» — новый слой, а без документа — новый документ, как в Photoshop.
  const native = () => window.__photocraftNative;
  const fromPanel = (e) => root.contains(e.target);
  const editingText = () => { try { return !!native().wantsKeyboard(); } catch { return false; } };
  let pasteCommand = null;   // какую вставку просили с клавиатуры
  let pasteTimer = 0;
  let clipMirrored = false;  // последняя копия из редактора дошла до системного буфера

  function toast(text, error) {
    let t = root.querySelector('.st-toast');
    if (!t) { t = document.createElement('div'); t.className = 'st-toast'; root.appendChild(t); }
    t.textContent = text;
    t.classList.toggle('err', !!error);
    t.classList.add('show');
    clearTimeout(t._timer);
    t._timer = setTimeout(() => t.classList.remove('show'), 3200);
  }
  async function runPaste(id) {
    try {
      const r = await PC.exec(id);
      if (r && r.pasted === false) toast('В буфере обмена нет картинки');
    } catch (e) {
      toast('Не удалось вставить: ' + (e.message || e), true);
    }
  }
  async function decodeImage(file) {
    const bmp = await createImageBitmap(file);
    if (bmp.width * bmp.height > 16384 * 16384) throw new Error('картинка слишком большая');
    const c = document.createElement('canvas');
    c.width = bmp.width; c.height = bmp.height;
    const g = c.getContext('2d', { willReadFrequently: true });
    g.drawImage(bmp, 0, 0);
    bmp.close?.();
    const data = g.getImageData(0, 0, c.width, c.height).data;
    return { w: c.width, h: c.height, rgba: new Uint8Array(data.buffer, data.byteOffset, data.byteLength) };
  }

  window.addEventListener('keydown', (e) => {
    if (fromPanel(e) || !PC.ready() || e.altKey || !(e.ctrlKey || e.metaKey) || e.code !== 'KeyV') return;
    if (editingText()) return; // в текстовом поле редактора Ctrl+V вставляет текст
    pasteCommand = e.shiftKey ? 'edit.pasteSpecial.pasteInPlace' : 'edit.paste';
    // Редактор не должен вставлять сам: содержимое буфера придёт следом, в событии paste.
    e.stopImmediatePropagation();
    clearTimeout(pasteTimer);
    // Если браузер так и не пришлёт paste — вставляем то, что скопировано внутри редактора.
    pasteTimer = setTimeout(() => { if (pasteCommand) { const id = pasteCommand; pasteCommand = null; runPaste(id); } }, 600);
  }, true);

  window.addEventListener('paste', (e) => {
    if (fromPanel(e) || !PC.ready()) return;
    const id = pasteCommand || 'edit.paste';
    pasteCommand = null;
    clearTimeout(pasteTimer);
    const items = [...((e.clipboardData && e.clipboardData.items) || [])];
    const item = items.find((i) => i.kind === 'file' && i.type.startsWith('image/'));
    if (!item) {
      if (editingText()) return; // текст — в поле ввода редактора
      // Скопировали что-то другое после картинки из редактора: та картинка уже не в буфере.
      if (clipMirrored) { native().clearClipboardImage(); clipMirrored = false; }
      const hasText = !!e.clipboardData && [...e.clipboardData.types].includes('text/plain');
      if (!hasText) {
        e.preventDefault();
        e.stopImmediatePropagation();
        runPaste(id);
        return;
      }
      // Текст получает редактор (инструмент «Текст» вставит его в надпись); если надпись не
      // редактируется — вставляем то, что скопировано внутри редактора.
      PC.call('ui.inspect').then((ui) => { if (!ui.textEdit) runPaste(id); }).catch(() => {});
      return;
    }
    e.preventDefault();
    e.stopImmediatePropagation();
    const file = item.getAsFile();
    if (!file) return;
    decodeImage(file)
      .then(({ w, h, rgba }) => {
        if (!native().setClipboardImage(w, h, rgba)) throw new Error('картинка не подходит');
        return runPaste(id);
      })
      .catch((err) => toast('Не удалось вставить картинку: ' + (err.message || err), true));
  }, true);

  // Копирование из редактора (Ctrl+C, Скопировать совмещённые) — в системный буфер обмена,
  // чтобы вставить в другие программы. Нужен HTTPS; без него копия остаётся внутри редактора.
  window.__studioClipboardWrite = (w, h, rgba) => {
    clipMirrored = false;
    if (!window.isSecureContext || !navigator.clipboard || !window.ClipboardItem) return;
    try {
      const c = document.createElement('canvas');
      c.width = w; c.height = h;
      c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(rgba.buffer, rgba.byteOffset, rgba.byteLength), w, h), 0, 0);
      const png = new Promise((resolve, reject) => c.toBlob((b) => (b ? resolve(b) : reject(new Error('png'))), 'image/png'));
      navigator.clipboard.write([new window.ClipboardItem({ 'image/png': png })]).then(() => { clipMirrored = true; }, () => {});
    } catch { /* копия остаётся внутри редактора */ }
  };

  // Перетаскивание файлов. На холст их принимает сам редактор (на документ — новым слоем,
  // мимо — открывает). Брошенные на панель помощника или край страницы файлы браузер открыл
  // бы вместо редактора — передаём их редактору: они открываются как новые документы.
  const isFileDrag = (e) => !!e.dataTransfer && [...e.dataTransfer.types].includes('Files');
  let dropHideTimer = 0;
  function showDrop(on) {
    root.classList.toggle('dropping', on);
    clearTimeout(dropHideTimer);
    if (on) dropHideTimer = setTimeout(() => root.classList.remove('dropping'), 400);
  }
  window.addEventListener('dragover', (e) => {
    if (!isFileDrag(e)) return;
    e.preventDefault();
    showDrop(true);
  });
  window.addEventListener('drop', (e) => {
    showDrop(false);
    if (!isFileDrag(e)) return;
    const canvas = document.getElementById('photocraft_canvas');
    if (!canvas || e.target === canvas) return;
    e.preventDefault();
    const files = [...e.dataTransfer.files];
    if (!files.length) return;
    const dt = new DataTransfer();
    files.forEach((f) => dt.items.add(f));
    // Точка у нижнего края холста — строка состояния, не документ: файлы откроются отдельно.
    const r = canvas.getBoundingClientRect();
    const opts = { bubbles: true, cancelable: true, dataTransfer: dt, clientX: r.left + r.width / 2, clientY: r.bottom - 4 };
    for (const type of ['dragenter', 'dragover', 'drop']) canvas.dispatchEvent(new DragEvent(type, opts));
    toast(files.length === 1 ? 'Открываю ' + files[0].name : 'Открываю файлы: ' + files.length);
  });

  // ------------------------------------------------------------------ тема и запуск
  function applyPanelTheme(themeId) {
    root.dataset.theme = DARK_THEMES.has(themeId) ? 'dark' : 'light';
    $('theme').innerHTML = root.dataset.theme === 'dark' ? ICON.sun : ICON.moon;
  }
  async function setTheme(themeId) {
    store.set('theme', themeId);
    applyPanelTheme(themeId);
    if (PC.ready()) await PC.exec('window.theme.' + themeId).catch(() => {});
  }
  async function watchTheme() {
    // Тему можно сменить и в меню редактора (Окно › Тема): панель подстраивается.
    for (;;) {
      await sleep(2500);
      if (document.hidden || !PC.ready()) continue;
      try {
        const t = await PC.exec('prefs.get', { path: 'interface.theme' });
        const id = typeof t === 'string' ? t : (t && t.value);
        if (id) applyPanelTheme(id);
      } catch { /* не важно */ }
    }
  }

  async function status() {
    try {
      const r = await fetch(CFG.api, { credentials: 'same-origin' });
      const j = await r.json();
      if (j.ok) $('sub').textContent = 'Модель: ' + j.model;
      else $('sub').textContent = j.login ? 'Нужно войти в систему' : (j.error || 'Нет связи с помощником');
    } catch {
      $('sub').textContent = 'Нет связи с сервером помощника';
    }
  }

  function autosize() { input.style.height = 'auto'; input.style.height = Math.min(160, input.scrollHeight) + 'px'; }

  function mount() {
    document.body.appendChild(root);
    root.querySelectorAll('.st-seg button').forEach((b) => { b.onclick = () => setMode(b.dataset.mode); });
    $('eye').onclick = () => setEye(!attachScreen);
    $('hide').onclick = () => setOpen(false);
    $('fab').onclick = () => setOpen(true);
    $('send').onclick = send;
    $('clear').onclick = () => { if (!busy) { convo = []; showEmpty(); } };
    $('theme').onclick = () => setTheme(root.dataset.theme === 'dark' ? 'neuLight' : 'neuDark');
    input.addEventListener('input', autosize);
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(); }
    });
    setMode(mode);
    setEye(attachScreen);
    applyPanelTheme(store.get('theme', CFG.defaultTheme));
    showEmpty();
    status();
  }

  async function boot() {
    mount();
    // Ждём, пока WebAssembly загрузится и поставит мост.
    while (!PC.ready()) await sleep(200);
    await sleep(300);
    root.classList.add('ready');
    if (CFG.language) await PC.exec('prefs.set', { path: 'interface.language', value: CFG.language }).catch(() => {});
    await setTheme(store.get('theme', CFG.defaultTheme));
    if (store.get('open', CFG.openOnStart)) setOpen(true);
    commands().catch(() => {});
    watchTheme();
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', boot);
  else boot();
})();
