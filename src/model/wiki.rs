use std::collections::BTreeMap;
use std::io::{self, Write};

use super::{
    canonical_u64, decode, encode, required, restriction_handle, value, Community, ModPermission,
    ModeratorRole, State,
};

pub const WIKI_SLUG_LIMIT: usize = 60;
pub const WIKI_TITLE_LIMIT: usize = 120;
pub const WIKI_BODY_LIMIT: usize = 10_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WikiRevision {
    pub title: String,
    pub body: String,
}

#[derive(Clone)]
pub struct WikiPage {
    pub slug: String,
    pub draft: WikiRevision,
    pub published: Option<WikiRevision>,
    pub revision: u64,
    pub editors: Vec<String>,
}

pub fn canonical_wiki_slug(input: &str) -> Option<String> {
    let slug = input.trim().to_ascii_lowercase();
    if slug.len() > WIKI_SLUG_LIMIT
        || slug
            .split('-')
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_alphanumeric()))
    {
        return None;
    }
    Some(slug)
}

impl WikiRevision {
    fn submitted(form: &BTreeMap<String, String>) -> Result<Self, String> {
        let title = required(form, "title", "Page title", WIKI_TITLE_LIMIT)?;
        if title.chars().any(char::is_control) {
            return Err("Enter the page title on one line.".into());
        }
        let body = form.get("body").map_or("", String::as_str);
        if body.chars().count() > WIKI_BODY_LIMIT {
            return Err(format!(
                "Page content can be up to {WIKI_BODY_LIMIT} characters."
            ));
        }
        Ok(Self {
            title: title.into(),
            body: body.into(),
        })
    }

    fn from_record(title: &str, body: &str) -> Option<Self> {
        let title = decode(title)?;
        let body = decode(body)?;
        if title.trim().is_empty()
            || title.chars().count() > WIKI_TITLE_LIMIT
            || title.chars().any(char::is_control)
            || body.chars().count() > WIKI_BODY_LIMIT
        {
            return None;
        }
        Some(Self { title, body })
    }
}

impl WikiPage {
    fn check_revision(&self, form: &BTreeMap<String, String>) -> Result<(), String> {
        if canonical_u64(value(form, "revision")) == Some(self.revision) {
            Ok(())
        } else {
            Err("The saved draft has changed. Reload the page to review the latest draft.".into())
        }
    }
}

impl Community {
    pub fn wiki_pages(&self) -> &[WikiPage] {
        &self.wiki_pages
    }

    pub fn wiki_page(&self, slug: &str) -> Option<&WikiPage> {
        self.wiki_pages.iter().find(|page| page.slug == slug)
    }

    pub(super) fn write_wiki(&self, file: &mut impl Write) -> io::Result<()> {
        for page in &self.wiki_pages {
            write!(
                file,
                "wiki-page\t{}\t{}\t{}\t{}\t{}",
                encode(&self.slug),
                encode(&page.slug),
                page.revision,
                encode(&page.draft.title),
                encode(&page.draft.body)
            )?;
            if let Some(published) = &page.published {
                write!(
                    file,
                    "\t{}\t{}",
                    encode(&published.title),
                    encode(&published.body)
                )?;
            }
            writeln!(file)?;
            for handle in &page.editors {
                writeln!(
                    file,
                    "wiki-editor\t{}\t{}\t{}",
                    encode(&self.slug),
                    encode(&page.slug),
                    encode(handle)
                )?;
            }
        }
        Ok(())
    }
}

impl State {
    pub fn can_edit_wiki_page(&self, item: &Community, page: &WikiPage) -> bool {
        self.can_moderate(item, ModPermission::Content)
            || (!self.guest()
                && page.editors.contains(&self.username)
                && self.is_eligible_wiki_editor(item, &self.username))
    }

    fn is_eligible_wiki_editor(&self, item: &Community, handle: &str) -> bool {
        if self.is_banned(&item.slug, handle) {
            return false;
        }
        if self.is_approved_member(&item.slug, handle) {
            return true;
        }
        if item.visibility == "private" {
            return false;
        }
        if handle == self.username {
            self.joined.contains(&item.slug)
        } else {
            self.account_profiles
                .get(handle)
                .is_some_and(|profile| profile.joined.contains(&item.slug))
        }
    }

    pub(super) fn create_wiki_page(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Content)?;
        let slug = canonical_wiki_slug(value(form, "page")).ok_or(
            "Use 1 to 60 letters, numbers, or single hyphens between words for the page address.",
        )?;
        let item = &mut self.communities[index];
        if item.wiki_page(&slug).is_some() {
            return Err("That wiki page address is already in use in this community.".into());
        }
        item.wiki_pages.push(WikiPage {
            slug,
            draft: WikiRevision::submitted(form)?,
            published: None,
            revision: 1,
            editors: Vec::new(),
        });
        Ok("Wiki page draft created.")
    }

    pub(super) fn save_wiki_page(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.community_index(form)?;
        let item = &self.communities[index];
        let slug = value(form, "page");
        let position = item
            .wiki_pages
            .iter()
            .position(|page| page.slug == slug && self.can_edit_wiki_page(item, page))
            .ok_or("This wiki page is unavailable for editing.")?;
        let page = &mut self.communities[index].wiki_pages[position];
        page.check_revision(form)?;
        let draft = WikiRevision::submitted(form)?;
        if draft != page.draft {
            page.revision = page
                .revision
                .checked_add(1)
                .ok_or("The wiki page has reached its revision limit.")?;
            page.draft = draft;
        }
        Ok("Wiki draft saved.")
    }

    pub(super) fn publish_wiki_page(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Content)?;
        let item = &mut self.communities[index];
        let slug = value(form, "page");
        let page = item
            .wiki_pages
            .iter_mut()
            .find(|page| page.slug == slug)
            .ok_or("Choose a wiki page in this community.")?;
        page.check_revision(form)?;
        if page.draft.body.trim().is_empty() {
            return Err("Add some content to the draft before publishing.".into());
        }
        if page.published.as_ref() == Some(&page.draft) {
            return Ok("This wiki draft is already published.");
        }
        page.published = Some(page.draft.clone());
        let community = item.slug.clone();
        self.record_moderation_action(
            &community,
            &format!("Published wiki page {slug} by u/{}", self.username),
        )?;
        Ok("Wiki page published.")
    }

    pub(super) fn set_wiki_editor(
        &mut self,
        form: &BTreeMap<String, String>,
        grant: bool,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Content)?;
        let item = &self.communities[index];
        let slug = value(form, "page");
        let position = item
            .wiki_pages
            .iter()
            .position(|page| page.slug == slug)
            .ok_or("Choose a wiki page in this community.")?;
        let handle = restriction_handle(value(form, "handle"))?;
        if grant {
            if item.owner.as_deref() == Some(handle.as_str())
                || self.moderator_role(item, &handle) == Some(ModeratorRole::Moderator)
            {
                return Err("This moderator already has wiki access.".into());
            }
            if !self.is_eligible_wiki_editor(item, &handle) {
                return Err(
                    "Choose a joined or approved member with access to this community.".into(),
                );
            }
        }
        let item = &mut self.communities[index];
        let editors = &mut item.wiki_pages[position].editors;
        let existing = editors.iter().position(|editor| editor == &handle);
        let (audit, notice) = match (grant, existing) {
            (true, None) => {
                editors.push(handle.clone());
                (
                    format!("Granted u/{handle} editing access to wiki page {slug}"),
                    "Wiki editor added.",
                )
            }
            (true, Some(_)) => return Ok("This member already has wiki editing access."),
            (false, Some(position)) => {
                editors.remove(position);
                (
                    format!("Revoked u/{handle} editing access to wiki page {slug}"),
                    "Wiki editor removed.",
                )
            }
            (false, None) => return Ok("This member’s wiki editing access is already removed."),
        };
        let community = item.slug.clone();
        self.record_moderation_action(&community, &audit)?;
        Ok(notice)
    }

    pub(super) fn read_wiki_page(
        &mut self,
        community: &str,
        slug: &str,
        revision: &str,
        title: &str,
        body: &str,
        published: &[&str],
    ) -> Option<()> {
        let community = decode(community)?;
        let slug = decode(slug)?;
        if canonical_wiki_slug(&slug).as_deref() != Some(slug.as_str()) {
            return None;
        }
        let revision = canonical_u64(revision).filter(|&revision| revision > 0)?;
        let draft = WikiRevision::from_record(title, body)?;
        let published = match published {
            [] => None,
            [title, body] => {
                let copy = WikiRevision::from_record(title, body)?;
                if copy.body.trim().is_empty() {
                    return None;
                }
                Some(copy)
            }
            _ => return None,
        };
        let item = self
            .communities
            .iter_mut()
            .find(|item| item.slug == community && item.owner.is_some())?;
        if item.wiki_page(&slug).is_some() {
            return None;
        }
        item.wiki_pages.push(WikiPage {
            slug,
            draft,
            published,
            revision,
            editors: Vec::new(),
        });
        Some(())
    }

    pub(super) fn read_wiki_editor(
        &mut self,
        community: &str,
        slug: &str,
        handle: &str,
    ) -> Option<()> {
        let community = decode(community)?;
        let slug = decode(slug)?;
        let handle = decode(handle)?;
        if restriction_handle(&handle).ok()? != handle {
            return None;
        }
        let page = self
            .communities
            .iter_mut()
            .find(|item| item.slug == community)?
            .wiki_pages
            .iter_mut()
            .find(|page| page.slug == slug)?;
        if page.editors.contains(&handle) {
            return None;
        }
        page.editors.push(handle);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const OWNER: &str = "wiki_owner";
    const EDITOR: &str = "wiki_editor";
    const PUBLIC: &str = "public-wiki";
    const PRIVATE: &str = "private-wiki";
    const PAGE: &str = "getting-started";

    struct StateFile(PathBuf);

    impl StateFile {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            Self(std::env::temp_dir().join(format!(
                "threadporch-wiki-{}-{timestamp}.state",
                std::process::id()
            )))
        }
    }

    impl Drop for StateFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn form(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
        fields
            .iter()
            .map(|&(key, value)| (key.into(), value.into()))
            .collect()
    }

    fn fixture() -> State {
        let mut state = State::seeded();
        state.activate_profile(Some(OWNER));
        for (slug, visibility) in [(PUBLIC, "public"), (PRIVATE, "private")] {
            state
                .create_community(&form(&[
                    ("slug", slug),
                    ("name", slug),
                    ("visibility", visibility),
                ]))
                .unwrap();
        }
        state
    }

    fn community<'a>(state: &'a State, slug: &str) -> &'a Community {
        state
            .communities
            .iter()
            .find(|item| item.slug == slug)
            .unwrap()
    }

    fn create_page(state: &mut State, community: &str, slug: &str, body: &str) {
        state
            .create_wiki_page(&form(&[
                ("community", community),
                ("page", slug),
                ("title", "Getting started"),
                ("body", body),
            ]))
            .unwrap();
    }

    #[test]
    fn publishing_keeps_the_live_copy_until_the_current_draft_is_published() {
        let file = StateFile::new();
        let state = fixture();
        state.save(&file.0).unwrap();
        let mut state = State::load(&file.0).unwrap();
        assert!(community(&state, PUBLIC).wiki_pages().is_empty());
        create_page(&mut state, PUBLIC, " Getting-Started ", "");
        create_page(&mut state, PRIVATE, PAGE, "Other wiki");
        let publish = form(&[("community", PUBLIC), ("page", PAGE), ("revision", "1")]);
        assert!(state.publish_wiki_page(&publish).is_err());

        let original = "  **Original**\n\t100% complete\n";
        let mut edit = publish.clone();
        edit.insert("title".into(), "Original title".into());
        edit.insert("body".into(), original.into());
        state.apply("save-wiki-page", &edit, &file.0).unwrap();
        assert!(state.apply("publish-wiki-page", &publish, &file.0).is_err());
        let mut publish = publish;
        publish.insert("revision".into(), "2".into());
        state.apply("publish-wiki-page", &publish, &file.0).unwrap();

        edit.insert("revision".into(), "2".into());
        edit.insert("title".into(), "Future title".into());
        edit.insert("body".into(), "Edited by another author".into());
        state.apply("save-wiki-page", &edit, &file.0).unwrap();
        assert!(state.apply("save-wiki-page", &edit, &file.0).is_err());
        let mut state = State::load(&file.0).unwrap();
        let page = community(&state, PUBLIC).wiki_page(PAGE).unwrap();
        assert_eq!(page.revision, 3);
        assert_eq!(page.draft.title, "Future title");
        let live = page.published.as_ref().unwrap();
        assert_eq!(live.title, "Original title");
        assert_eq!(live.body, original);
        assert!(community(&state, PRIVATE)
            .wiki_page(PAGE)
            .unwrap()
            .published
            .is_none());
        publish.insert("revision".into(), "3".into());
        state.apply("publish-wiki-page", &publish, &file.0).unwrap();
        let page = community(&state, PUBLIC).wiki_page(PAGE).unwrap();
        assert_eq!(page.published.as_ref(), Some(&page.draft));
    }

    #[test]
    fn editors_are_page_scoped_and_need_current_community_access() {
        let file = StateFile::new();
        let mut state = fixture();
        create_page(&mut state, PUBLIC, PAGE, "Public draft");
        create_page(&mut state, PUBLIC, "staff", "Staff draft");
        create_page(&mut state, PRIVATE, PAGE, "Private draft");
        let public_grant = form(&[("community", PUBLIC), ("page", PAGE), ("handle", EDITOR)]);
        assert!(state.set_wiki_editor(&public_grant, true).is_err());
        state.activate_profile(Some(EDITOR));
        state.join_community(&public_grant).unwrap();
        state.activate_profile(Some(OWNER));
        state.set_wiki_editor(&public_grant, true).unwrap();

        let mut private_grant = public_grant.clone();
        private_grant.insert("community".into(), PRIVATE.into());
        assert!(state.set_wiki_editor(&private_grant, true).is_err());
        state.approve_member(&private_grant).unwrap();
        state
            .apply("grant-wiki-editor", &private_grant, &file.0)
            .unwrap();
        let mut state = State::load(&file.0).unwrap();
        state.activate_profile(Some(EDITOR));
        let public = community(&state, PUBLIC);
        let private = community(&state, PRIVATE);
        assert!(state.can_edit_wiki_page(public, public.wiki_page(PAGE).unwrap()));
        assert!(!state.can_edit_wiki_page(public, public.wiki_page("staff").unwrap()));
        assert!(state.can_edit_wiki_page(private, private.wiki_page(PAGE).unwrap()));
        let mut public_edit = form(&[
            ("community", PUBLIC),
            ("page", PAGE),
            ("revision", "1"),
            ("title", "Member revision"),
            ("body", "Member’s proposal"),
        ]);
        state
            .apply("save-wiki-page", &public_edit, &file.0)
            .unwrap();
        public_edit.insert("revision".into(), "2".into());
        assert!(state.publish_wiki_page(&public_edit).is_err());
        assert!(state.set_wiki_editor(&public_grant, false).is_err());
        state.join_community(&public_grant).unwrap();
        assert!(state.save_wiki_page(&public_edit).is_err());
        state.join_community(&public_grant).unwrap();

        state.activate_profile(Some(OWNER));
        state.revoke_approved_member(&private_grant).unwrap();
        state
            .restrict_user(&form(&[
                ("community", PUBLIC),
                ("username", EDITOR),
                ("kind", "ban"),
            ]))
            .unwrap();
        state.activate_profile(Some(EDITOR));
        for slug in [PRIVATE, PUBLIC] {
            let item = community(&state, slug);
            assert!(!state.can_edit_wiki_page(item, item.wiki_page(PAGE).unwrap()));
        }

        state.activate_profile(Some(OWNER));
        state.set_wiki_editor(&public_grant, false).unwrap();
        assert!(community(&state, PUBLIC)
            .wiki_page(PAGE)
            .unwrap()
            .editors
            .is_empty());
        state.delete_account_profile(EDITOR);
        assert!(community(&state, PRIVATE)
            .wiki_page(PAGE)
            .unwrap()
            .editors
            .is_empty());
        assert!(state
            .mod_log
            .iter()
            .any(|entry| entry.action.contains("Granted")));
        assert!(state
            .mod_log
            .iter()
            .any(|entry| entry.action.contains("Revoked")));
    }
}
