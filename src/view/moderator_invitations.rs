use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use crate::http::Response;
use crate::model::{ModPermission, ModeratorRole, State};

use super::{action_fields, community, encode, escape, field, layout, missing};

pub(super) const PATH: &str = "/moderation/invitations";

pub(super) fn permission_list(role: ModeratorRole) -> String {
    role.permissions()
        .map(ModPermission::label)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn render(state: &State, error: Option<&str>) -> String {
    let mut html = String::from("<section class='panel standalone'><h1>Moderator invitations</h1>");
    if let Some(error) = error {
        write!(
            html,
            "<p class='notice setup-error' role='alert'>{}</p>",
            escape(error)
        )
        .unwrap();
    }
    let pending = state.incoming_moderator_invitations().collect::<Vec<_>>();
    if pending.is_empty() {
        html.push_str("<p>No pending invitations.</p>");
    } else {
        html.push_str("<ul class='creator-list'>");
        for invite in pending.iter().rev() {
            let Some(item) = community(state, &invite.community) else {
                continue;
            };
            let id = invite.id.to_string();
            let name = escape(&item.name);
            let permissions = permission_list(invite.role);
            let slug = escape(&item.slug);
            let target = [("id", id.as_str())];
            let accept = action_fields("accept-moderator-invitation", PATH, &target);
            let decline = action_fields("decline-moderator-invitation", PATH, &target);
            write!(html, "<li class='creator-row'><div class='creator-row-main'><h2>{name}</h2><p class='muted'>{permissions}</p></div><div class='creator-row-actions'><form action='/action' method='post'>{accept}<button class='button dark' type='submit' aria-label='Accept moderator invitation for c/{slug}'>Accept</button></form><form action='/action' method='post'>{decline}<button class='button light' type='submit' aria-label='Decline moderator invitation for c/{slug}'>Decline</button></form></div></li>").unwrap();
        }
        html.push_str("</ul>");
    }

    let mut roles = state
        .moderator_roles
        .iter()
        .filter(|entry| entry.handle == state.username)
        .peekable();
    if roles.peek().is_some() {
        html.push_str("<section class='preference-list'><h2>Your moderator roles</h2><ul class='creator-list'>");
        for role in roles {
            let Some(item) = community(state, &role.community) else {
                continue;
            };
            let name = escape(&item.name);
            let slug = encode(&item.slug);
            let permissions = permission_list(role.role);
            write!(html, "<li class='creator-row'><div class='creator-row-main'><strong>{name}</strong><p class='muted'>{permissions}</p></div><div class='creator-row-actions'><a class='button light' href='/moderation/tools?community={slug}' aria-label='Mod tools for {name}'>Mod tools</a></div></li>").unwrap();
        }
        html.push_str("</ul></section>");
    }
    html.push_str("</section>");
    html
}

pub(super) fn respond(
    state: &mut State,
    store: &Path,
    action: &str,
    form: &BTreeMap<String, String>,
) -> Response {
    let id = field(form, "id").parse::<u64>().ok();
    if !state
        .incoming_moderator_invitations()
        .any(|invite| Some(invite.id) == id)
    {
        return missing(state);
    }
    match state.apply(action, form, store) {
        Ok(notice) => Response::redirect(&format!("{PATH}?notice={}", encode(notice))),
        Err(error) => Response::html(layout(
            state,
            PATH,
            "Moderator invitations",
            &render(state, Some(&error)),
            "",
            "",
        )),
    }
}
