use std::collections::BTreeMap;
use std::fmt::Write;

use super::{action_fields, community_art, escape, field, hidden, icon};
use crate::model::SetupArtwork;

pub(super) const TOPICS: &[(&str, &str)] = &[
    ("general", "General"),
    ("anime-cosplay", "Anime & Cosplay"),
    ("art", "Art"),
    ("business-finance", "Business & Finance"),
    ("collectibles-hobbies", "Collectibles & Other Hobbies"),
    ("education-career", "Education & Career"),
    ("fashion-beauty", "Fashion & Beauty"),
    ("food-drinks", "Food & Drinks"),
    ("games", "Games"),
    ("health", "Health"),
    ("home-garden", "Home & Garden"),
    ("humanities-law", "Humanities & Law"),
    ("identity-relationships", "Identity & Relationships"),
    ("internet-culture", "Internet Culture"),
    ("movies-tv", "Movies & TV"),
    ("music", "Music"),
    ("nature-outdoors", "Nature & Outdoors"),
    ("news-politics", "News & Politics"),
    ("places-travel", "Places & Travel"),
    ("pop-culture", "Pop Culture"),
    ("qas-stories", "Q&As & Stories"),
    ("reading-writing", "Reading & Writing"),
    ("sciences", "Sciences"),
    ("spooky", "Spooky"),
    ("sports", "Sports"),
    ("technology", "Technology"),
    ("vehicles", "Vehicles"),
    ("wellness", "Wellness"),
    ("adult-content", "Adult Content"),
    ("mature-topics", "Mature Topics"),
];

const ACCESS: &[(&str, &str, &str, &str)] = &[
    (
        "public",
        "Public",
        "Anyone can read and contribute.",
        "home",
    ),
    (
        "restricted",
        "Restricted",
        "Anyone can read. The owner and moderators can contribute.",
        "sliders",
    ),
    (
        "private",
        "Private",
        "The owner and moderators can read and contribute.",
        "shield",
    ),
];

const START: &str = "/communities/start";
const DETAILS: [&str; 4] = ["slug", "name", "description", "guidelines"];

pub(super) fn topic_label(topic: &str) -> &str {
    TOPICS
        .iter()
        .find(|(value, _)| *value == topic)
        .map_or("General", |(_, label)| label)
}

struct Draft<'a> {
    form: &'a BTreeMap<String, String>,
    artwork: Option<&'a SetupArtwork>,
    topic: &'static str,
    visibility: &'static str,
    mature: bool,
}

impl<'a> Draft<'a> {
    fn from(form: &'a BTreeMap<String, String>, artwork: Option<&'a SetupArtwork>) -> Self {
        let topic = TOPICS
            .iter()
            .find(|(value, _)| *value == field(form, "topic"))
            .map_or("general", |(value, _)| value);
        let visibility = ACCESS
            .iter()
            .find(|(value, _, _, _)| *value == field(form, "visibility"))
            .map_or("public", |(value, _, _, _)| value);
        Self {
            form,
            artwork,
            topic,
            visibility,
            mature: field(form, "mature") == "on",
        }
    }

    fn access_fields(&self) -> String {
        hidden("visibility", self.visibility) + &self.mature_field()
    }

    fn mature_field(&self) -> String {
        if self.mature {
            hidden("mature", "on")
        } else {
            String::new()
        }
    }

    fn detail_fields(&self) -> String {
        let values: String = DETAILS
            .into_iter()
            .map(|key| hidden(key, field(self.form, key)))
            .collect();
        values + &self.artwork_field()
    }

    fn artwork_field(&self) -> String {
        self.artwork.map_or_else(String::new, |item| {
            hidden("artwork_preview", &item.id.to_string())
        })
    }

    fn artwork_url(&self, kind: &str) -> Option<String> {
        let artwork = self.artwork?;
        let present = match kind {
            "icon" => artwork.icon.is_some(),
            "banner" => artwork.banner.is_some(),
            _ => false,
        };
        present.then(|| super::community_preview_image_url(artwork.id, kind))
    }

    fn badges(&self) -> String {
        let visibility = ACCESS
            .iter()
            .find(|(value, _, _, _)| *value == self.visibility)
            .map_or("Public", |(_, label, _, _)| label);
        let mature = if self.mature {
            "<span class='flair sensitive'>Mature (18+)</span>"
        } else {
            ""
        };
        format!(
            "<div class='setup-badges'><span class='flair'>{}</span><span class='flair'>{visibility}</span>{mature}</div>",
            escape(topic_label(self.topic))
        )
    }
}

pub(super) fn render(query: &BTreeMap<String, String>) -> String {
    let selection: BTreeMap<_, _> = ["topic", "visibility", "mature"]
        .into_iter()
        .filter_map(|key| query.get(key).map(|value| (key.to_owned(), value.clone())))
        .collect();
    let draft = Draft::from(&selection, None);
    match field(query, "step") {
        "2" => access_step(&draft),
        "3" => details_step(&draft),
        _ => topic_step(&draft),
    }
}

pub(super) fn review(form: &BTreeMap<String, String>, artwork: Option<&SetupArtwork>) -> String {
    let draft = Draft::from(form, artwork);
    match field(form, "setup_step") {
        "1" => topic_step(&draft),
        "2" => access_step(&draft),
        "3" => details_step(&draft),
        _ if field(form, "slug").trim().is_empty() || field(form, "name").trim().is_empty() => {
            details_step(&draft)
        }
        _ => preview(&draft),
    }
}

fn frame(step: usize, title: &str, summary: &str, body: &str) -> String {
    let mut progress = String::from("<ol class='setup-steps' aria-label='Community setup steps'>");
    for (index, label) in ["Topic", "Access", "Details", "Review"]
        .into_iter()
        .enumerate()
    {
        let number = index + 1;
        let current = if number == step {
            " aria-current='step'"
        } else {
            ""
        };
        write!(progress, "<li{current}>{label}</li>").unwrap();
    }
    progress.push_str("</ol>");
    format!(
        "<section class='panel standalone community-setup'>{progress}<header class='setup-header'><h1>{title}</h1><p>{summary}</p></header>{body}</section>"
    )
}

fn checked(selected: bool) -> &'static str {
    if selected {
        " checked"
    } else {
        ""
    }
}

fn next(step: usize, label: &str) -> String {
    format!(
        "<button class='button dark setup-primary' type='submit' name='setup_step' value='{step}'>{label}</button>"
    )
}

fn back(step: usize) -> String {
    format!(
        "<button class='button light setup-back' type='submit' name='setup_step' value='{step}' formnovalidate>Back</button>"
    )
}

fn topic_step(draft: &Draft<'_>) -> String {
    let mut options = String::from(
        "<fieldset class='topic-grid'><legend class='sr-only'>Community topic</legend>",
    );
    for (value, label) in TOPICS {
        write!(
            options,
            "<label class='setup-choice'><input type='radio' name='topic' value='{value}'{} required><span>{}</span></label>",
            checked(*value == draft.topic),
            escape(label)
        )
        .unwrap();
    }
    options.push_str("</fieldset>");
    let body = format!(
        "<form method='post' action='/action' class='stack-form setup-form'>{}{}{}{options}<div class='setup-actions form-actions'>{}<a class='button light setup-back' href='/?view=communities'>Cancel</a></div></form>",
        hidden("review_community", "1"),
        draft.access_fields(),
        draft.detail_fields(),
        next(2, "Continue")
    );
    frame(
        1,
        "Choose a topic",
        "Pick the subject that best describes what people will talk about.",
        &body,
    )
}

fn access_step(draft: &Draft<'_>) -> String {
    let mut options = String::from(
        "<fieldset class='setup-access'><legend class='sr-only'>Community access</legend>",
    );
    for (value, label, detail, glyph) in ACCESS {
        write!(
            options,
            "<label class='setup-choice'><input type='radio' name='visibility' value='{value}'{} required><span class='setup-choice-icon'>{}</span><span><strong>{label}</strong><small>{detail}</small></span></label>",
            checked(*value == draft.visibility),
            icon(glyph)
        )
        .unwrap();
    }
    options.push_str("</fieldset>");
    let body = format!(
        "<form method='post' action='/action' class='stack-form setup-form'>{}{}{}{options}<label class='checkbox setup-mature'><input type='checkbox' name='mature'{}><span><strong>Mature (18+)</strong><small>Label the community so people’s sensitive-content preferences apply.</small></span></label><div class='setup-actions form-actions'>{}{}</div></form>",
        hidden("review_community", "1"),
        hidden("topic", draft.topic),
        draft.detail_fields(),
        checked(draft.mature),
        next(3, "Continue"),
        back(1)
    );
    frame(
        2,
        "Set community access",
        "Choose who can read discussions and take part.",
        &body,
    )
}

fn details_step(draft: &Draft<'_>) -> String {
    let icon_url = draft.artwork_url("icon");
    let banner_url = draft.artwork_url("banner");
    let artwork_inputs = artwork_fields(icon_url.as_deref(), banner_url.as_deref());
    let body = format!(
        r#"<form method="post" action="/communities/appearance" enctype="multipart/form-data" class="stack-form setup-form">{review}{topic}{access}{artwork}{badges}
<label>Community address<input name="slug" value="{slug}" required minlength="2" maxlength="32" pattern="[A-Za-z0-9][A-Za-z0-9\-]{{0,30}}[A-Za-z0-9]" placeholder="city-gardening"><small>Use 2–32 letters, numbers, or hyphens. Begin and end with a letter or number.</small></label>
<label>Community name<input name="name" value="{name}" required maxlength="60" placeholder="City Gardening"></label>
<label>Description <small>Optional</small><textarea name="description" rows="3" maxlength="500" placeholder="What will people discuss here?">{description}</textarea></label>
<label>Topic guidelines <small>Optional</small><textarea name="guidelines" rows="4" maxlength="2000" placeholder="What fits the community’s focus?">{guidelines}</textarea></label>
{artwork_inputs}
<div class="setup-actions form-actions">{next}{back}</div></form>"#,
        review = hidden("review_community", "1"),
        topic = hidden("topic", draft.topic),
        access = draft.access_fields(),
        artwork = draft.artwork_field(),
        badges = draft.badges(),
        slug = escape(field(draft.form, "slug")),
        name = escape(field(draft.form, "name")),
        description = escape(field(draft.form, "description")),
        guidelines = escape(field(draft.form, "guidelines")),
        next = next(4, "Review"),
        back = back(2)
    );
    frame(
        3,
        "Name your community",
        "Choose its address and tell people what belongs in the discussion.",
        &body,
    )
}

pub(super) fn artwork_fields(icon: Option<&str>, banner: Option<&str>) -> String {
    let icon_upload = artwork_picker(icon, "icon", "Community icon", "Square crop");
    let banner_upload = artwork_picker(banner, "banner", "Community banner", "Wide crop");
    format!("<fieldset class='setup-artwork-fields'><legend>Images <small>Optional</small></legend><p id='community-artwork-help' class='muted'>Use PNG, JPEG, or WebP images up to 512 KiB each. The preview shows how they’ll be cropped.</p><div class='setup-artwork-pickers'>{icon_upload}{banner_upload}</div></fieldset>")
}

fn artwork_picker(current_url: Option<&str>, kind: &str, label: &str, crop: &str) -> String {
    let current = current_url.map_or_else(String::new, |url| {
        format!("<img class='setup-upload-thumb setup-upload-{kind}' src='{}' alt='Selected {kind}'><label class='checkbox'><input type='checkbox' name='remove_{kind}'> Remove current {kind}</label>", escape(url))
    });
    let label = if current.is_empty() {
        label.to_owned()
    } else {
        format!("Replace {}", label.to_lowercase())
    };
    format!("<div class='setup-artwork-picker'><label>{label}<small>{crop}</small><input type='file' name='{kind}_image' accept='image/png,image/jpeg,image/webp' aria-describedby='community-artwork-help'></label>{current}</div>")
}

fn preview(draft: &Draft<'_>) -> String {
    let slug = field(draft.form, "slug").trim().to_ascii_lowercase();
    let name = field(draft.form, "name").trim();
    let description = field(draft.form, "description").trim();
    let guidelines = field(draft.form, "guidelines").trim();
    let fields = action_fields(
        "create-community",
        START,
        &[
            ("slug", &slug),
            ("name", name),
            ("description", description),
            ("guidelines", guidelines),
            ("topic", draft.topic),
            ("visibility", draft.visibility),
            ("setup_step", "3"),
        ],
    );
    let mut details = format!(
        "<div class='community-art-address'>c/{}</div><h2 class='community-art-name'>{}</h2>",
        escape(&slug),
        escape(name)
    );
    if !description.is_empty() {
        write!(details, "<p>{}</p>", escape(description)).unwrap();
    }
    details.push_str(&draft.badges());
    let icon_url = draft.artwork_url("icon");
    let banner_url = draft.artwork_url("banner");
    let art = community_art(icon_url.as_deref(), banner_url.as_deref(), &details);
    let mut overview = format!("<div class='setup-review'>{art}");
    if !guidelines.is_empty() {
        write!(
            overview,
            "<div class='setup-review-notes'><h3>Topic guidelines</h3><p>{}</p></div>",
            escape(guidelines)
        )
        .unwrap();
    }
    overview.push_str("</div>");
    let body = format!(
        "{overview}<form method='post' action='/action' class='setup-actions form-actions'>{fields}{}{}<button class='button dark setup-primary' type='submit'>Create community</button><button class='button light setup-back' type='submit' name='review_community' value='1'>Edit details</button></form>",
        draft.mature_field(),
        draft.artwork_field()
    );
    frame(
        4,
        "Review your community",
        "Preview your icon, banner, and community details before creating it.",
        &body,
    )
}
