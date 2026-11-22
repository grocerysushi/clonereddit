for (const share of document.querySelectorAll('[data-comment-share]')) {
  const link = share.querySelector('[data-comment-open]');
  const field = share.querySelector('[data-comment-link]');
  const button = share.querySelector('[data-comment-copy]');
  const status = share.querySelector('[data-comment-share-status]');
  if (!link || !field || !button || !status) continue;

  field.value = link.href;
  if (!navigator.clipboard?.writeText) continue;

  button.hidden = false;
  button.addEventListener('click', async () => {
    status.textContent = '';
    try {
      await navigator.clipboard.writeText(field.value);
      status.textContent = 'Link copied.';
    } catch {
      field.focus();
      field.select();
      status.textContent = 'Copy the selected link manually.';
    }
  });
}
