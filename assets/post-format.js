const commands = ['bold', 'italic', 'link', 'bullets', 'numbered', 'quote', 'code'];
const imageKeyLength = 48;
const maxDescriptionLength = 160;

function writeText(wasm, pointer, text) {
  const input = new Uint16Array(wasm.memory.buffer, pointer, text.length);
  for (let index = 0; index < text.length; index++) input[index] = text.charCodeAt(index);
}

function bodyLength(body, wasm) {
  const value = body.value;
  if (value.length <= wasm.post_format_capacity()) writeText(wasm, wasm.post_format_input(), value);
  return value.length;
}

function applyEdit(body, wasm) {
  const [pointer, size, from, to, start, end] = [0, 1, 2, 3, 4, 5]
    .map((part) => wasm.post_format_part(part));
  const output = new Uint16Array(wasm.memory.buffer, pointer, size);
  body.setRangeText(String.fromCharCode(...output), from, to);
  body.focus({ preventScroll: true });
  body.setSelectionRange(start, end);
  body.dispatchEvent(new Event('input', { bubbles: true }));
}

function attachInlineImages(editor, body, wasm, announce) {
  const controls = editor.querySelector('[data-post-inline-image]');
  if (!controls) return;
  const image = controls.querySelector('[data-inline-image-file]');
  const description = controls.querySelector('[data-inline-image-alt]');
  const insert = controls.querySelector('[data-inline-image-insert]');
  if (!image || !description || !insert) return;

  let pending = false;
  function prepareImage(key, alt, at) {
    const length = bodyLength(body, wasm);
    const input = wasm.post_format_image_input();
    writeText(wasm, input, key);
    writeText(wasm, input + imageKeyLength * Uint16Array.BYTES_PER_ELEMENT, alt);
    return wasm.post_format_insert_image(length, at, body.maxLength, alt.length);
  }

  editor.closest('form')?.addEventListener('submit', (event) => {
    if (!pending) return;
    event.preventDefault();
    announce('Wait for the image upload to finish before saving or publishing.');
  });
  insert.addEventListener('click', async () => {
    if (pending) return;
    const file = image.files[0];
    if (!file) {
      announce('Choose an image to insert.');
      return;
    }
    const alt = description.value;
    if (alt.length > maxDescriptionLength) {
      announce('Use an image description of 160 characters or fewer.');
      return;
    }
    const originalBody = body.value;
    const originalCaret = body.selectionEnd;
    if (prepareImage('0'.repeat(imageKeyLength), alt, originalCaret) !== 1) {
      announce('The image would exceed the body text limit or cannot be inserted at this position.');
      return;
    }

    pending = true;
    controls.disabled = true;
    announce('Uploading image…');
    try {
      const fields = new FormData();
      fields.append('image', file);
      const response = await fetch('/posts/inline-image', { method: 'POST', body: fields });
      const key = (await response.text()).trim();
      if (!response.ok) {
        announce(key || 'Could not upload the image. Try again.');
        return;
      }
      const at = body.value === originalBody ? originalCaret : body.selectionEnd;
      if (!/^[0-9a-f]{48}$/.test(key) || prepareImage(key, alt, at) !== 1) {
        announce('Could not insert the image at this position. Shorten the body text or place your cursor between paragraphs.');
        return;
      }
      applyEdit(body, wasm);
      image.value = '';
      description.value = '';
      announce('Image inserted. Move its Markdown line to change its position.');
    } catch {
      announce('Could not upload the image. Try again.');
    } finally {
      controls.disabled = false;
      pending = false;
    }
  });
  controls.hidden = false;
}

function attach(editor, wasm) {
  const body = editor.querySelector('textarea[name=body]');
  const controls = editor.querySelector('[data-post-format-controls]');
  const status = editor.querySelector('[data-post-format-status]');
  const format = editor.querySelector('[data-comment-format]');
  if (!body || !controls) return;
  const announce = (message) => { if (status) status.textContent = message; };

  controls.addEventListener('click', (event) => {
    const button = event.target.closest('button[data-post-format]');
    if (!button || !controls.contains(button)) return;
    const command = commands.indexOf(button.dataset.postFormat);
    if (command < 0) return;
    const length = bodyLength(body, wasm);
    const result = wasm.post_format_apply(command, length, body.selectionStart, body.selectionEnd,
      body.maxLength < 0 ? wasm.post_format_capacity() : body.maxLength);
    announce(result === 2 ? 'The formatting would exceed the body text limit.' : '');
    if (result !== 1) return;
    if (format) format.value = 'markdown';
    applyEdit(body, wasm);
  });
  if (status) body.addEventListener('input', () => announce(''));
  attachInlineImages(editor, body, wasm, announce);
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
