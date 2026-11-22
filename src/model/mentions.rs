use std::collections::BTreeSet;

use pulldown_cmark::{utils::TextMergeStream, Event, Parser, Tag, TagEnd};

use crate::references::{segments, Segment};

use super::{next_id, unix_seconds, Comment, Notification, Post, State, DELETED_ACCOUNT};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscussionNotification {
    Reply(u64),
    MentionPost,
    MentionComment(u64),
}

impl DiscussionNotification {
    pub fn comment(self) -> Option<u64> {
        match self {
            Self::Reply(id) | Self::MentionComment(id) => Some(id),
            Self::MentionPost => None,
        }
    }
}

pub(super) fn post_mentions(post: &Post) -> BTreeSet<String> {
    let mut recipients = BTreeSet::new();
    if !post.repost_title_from_source {
        collect_mentions(&post.title, &mut recipients);
    }
    collect_body_mentions(&post.body, true, &mut recipients);
    recipients
}

pub(super) fn comment_mentions(comment: &Comment) -> BTreeSet<String> {
    let mut recipients = BTreeSet::new();
    collect_body_mentions(&comment.body, comment.markdown, &mut recipients);
    recipients
}

fn collect_body_mentions(body: &str, markdown: bool, recipients: &mut BTreeSet<String>) {
    if !body.contains("u/") {
        return;
    }
    if !markdown {
        collect_mentions(body, recipients);
        return;
    }
    let mut skipped = 0;
    for event in TextMergeStream::new(Parser::new(body)) {
        match event {
            Event::Start(Tag::Link { .. } | Tag::Image { .. } | Tag::CodeBlock(_)) => {
                skipped += 1;
            }
            Event::End(TagEnd::Link | TagEnd::Image | TagEnd::CodeBlock) => skipped -= 1,
            Event::Text(text) if skipped == 0 => collect_mentions(&text, recipients),
            _ => {}
        }
    }
}

fn collect_mentions(text: &str, recipients: &mut BTreeSet<String>) {
    if text.contains("u/") {
        recipients.extend(segments(text).filter_map(|segment| match segment {
            Segment::Reference { href, .. } => href.strip_prefix("/u/").map(str::to_owned),
            Segment::Text(_) => None,
        }));
    }
}

impl State {
    pub(super) fn notify_post_mentions(
        &mut self,
        id: u64,
        previous: &BTreeSet<String>,
    ) -> Result<(), String> {
        let Some(post) = self.posts.iter().find(|post| post.id == id) else {
            return Ok(());
        };
        let mentioned = post_mentions(post);
        self.notify_mentions(
            id,
            DiscussionNotification::MentionPost,
            &mentioned,
            previous,
        )
    }

    pub(super) fn notify_comment_mentions(
        &mut self,
        id: u64,
        previous: &BTreeSet<String>,
    ) -> Result<(), String> {
        let Some(comment) = self.comments.iter().find(|comment| comment.id == id) else {
            return Ok(());
        };
        let mentioned = comment_mentions(comment);
        self.notify_mentions(
            comment.post,
            DiscussionNotification::MentionComment(id),
            &mentioned,
            previous,
        )
    }

    fn notify_mentions(
        &mut self,
        post_id: u64,
        target: DiscussionNotification,
        mentioned: &BTreeSet<String>,
        previous: &BTreeSet<String>,
    ) -> Result<(), String> {
        if mentioned.is_empty() {
            return Ok(());
        }
        let Some(post) = self.posts.iter().find(|post| post.id == post_id) else {
            return Ok(());
        };
        let Some(author) = self.discussion_notification_author(post, target) else {
            return Ok(());
        };
        let recipients: Vec<_> = mentioned
            .difference(previous)
            .filter(|recipient| {
                recipient.as_str() != author
                    && self.has_profile(recipient)
                    && self.notification_preferences_for(recipient).mentions
                    && self.can_receive_discussion_notification(post, author, recipient)
                    && !self.has_discussion_notification(recipient, post_id, target)
            })
            .collect();
        let context = if target.comment().is_some() {
            "a comment"
        } else {
            "a post"
        };
        let body = format!("u/{author} mentioned you in {context}.");
        for recipient in recipients {
            self.notifications.push(Notification {
                id: next_id(&mut self.next_notification)?,
                recipient: recipient.clone(),
                body: body.clone(),
                at: unix_seconds(),
                read: false,
                post: Some(post_id),
                discussion: Some(target),
                appeal: false,
                moderation: false,
                removal: None,
                silent: false,
            });
        }
        Ok(())
    }

    pub(super) fn has_discussion_notification(
        &self,
        recipient: &str,
        post: u64,
        target: DiscussionNotification,
    ) -> bool {
        self.notifications.iter().any(|item| {
            item.recipient == recipient
                && item.post == Some(post)
                && item.discussion == Some(target)
        })
    }

    pub fn can_view_discussion_notification(&self, item: &Notification) -> bool {
        let Some(target) = item.discussion else {
            return true;
        };
        let Some(post) = item
            .post
            .and_then(|id| self.posts.iter().find(|post| post.id == id))
        else {
            return false;
        };
        self.discussion_notification_author(post, target)
            .is_some_and(|author| {
                self.can_receive_discussion_notification(post, author, &self.username)
            })
    }

    fn discussion_notification_author<'a>(
        &'a self,
        post: &'a Post,
        target: DiscussionNotification,
    ) -> Option<&'a str> {
        if let Some(id) = target.comment() {
            self.comments
                .iter()
                .find(|comment| comment.id == id && comment.post == post.id && !comment.deleted)
                .filter(|comment| !self.is_comment_removed(comment.id))
                .map(|comment| comment.author.as_str())
        } else {
            Some(&post.author)
        }
    }

    fn can_receive_discussion_notification(
        &self,
        post: &Post,
        author: &str,
        recipient: &str,
    ) -> bool {
        !author.is_empty()
            && author != DELETED_ACCOUNT
            && !self.either_blocked(recipient, author)
            && !self.is_post_removed(post.id)
            && self.communities.iter().any(|item| {
                item.slug == post.community && self.can_view_community_as(item, recipient)
            })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::{fs, path::PathBuf, time::SystemTime};

    use super::*;

    const AUTHOR: &str = "mentions_author";
    const READER: &str = "mentions_reader";
    const OTHER: &str = "mentions_other";
    const PRIVATE: &str = "mention-private";
    const PUBLIC: &str = "open-debate";

    struct StateFile(PathBuf);

    impl Drop for StateFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn form(fields: &[(&str, &str)]) -> BTreeMap<String, String> {
        fields
            .iter()
            .map(|&(name, value)| (name.into(), value.into()))
            .collect()
    }

    #[test]
    fn parses_mentions_without_pinging_links_code_images_or_invalid_handles() {
        let body = concat!(
            "Hello **u/ALICE**, /u/alice and (‘u/Other_1’).\n\n",
            "`u/inline` [u/label](https://example.com/u/path) ![u/alt](https://example.com/image)\n\n",
            "https://example.com/u/address user@u/email u/name?query=1 u/invalid-name\n\n",
            "```text\nu/fenced\n```\n\n    u/indented\n",
        );
        let mut found = BTreeSet::new();
        collect_body_mentions(body, true, &mut found);
        assert_eq!(found, BTreeSet::from(["alice".into(), "other_1".into()]));
    }

    #[test]
    fn mention_delivery_respects_private_access_and_deduplicates_edits_across_restarts() {
        let file = StateFile(std::env::temp_dir().join(format!(
            "threadporch-mentions-{}-{:?}.state",
            std::process::id(),
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos(),
        )));
        let mut state = State::seeded();
        for handle in [READER, OTHER, AUTHOR] {
            state.activate_profile(Some(handle));
        }
        state
            .create_community(&form(&[
                ("slug", PRIVATE),
                ("name", "Mention circle"),
                ("visibility", "private"),
            ]))
            .unwrap();
        let membership = form(&[("community", PRIVATE), ("handle", READER)]);
        state.approve_member(&membership).unwrap();
        state
            .apply(
                "create",
                &form(&[
                    ("community", PRIVATE),
                    ("title", "Hello u/MENTIONS_READER and u/mentions_other"),
                    (
                        "body",
                        "Thanks again, **u/mentions_reader** and u/mentions_author.",
                    ),
                ]),
                &file.0,
            )
            .unwrap();
        let post = state.posts.last().unwrap().id;
        let id = post.to_string();
        assert_eq!(state.notifications.len(), 1);
        assert_eq!(state.notifications[0].recipient, READER);
        assert_eq!(
            state.notifications[0].discussion,
            Some(DiscussionNotification::MentionPost)
        );

        state
            .approve_member(&form(&[("community", PRIVATE), ("handle", OTHER)]))
            .unwrap();
        let edit = |body| form(&[("id", &id), ("title", "Edited title"), ("body", body)]);
        state.apply("edit", &edit("Only prose."), &file.0).unwrap();
        state
            .apply(
                "edit",
                &edit("Now u/mentions_reader and u/mentions_other can read it."),
                &file.0,
            )
            .unwrap();
        assert_eq!(state.notifications.len(), 2);

        let mut state = State::load(&file.0).unwrap();
        state.activate_profile(Some(READER));
        let notice = &state.notifications[0];
        assert_eq!(notice.post, Some(post));
        assert!(state.can_view_discussion_notification(notice));
        state.activate_profile(Some(AUTHOR));
        state.revoke_approved_member(&membership).unwrap();
        state.activate_profile(Some(READER));
        assert!(!state.can_view_discussion_notification(&state.notifications[0]));
    }

    #[test]
    fn mentions_and_regular_replies_share_the_comment_target_and_honor_preferences() {
        let mut state = State::seeded();
        state.activate_profile(Some(READER));
        state
            .create_post(&form(&[("community", PUBLIC), ("title", "Comment here")]))
            .unwrap();
        let post = state.posts.last().unwrap().id.to_string();
        state.activate_profile(Some(AUTHOR));
        let comment = |body| form(&[("post", &post), ("body", body), ("body_format", "markdown")]);
        state
            .create_comment(&comment("Hello **u/mentions_reader**."))
            .unwrap();
        let first = state.comments.last().unwrap().id;
        assert_eq!(
            state.notifications.len(),
            1,
            "avoid a duplicate reply notification"
        );
        assert_eq!(
            state.notifications[0].discussion,
            Some(DiscussionNotification::MentionComment(first))
        );

        state.activate_profile(Some(READER));
        let mut preferences = state.notification_preferences();
        preferences.mentions = false;
        state.set_notification_preferences(preferences);
        state.activate_profile(Some(AUTHOR));
        state
            .create_comment(&comment("u/mentions_reader mentioned again."))
            .unwrap();
        let second = state.comments.last().unwrap().id;
        assert_eq!(
            state.notifications[1].discussion,
            Some(DiscussionNotification::Reply(second))
        );

        state
            .delete_comment(&form(&[("id", &first.to_string())]))
            .unwrap();
        state.activate_profile(Some(READER));
        assert!(!state.can_view_discussion_notification(&state.notifications[0]));
        assert!(state.can_view_discussion_notification(&state.notifications[1]));
        state.blocked.insert(AUTHOR.into());
        assert!(!state.can_view_discussion_notification(&state.notifications[1]));
        state.activate_profile(Some(AUTHOR));
        state
            .create_comment(&comment("u/mentions_reader blocked us."))
            .unwrap();
        assert_eq!(state.notifications.len(), 2);
    }
}
