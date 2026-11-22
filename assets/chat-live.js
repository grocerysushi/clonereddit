const states = [
  ['live', 'Live updates on.'],
  ['retrying', 'Reconnecting to chat…'],
  ['signed-out', 'Sign in to resume live chat.'],
  ['unavailable', 'This chat is unavailable.'],
];

function unsigned64(text) {
  if (!/^\d{1,20}$/.test(text ?? '')) throw new Error('Invalid chat cursor');
  const value = BigInt(text);
  if (value > 18446744073709551615n) throw new Error('Invalid chat cursor');
  return value;
}

function fragment(html, wasm) {
  const document = new DOMParser().parseFromString(html, 'text/html');
  const delta = document.querySelector('template[data-live-cursor][data-live-unread]');
  if (!delta) throw new Error('Missing chat delta');
  const cursor = unsigned64(delta.dataset.liveCursor);
  const unread = unsigned64(delta.dataset.liveUnread);
  const rows = Array.from(delta.content.children);
  for (const row of rows) {
    if (row.localName !== 'li' || !wasm.chat_row(unsigned64(row.dataset.messageId))) {
      throw new Error('Invalid chat row');
    }
  }
  return { cursor, unread, rows };
}

async function attach(root) {
  const list = root.querySelector('[data-live-messages]');
  const read = root.querySelector('[data-live-read]');
  const status = root.querySelector('[data-live-status]');
  if (!list || !read || !status || !root.dataset.livePeer || !root.dataset.liveViewer) return;

  function show([state, text]) {
    if (root.dataset.liveState === state) return;
    root.dataset.liveState = state;
    status.textContent = text;
  }

  list.scrollTop = list.scrollHeight;
  list.tabIndex = 0;
  let wasm;
  try {
    const response = await fetch('/chat-live.wasm');
    if (!response.ok) throw new Error('Chat WASM unavailable');
    const { instance } = await WebAssembly.instantiate(await response.arrayBuffer());
    wasm = instance.exports;
    wasm.chat_init(unsigned64(root.dataset.liveCursor));
  } catch {
    show(['unavailable', 'Live updates unavailable.']);
    return;
  }

  const endpoint = new URL('/messages/live', location.href);
  endpoint.searchParams.set('peer', root.dataset.livePeer);
  endpoint.searchParams.set('viewer', root.dataset.liveViewer);
  let timer;
  let inFlight;

  function schedule() {
    clearTimeout(timer);
    const delay = wasm.chat_delay_ms();
    if (delay >= 0) timer = setTimeout(poll, delay);
  }

  function visibility() {
    wasm.chat_set_visible(Number(!document.hidden));
    if (document.hidden && inFlight) {
      inFlight.paused = true;
      inFlight.abort.abort();
    }
    schedule();
  }

  function append(delta) {
    const follow = list.scrollHeight - list.scrollTop - list.clientHeight <= 48;
    const rows = delta.rows.map((row) => document.importNode(row, true));
    list.append(...rows);
    root.dataset.liveCursor = String(delta.cursor);
    root.dataset.liveUnread = String(delta.unread);
    read.hidden = delta.unread === 0n;
    if (follow) list.scrollTop = list.scrollHeight;
  }

  async function poll() {
    const timeout = wasm.chat_begin();
    if (!timeout) return;
    const request = { abort: new AbortController(), paused: false };
    inFlight = request;
    const deadline = setTimeout(() => request.abort.abort(), timeout);
    let result = 0;
    let delta;
    try {
      endpoint.searchParams.set('after', BigInt.asUintN(64, wasm.chat_cursor()).toString());
      const response = await fetch(endpoint, { cache: 'no-store', signal: request.abort.signal });
      if (response.status === 200) delta = fragment(await response.text(), wasm);
      result = response.status;
    } catch {
      // A transport, timeout, or invalid reply is a retry signal for Rust.
    } finally {
      clearTimeout(deadline);
      inFlight = undefined;
    }
    const apply = wasm.chat_finish(request.paused ? 1 : result, delta?.cursor ?? 0n);
    if (apply) append(delta);
    show(states[wasm.chat_status()]);
    schedule();
  }

  document.addEventListener('visibilitychange', visibility);
  show(states[wasm.chat_status()]);
  visibility();
}

const conversation = document.querySelector('[data-live-chat]');
if (conversation) attach(conversation);
