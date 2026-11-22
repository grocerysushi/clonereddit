use std::collections::BTreeMap;
use std::io::{self, Write};

use super::{
    bounded, decode, encode, id_field, next_id, number, value, Comment, Community, ContentKind,
    Post, Report, State,
};

const REPORT_SENT: &str = "Report sent to the community moderators.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportReason {
    Spam,
    Harassment,
    ThreatsOrViolence,
    PersonalInformation,
    CommunityRuleViolation,
    Other,
}

impl ReportReason {
    pub const ALL: [Self; 6] = [
        Self::Spam,
        Self::Harassment,
        Self::ThreatsOrViolence,
        Self::PersonalInformation,
        Self::CommunityRuleViolation,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spam => "spam",
            Self::Harassment => "harassment",
            Self::ThreatsOrViolence => "threats-or-violence",
            Self::PersonalInformation => "personal-information",
            Self::CommunityRuleViolation => "community-rule-violation",
            Self::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Spam => "Spam",
            Self::Harassment => "Harassment",
            Self::ThreatsOrViolence => "Threats or violence",
            Self::PersonalInformation => "Personal information",
            Self::CommunityRuleViolation => "Community rule violation",
            Self::Other => "Other",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|reason| reason.as_str() == value)
    }
}

impl Report {
    pub fn kind(&self) -> ContentKind {
        if self.comment.is_some() {
            ContentKind::Comment
        } else {
            ContentKind::Post
        }
    }

    pub fn content_id(&self) -> u64 {
        self.comment.unwrap_or(self.post)
    }

    pub(super) fn write_record(&self, file: &mut impl Write) -> io::Result<()> {
        write!(
            file,
            "report\t{}\t{}\t{}\t{}\t{}\t{}",
            self.id,
            self.post,
            encode(&self.region),
            encode(&self.details),
            self.reviewed,
            encode(&self.reporter)
        )?;
        if let Some(reason) = self.reason {
            match self.comment {
                Some(comment) => write!(file, "\tcomment\t{comment}\t{}", reason.as_str())?,
                None => write!(file, "\tpost\t{}", reason.as_str())?,
            }
            if let Some(community) = &self.community {
                write!(file, "\t{}", encode(community))?;
            }
        }
        writeln!(file)
    }
}

impl State {
    pub fn report_target(&self, kind: ContentKind, id: u64) -> Option<(&Post, Option<&Comment>)> {
        let comment = match kind {
            ContentKind::Post => None,
            ContentKind::Comment => Some(self.comments.iter().find(|item| {
                item.id == id
                    && !item.deleted
                    && !self.is_comment_removed(id)
                    && !self.blocked.contains(&item.author)
            })?),
        };
        let post_id = comment.map_or(id, |item| item.post);
        self.viewable_post_community(post_id)?;
        let post = self.posts.iter().find(|item| item.id == post_id)?;
        Some((post, comment))
    }

    pub fn report_community(&self, report: &Report) -> Option<&Community> {
        let slug = match self.posts.iter().find(|item| item.id == report.post) {
            Some(post) => post.community.as_str(),
            None => {
                report.reason?;
                report.community.as_deref()?
            }
        };
        self.communities.iter().find(|item| item.slug == slug)
    }

    pub(super) fn report_content(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let kind = ContentKind::parse(value(form, "kind")).ok_or("Choose a post or comment.")?;
        let id = id_field(form, "id")?;
        let (post, comment) = self
            .report_target(kind, id)
            .ok_or("This post or comment is unavailable.")?;
        let community = post.community.clone();
        let post = post.id;
        let comment = comment.map(|item| item.id);
        let reason =
            ReportReason::parse(value(form, "reason")).ok_or("Choose a reason for this report.")?;
        let details = bounded(form, "details", "Report details", 4_000)?;
        if self.reports.iter().any(|report| {
            !report.reviewed
                && report.reporter == self.username
                && report.post == post
                && report.comment == comment
                && report.reason == Some(reason)
                && report.details == details
        }) {
            return Ok(REPORT_SENT);
        }
        let id = next_id(&mut self.next_report)?;
        self.reports.push(Report {
            id,
            post,
            comment,
            reason: Some(reason),
            community: Some(community),
            region: String::new(),
            details: details.into(),
            reviewed: false,
            reporter: self.username.clone(),
        });
        Ok(REPORT_SENT)
    }

    pub(super) fn read_report_record(&mut self, fields: &[&str]) -> Option<()> {
        let ["report", id, post, region, details, status @ ..] = fields else {
            return None;
        };
        let (reviewed, reporter, target) = match status {
            [] => (false, self.username.clone(), &[][..]),
            [reviewed] => (number(reviewed)?, self.username.clone(), &[][..]),
            [reviewed, reporter, target @ ..] => (number(reviewed)?, decode(reporter)?, target),
        };
        let (comment, reason, scope) = match target {
            [] => (None, None, &[][..]),
            ["post", reason, scope @ ..] => (None, Some(ReportReason::parse(reason)?), scope),
            ["comment", id, reason, scope @ ..] => {
                (Some(number(id)?), Some(ReportReason::parse(reason)?), scope)
            }
            _ => return None,
        };
        let community = match scope {
            [] => None,
            [slug] => Some(decode(slug)?),
            _ => return None,
        };
        self.reports.push(Report {
            id: number(id)?,
            post: number(post)?,
            comment,
            reason,
            community,
            region: decode(region)?,
            details: decode(details)?,
            reviewed,
            reporter,
        });
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    const OWNER: &str = "report_owner";
    const REPORTER: &str = "report_member";
    const AUTHOR: &str = "report_author";
    const OTHER_OWNER: &str = "report_other";
    const ROOM: &str = "report-room";
    const OTHER_ROOM: &str = "other-report-room";

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "commonline-reporting-{}-{}.state",
                std::process::id(),
                SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
            )))
        }
    }

    impl Drop for Fixture {
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

    fn room(state: &mut State, slug: &str, visibility: &str) -> u64 {
        state
            .create_community(&form(&[
                ("slug", slug),
                ("name", slug),
                ("visibility", visibility),
            ]))
            .unwrap();
        state
            .create_post(&form(&[("community", slug), ("title", "Reportable post")]))
            .unwrap();
        state.posts.last().unwrap().id
    }

    #[test]
    fn typed_reports_keep_status_scope_and_private_details_across_reload() {
        let fixture = Fixture::new();
        let mut state = State::seeded();
        state.activate_profile(Some(OTHER_OWNER));
        let other_post = room(&mut state, OTHER_ROOM, "public").to_string();
        state.activate_profile(Some(OWNER));
        let post = room(&mut state, ROOM, "public").to_string();
        state
            .create_comment(&form(&[("post", &post), ("body", "Comment subject")]))
            .unwrap();
        let comment = state.comments.last().unwrap().id.to_string();
        state.activate_profile(Some(REPORTER));
        let comment_report = form(&[
            ("kind", "comment"),
            ("id", &comment),
            ("reason", "harassment"),
            ("details", "Private report context"),
            ("post", &other_post),
        ]);
        state
            .apply("report-content", &comment_report, &fixture.0)
            .unwrap();
        state
            .apply("report-content", &comment_report, &fixture.0)
            .unwrap();
        assert_eq!(state.reports.len(), 1);
        let report = &state.reports[0];
        assert_eq!(report.post.to_string(), post);
        assert_eq!(report.kind(), ContentKind::Comment);
        assert_eq!(report.content_id().to_string(), comment);
        assert_eq!(report.community.as_deref(), Some(ROOM));
        let report_id = report.id.to_string();

        let mut post_report = form(&[
            ("kind", "post"),
            ("id", &post),
            ("reason", "arbitrary label"),
        ]);
        assert!(state
            .apply("report-content", &post_report, &fixture.0)
            .is_err());
        post_report.insert("reason".into(), "spam".into());
        post_report.insert("details".into(), "a".repeat(4_001));
        assert!(state
            .apply("report-content", &post_report, &fixture.0)
            .is_err());
        post_report.remove("details");
        state
            .apply("report-content", &post_report, &fixture.0)
            .unwrap();
        state
            .apply(
                "report",
                &form(&[
                    ("post", &post),
                    ("region", "US"),
                    ("details", "Legacy legal report"),
                ]),
                &fixture.0,
            )
            .unwrap();

        let review = form(&[("id", &report_id), ("reviewed", "yes")]);
        let legal_review = form(&[
            ("id", &state.reports[2].id.to_string()),
            ("reviewed", "yes"),
        ]);
        assert!(state.apply("review-report", &review, &fixture.0).is_err());
        state.reports[0].community = Some(OTHER_ROOM.into());
        state.reports[1].community = None;
        state.activate_profile(Some(OTHER_OWNER));
        assert!(state.apply("review-report", &review, &fixture.0).is_err());
        state.activate_profile(Some(OWNER));
        state
            .apply("delete-comment", &form(&[("id", &comment)]), &fixture.0)
            .unwrap();
        state
            .apply("delete", &form(&[("id", &post)]), &fixture.0)
            .unwrap();

        let mut state = State::load(&fixture.0).unwrap();
        state.activate_profile(Some(OTHER_OWNER));
        assert!(state.apply("review-report", &review, &fixture.0).is_err());
        state.activate_profile(Some(OWNER));
        assert!(state
            .apply("review-report", &legal_review, &fixture.0)
            .is_err());
        state.apply("review-report", &review, &fixture.0).unwrap();
        assert!(state
            .notifications
            .iter()
            .all(|item| !item.body.contains("Private report context")));
        assert!(state
            .mod_log
            .iter()
            .all(|event| !event.action.contains("Private report context")));

        let state = State::load(&fixture.0).unwrap();
        assert_eq!(state.reports.len(), 3);
        assert_eq!(state.reports[0].reporter, REPORTER);
        assert_eq!(state.reports[0].reason, Some(ReportReason::Harassment));
        assert_eq!(state.reports[0].details, "Private report context");
        assert!(state.reports[0].reviewed);
        assert_eq!(state.reports[0].community.as_deref(), Some(ROOM));
        assert_eq!(state.reports[1].kind(), ContentKind::Post);
        assert_eq!(state.reports[1].content_id().to_string(), post);
        assert_eq!(state.reports[1].reason, Some(ReportReason::Spam));
        assert_eq!(state.reports[1].community.as_deref(), Some(ROOM));
        assert_eq!(state.reports[2].reason, None);
        assert_eq!(state.reports[2].community, None);
        assert_eq!(state.reports[2].region, "US");
        let lengths: Vec<_> = fs::read_to_string(&fixture.0)
            .unwrap()
            .lines()
            .filter(|line| line.starts_with("report\t"))
            .map(|line| line.split('\t').count())
            .collect();
        assert_eq!(lengths, [11, 10, 7]);
    }

    #[test]
    fn typed_targets_follow_direct_and_thread_visibility() {
        let fixture = Fixture::new();
        let mut state = State::seeded();
        state.activate_profile(Some(OWNER));
        let post = room(&mut state, ROOM, "private");
        for member in [REPORTER, AUTHOR] {
            state
                .approve_member(&form(&[("community", ROOM), ("handle", member)]))
                .unwrap();
        }
        state.activate_profile(Some(AUTHOR));
        state
            .create_comment(&form(&[
                ("post", &post.to_string()),
                ("body", "Thread content"),
            ]))
            .unwrap();
        let comment = state.comments.last().unwrap().id;
        state.activate_profile(Some(OTHER_OWNER));
        assert!(state.report_target(ContentKind::Post, post).is_none());
        assert!(state.report_target(ContentKind::Comment, comment).is_none());
        assert!(state
            .apply(
                "report-content",
                &form(&[
                    ("kind", "comment"),
                    ("id", &comment.to_string()),
                    ("reason", "other")
                ]),
                &fixture.0
            )
            .is_err());

        state.activate_profile(Some(REPORTER));
        assert!(state.report_target(ContentKind::Post, post).is_some());
        assert!(state.report_target(ContentKind::Comment, comment).is_some());
        state
            .apply("block", &form(&[("author", OWNER)]), &fixture.0)
            .unwrap();
        assert!(state.report_target(ContentKind::Post, post).is_some());
        assert!(state.report_target(ContentKind::Comment, comment).is_some());
        state
            .apply("block", &form(&[("author", AUTHOR)]), &fixture.0)
            .unwrap();
        assert!(state.report_target(ContentKind::Comment, comment).is_none());
        state
            .apply("block", &form(&[("author", AUTHOR)]), &fixture.0)
            .unwrap();

        state.activate_profile(Some(OWNER));
        state
            .apply(
                "remove-content",
                &form(&[
                    ("community", ROOM),
                    ("kind", "comment"),
                    ("id", &comment.to_string()),
                    ("reason", "Removed for test"),
                ]),
                &fixture.0,
            )
            .unwrap();
        state.activate_profile(Some(REPORTER));
        assert!(state.report_target(ContentKind::Comment, comment).is_none());
        state.activate_profile(Some(OWNER));
        state
            .apply(
                "restore-content",
                &form(&[
                    ("community", ROOM),
                    ("kind", "comment"),
                    ("id", &comment.to_string()),
                ]),
                &fixture.0,
            )
            .unwrap();
        state
            .apply(
                "remove-content",
                &form(&[
                    ("community", ROOM),
                    ("kind", "post"),
                    ("id", &post.to_string()),
                    ("reason", "Removed for test"),
                ]),
                &fixture.0,
            )
            .unwrap();
        state.activate_profile(Some(REPORTER));
        assert!(state.report_target(ContentKind::Post, post).is_none());
        assert!(state.report_target(ContentKind::Comment, comment).is_none());
        assert!(state.reports.is_empty());
    }
}
