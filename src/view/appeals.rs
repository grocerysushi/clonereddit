use std::fmt::Write;

use super::{action_fields, community, escape, restricted::restriction_expiry};
use crate::model::{Appeal, AppealStatus, ContentAppeal, State};
use crate::schedule_time::{display, now, to_local};

pub(super) fn render(
    state: &State,
    draft: Option<(u64, &str)>,
    content_draft: Option<(u64, &str)>,
    error: &str,
) -> String {
    let mut html = String::from("<section class='panel standalone moderation-tool'><h1>Your appeals</h1><p>Ask moderators to review a removed post or comment, a community ban, or a mod mail mute.</p>");
    if !error.is_empty() {
        write!(
            html,
            "<p class='notice account-error' role='alert'>{}</p>",
            escape(error)
        )
        .unwrap();
    }
    render_removals(&mut html, state, content_draft);
    render_restrictions(&mut html, state, draft);
    render_history(&mut html, state);
    html.push_str("</section>");
    html
}

fn render_removals(html: &mut String, state: &State, draft: Option<(u64, &str)>) {
    html.push_str("<section class='preference-list' aria-labelledby='appealable-removals'><h2 id='appealable-removals'>Content you can appeal</h2>");
    let mut removals = state
        .content_removals
        .iter()
        .rev()
        .filter(|item| state.can_appeal_removal(item))
        .peekable();
    if removals.peek().is_none() {
        html.push_str("<p class='muted'>You have no removals available to appeal.</p>");
    }
    for removal in removals {
        let episode = removal.episode;
        let name = removal_community_name(state, &removal.community);
        let label = removal.kind.label();
        let fields = action_fields(
            "submit-content-appeal",
            "/appeals",
            &[("removal", &episode.to_string())],
        );
        let typed = draft
            .filter(|(id, _)| *id == episode)
            .map_or("", |(_, body)| body);
        write!(html, "<article class='log-item' id='removal-{episode}' tabindex='-1'><h3>{name} · {label} #{}</h3><p class='muted'>Removed {}</p><blockquote class='moderation-reply'><strong>Removal reason</strong><p class='account-message-body'>{}</p></blockquote><form class='stack-form content-appeal-form' method='post' action='/action'>{fields}<label>Why should moderators restore this {}?<textarea name='body' required rows='4' maxlength='4000'>{}</textarea></label><button class='button dark' type='submit'>Submit appeal</button></form></article>", removal.id, date(removal.removed_at), escape(&removal.reason_summary()), removal.kind.as_str(), escape(typed)).unwrap();
    }
    html.push_str("</section>");
}

fn render_restrictions(html: &mut String, state: &State, draft: Option<(u64, &str)>) {
    html.push_str("<section class='preference-list' aria-labelledby='active-restrictions'><h2 id='active-restrictions'>Active restrictions</h2>");
    let now = now();
    let mut restrictions = state
        .restrictions
        .iter()
        .rev()
        .filter(|item| item.username == state.username && item.is_active_at(now))
        .peekable();
    if restrictions.peek().is_none() {
        html.push_str("<p class='muted'>You have no active community restrictions.</p>");
    }
    for restriction in restrictions {
        let name = community_name(state, &restriction.community);
        let id = restriction.id;
        let expiry = restriction_expiry(restriction.expires_at);
        write!(html, "<article class='log-item' id='restriction-{id}'><h3>{name} · {}</h3><p class='muted'>Applied {}</p><p class='muted'>{expiry}</p>", restriction_name(&restriction.kind), date(restriction.created_at)).unwrap();
        if state.can_submit_appeal(restriction) {
            let fields = action_fields(
                "submit-appeal",
                "/appeals",
                &[("restriction", &id.to_string())],
            );
            let typed = draft
                .filter(|(restriction, _)| *restriction == id)
                .map_or("", |(_, body)| body);
            write!(html, "<form class='stack-form' method='post' action='/action'>{fields}<label>Why should the team lift this restriction?<textarea name='body' required rows='4' maxlength='4000'>{}</textarea></label><button class='button dark' type='submit'>Submit appeal</button></form>", escape(typed)).unwrap();
        } else if let Some(case) = state
            .appeals
            .iter()
            .find(|case| case.restriction_id == id && case.appellant == state.username)
        {
            write!(html, "<p><a class='subtle-link' href='#appeal-{}'>View your appeal</a> <span class='muted'>{}</span></p>", case.id, case.status.label()).unwrap();
        }
        html.push_str("</article>");
    }
    html.push_str("</section>");
}

fn render_history(html: &mut String, state: &State) {
    enum Case<'a> {
        Restriction(&'a Appeal),
        Removal(&'a ContentAppeal),
    }
    html.push_str("<section class='preference-list' aria-labelledby='appeal-history'><h2 id='appeal-history'>Appeal history</h2>");
    let mut cases: Vec<_> = state
        .appeals
        .iter()
        .filter(|case| case.appellant == state.username)
        .map(|case| (case.id, Case::Restriction(case)))
        .chain(
            state
                .content_appeals
                .iter()
                .filter(|case| case.appellant == state.username)
                .map(|case| (case.id, Case::Removal(case))),
        )
        .collect();
    cases.sort_by_key(|(id, _)| std::cmp::Reverse(*id));
    if cases.is_empty() {
        html.push_str("<p class='muted'>You haven’t submitted an appeal.</p>");
    }
    for (_, case) in cases {
        match case {
            Case::Restriction(case) => {
                let name = community_name(state, &case.community);
                write!(html, "<article class='log-item' id='appeal-{}'><span class='flair'>{}</span><h3>{name} · {}</h3><p class='muted'>Submitted {}</p><p class='account-message-body'>{}</p>", case.id, case.status.label(), restriction_name(&case.kind), date(case.created_at), escape(&case.body)).unwrap();
                render_response(html, case.status, case.decided_at, &case.response);
            }
            Case::Removal(case) => {
                let name = removal_community_name(state, &case.community);
                write!(html, "<article class='log-item' id='removal-appeal-{}' tabindex='-1'><span class='flair'>{}</span><h3>{name} · {} #{} removal</h3><p class='muted'>Submitted {}</p><p class='account-message-body'>{}</p><blockquote class='moderation-reply'><strong>Removal reason</strong><p class='account-message-body'>{}</p></blockquote>", case.id, case.status.label(), case.kind.label(), case.content_id, date(case.created_at), escape(&case.body), escape(&case.reason)).unwrap();
                render_response(html, case.status, case.decided_at, &case.response);
            }
        }
        html.push_str("</article>");
    }
    html.push_str("</section>");
}

fn render_response(html: &mut String, status: AppealStatus, decided: Option<u64>, response: &str) {
    if let Some(decided) = decided {
        let heading = match status {
            AppealStatus::Expired | AppealStatus::Restored | AppealStatus::Unavailable => {
                status.label()
            }
            _ => "Decision from the moderation team",
        };
        write!(html, "<blockquote class='moderation-reply'><strong>{heading}</strong><p class='muted'>{}</p><p class='account-message-body'>{}</p></blockquote>", date(decided), escape(response)).unwrap();
    }
}

fn community_name(state: &State, slug: &str) -> String {
    escape(community(state, slug).map_or(slug, |item| item.name.as_str()))
}

fn removal_community_name(state: &State, slug: &str) -> String {
    escape(
        community(state, slug)
            .filter(|item| state.can_view_community(item))
            .map_or(slug, |item| item.name.as_str()),
    )
}

fn restriction_name(kind: &str) -> &'static str {
    if kind == "mute" {
        "Mod mail mute"
    } else {
        "Community ban"
    }
}

fn date(timestamp: u64) -> String {
    format!(
        "<time datetime='{}Z'>{}</time>",
        to_local(timestamp, "UTC"),
        escape(&display(timestamp, "America/Chicago"))
    )
}
