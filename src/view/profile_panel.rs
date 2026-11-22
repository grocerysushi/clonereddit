use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    action_fields, action_form, community_icon, community_members, community_owner, community_url,
    encode, escape, icon, initials, profile_name, profile_url, score,
};
use crate::model::{Comment, Post, State};

const SETTINGS: [(&str, &str); 6] = [
    ("identity", "Profile details"),
    ("visibility", "Visibility"),
    ("images", "Avatar and cover"),
    ("flair", "Profile flair"),
    ("tools", "Profile tools"),
    ("social", "Social links"),
];

fn settings_url(section: &str) -> String {
    format!("/profile/settings?section={section}")
}

pub(super) fn render(state: &State, origin: &str, public: bool, active_tab: &str) -> String {
    let public_url = profile_url(state);
    let share_url = escape(&format!("{}{public_url}", origin.trim_end_matches('/')));
    let base = if public {
        public_url
    } else {
        "/profile".into()
    };
    let back = if active_tab.is_empty() {
        base
    } else {
        format!("{base}?tab={}", encode(active_tab))
    };
    let name = escape(profile_name(state));
    let handle = escape(&state.username);
    let cover = cover(state, public);
    let avatar = avatar(state, public);
    let mut html = format!(
        "<aside class='profile-sidebar' aria-label='Profile details'><section class='panel profile-card'>{cover}<div class='profile-card-body'><header class='profile-card-identity'>{avatar}<div><h2 class='profile-card-name'>{name}</h2><p class='profile-card-handle'>u/{handle}</p></div></header>"
    );
    if !state.bio.is_empty() {
        write!(
            html,
            "<p class='profile-card-bio'>{}</p>",
            escape(&state.bio)
        )
        .unwrap();
    }
    if !state.profile_flair.is_empty() {
        write!(
            html,
            "<span class='flair profile-card-flair'>{}</span>",
            escape(&state.profile_flair)
        )
        .unwrap();
    }
    write!(
        html,
        "<details class='profile-share'><summary class='button light'>Share profile</summary><label>Public profile link<input type='url' readonly value='{share_url}'></label><a class='subtle-link' href='{}'>Open public profile</a></details>", profile_url(state)
    )
    .unwrap();
    html.push_str(&statistics(state, public));
    html.push_str("</div></section>");

    html.push_str("<section class='panel profile-card profile-card-section'><h2>Achievements</h2>");
    html.push_str(&trophy_list(state, public));
    if !public {
        html.push_str("<a class='subtle-link' href='/profile/achievements'>View achievements</a>");
    }
    html.push_str("</section>");
    html.push_str(&owned_communities(state, &back, public));
    let shared = state
        .custom_feeds
        .iter()
        .filter(|feed| !feed.private && feed.show_on_profile)
        .map(|feed| {
            format!(
                "<li><a class='subtle-link' href='/feeds/shared?id={}'>{}</a></li>",
                feed.id,
                escape(&feed.name)
            )
        })
        .collect::<String>();
    if !shared.is_empty() {
        write!(html, "<section class='panel profile-card profile-card-section'><h2>Custom feeds</h2><ul class='profile-social-list'>{shared}</ul></section>").unwrap();
    }

    if !public {
        html.push_str("<section class='panel profile-card profile-card-section'><h2>Settings</h2>");
        html.push_str(&settings_links(None));
        html.push_str("</section>");
    }
    let links = social_links(state, false);
    if !public || !links.is_empty() {
        html.push_str(
            "<section class='panel profile-card profile-card-section'><h2>Social links</h2>",
        );
        html.push_str(&links);
        if !public {
            html.push_str("<a class='button light' href='/profile/settings?section=social'>Add or edit links</a>");
        }
        html.push_str("</section>");
    }
    html.push_str("</aside>");
    html
}

fn avatar(state: &State, public: bool) -> String {
    if state.avatar.is_some() && !(public && state.profile_nsfw && state.sensitive != "show") {
        let base = if public {
            profile_url(state)
        } else {
            "/profile".to_owned()
        };
        format!("<span class='profile-card-avatar'><img src='{base}/avatar' alt=''></span>")
    } else {
        format!(
            "<span class='profile-card-avatar' aria-hidden='true'>{}</span>",
            escape(&initials(profile_name(state)))
        )
    }
}

fn cover(state: &State, public: bool) -> String {
    let reveal = !public || !state.profile_nsfw || state.sensitive == "show";
    if state.cover.is_some() && reveal {
        let base = if public {
            profile_url(state)
        } else {
            "/profile".to_owned()
        };
        format!("<div class='profile-cover'><img src='{base}/cover' alt=''></div>")
    } else {
        "<div class='profile-cover profile-cover-default' aria-hidden='true'></div>".into()
    }
}

fn profile_posts(state: &State, public: bool) -> impl DoubleEndedIterator<Item = &Post> {
    state.posts.iter().filter(move |post| {
        post.author == state.username
            && (!public
                || state.public_posts
                    && !state.profile_hidden_posts.contains(&post.id)
                    && super::community(state, &post.community)
                        .is_some_and(|item| item.visibility != "private"))
    })
}

fn profile_comments(state: &State, public: bool) -> impl DoubleEndedIterator<Item = &Comment> {
    state.comments.iter().filter(move |comment| {
        !comment.deleted
            && comment.author == state.username
            && (!public
                || state.public_comments
                    && !state.profile_hidden_comments.contains(&comment.id)
                    && state
                        .posts
                        .iter()
                        .find(|post| post.id == comment.post)
                        .and_then(|post| super::community(state, &post.community))
                        .is_some_and(|item| item.visibility != "private"))
    })
}

fn statistics(state: &State, public: bool) -> String {
    let mut communities = BTreeSet::new();
    let mut contributions = 0;
    let mut votes = 0_i64;
    for post in profile_posts(state, public) {
        contributions += 1;
        votes += i64::from(score(state, post));
        communities.insert(post.community.as_str());
    }
    let post_communities: BTreeMap<u64, &str> = state
        .posts
        .iter()
        .map(|post| (post.id, post.community.as_str()))
        .collect();
    for comment in profile_comments(state, public) {
        contributions += 1;
        votes += i64::from(state.comment_score(comment));
        communities.extend(post_communities.get(&comment.post).copied());
    }
    let joined = if state.profile_created > 0 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let days = now.saturating_sub(state.profile_created) / 86_400;
        let age = match days {
            0 => "Today".into(),
            1 => "1 day ago".into(),
            _ => format!("{days} days ago"),
        };
        format!("<div><dt>Joined</dt><dd>{age}</dd></div>")
    } else {
        String::new()
    };
    format!(
        "<dl class='profile-stat-grid'><div><dt>Net votes</dt><dd>{votes}</dd></div><div><dt>Contributions</dt><dd>{contributions}</dd></div><div><dt>Active communities</dt><dd>{}</dd></div>{joined}</dl>",
        communities.len()
    )
}

pub(super) fn trophies(state: &State) -> String {
    trophy_list(state, false)
}

fn trophy_list(state: &State, public: bool) -> String {
    let milestones = [
        (
            profile_posts(state, public).next().is_some(),
            "Started a conversation",
            "Published a post",
            "chat",
        ),
        (
            profile_comments(state, public).next().is_some(),
            "Joined the discussion",
            "Wrote a reply",
            "log",
        ),
        (
            state.communities.iter().any(|item| {
                community_owner(state, item) && (!public || item.visibility != "private")
            }),
            "Community builder",
            "Created a community",
            "spark",
        ),
        (
            !public && !state.custom_feeds.is_empty(),
            "Feed curator",
            "Created a custom feed",
            "grid",
        ),
    ];
    let mut html = String::new();
    for (_, label, detail, glyph) in milestones.into_iter().filter(|(earned, _, _, _)| *earned) {
        write!(
            html,
            "<li class='profile-trophy'><span class='profile-trophy-icon'>{}</span><span><strong>{label}</strong><small>{detail}</small></span></li>",
            icon(glyph)
        )
        .unwrap();
    }
    if html.is_empty() {
        "<p class='muted profile-trophy-empty'>No achievements yet.</p>".into()
    } else {
        format!("<ul class='profile-trophies'>{html}</ul>")
    }
}

fn owned_communities(state: &State, back: &str, public: bool) -> String {
    let mut items = String::new();
    for item in state
        .communities
        .iter()
        .filter(|item| community_owner(state, item) && (!public || item.visibility != "private"))
    {
        let label = if state.joined.contains(&item.slug) {
            "Leave"
        } else {
            "Join"
        };
        let join = if public {
            String::new()
        } else {
            action_form(
                "join",
                back,
                &[("community", &item.slug)],
                label,
                "",
                "button light",
            )
        };
        write!(
            items,
            "<li class='profile-owned-row'><a href='{}'>{}<span><strong>{}</strong><small>{}</small></span></a>{join}</li>",
            community_url(&item.slug),
            community_icon(item),
            escape(&item.name),
            community_members(state, item)
        )
        .unwrap();
    }
    if items.is_empty() {
        String::new()
    } else {
        format!("<section class='panel profile-card profile-card-section'><h2>Moderates</h2><ul class='profile-owned-list'>{items}</ul></section>")
    }
}

fn settings_links(active: Option<&str>) -> String {
    let items: String = SETTINGS
        .into_iter()
        .map(|(section, label)| {
            let current = if active == Some(section) {
                " aria-current='page'"
            } else {
                ""
            };
            format!(
                "<li><a href='{}'{current}>{label}</a></li>",
                settings_url(section)
            )
        })
        .collect();
    format!("<ul class='profile-settings-list'>{items}</ul>")
}

pub(super) fn settings(state: &State, section: &str) -> String {
    let (title, body) = match section {
        "visibility" => ("Visibility", visibility(state)),
        "images" => ("Avatar and cover", images(state)),
        "flair" => ("Profile flair", flair(state)),
        "tools" => ("Profile tools", tools(state)),
        "social" => ("Social links", social_settings(state)),
        _ => ("Profile details", identity(state)),
    };
    let navigation = settings_links(Some(section));
    format!(
        "<section class='panel standalone profile-settings-page'><a class='back-link' href='/profile'>← Your profile</a><h1>{title}</h1><nav class='profile-settings-tabs' aria-label='Profile settings'>{navigation}</nav>{body}</section>"
    )
}

fn identity(state: &State) -> String {
    let fields = action_fields("profile", &settings_url("identity"), &[]);
    format!(
        "<form class='stack-form' method='post' action='/action'>{fields}<label>Username<input name='username' value='{}' readonly></label><label>Display name<input name='display_name' value='{}' maxlength='60'></label><label>Bio<textarea name='bio' rows='5' maxlength='280'>{}</textarea></label><button class='button dark' type='submit'>Save profile</button></form>",
        escape(&state.username),
        escape(&state.display_name),
        escape(&state.bio)
    )
}

fn visibility(state: &State) -> String {
    let back = settings_url("visibility");
    let fields = action_fields("profile-visibility", &back, &[]);
    let checked = |enabled| if enabled { " checked" } else { "" };
    let mut html = format!(
        "<form class='stack-form' method='post' action='/action'>{fields}<fieldset><legend>Public profile</legend><label class='checkbox'><input type='checkbox' name='posts'{}> Show posts</label><label class='checkbox'><input type='checkbox' name='comments'{}> Show replies</label><label class='checkbox'><input type='checkbox' name='nsfw'{}> Mark profile as sensitive (18+)</label></fieldset><button class='button dark' type='submit'>Save visibility</button></form><section class='preference-list'><h2>Individual activity</h2><p class='muted'>Choose which posts and replies appear on your public profile.</p>",
        checked(state.public_posts),
        checked(state.public_comments),
        checked(state.profile_nsfw)
    );
    for post in profile_posts(state, false).rev() {
        visibility_item(
            &mut html,
            &back,
            &format!("/thread?id={}", post.id),
            &post.title,
            "profile-hide-post",
            post.id,
            state.profile_hidden_posts.contains(&post.id),
        );
    }
    for comment in profile_comments(state, false).rev() {
        let label: String = comment.body.chars().take(96).collect();
        visibility_item(
            &mut html,
            &back,
            &format!("/thread?id={}#comment-{}", comment.post, comment.id),
            &label,
            "profile-hide-comment",
            comment.id,
            state.profile_hidden_comments.contains(&comment.id),
        );
    }
    html.push_str("</section>");
    html
}

fn visibility_item(
    html: &mut String,
    back: &str,
    url: &str,
    title: &str,
    action: &str,
    id: u64,
    hidden: bool,
) {
    let toggle = action_form(
        action,
        back,
        &[("id", &id.to_string())],
        if hidden {
            "Show on profile"
        } else {
            "Hide from profile"
        },
        "",
        "button light",
    );
    write!(
        html,
        "<div class='preference-row profile-visibility-row'><a class='subtle-link' href='{url}'>{}</a>{toggle}</div>",
        escape(title)
    )
    .unwrap();
}

fn images(state: &State) -> String {
    let back = settings_url("images");
    let mut html = String::from(
        "<p class='muted'>Choose a PNG, JPEG, or WebP image up to 512 KiB.</p><div class='profile-image-settings'>",
    );
    for (kind, label, image, preview, clear) in [
        (
            "avatar",
            "Avatar",
            state.avatar.is_some(),
            avatar(state, false),
            "clear-avatar",
        ),
        (
            "cover",
            "Cover",
            state.cover.is_some(),
            cover(state, false).to_owned(),
            "clear-cover",
        ),
    ] {
        write!(
            html,
            "<section class='profile-image-preview'><h2>{label}</h2>{preview}<form class='stack-form' method='post' action='/profile/image' enctype='multipart/form-data'><input type='hidden' name='kind' value='{kind}'><label>{label} file<input type='file' name='image' accept='image/png,image/jpeg,image/webp' required></label><button class='button dark' type='submit'>Upload {kind}</button></form>"
        )
        .unwrap();
        if image {
            html.push_str(&action_form(
                clear,
                &back,
                &[],
                &format!("Remove {kind}"),
                "",
                "button light",
            ));
        }
        html.push_str("</section>");
    }
    html.push_str("</div>");
    html
}

fn flair(state: &State) -> String {
    let fields = action_fields("profile-flair", &settings_url("flair"), &[]);
    format!(
        "<form class='stack-form' method='post' action='/action'>{fields}<label>Profile label <small>Optional</small><input name='flair' maxlength='32' value='{}'></label><button class='button dark' type='submit'>Save flair</button></form>",
        escape(&state.profile_flair)
    )
}

fn tools(state: &State) -> String {
    format!(
        "<ul class='profile-settings-list profile-tools-list'><li><a href='{}'>View public profile</a></li><li><a href='/profile/achievements'>View achievements</a></li><li><a href='/?view=saved'>Saved posts</a></li><li><a href='/settings'>Feed controls</a></li><li><a href='/moderation?section=manage'>Manage your communities</a></li></ul>",
        escape(&profile_url(state))
    )
}

fn social_settings(state: &State) -> String {
    let fields = action_fields("add-social-link", &settings_url("social"), &[]);
    let links = social_links(state, true);
    format!(
        "<form class='stack-form' method='post' action='/action'>{fields}<label>Link name<input name='label' maxlength='40' required></label><label>HTTPS address<input type='url' name='url' maxlength='600' placeholder='https://' required></label><button class='button dark' type='submit'>Add link</button></form>{links}"
    )
}

fn social_links(state: &State, settings: bool) -> String {
    let mut items = String::new();
    for link in state.social_links.iter().filter(|link| {
        link.url.split_once("://").is_some_and(|(scheme, address)| {
            scheme.eq_ignore_ascii_case("https") && !address.is_empty()
        })
    }) {
        let remove = if settings {
            action_form(
                "delete-social-link",
                &settings_url("social"),
                &[("id", &link.id.to_string())],
                "Remove",
                "",
                "button light",
            )
        } else {
            String::new()
        };
        write!(
            items,
            "<li><a class='subtle-link' href='{}' target='_blank' rel='noopener noreferrer'>{}</a>{remove}</li>",
            escape(&link.url),
            escape(&link.label)
        )
        .unwrap();
    }
    if items.is_empty() {
        String::new()
    } else {
        format!("<ul class='profile-social-list'>{items}</ul>")
    }
}
