use std::{collections::BTreeMap, fmt::Write};

use crate::model::{
    Community, ModPermission, State, WikiPage, WIKI_BODY_LIMIT, WIKI_SLUG_LIMIT, WIKI_TITLE_LIMIT,
};

use super::{action_fields, community_url, encode, escape, field, nsfw_badge, post_rich_text};

pub(super) const TITLE: &str = "Wiki";

pub(super) fn tools_url(item: &Community, page: &str) -> String {
    page_url("/moderation/tools", item, "&section=wiki", page)
}

pub(super) fn public_url(item: &Community, page: &str) -> String {
    page_url("/communities/wiki", item, "", page)
}

pub(super) fn edit_url(item: &Community, page: &str) -> String {
    page_url("/communities/wiki/edit", item, "", page)
}

fn page_url(route: &str, item: &Community, section: &str, page: &str) -> String {
    let mut url = format!("{route}?community={}{section}", encode(&item.slug));
    if !page.is_empty() {
        write!(url, "&page={}", encode(page)).unwrap();
    }
    url
}

pub(super) fn tools(
    state: &State,
    item: &Community,
    query: &BTreeMap<String, String>,
    rejected: Option<(&BTreeMap<String, String>, &str)>,
) -> String {
    if !state.can_moderate(item, ModPermission::Content) {
        return String::new();
    }
    let selected_slug = field(query, "page");
    let selected = item.wiki_page(selected_slug);
    let error = rejected.map(|(_, error)| error).or_else(|| {
        (!selected_slug.is_empty() && selected.is_none())
            .then_some("That wiki page is unavailable in this community.")
    });
    let mut html = error.map_or_else(String::new, error_notice);
    if let Some(page) = selected {
        let back = tools_url(item, &page.slug);
        let submitted = submitted_form(rejected, "save-wiki-page");
        let links = page_links(item, page, &tools_url(item, ""), "All wiki pages");
        let status = page_status(page);
        let title = escape(&page.draft.title);
        let slug = escape(&page.slug);
        write!(html, "<div class='wiki-toolbar'>{links}</div><section class='panel mod-section wiki-editor' aria-labelledby='wiki-edit-heading'><header class='wiki-row'><div><h2 id='wiki-edit-heading'>{title}</h2><p class='muted'>{slug}</p></div><span class='flair'>{status}</span></header>{}</section>", draft_form(item, &back, Some(page), submitted)).unwrap();
        html.push_str(&publish_panel(item, page, &back, submitted.is_some()));
        html.push_str(&editors_panel(
            item,
            page,
            &back,
            submitted_form(rejected, "grant-wiki-editor"),
        ));
    } else {
        let back = tools_url(item, "");
        let wiki = escape(&public_url(item, ""));
        write!(
            html,
            "<div class='wiki-toolbar'><a class='button light' href='{wiki}'>View wiki</a></div>"
        )
        .unwrap();
        html.push_str(&moderator_list(item));
        write!(html, "<section class='panel mod-section wiki-editor' aria-labelledby='wiki-create-heading'><h2 id='wiki-create-heading'>Create a wiki page</h2>{}</section>", draft_form(item, &back, None, submitted_form(rejected, "create-wiki-page"))).unwrap();
    }
    html
}

pub(super) fn public(state: &State, item: &Community, page: Option<&WikiPage>) -> String {
    let Some(page) = page else {
        return public_index(state, item);
    };
    let Some(published) = &page.published else {
        return String::new();
    };
    let wiki = escape(&public_url(item, ""));
    let edit = edit_link(state, item, page);
    let badge = mature_badge(item);
    let title = escape(&published.title);
    let content = post_rich_text::render(&published.body, |_| None);
    let body = format!("<div class='post-body wiki-content'>{content}</div>");
    let body = if item.mature && state.sensitive != "show" {
        format!("<details class='sensitive-preview'><summary>Show mature content</summary>{body}</details>")
    } else {
        body
    };
    format!(
        "<article class='panel standalone wiki-public'><nav class='wiki-toolbar' aria-label='Wiki page'><a class='subtle-link' href='{wiki}'>All wiki pages</a>{edit}</nav>{badge}<h1>{title}</h1>{body}</article>"
    )
}

pub(super) fn edit(
    state: &State,
    item: &Community,
    page: &WikiPage,
    rejected: Option<(&BTreeMap<String, String>, &str)>,
) -> String {
    if !state.can_edit_wiki_page(item, page) {
        return String::new();
    }
    let back = edit_url(item, &page.slug);
    let links = page_links(item, page, &public_url(item, ""), "All wiki pages");
    let badge = mature_badge(item);
    let title = escape(&page.draft.title);
    let error = rejected.map_or_else(String::new, |(_, message)| error_notice(message));
    let form = draft_form(
        item,
        &back,
        Some(page),
        submitted_form(rejected, "save-wiki-page"),
    );
    format!("<section class='panel standalone wiki-public'><nav class='wiki-toolbar' aria-label='Wiki page'>{links}</nav>{badge}<h1>Edit draft</h1><p>{title}</p><p>Moderators publish saved drafts.</p>{error}{form}</section>")
}

fn submitted_form<'a>(
    rejected: Option<(&'a BTreeMap<String, String>, &str)>,
    action: &str,
) -> Option<&'a BTreeMap<String, String>> {
    rejected
        .map(|(form, _)| form)
        .filter(|form| field(form, "action") == action)
}

fn error_notice(error: &str) -> String {
    format!(
        "<p class='notice setup-error' role='alert'>{}</p>",
        escape(error)
    )
}

fn page_status(page: &WikiPage) -> &'static str {
    match &page.published {
        Some(version) if version == &page.draft => "Published",
        Some(_) => "Unpublished edits",
        None => "Draft",
    }
}

fn page_links(item: &Community, page: &WikiPage, back: &str, back_label: &str) -> String {
    let mut html = format!(
        "<a class='subtle-link' href='{}'>{back_label}</a>",
        escape(back)
    );
    if page.published.is_some() {
        let published = escape(&public_url(item, &page.slug));
        write!(
            html,
            "<a class='button light' href='{published}'>View published page</a>"
        )
        .unwrap();
    }
    html
}

fn moderator_list(item: &Community) -> String {
    let mut html = String::from(
        "<section class='panel mod-section' aria-labelledby='wiki-pages-heading'><h2 id='wiki-pages-heading'>Wiki pages</h2>",
    );
    if item.wiki_pages().is_empty() {
        html.push_str("<p class='muted'>No wiki pages yet.</p>");
    } else {
        html.push_str("<ul class='wiki-list' role='list'>");
        for page in item.wiki_pages() {
            let title = escape(&page.draft.title);
            let slug = escape(&page.slug);
            let edit = escape(&tools_url(item, &page.slug));
            let status = page_status(page);
            write!(html, "<li class='wiki-row'><div><strong>{title}</strong><p class='muted'>{slug}</p></div><div class='wiki-toolbar'><span class='flair'>{status}</span><a class='button light' href='{edit}' aria-label='Edit draft {title}'>Edit draft</a></div></li>").unwrap();
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html
}

fn public_index(state: &State, item: &Community) -> String {
    let name = escape(&item.name);
    let community = escape(&community_url(&item.slug));
    let badge = mature_badge(item);
    let mut html = format!("<section class='panel standalone wiki-public'><div class='wiki-toolbar'><a class='subtle-link' href='{community}'>{name}</a>{badge}</div><h1>Community wiki</h1>");
    if state.can_moderate(item, ModPermission::Content) {
        let manage = escape(&tools_url(item, ""));
        write!(
            html,
            "<a class='button light' href='{manage}'>Manage wiki</a>"
        )
        .unwrap();
    }
    let mut live = item
        .wiki_pages()
        .iter()
        .filter_map(|page| page.published.as_ref().map(|published| (page, published)))
        .peekable();
    if live.peek().is_none() {
        html.push_str("<p>No published pages yet.</p>");
    } else {
        html.push_str("<ul class='wiki-list' role='list'>");
        for (page, published) in live {
            let title = escape(&published.title);
            let url = escape(&public_url(item, &page.slug));
            let edit = edit_link(state, item, page);
            write!(html, "<li class='wiki-row'><a class='wiki-page-link' href='{url}'>{title}</a>{edit}</li>").unwrap();
        }
        html.push_str("</ul>");
    }
    let mut drafts = item
        .wiki_pages()
        .iter()
        .filter(|page| page.published.is_none() && state.can_edit_wiki_page(item, page))
        .peekable();
    if drafts.peek().is_some() {
        html.push_str("<section class='preference-list' aria-labelledby='private-wiki-drafts'><h2 id='private-wiki-drafts'>Your draft pages</h2><ul class='wiki-list' role='list'>");
        for page in drafts {
            write!(
                html,
                "<li class='wiki-row'><strong>{}</strong>{}</li>",
                escape(&page.draft.title),
                edit_link(state, item, page)
            )
            .unwrap();
        }
        html.push_str("</ul></section>");
    }
    html.push_str("</section>");
    html
}

fn mature_badge(item: &Community) -> &'static str {
    if item.mature {
        nsfw_badge()
    } else {
        ""
    }
}

fn edit_link(state: &State, item: &Community, page: &WikiPage) -> String {
    if !state.can_edit_wiki_page(item, page) {
        return String::new();
    }
    let url = if state.can_moderate(item, ModPermission::Content) {
        tools_url(item, &page.slug)
    } else {
        edit_url(item, &page.slug)
    };
    let title = page
        .published
        .as_ref()
        .map_or(page.draft.title.as_str(), |published| &published.title);
    format!(
        "<a class='button light' href='{}' aria-label='Edit draft {}'>Edit draft</a>",
        escape(&url),
        escape(title)
    )
}

fn draft_form(
    item: &Community,
    back: &str,
    page: Option<&WikiPage>,
    submitted: Option<&BTreeMap<String, String>>,
) -> String {
    let title = escape(
        submitted
            .map(|form| field(form, "title"))
            .or(page.map(|page| page.draft.title.as_str()))
            .unwrap_or_default(),
    );
    let body = escape(
        submitted
            .map(|form| field(form, "body"))
            .or(page.map(|page| page.draft.body.as_str()))
            .unwrap_or_default(),
    );
    let (fields, button, slug_input, recovery) = if let Some(page) = page {
        let saved_revision = page.revision.to_string();
        let revision = submitted.map_or(saved_revision.as_str(), |form| field(form, "revision"));
        let fields = action_fields(
            "save-wiki-page",
            back,
            &[
                ("community", &item.slug),
                ("page", &page.slug),
                ("revision", revision),
            ],
        );
        let recovery = if revision == saved_revision {
            String::new()
        } else {
            format!(
                "<a class='button light' href='{}'>Reload latest draft</a>",
                escape(back)
            )
        };
        (fields, "Save draft", String::new(), recovery)
    } else {
        let fields = action_fields("create-wiki-page", back, &[("community", &item.slug)]);
        let slug = submitted.map_or("", |form| field(form, "page"));
        let slug_input = format!("<label>Page address<input name='page' value='{}' maxlength='{WIKI_SLUG_LIMIT}' spellcheck='false' autocapitalize='none' required aria-describedby='wiki-slug-help'></label><small id='wiki-slug-help' class='muted'>Use letters, numbers, and single hyphens, such as getting-started.</small>", escape(slug));
        (fields, "Create draft", slug_input, String::new())
    };
    format!(
        r#"<form class="stack-form" method="post" action="/action">{fields}
{slug_input}
<label>Page title<input name="title" value="{title}" maxlength="{WIKI_TITLE_LIMIT}" required></label>
<label>Page content<textarea name="body" rows="12" maxlength="{WIKI_BODY_LIMIT}" aria-describedby="wiki-format-help">{body}</textarea></label>
<small class="muted" id="wiki-format-help">Markdown supports headings, lists, emphasis, code, and web links.</small>
<div class="form-actions wiki-toolbar"><button type="submit" class="button dark">{button}</button>{recovery}</div>
</form>"#
    )
}

fn publish_panel(item: &Community, page: &WikiPage, back: &str, save_failed: bool) -> String {
    let fields = action_fields(
        "publish-wiki-page",
        back,
        &[
            ("community", &item.slug),
            ("page", &page.slug),
            ("revision", &page.revision.to_string()),
        ],
    );
    let label = if page.published.is_some() {
        "Publish changes"
    } else {
        "Publish page"
    };
    let (detail, disabled) = if save_failed {
        ("Save your edits before publishing.", " disabled")
    } else if page.draft.body.trim().is_empty() {
        ("Save some page content before publishing.", " disabled")
    } else if page.published.as_ref() == Some(&page.draft) {
        ("The published page is up to date.", " disabled")
    } else {
        (
            "Publishing makes the saved draft visible to everyone who can read this community.",
            "",
        )
    };
    format!("<section class='panel mod-section wiki-editor' aria-labelledby='wiki-publish-heading'><h2 id='wiki-publish-heading'>Publishing</h2><p class='muted'>{detail}</p><form method='post' action='/action'>{fields}<button class='button dark' type='submit'{disabled}>{label}</button></form></section>")
}

fn editors_panel(
    item: &Community,
    page: &WikiPage,
    back: &str,
    submitted: Option<&BTreeMap<String, String>>,
) -> String {
    let fields = action_fields(
        "grant-wiki-editor",
        back,
        &[("community", &item.slug), ("page", &page.slug)],
    );
    let handle = escape(submitted.map_or("", |form| field(form, "handle")));
    let mut html = format!("<section class='panel mod-section wiki-editor' aria-labelledby='wiki-editors-heading'><h2 id='wiki-editors-heading'>Page editors</h2><p class='muted'>Grant draft editing to a member who has joined or been approved. Moderators publish changes.</p><form class='stack-form wiki-grant' method='post' action='/action'>{fields}<label>Member username<input name='handle' value='{handle}' maxlength='28' spellcheck='false' autocapitalize='none' required></label><button class='button light' type='submit'>Grant editing access</button></form>");
    if page.editors.is_empty() {
        html.push_str("<p class='muted'>No member editors for this page.</p>");
    } else {
        html.push_str("<ul class='wiki-list' role='list'>");
        for editor in &page.editors {
            let fields = action_fields(
                "revoke-wiki-editor",
                back,
                &[
                    ("community", &item.slug),
                    ("page", &page.slug),
                    ("handle", editor),
                ],
            );
            let editor = escape(editor);
            write!(html, "<li class='wiki-row'><span>u/{editor}</span><form method='post' action='/action'>{fields}<button class='button light' type='submit' aria-label='Revoke editing access for u/{editor}'>Revoke</button></form></li>").unwrap();
        }
        html.push_str("</ul>");
    }
    html.push_str("</section>");
    html
}
