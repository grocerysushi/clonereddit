use super::escape;

pub(super) fn editor(body: &str, rows: usize) -> String {
    editor_with_controls(body, rows, "")
}

pub(super) fn editor_with_inline_images(body: &str, rows: usize) -> String {
    let controls = r#"<fieldset class="post-inline-image-controls" data-post-inline-image aria-describedby="post-inline-image-help" hidden>
<legend>Inline image</legend>
<p class="muted" id="post-inline-image-help">Place your cursor where the image should appear. Choose a PNG, JPEG, WebP, or GIF up to 2 MiB.</p>
<div class="post-inline-image-fields">
<label>Image file<input type="file" data-inline-image-file accept="image/png,image/jpeg,image/webp,image/gif"></label>
<label>Image description <small>Optional</small><input data-inline-image-alt maxlength="160" placeholder="Describe what’s in the image"></label>
<button type="button" class="button light" data-inline-image-insert>Insert image</button>
</div>
</fieldset>"#;
    editor_with_controls(body, rows, controls)
}

fn editor_with_controls(body: &str, rows: usize, inline_images: &str) -> String {
    format!(
        r#"<div class="post-format-editor" data-post-format-editor>
<label for="post-format-body">Body text <small>Optional</small></label>
<div class="post-format-controls" data-post-format-controls role="group" aria-label="Post formatting" hidden>
<button type="button" data-post-format="bold"><strong>Bold</strong></button>
<button type="button" data-post-format="italic"><em>Italic</em></button>
<button type="button" data-post-format="link">Link</button>
<button type="button" data-post-format="bullets">Bulleted list</button>
<button type="button" data-post-format="numbered">Numbered list</button>
<button type="button" data-post-format="quote">Block quote</button>
</div>
<textarea id="post-format-body" name="body" rows="{rows}" maxlength="8000" placeholder="Add context or ask a question">{}</textarea>
{inline_images}
<p data-post-format-status class="muted" role="status"></p>
<details class="markup-guide"><summary>Formatting help</summary>
<p>Use <code>**bold**</code>, <code>*italic*</code>, <code>&#96;inline code&#96;</code>, or <code>[link text](https://example.com)</code>. Start a new line with <code>- </code> for a bullet, <code>1. </code> for a numbered item, or <code>&gt; </code> for a block quote.</p>
</details>
</div><script type="module" src="/post-format.js"></script>"#,
        escape(body)
    )
}
