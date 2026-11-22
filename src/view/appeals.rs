use std::fmt::Write;

use super::{action_fields, community, escape};
use crate::model::State;
use crate::schedule_time::{display, to_local};

pub(super) fn render(state: &State, draft: Option<(u64, &str)>, error: &str) -> String {
    let mut html = String::from("<section class='panel standalone moderation-tool'><h1>Your appeals</h1><p>Ask the moderation team to review a community ban or mod mail mute. Your appeal and the decision appear here.</p>");
    if !error.is_empty() {
        write!(
            html,
            "<p class='notice account-error' role='alert'>{}</p>",
            escape(error)
        )
        .unwrap();
    }
    html.push_str("<section class='preference-list' aria-labelledby='active-restrictions'><h2 id='active-restrictions'>Active restrictions</h2>");
    let mut restrictions = state
        .restrictions
        .iter()
        .rev()
        .filter(|item| item.username == state.username)
        .peekable();
    if restrictions.peek().is_none() {
        html.push_str("<p class='muted'>You have no active community restrictions.</p>");
    }
    for restriction in restrictions {
        let name = community_name(state, &restriction.community);
        let id = restriction.id;
        write!(html, "<article class='log-item' id='restriction-{id}'><h3>{name} · {}</h3><p class='muted'>Applied {}</p>", restriction_name(&restriction.kind), date(restriction.created_at)).unwrap();
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
    html.push_str("</section><section class='preference-list' aria-labelledby='appeal-history'><h2 id='appeal-history'>Appeal history</h2>");
    let mut cases = state
        .appeals
        .iter()
        .rev()
        .filter(|case| case.appellant == state.username)
        .peekable();
    if cases.peek().is_none() {
        html.push_str("<p class='muted'>You haven’t submitted an appeal.</p>");
    }
    for case in cases {
        let name = community_name(state, &case.community);
        write!(html, "<article class='log-item' id='appeal-{}'><span class='flair'>{}</span><h3>{name} · {}</h3><p class='muted'>Submitted {}</p><p class='account-message-body'>{}</p>", case.id, case.status.label(), restriction_name(&case.kind), date(case.created_at), escape(&case.body)).unwrap();
        if let Some(decided) = case.decided_at {
            write!(html, "<blockquote class='moderation-reply'><strong>Decision from the moderation team</strong><p class='muted'>{}</p><p class='account-message-body'>{}</p></blockquote>", date(decided), escape(&case.response)).unwrap();
        }
        html.push_str("</article>");
    }
    html.push_str("</section></section>");
    html
}

fn community_name(state: &State, slug: &str) -> String {
    escape(community(state, slug).map_or(slug, |item| item.name.as_str()))
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
