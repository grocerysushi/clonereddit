const commands = ['bold', 'italic', 'link', 'bullets', 'numbered', 'quote'];

function attach(editor, wasm) {
  const body = editor.querySelector('textarea[name=body]');
  const controls = editor.querySelector('[data-post-format-controls]');
  const status = editor.querySelector('[data-post-format-status]');
  if (!body || !controls) return;

  controls.addEventListener('click', (event) => {
    const button = event.target.closest('button[data-post-format]');
    if (!button || !controls.contains(button)) return;
    const command = commands.indexOf(button.dataset.postFormat);
    if (command < 0) return;
    const capacity = wasm.post_format_capacity();
    const value = body.value;
    const length = value.length;
    if (length <= capacity) {
      const input = new Uint16Array(wasm.memory.buffer, wasm.post_format_input(), length);
      for (let index = 0; index < length; index++) input[index] = value.charCodeAt(index);
    }
    const result = wasm.post_format_apply(command, length, body.selectionStart, body.selectionEnd,
      body.maxLength < 0 ? capacity : body.maxLength);
    if (status) status.textContent = result === 2 ? 'The formatting would exceed the body text limit.' : '';
    if (result !== 1) return;

    const [pointer, size, from, to, start, end] = [0, 1, 2, 3, 4, 5]
      .map((part) => wasm.post_format_part(part));
    const output = new Uint16Array(wasm.memory.buffer, pointer, size);
    body.setRangeText(String.fromCharCode(...output), from, to);
    body.focus({ preventScroll: true });
    body.setSelectionRange(start, end);
    body.dispatchEvent(new Event('input', { bubbles: true }));
  });
  if (status) body.addEventListener('input', () => { status.textContent = ''; });
  controls.hidden = false;
}

async function start(editors) {
  try {
    const response = await fetch('/post-format.wasm');
    if (!response.ok) return;
    const { instance } = await WebAssembly.instantiate(await response.arrayBuffer());
    for (const editor of editors) attach(editor, instance.exports);
  } catch {
    // The Markdown textarea and formatting help still work without WebAssembly.
  }
}

const editors = document.querySelectorAll('[data-post-format-editor]');
if (editors.length) start(editors);
