use super::escape;

pub(super) fn render(body: &str, editing: bool, markdown: bool, required: bool) -> String {
    let (label, rows, placeholder) = if editing {
        ("Edit comment", 4, "")
    } else {
        (
            r#"<span class="sr-only">Write a reply</span>"#,
            3,
            r#" placeholder="Add to the discussion""#,
        )
    };
    let format = if editing {
        let (plain, markdown) = if markdown {
            ("", " selected")
        } else {
            (" selected", "")
        };
        format!(
            r#"<label>Text format <select name="body_format" data-comment-format><option value="plain"{plain}>Plain text</option><option value="markdown"{markdown}>Markdown</option></select></label>"#
        )
    } else {
        r#"<input type="hidden" name="body_format" value="markdown">"#.to_owned()
    };
    let required = if required { " required" } else { "" };
    format!(
        r#"<div class="post-format-editor comment-format-editor" data-post-format-editor>
{format}
<div class="post-format-controls" data-post-format-controls role="group" aria-label="Comment formatting" hidden>
<button type="button" data-post-format="bold"><strong>Bold</strong></button>
<button type="button" data-post-format="italic"><em>Italic</em></button>
<button type="button" data-post-format="link">Link</button>
<button type="button" data-post-format="code">Code</button>
</div>
<label>{label}<textarea name="body" rows="{rows}" maxlength="4000"{placeholder}{required}>{}</textarea></label>
<p data-post-format-status class="muted" role="status"></p>
<details class="markup-guide"><summary>Formatting help</summary>
<p>In Markdown, use <code>**bold**</code>, <code>*italic*</code>, <code>[link text](https://example.com)</code>, or <code>&#96;inline code&#96;</code>.</p>
</details>
</div>"#,
        escape(body)
    )
}
