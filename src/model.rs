use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "model/account_data.rs"]
mod account_data;
use account_data::ProfileData;
#[path = "model/account_deletion.rs"]
mod account_deletion;
pub use account_deletion::DELETED_ACCOUNT;
#[path = "model/approved_members.rs"]
mod approved_members;
pub use approved_members::ApprovedMember;
#[path = "model/automod.rs"]
mod automod;
pub use automod::{AutoModAction, AutoModFlag, AutoModRule, AutoModScope};
use automod::{AUTOMOD_REMOVED_COMMENT, AUTOMOD_REMOVED_POST};
#[path = "model/chat_requests.rs"]
mod chat_requests;
pub use chat_requests::{ChatRequest, ChatRequestPermission};
#[path = "model/community_rules.rs"]
mod community_rules;
pub use community_rules::{
    CommunityRule, COMMUNITY_RULE_DESCRIPTION_MAX, COMMUNITY_RULE_TITLE_MAX,
};
#[path = "model/content_moderation.rs"]
mod content_moderation;
pub use content_moderation::{ContentKind, ContentRemoval};
#[path = "model/discussion_locks.rs"]
mod discussion_locks;
#[path = "model/highlights.rs"]
mod highlights;
#[path = "model/moderation_appeals.rs"]
mod moderation_appeals;
pub use moderation_appeals::{Appeal, AppealStatus};
#[path = "model/moderation_roles.rs"]
mod moderation_roles;
pub use moderation_roles::{ModPermission, ModeratorAssignment, ModeratorRole};
#[path = "model/notification_preferences.rs"]
mod notification_preferences;
pub use notification_preferences::NotificationPreferences;
#[path = "model/reporting.rs"]
mod reporting;
pub use reporting::{ReportReason, ReportedRule};
#[path = "model/timed_restrictions.rs"]
mod timed_restrictions;
use timed_restrictions::{representable_expiration, restriction_expiration};

const FORMAT: &str = "commonline-v2";
const LEGACY_FORMAT: &str = "commonline-v1";
const TEXT_LIMIT: usize = 8_000;
const RECENT_LIMIT: usize = 6;
const HISTORY_LIMIT: usize = 50;
const IMAGE_LIMIT: usize = 512 * 1024;
const SOCIAL_LINK_LIMIT: usize = 6;
const ARTWORK_PREVIEW_LIMIT: usize = 4;
const ARTWORK_PREVIEW_EXPIRED: &str = "This artwork preview expired. Preview the images again.";
const MODERATION_LOG_LIMIT: usize = 300;
const SAVED_RESPONSE_LIMIT: usize = 12;

#[derive(Clone)]
pub struct Community {
    pub slug: String,
    pub name: String,
    pub description: String,
    pub guidelines: String,
    pub color: String,
    pub icon: String,
    pub icon_image: Option<ProfileAvatar>,
    pub banner_image: Option<ProfileAvatar>,
    pub members: u32,
    pub owner: Option<String>,
    highlighted_posts: Vec<u64>,
    rules: Vec<CommunityRule>,
    pub topic: String,
    pub visibility: String,
    pub mature: bool,
}

#[derive(Clone)]
pub struct Post {
    pub id: u64,
    pub community: String,
    pub author: String,
    pub title: String,
    pub body: String,
    pub flair: String,
    pub age: String,
    pub created_at: Option<u64>,
    pub url: String,
    pub score: i32,
    pub reply_count: u32,
    pub sensitive: bool,
    pub spoiler: bool,
    pub brand_affiliate: bool,
    pub image: bool,
    pub poll: Option<Poll>,
    pub repost_of: Option<u64>,
    pub repost_title_from_source: bool,
    pub media: Vec<PostMedia>,
}

impl Post {
    pub fn age_seconds(&self, now: u64) -> u64 {
        self.created_at.map_or_else(
            || legacy_age_seconds(&self.age),
            |created| now.saturating_sub(created),
        )
    }

    pub fn age_label(&self, now: u64) -> String {
        let seconds = self.age_seconds(now);
        match seconds {
            0..=59 => "just now".into(),
            60..=3_599 => format!("{}m", seconds / 60),
            3_600..=86_399 => format!("{}h", seconds / 3_600),
            _ => format!("{}d", seconds / 86_400),
        }
    }
}

#[derive(Clone)]
pub struct Poll {
    pub options: Vec<String>,
    pub votes: BTreeMap<String, usize>,
    pub closes_at: u64,
}

#[derive(Clone)]
pub struct PostMedia {
    pub key: String,
    pub content_type: String,
}

#[derive(Clone)]
pub struct Comment {
    pub id: u64,
    pub post: u64,
    pub parent: Option<u64>,
    pub author: String,
    pub body: String,
    pub age: String,
    pub score: i32,
    pub edited_at: Option<u64>,
    pub deleted: bool,
}

#[derive(Clone)]
pub struct Report {
    pub id: u64,
    pub post: u64,
    pub comment: Option<u64>,
    pub reason: Option<ReportReason>,
    pub rule: Option<ReportedRule>,
    pub community: Option<String>,
    pub region: String,
    pub details: String,
    pub reviewed: bool,
    pub reporter: String,
}

#[derive(Clone)]
pub struct DirectMessage {
    pub id: u64,
    pub from: String,
    pub to: String,
    pub body: String,
    pub at: u64,
    pub read: bool,
}

#[derive(Clone)]
pub struct Notification {
    pub id: u64,
    pub recipient: String,
    pub body: String,
    pub at: u64,
    pub read: bool,
    pub post: Option<u64>,
    pub appeal: bool,
    pub moderation: bool,
    pub silent: bool,
}

#[derive(Clone)]
pub struct ModMail {
    pub id: u64,
    pub community: String,
    pub sender: String,
    pub subject: String,
    pub body: String,
    pub reply: String,
}

#[derive(Clone)]
pub struct ModerationEvent {
    pub id: u64,
    pub community: String,
    pub action: String,
    pub at: u64,
}

#[derive(Clone)]
pub struct SavedResponse {
    pub id: u64,
    pub community: String,
    pub title: String,
    pub body: String,
}

#[derive(Clone)]
pub struct CustomFeed {
    pub id: u64,
    pub name: String,
    pub communities: BTreeSet<String>,
    pub description: String,
    pub private: bool,
    pub show_on_profile: bool,
}

#[derive(Clone)]
pub struct ProfileAvatar {
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

#[derive(Clone)]
pub struct SetupArtwork {
    pub id: u64,
    pub icon: Option<ProfileAvatar>,
    pub banner: Option<ProfileAvatar>,
    community: Option<String>,
    creator: String,
}

impl SetupArtwork {
    fn update(
        &mut self,
        icon: Option<(&str, Vec<u8>)>,
        banner: Option<(&str, Vec<u8>)>,
        remove_icon: bool,
        remove_banner: bool,
    ) -> Result<(), String> {
        if remove_icon {
            self.icon = None;
        }
        if remove_banner {
            self.banner = None;
        }
        if let Some((content_type, bytes)) = icon {
            self.icon = Some(checked_image(content_type, bytes)?);
        }
        if let Some((content_type, bytes)) = banner {
            self.banner = Some(checked_image(content_type, bytes)?);
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct SocialLink {
    pub id: u64,
    pub label: String,
    pub url: String,
}

#[derive(Clone)]
pub struct PostDraft {
    pub community: String,
    pub title: String,
    pub body: String,
    pub url: String,
    pub flair: String,
    pub sensitive: bool,
    pub spoiler: bool,
    pub brand_affiliate: bool,
}

#[derive(Clone)]
pub struct ScheduledItem {
    pub id: u64,
    pub community: String,
    pub author: Option<String>,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub url: String,
    pub flair: String,
    pub sensitive: bool,
    pub spoiler: bool,
    pub brand_affiliate: bool,
    pub publish_at: u64,
    pub zone: String,
    pub event_start: Option<u64>,
    pub event_end: Option<u64>,
    pub status: String,
    pub post_id: Option<u64>,
}

#[derive(Clone)]
pub struct CommunityRestriction {
    pub id: u64,
    pub community: String,
    pub username: String,
    pub kind: String,
    pub reason: String,
    pub created_at: u64,
    pub expires_at: Option<u64>,
}

#[derive(Clone)]
pub struct State {
    pub communities: Vec<Community>,
    pub posts: Vec<Post>,
    locked_discussions: BTreeSet<u64>,
    locked_comments: BTreeSet<u64>,
    pub comments: Vec<Comment>,
    pub votes: BTreeMap<u64, i8>,
    pub comment_votes: BTreeMap<u64, i8>,
    pub saved: BTreeSet<u64>,
    pub hidden: BTreeSet<u64>,
    pub joined: BTreeSet<String>,
    pub muted: BTreeSet<String>,
    pub blocked: BTreeSet<String>,
    pub keywords: Vec<String>,
    pub sensitive: String,
    pub reports: Vec<Report>,
    pub messages: Vec<DirectMessage>,
    chat_request_permission: ChatRequestPermission,
    chat_requests: Vec<ChatRequest>,
    declined_chat_requests: BTreeSet<(String, String)>,
    pub notifications: Vec<Notification>,
    notification_preferences: NotificationPreferences,
    pub mod_mail: Vec<ModMail>,
    pub mod_log: Vec<ModerationEvent>,
    pub saved_responses: Vec<SavedResponse>,
    pub moderator_roles: Vec<ModeratorAssignment>,
    pub approved_members: Vec<ApprovedMember>,
    pub content_removals: Vec<ContentRemoval>,
    pub automod_rules: Vec<AutoModRule>,
    pub automod_flags: Vec<AutoModFlag>,
    pub scheduled: Vec<ScheduledItem>,
    pub restrictions: Vec<CommunityRestriction>,
    pub appeals: Vec<Appeal>,
    pub custom_feeds: Vec<CustomFeed>,
    pub recent: Vec<String>,
    pub history: Vec<u64>,
    pub username: String,
    pub display_name: String,
    pub bio: String,
    pub avatar: Option<ProfileAvatar>,
    pub cover: Option<ProfileAvatar>,
    pub public_posts: bool,
    pub public_comments: bool,
    pub profile_nsfw: bool,
    pub profile_hidden_posts: BTreeSet<u64>,
    pub profile_hidden_comments: BTreeSet<u64>,
    pub profile_flair: String,
    pub social_links: Vec<SocialLink>,
    pub profile_created: u64,
    pub draft: Option<PostDraft>,
    account_profiles: BTreeMap<String, ProfileData>,
    managed_profiles: bool,
    legacy_profile: Option<String>,
    next_post: u64,
    next_comment: u64,
    next_report: u64,
    next_message: u64,
    next_chat_request: u64,
    next_notification: u64,
    next_mod_mail: u64,
    next_mod_event: u64,
    next_community_rule: u64,
    next_automod_rule: u64,
    next_automod_flag: u64,
    next_saved_response: u64,
    next_scheduled: u64,
    next_restriction: u64,
    next_appeal: u64,
    next_feed: u64,
    next_social_link: u64,
    pending_artwork: VecDeque<SetupArtwork>,
    next_artwork_preview: u64,
}

impl State {
    fn seeded() -> Self {
        let communities = [
            (
                "open-debate",
                "Open Debate",
                "Make the strongest case you can, then see what others think.",
                "lilac",
                "◌",
                24_800,
            ),
            (
                "public-square",
                "Public Square",
                "Talk about the streets, parks, and public spaces we share.",
                "grass",
                "▦",
                18_400,
            ),
            (
                "technology",
                "Technology",
                "How technology works and how it changes life around us.",
                "sky",
                "⌘",
                31_200,
            ),
            (
                "media-lab",
                "Media Lab",
                "Follow how stories are reported, framed, and shared.",
                "apricot",
                "▷",
                9_700,
            ),
            (
                "culture",
                "Culture",
                "Books, art, language, and the things we make together.",
                "rose",
                "✦",
                13_600,
            ),
            (
                "after-hours",
                "After Hours",
                "Personal conversations and stories from everyday life.",
                "lilac",
                "☾",
                6_200,
            ),
            (
                "commons-lab",
                "Commons Lab",
                "Help shape the site, from feed controls to the public archive.",
                "sand",
                "◇",
                4_800,
            ),
        ]
        .into_iter()
        .map(
            |(slug, name, description, color, icon, members)| Community {
                slug: slug.into(),
                name: name.into(),
                description: description.into(),
                guidelines: String::new(),
                color: color.into(),
                icon: icon.into(),
                icon_image: None,
                banner_image: None,
                members,
                owner: None,
                highlighted_posts: Vec::new(),
                rules: Vec::new(),
                topic: String::new(),
                visibility: "public".into(),
                mature: false,
            },
        )
        .collect();

        let seeded_at = unix_seconds();
        let mut posts: Vec<_> = [
            (1, "after-hours", "saturnreturns", "What is something you were wrong about for years?", "I used to think changing my mind meant I had lost an argument. What did it take for you to change yours?", "Discussion", "1d", 92, 17, true, false),
            (2, "culture", "irisarchive", "What should a public archive keep?", "A community radio station near me is digitizing its call-in tapes. Should it keep every call, or only the ones people still remember?", "Question", "12h", 174, 31, false, false),
            (3, "commons-lab", "jules", "What should a good report status tell you?", "I would like to see when a report was received, which region it applies to, and how to request another review. What would you add?", "Feedback", "8h", 64, 12, false, false),
            (4, "media-lab", "papertrail", "How do you verify a video when everyone shares the same clip?", "The first link I could find was a repost without a date or location. I’m collecting ways people trace clips back to their source.", "Question", "6h", 118, 28, false, false),
            (5, "technology", "signalpath", "What would you put in a genuinely useful feed filter?", "Keyword mutes seem easy until a word has more than one meaning. How do you handle filters you want to apply most of the time?", "Discussion", "4h", 208, 46, false, false),
            (6, "public-square", "daylightmaps", "What makes a public square worth spending time in?", "I took this on a quiet afternoon. A place to sit, a little shade, and people coming and going made me stay longer than I planned. What gets a square right where you live?", "Photo", "3h", 318, 63, false, true),
            (7, "open-debate", "marginnotes", "Should online communities be able to ban political topics?", "Some people want a place to talk about a hobby in peace. Others think broad topic bans can erase conversations that matter to that hobby. Where would you draw the line?", "Discussion", "2h", 486, 124, false, false),
        ].into_iter().map(|(id, community, author, title, body, flair, age, score, reply_count, sensitive, image)| Post {
            id, community: community.into(), author: author.into(), title: title.into(),
            body: body.into(), flair: flair.into(), age: age.into(),
            created_at: Some(seeded_at.saturating_sub(legacy_age_seconds(age))), url: String::new(),
            score, reply_count, sensitive, spoiler: false, brand_affiliate: false, image,
            poll: None, repost_of: None, repost_title_from_source: false, media: Vec::new(),
        }).collect();
        posts.extend(seed_news(8, seeded_at));

        let comments = [
            (1, 7, None, "westbound", "I want hobby spaces to be easy to browse. A topic filter I can turn off would get me most of the way there.", "1h", 43),
            (2, 7, Some(1), "marginnotes", "That makes sense to me. The same thread could stay visible to people who want the broader discussion.", "54m", 19),
            (3, 7, None, "fieldwork", "The harder part is choosing a label. My transit club talks about budgets because they affect how often the trains run.", "46m", 28),
            (4, 6, None, "streetlevel", "Seats you can move make a bigger difference than I expected. People can sit together or find their own patch of shade.", "2h", 32),
            (5, 5, None, "clearcache", "I want an on/off state for each term and a preview before applying a new filter.", "2h", 17),
            (6, 3, None, "openledger", "A public summary with no personal information would help me understand how often the process is used.", "5h", 11),
        ].into_iter().map(|(id, post, parent, author, body, age, score)| Comment {
            id, post, parent, author: author.into(), body: body.into(), age: age.into(), score,
            edited_at: None, deleted: false,
        }).collect();

        Self {
            communities,
            posts,
            locked_discussions: BTreeSet::new(),
            locked_comments: BTreeSet::new(),
            comments,
            votes: BTreeMap::new(),
            comment_votes: BTreeMap::new(),
            saved: BTreeSet::new(),
            hidden: BTreeSet::new(),
            joined: ["open-debate", "public-square", "technology"]
                .map(String::from)
                .into(),
            muted: BTreeSet::new(),
            blocked: BTreeSet::new(),
            keywords: Vec::new(),
            sensitive: "blur".into(),
            reports: Vec::new(),
            messages: Vec::new(),
            chat_request_permission: ChatRequestPermission::default(),
            chat_requests: Vec::new(),
            declined_chat_requests: BTreeSet::new(),
            notifications: Vec::new(),
            notification_preferences: NotificationPreferences::default(),
            mod_mail: Vec::new(),
            mod_log: Vec::new(),
            saved_responses: Vec::new(),
            moderator_roles: Vec::new(),
            approved_members: Vec::new(),
            content_removals: Vec::new(),
            automod_rules: Vec::new(),
            automod_flags: Vec::new(),
            scheduled: Vec::new(),
            restrictions: Vec::new(),
            appeals: Vec::new(),
            custom_feeds: Vec::new(),
            recent: Vec::new(),
            history: Vec::new(),
            username: "jules".into(),
            display_name: String::new(),
            bio: String::new(),
            avatar: None,
            cover: None,
            public_posts: true,
            public_comments: true,
            profile_nsfw: false,
            profile_hidden_posts: BTreeSet::new(),
            profile_hidden_comments: BTreeSet::new(),
            profile_flair: String::new(),
            social_links: Vec::new(),
            profile_created: unix_seconds(),
            draft: None,
            account_profiles: BTreeMap::new(),
            managed_profiles: false,
            legacy_profile: None,
            next_post: 10,
            next_comment: 7,
            next_report: 1,
            next_message: 1,
            next_chat_request: 1,
            next_notification: 1,
            next_mod_mail: 1,
            next_mod_event: 1,
            next_community_rule: 1,
            next_automod_rule: 1,
            next_automod_flag: 1,
            next_saved_response: 1,
            next_scheduled: 1,
            next_restriction: 1,
            next_appeal: 1,
            next_feed: 1,
            next_social_link: 1,
            pending_artwork: VecDeque::new(),
            next_artwork_preview: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(1, |time| time.as_nanos() as u64),
        }
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let mut state = Self::seeded();
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(state),
            Err(error) => return Err(error),
        };

        let mut lines = contents.lines();
        let legacy = match lines.next() {
            Some(FORMAT) => false,
            Some(LEGACY_FORMAT) => true,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Unsupported Commonline state format",
                ))
            }
        };
        state.posts.clear();
        state.comments.clear();
        state.joined.clear();
        for (index, line) in lines.enumerate() {
            state.read_record(line).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("Invalid Commonline state record at line {}", index + 2),
                )
            })?;
        }
        if state.posts.iter().any(|post| post.created_at.is_none()) {
            let now = unix_seconds();
            let snapshot_at = fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
                .map_or(now, |duration| duration.as_secs().min(now));
            for post in &mut state.posts {
                if post.created_at.is_none() {
                    post.created_at =
                        Some(snapshot_at.saturating_sub(legacy_age_seconds(&post.age)));
                }
            }
        }
        if state.guest() {
            state.activate_profile(None);
        }
        if legacy {
            let first_id = state.next_post;
            state.next_post = first_id.checked_add(2).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Too many posts to add the demo news samples",
                )
            })?;
            state.posts.extend(seed_news(first_id, unix_seconds()));
        }
        Ok(state)
    }

    pub fn record_visit(&mut self, slug: &str, path: &Path) {
        if self.guest()
            || self.recent.first().is_some_and(|recent| recent == slug)
            || !self
                .communities
                .iter()
                .any(|community| community.slug == slug && self.can_view_community(community))
        {
            return;
        }
        let previous = self.recent.clone();
        self.recent.retain(|recent| recent != slug);
        self.recent.insert(0, slug.to_owned());
        self.recent.truncate(RECENT_LIMIT);
        if self.save(path).is_err() {
            self.recent = previous;
        }
    }

    pub fn apply(
        &mut self,
        action: &str,
        form: &BTreeMap<String, String>,
        path: &Path,
    ) -> Result<&'static str, String> {
        if self.guest() {
            return Err("Sign in to continue.".into());
        }
        let mut updated = self.clone();
        let message = updated.change(action, form)?;
        self.commit(updated, path)?;
        Ok(message)
    }

    pub fn publish_due(&mut self, path: &Path, now: u64) -> io::Result<usize> {
        let mut due: Vec<_> = self
            .scheduled
            .iter()
            .enumerate()
            .filter(|(_, item)| item.status == "pending" && item.publish_at <= now)
            .map(|(index, item)| (item.publish_at, item.id, index))
            .collect();
        if due.is_empty() {
            return Ok(0);
        }
        due.sort_unstable();
        let mut updated = self.clone();
        for &(_, _, index) in &due {
            updated.publish_scheduled_item(index, now)?;
        }
        updated.save(path)?;
        *self = updated;
        Ok(due.len())
    }

    fn publish_scheduled_item(&mut self, index: usize, now: u64) -> io::Result<()> {
        let item = &mut self.scheduled[index];
        let owner = self
            .communities
            .iter()
            .find(|community| community.slug == item.community)
            .and_then(|community| community.owner.as_ref())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Scheduled item has no community owner",
                )
            })?;
        let id = next_id(&mut self.next_post).map_err(io::Error::other)?;
        let flair = match item.flair.as_str() {
            "" if item.kind == "event" => "Event",
            "" => "Discussion",
            flair => flair,
        };
        self.posts.push(Post {
            id,
            community: item.community.clone(),
            author: item.author.as_ref().unwrap_or(owner).clone(),
            title: item.title.clone(),
            body: item.body.clone(),
            url: item.url.clone(),
            flair: flair.into(),
            age: "just now".into(),
            created_at: Some(now),
            score: 1,
            reply_count: 0,
            sensitive: item.sensitive,
            spoiler: item.spoiler,
            brand_affiliate: item.brand_affiliate,
            image: false,
            poll: None,
            repost_of: None,
            repost_title_from_source: false,
            media: Vec::new(),
        });
        item.status = "published".into();
        item.post_id = Some(id);
        let message = if item.kind == "event" {
            "Scheduled event published."
        } else {
            "Scheduled post published."
        };
        let community = item.community.clone();
        self.record_moderation(&community, message)
            .map_err(io::Error::other)?;
        self.screen_post(id).map_err(io::Error::other)?;
        Ok(())
    }

    pub fn record_post_visit(&mut self, id: u64, path: &Path) {
        if self.guest()
            || self.history.first() == Some(&id)
            || self.viewable_post_community(id).is_none()
        {
            return;
        }
        let previous = self.history.clone();
        self.history.retain(|visited| *visited != id);
        self.history.insert(0, id);
        self.history.truncate(HISTORY_LIMIT);
        if self.save(path).is_err() {
            self.history = previous;
        }
    }

    pub fn set_profile_image(
        &mut self,
        kind: &str,
        content_type: &str,
        bytes: Vec<u8>,
        path: &Path,
    ) -> Result<&'static str, String> {
        if self.guest() {
            return Err("Sign in to continue.".into());
        }
        let avatar = checked_image(content_type, bytes)?;
        let mut updated = self.clone();
        let message = match kind {
            "avatar" => {
                updated.avatar = Some(avatar);
                "Profile picture updated."
            }
            "cover" => {
                updated.cover = Some(avatar);
                "Cover photo updated."
            }
            _ => return Err("Choose a profile picture or cover photo.".into()),
        };
        self.commit(updated, path)?;
        Ok(message)
    }

    pub fn setup_artwork(&self, id: u64) -> Option<&SetupArtwork> {
        self.pending_artwork.iter().find(|artwork| {
            artwork.id == id
                && artwork.creator == self.username
                && artwork
                    .community
                    .as_deref()
                    .is_none_or(|slug| self.owns_community(slug))
        })
    }

    pub fn community_artwork(&self, slug: &str, id: u64) -> Option<&SetupArtwork> {
        self.setup_artwork(id)
            .filter(|artwork| artwork.community.as_deref() == Some(slug))
    }

    pub fn preview_artwork(
        &mut self,
        original: Option<u64>,
        icon: Option<(&str, Vec<u8>)>,
        banner: Option<(&str, Vec<u8>)>,
        remove_icon: bool,
        remove_banner: bool,
    ) -> Result<Option<u64>, String> {
        if self.guest() {
            return Err("Sign in to continue.".into());
        }
        let previous = original
            .map(|id| {
                self.setup_artwork(id)
                    .filter(|artwork| artwork.community.is_none())
                    .ok_or(ARTWORK_PREVIEW_EXPIRED)
            })
            .transpose()?;
        if icon.is_none() && banner.is_none() && !remove_icon && !remove_banner {
            return Ok(original);
        }
        let mut artwork = previous.cloned().unwrap_or(SetupArtwork {
            id: 0,
            icon: None,
            banner: None,
            community: None,
            creator: self.username.clone(),
        });
        artwork.update(icon, banner, remove_icon, remove_banner)?;
        if artwork.icon.is_none() && artwork.banner.is_none() {
            if let Some(id) = original {
                self.pending_artwork.retain(|previous| previous.id != id);
            }
            return Ok(None);
        }
        self.store_artwork(artwork, original).map(Some)
    }

    pub fn preview_community_artwork(
        &mut self,
        slug: &str,
        original: Option<u64>,
        icon: Option<(&str, Vec<u8>)>,
        banner: Option<(&str, Vec<u8>)>,
        remove_icon: bool,
        remove_banner: bool,
    ) -> Result<u64, String> {
        let community = self
            .communities
            .iter()
            .find(|community| {
                community.slug == slug && community.owner.as_deref() == Some(self.username.as_str())
            })
            .ok_or("Only this community’s owner can edit its artwork.")?;
        let mut artwork = match original {
            Some(id) => {
                let previous = self
                    .community_artwork(slug, id)
                    .ok_or(ARTWORK_PREVIEW_EXPIRED)?;
                if icon.is_none() && banner.is_none() && !remove_icon && !remove_banner {
                    return Ok(id);
                }
                previous.clone()
            }
            None => SetupArtwork {
                id: 0,
                icon: community.icon_image.clone(),
                banner: community.banner_image.clone(),
                community: Some(slug.to_owned()),
                creator: self.username.clone(),
            },
        };
        artwork.update(icon, banner, remove_icon, remove_banner)?;
        // Keep other open tabs tied to the images they previewed.
        self.store_artwork(artwork, None)
    }

    fn store_artwork(
        &mut self,
        mut artwork: SetupArtwork,
        original: Option<u64>,
    ) -> Result<u64, String> {
        let id = match original {
            Some(id) => {
                self.pending_artwork.retain(|previous| previous.id != id);
                id
            }
            None => next_id(&mut self.next_artwork_preview)?,
        };
        artwork.id = id;
        if self.pending_artwork.len() == ARTWORK_PREVIEW_LIMIT {
            self.pending_artwork.pop_front();
        }
        self.pending_artwork.push_back(artwork);
        Ok(id)
    }

    fn take_artwork(&mut self, id: u64, slug: Option<&str>) -> Result<SetupArtwork, String> {
        self.pending_artwork
            .iter()
            .position(|artwork| {
                artwork.id == id
                    && artwork.creator == self.username
                    && artwork.community.as_deref() == slug
            })
            .and_then(|index| self.pending_artwork.remove(index))
            .ok_or_else(|| ARTWORK_PREVIEW_EXPIRED.into())
    }

    fn commit(&mut self, updated: Self, path: &Path) -> Result<(), String> {
        updated
            .save(path)
            .map_err(|error| format!("Could not save changes: {error}"))?;
        *self = updated;
        Ok(())
    }

    fn change(
        &mut self,
        action: &str,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        match action {
            "vote" => {
                let id = self.post_id(form, "id")?;
                vote(&mut self.votes, id, direction(form)?);
                Ok("Vote updated.")
            }
            "comment-vote" => {
                let id = id_field(form, "id")?;
                if self.is_comment_removed(id)
                    || !self.comments.iter().any(|comment| {
                        comment.id == id
                            && !comment.deleted
                            && self.viewable_post_community(comment.post).is_some()
                    })
                {
                    return Err("This comment could not be found.".into());
                }
                vote(&mut self.comment_votes, id, direction(form)?);
                Ok("Vote updated.")
            }
            "save" => {
                let id = self.post_id(form, "id")?;
                if toggle(&mut self.saved, id) {
                    Ok("Saved to your list.")
                } else {
                    Ok("Removed from your saved list.")
                }
            }
            "hide" => {
                let id = self.post_id(form, "id")?;
                if toggle(&mut self.hidden, id) {
                    Ok("Hidden from your feed.")
                } else {
                    Ok("Visible in your feed.")
                }
            }
            "join" => self.join_community(form),
            "mute" => {
                let community = self.community_slug(form)?;
                if toggle(&mut self.muted, community) {
                    Ok("Community muted in your feed.")
                } else {
                    Ok("Community unmuted.")
                }
            }
            "block" => {
                let author = checked_handle(value(form, "author"))?;
                if author == self.username {
                    return Err("Choose another account to block.".into());
                }
                if self.blocked.remove(&author) {
                    Ok("Account unblocked.")
                } else {
                    self.cancel_chat_requests_with(&author);
                    self.blocked.insert(author);
                    Ok("Account blocked in your feed.")
                }
            }
            "preferences" => self.update_preferences(form),
            "create" => self.create_post(form),
            "create-poll" => self.create_poll(form),
            "create-repost" => self.create_repost(form),
            "vote-poll" => self.vote_poll(form),
            "save-draft" => self.save_draft(form),
            "delete-draft" => {
                self.draft = None;
                Ok("Draft discarded.")
            }
            "comment" => self.create_comment(form),
            "edit-comment" => self.edit_comment(form),
            "delete-comment" => self.delete_comment(form),
            "report" => self.create_report(form),
            "report-content" => self.report_content(form),
            "send-direct-message" => self.send_direct_message(form),
            "accept-chat-request" => self.resolve_chat_request(form, true),
            "decline-chat-request" => self.resolve_chat_request(form, false),
            "read-conversation" => self.read_conversation(form),
            "read-notifications" => self.read_notifications(),
            "submit-appeal" => self.submit_appeal(form),
            "decide-appeal" => self.decide_appeal(form),
            "review-report" => self.review_report(form),
            "send-mod-mail" => self.send_mod_mail(form),
            "reply-mod-mail" => self.reply_mod_mail(form),
            "add-saved-response" => self.add_saved_response(form),
            "delete-saved-response" => self.delete_saved_response(form),
            "set-moderator-role" => self.set_moderator_role(form),
            "remove-moderator-role" => self.remove_moderator_role(form),
            "approve-member" => self.approve_member(form),
            "revoke-approved-member" => self.revoke_approved_member(form),
            "highlight-post" => self.set_highlight(form, true),
            "unhighlight-post" => self.set_highlight(form, false),
            "move-highlight" => self.move_highlight(form),
            "save-community-rule" => self.save_community_rule(form),
            "move-community-rule" => self.move_community_rule(form),
            "delete-community-rule" => self.delete_community_rule(form),
            "remove-content" => self.moderate_content(form, false),
            "restore-content" => self.moderate_content(form, true),
            "lock-discussion" => self.set_reply_lock(form, ContentKind::Post, true),
            "unlock-discussion" => self.set_reply_lock(form, ContentKind::Post, false),
            "lock-comment" => self.set_reply_lock(form, ContentKind::Comment, true),
            "unlock-comment" => self.set_reply_lock(form, ContentKind::Comment, false),
            "save-automod-rule" => self.save_automod_rule(form),
            "delete-automod-rule" => self.delete_automod_rule(form),
            "dismiss-automod-flag" => self.dismiss_automod_flag(form),
            "schedule-item" => self.schedule_item(form),
            "edit-scheduled-item" => self.edit_scheduled_item(form),
            "cancel-scheduled-item" => self.cancel_scheduled_item(form),
            "restrict-user" => self.restrict_user(form),
            "lift-restriction" => self.lift_restriction(form),
            "create-feed" => self.create_feed(form),
            "edit-feed" => self.edit_feed(form),
            "delete-feed" => self.delete_feed(form),
            "profile" => self.update_profile(form),
            "clear-avatar" => {
                self.avatar = None;
                Ok("Profile picture removed.")
            }
            "clear-cover" => {
                self.cover = None;
                Ok("Cover photo removed.")
            }
            "profile-visibility" => {
                self.public_posts = value(form, "posts") == "on";
                self.public_comments = value(form, "comments") == "on";
                self.profile_nsfw = value(form, "nsfw") == "on";
                Ok("Profile visibility updated.")
            }
            "profile-flair" => {
                self.profile_flair = bounded(form, "flair", "Profile flair", 32)?.into();
                Ok("Profile flair updated.")
            }
            "profile-hide-post" => self.hide_profile_post(form),
            "profile-hide-comment" => self.hide_profile_comment(form),
            "add-social-link" => self.add_social_link(form),
            "delete-social-link" => self.delete_social_link(form),
            "edit" => self.edit_post(form),
            "delete" => self.delete_post(form),
            "create-community" => self.create_community(form),
            "edit-community" => self.edit_community(form),
            "save-community-artwork" => self.save_community_artwork(form),
            "pin-community" => self.pin_community(form),
            _ => Err("This action is unavailable.".into()),
        }
    }

    fn post_id(&self, form: &BTreeMap<String, String>, key: &str) -> Result<u64, String> {
        let id = id_field(form, key)?;
        self.viewable_post_community(id)
            .map(|_| id)
            .ok_or_else(|| "This post could not be found.".into())
    }

    pub fn can_view_community(&self, item: &Community) -> bool {
        item.visibility != "private"
            || self.can_moderate(item, ModPermission::Members)
            || self.is_approved_member(&item.slug, &self.username)
    }

    pub fn can_contribute(&self, item: &Community) -> bool {
        !self.username.is_empty()
            && !self.is_banned(&item.slug, &self.username)
            && (item.visibility == "public"
                || self.can_moderate(item, ModPermission::Members)
                || self.is_approved_member(&item.slug, &self.username))
    }

    pub fn can_edit_post(&self, post: &Post) -> bool {
        post.author == self.username
            && !self.is_post_removed(post.id)
            && self
                .communities
                .iter()
                .find(|community| community.slug == post.community)
                .is_some_and(|community| self.can_contribute(community))
    }

    pub fn is_banned(&self, slug: &str, handle: &str) -> bool {
        self.has_restriction(slug, handle, "ban")
    }

    pub fn is_mod_mail_muted(&self, slug: &str, handle: &str) -> bool {
        self.has_restriction(slug, handle, "mute")
    }

    fn has_restriction(&self, slug: &str, handle: &str, kind: &str) -> bool {
        let now = unix_seconds();
        restriction_handle(handle).is_ok_and(|handle| {
            self.restrictions.iter().any(|item| {
                item.community == slug
                    && item.username == handle
                    && item.kind == kind
                    && item.is_active_at(now)
            })
        })
    }

    fn community_for_post(&self, id: u64) -> Option<&Community> {
        let post = self.posts.iter().find(|post| post.id == id)?;
        self.communities
            .iter()
            .find(|community| community.slug == post.community)
    }

    fn viewable_post_community(&self, id: u64) -> Option<&Community> {
        self.community_for_post(id)
            .filter(|community| self.can_view_community(community) && !self.is_post_removed(id))
    }

    fn community_slug(&self, form: &BTreeMap<String, String>) -> Result<String, String> {
        let index = self.community_index(form)?;
        let community = &self.communities[index];
        if !self.can_view_community(community) {
            return Err("Choose a community.".into());
        }
        Ok(community.slug.clone())
    }

    fn community_index(&self, form: &BTreeMap<String, String>) -> Result<usize, String> {
        let slug = value(form, "community");
        self.communities
            .iter()
            .position(|community| community.slug == slug)
            .ok_or_else(|| "Choose a community.".into())
    }

    fn owned_community_index(&self, form: &BTreeMap<String, String>) -> Result<usize, String> {
        let index = self.community_index(form)?;
        if self.communities[index].owner.as_deref() == Some(self.username.as_str()) {
            return Ok(index);
        }
        Err("Only this community’s owner can change its settings.".into())
    }

    fn join_community(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let index = self.community_index(form)?;
        if !self.can_view_community(&self.communities[index]) {
            return Err("This community isn’t available.".into());
        }
        let slug = self.communities[index].slug.clone();
        let joined = toggle(&mut self.joined, slug.clone());
        if self.communities[index].owner.is_some() {
            self.communities[index].members = self.account_join_count(&slug);
        }
        if joined {
            Ok("Community joined.")
        } else {
            Ok("Community left.")
        }
    }

    fn create_community(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let slug = checked_slug(value(form, "slug"))?;
        if self
            .communities
            .iter()
            .any(|community| community.slug == slug)
        {
            return Err("That community address is already in use.".into());
        }
        let (name, description, guidelines) = community_details(form)?;
        let topic = checked_topic(value(form, "topic"))?;
        let (visibility, mature) =
            community_visibility(form)?.unwrap_or_else(|| ("public".into(), false));
        let (icon_image, banner_image) = if value(form, "artwork_preview").is_empty() {
            (None, None)
        } else {
            let id = artwork_preview_id(form)?;
            let artwork = self.take_artwork(id, None)?;
            (artwork.icon, artwork.banner)
        };
        let colors = ["lilac", "grass", "sky", "apricot", "rose", "sand"];
        let color = colors[self.communities.len() % colors.len()];
        self.joined.insert(slug.clone());
        self.communities.push(Community {
            slug: slug.clone(),
            name,
            description,
            guidelines,
            color: color.into(),
            icon: "◇".into(),
            icon_image,
            banner_image,
            members: 1,
            owner: Some(self.username.clone()),
            highlighted_posts: Vec::new(),
            rules: Vec::new(),
            topic,
            visibility,
            mature,
        });
        self.record_moderation(&slug, "Community created.")
    }

    fn edit_community(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let index = self.owned_community_index(form)?;
        let (name, description, guidelines) = community_details(form)?;
        let community = &mut self.communities[index];
        community.name = name;
        community.description = description;
        community.guidelines = guidelines;
        if form.contains_key("topic") {
            community.topic = checked_topic(value(form, "topic"))?;
        }
        if let Some((visibility, mature)) = community_visibility(form)? {
            community.visibility = visibility;
            community.mature = mature;
        }
        let slug = community.slug.clone();
        self.record_moderation(&slug, "Community settings updated.")
    }

    fn save_community_artwork(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.owned_community_index(form)?;
        let slug = self.communities[index].slug.clone();
        let id = artwork_preview_id(form)?;
        let artwork = self.take_artwork(id, Some(&slug))?;
        let community = &mut self.communities[index];
        community.icon_image = artwork.icon;
        community.banner_image = artwork.banner;
        self.pending_artwork
            .retain(|preview| preview.community.as_deref() != Some(slug.as_str()));
        self.record_moderation(&slug, "Community artwork updated.")
    }

    fn update_preferences(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let sensitive = value(form, "sensitive");
        if !matches!(sensitive, "show" | "blur" | "hide") {
            return Err("Choose how to display sensitive posts.".into());
        }
        let mut seen = BTreeSet::new();
        let keywords = bounded(form, "keywords", "Muted keywords", 2_000)?;
        let keywords: Vec<String> = keywords
            .lines()
            .map(str::trim)
            .filter(|word| !word.is_empty())
            .filter(|word| seen.insert(word.to_lowercase()))
            .map(str::to_owned)
            .collect();
        if keywords.len() > 40 || keywords.iter().any(|word| word.chars().count() > 80) {
            return Err("Use up to 40 muted keywords or phrases with 80 characters each.".into());
        }
        self.sensitive = sensitive.into();
        self.keywords = keywords;
        Ok("Feed controls updated.")
    }

    fn schedule_item(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Schedule)?;
        let community = self.communities[index].slug.clone();
        let id = next_id(&mut self.next_scheduled)?;
        let mut item = scheduled_item(form, id, &community, "America/Chicago", unix_seconds())?;
        item.author = Some(self.username.clone());
        let message = if item.kind == "event" {
            "Event scheduled."
        } else {
            "Post scheduled."
        };
        self.scheduled.push(item);
        self.record_moderation(&community, message)
    }

    fn edit_scheduled_item(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let now = unix_seconds();
        let index = self.pending_scheduled_index(form, now)?;
        let previous = &self.scheduled[index];
        let community = previous.community.clone();
        let mut updated = scheduled_item(form, previous.id, &community, &previous.zone, now)?;
        updated.author = Some(self.username.clone());
        self.scheduled[index] = updated;
        self.record_moderation(&community, "Schedule updated.")
    }

    fn cancel_scheduled_item(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.pending_scheduled_index(form, unix_seconds())?;
        let item = &mut self.scheduled[index];
        item.status = "canceled".into();
        let community = item.community.clone();
        self.record_moderation(&community, "Scheduled item canceled.")
    }

    fn pending_scheduled_index(
        &self,
        form: &BTreeMap<String, String>,
        now: u64,
    ) -> Result<usize, String> {
        let community_index = self.moderated_community_index(form, ModPermission::Schedule)?;
        let community = &self.communities[community_index].slug;
        let id = canonical_u64(value(form, "id")).ok_or("Choose a valid scheduled item.")?;
        let index = self
            .scheduled
            .iter()
            .position(|item| item.id == id && &item.community == community)
            .ok_or("This scheduled item is unavailable in this community.")?;
        let item = &self.scheduled[index];
        if item.status != "pending" {
            return Err("Only pending items can be changed.".into());
        }
        if item.publish_at <= now {
            return Err("This scheduled item is already due.".into());
        }
        Ok(index)
    }

    fn create_post(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let post = self.new_post(form, "Discussion")?;
        self.add_post(post, form, "Post published.")
    }

    pub fn create_media_post(
        &mut self,
        form: &BTreeMap<String, String>,
        media: Vec<PostMedia>,
        path: &Path,
    ) -> Result<&'static str, String> {
        if self.guest() {
            return Err("Sign in to continue.".into());
        }
        validate_post_media(&media)?;
        let mut updated = self.clone();
        let mut post = updated.new_post(form, "Discussion")?;
        post.media = media;
        let notice = updated.add_post(post, form, "Post published.")?;
        self.commit(updated, path)?;
        Ok(notice)
    }

    fn new_post(
        &mut self,
        form: &BTreeMap<String, String>,
        fallback_flair: &str,
    ) -> Result<Post, String> {
        let index = self.community_index(form)?;
        let item = &self.communities[index];
        if !self.can_contribute(item) {
            return Err("This community limits who can publish a post.".into());
        }
        let community = item.slug.clone();
        let (title, body, url) = post_content(form)?;
        let flair = bounded(form, "flair", "Flair", 32)?;
        let flair = match flair {
            "" if self.profile_flair.is_empty() => fallback_flair,
            "" => &self.profile_flair,
            _ => flair,
        };
        let id = next_id(&mut self.next_post)?;
        Ok(Post {
            id,
            community,
            author: self.username.clone(),
            title,
            body,
            flair: flair.into(),
            age: "just now".into(),
            created_at: Some(unix_seconds()),
            url,
            score: 1,
            reply_count: 0,
            sensitive: value(form, "sensitive") == "on",
            spoiler: value(form, "spoiler") == "on",
            brand_affiliate: value(form, "brand_affiliate") == "on",
            image: false,
            poll: None,
            repost_of: None,
            repost_title_from_source: false,
            media: Vec::new(),
        })
    }

    fn add_post(
        &mut self,
        post: Post,
        form: &BTreeMap<String, String>,
        published: &'static str,
    ) -> Result<&'static str, String> {
        let id = post.id;
        self.posts.push(post);
        if value(form, "from_draft") == "1" {
            self.draft = None;
        }
        let removed = self.screen_post(id)?;
        Ok(if removed {
            AUTOMOD_REMOVED_POST
        } else {
            published
        })
    }

    fn create_poll(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let mut options = Vec::new();
        let mut seen = BTreeSet::new();
        for index in 1..=6 {
            let option = bounded(form, &format!("option_{index}"), "Poll choice", 80)?;
            if option.is_empty() {
                continue;
            }
            if !seen.insert(option.to_lowercase()) {
                return Err("Choose distinct answers for the poll.".into());
            }
            options.push(option.into());
        }
        if options.len() < 2 {
            return Err("Add at least two choices to the poll.".into());
        }
        let duration = match value(form, "duration") {
            "1" => 1,
            "3" => 3,
            "7" => 7,
            _ => return Err("Choose a poll duration of 1, 3, or 7 days.".into()),
        };
        let mut post = self.new_post(form, "Poll")?;
        post.poll = Some(Poll {
            options,
            votes: BTreeMap::new(),
            closes_at: unix_seconds() + duration * 86_400,
        });
        self.add_post(post, form, "Poll published.")
    }

    fn vote_poll(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let id = self.post_id(form, "id")?;
        let option = canonical_u64(value(form, "option"))
            .and_then(|index| usize::try_from(index).ok())
            .ok_or("Choose a poll option.")?;
        let username = checked_handle(&self.username)?;
        let poll = self
            .posts
            .iter_mut()
            .find(|post| post.id == id)
            .and_then(|post| post.poll.as_mut())
            .ok_or("This poll is unavailable.")?;
        if unix_seconds() >= poll.closes_at {
            return Err("This poll has closed.".into());
        }
        if option >= poll.options.len() {
            return Err("Choose a poll option.".into());
        }
        if poll.votes.contains_key(&username) {
            return Err("You have already voted in this poll.".into());
        }
        poll.votes.insert(username, option);
        Ok("Poll vote recorded.")
    }

    pub fn repost_source(&self, id: u64) -> Option<&Post> {
        if self.is_post_removed(id) {
            return None;
        }
        let post = self.posts.iter().find(|post| post.id == id)?;
        let community = self
            .communities
            .iter()
            .find(|community| community.slug == post.community)?;
        if community.visibility == "private" || !self.can_view_community(community) {
            return None;
        }
        Some(post)
    }

    fn create_repost(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let source_id =
            canonical_u64(value(form, "source")).ok_or("Choose a discussion to repost.")?;
        let source = self
            .repost_source(source_id)
            .ok_or("This discussion is unavailable for reposting.")?;
        if source.repost_of.is_some() {
            return Err("Open the original discussion to repost it.".into());
        }
        if source.community == value(form, "community") {
            return Err("Choose a different community for the repost.".into());
        }
        let sensitive = source.sensitive
            || self
                .community_for_post(source_id)
                .is_some_and(|item| item.mature);
        let spoiler = source.spoiler;
        let mut repost_form = form.clone();
        repost_form.remove("url");
        let default_title = value(form, "title").is_empty();
        if default_title {
            repost_form.insert("title".into(), "Reposted discussion".into());
        }
        let mut post = self.new_post(&repost_form, "Repost")?;
        post.repost_of = Some(source_id);
        post.repost_title_from_source = default_title;
        post.sensitive |= sensitive;
        post.spoiler |= spoiler;
        self.add_post(post, form, "Repost published.")
    }

    fn save_draft(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let community = if value(form, "community").is_empty() {
            String::new()
        } else {
            self.community_slug(form)?
        };
        let title = bounded(form, "title", "Title", 160)?;
        let body = bounded(form, "body", "Post text", TEXT_LIMIT)?;
        let url = bounded(form, "url", "Link", 2_000)?;
        let flair = bounded(form, "flair", "Flair", 32)?;
        if title.is_empty() && body.is_empty() && url.is_empty() {
            return Err("Add a title, some post text, or a link before saving a draft.".into());
        }
        self.draft = Some(PostDraft {
            community,
            title: title.into(),
            body: body.into(),
            url: url.into(),
            flair: flair.into(),
            sensitive: value(form, "sensitive") == "on",
            spoiler: value(form, "spoiler") == "on",
            brand_affiliate: value(form, "brand_affiliate") == "on",
        });
        Ok("Draft saved.")
    }

    fn create_comment(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let post_id = self.post_id(form, "post")?;
        if !self
            .community_for_post(post_id)
            .is_some_and(|community| self.can_contribute(community))
        {
            return Err("This community limits who can add a comment.".into());
        }
        if self.is_discussion_locked(post_id) {
            return Err("This discussion is locked.".into());
        }
        let body = required(form, "body", "Comment", 4_000)?;
        let parent = match value(form, "parent") {
            "" => None,
            _ => {
                let id = id_field(form, "parent")?;
                let parent = self
                    .comments
                    .iter()
                    .find(|comment| comment.id == id && comment.post == post_id)
                    .ok_or("This reply must belong to the same discussion.")?;
                if parent.deleted || self.is_comment_removed(id) {
                    return Err("This comment is no longer open for replies.".into());
                }
                if self.is_comment_locked(id) {
                    return Err("Replies to this comment are locked.".into());
                }
                Some(id)
            }
        };
        let id = next_id(&mut self.next_comment)?;
        self.comments.push(Comment {
            id,
            post: post_id,
            parent,
            author: self.username.clone(),
            body: body.into(),
            age: "just now".into(),
            score: 1,
            edited_at: None,
            deleted: false,
        });
        if let Some(post) = self.posts.iter_mut().find(|post| post.id == post_id) {
            post.reply_count = post.reply_count.saturating_add(1);
        }
        if self.screen_comment(id)? {
            return Ok(AUTOMOD_REMOVED_COMMENT);
        }
        self.notify_comment(post_id, parent)?;
        Ok("Comment posted.")
    }

    fn own_visible_comment_index(&self, form: &BTreeMap<String, String>) -> Result<usize, String> {
        let id = id_field(form, "id")?;
        if self.is_comment_removed(id) {
            return Err("This comment is unavailable.".into());
        }
        let index = self
            .comments
            .iter()
            .position(|comment| {
                comment.id == id && !comment.deleted && comment.author == self.username
            })
            .ok_or("Choose one of your own comments.")?;
        let comment = &self.comments[index];
        if self.viewable_post_community(comment.post).is_none() {
            return Err("This comment is unavailable.".into());
        }
        Ok(index)
    }

    fn edit_comment(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let index = self.own_visible_comment_index(form)?;
        if !self
            .community_for_post(self.comments[index].post)
            .is_some_and(|community| self.can_contribute(community))
        {
            return Err("This community limits who can edit a comment.".into());
        }
        let body = required(form, "body", "Comment", 4_000)?;
        let comment = &mut self.comments[index];
        if comment.body == body {
            return Ok("Comment unchanged.");
        }
        comment.body = body.into();
        comment.edited_at = Some(unix_seconds());
        let id = comment.id;
        let removed = self.screen_comment(id)?;
        Ok(if removed {
            AUTOMOD_REMOVED_COMMENT
        } else {
            "Comment updated."
        })
    }

    fn delete_comment(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let index = self.own_visible_comment_index(form)?;
        let comment = &mut self.comments[index];
        comment.author.clear();
        comment.body.clear();
        comment.edited_at = None;
        comment.deleted = true;
        comment.score = 0;
        self.comment_votes.remove(&comment.id);
        self.profile_hidden_comments.remove(&comment.id);
        for profile in self.account_profiles.values_mut() {
            profile.comment_votes.remove(&comment.id);
            profile.hidden_comments.remove(&comment.id);
        }
        if let Some(post) = self.posts.iter_mut().find(|post| post.id == comment.post) {
            post.reply_count = post.reply_count.saturating_sub(1);
        }
        let id = comment.id;
        self.locked_comments.remove(&id);
        self.clear_automod_flag(ContentKind::Comment, id);
        Ok("Comment deleted.")
    }

    fn create_report(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let post = self.post_id(form, "post")?;
        let region = required(form, "region", "Country or region", 120)?;
        let details = required(form, "details", "Report details", 4_000)?;
        let id = next_id(&mut self.next_report)?;
        self.reports.push(Report {
            id,
            post,
            comment: None,
            reason: None,
            rule: None,
            community: None,
            region: region.into(),
            details: details.into(),
            reviewed: false,
            reporter: self.username.clone(),
        });
        Ok("Report saved to the legal review log.")
    }

    fn review_report(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let id = id_field(form, "id")?;
        let index = self
            .reports
            .iter()
            .position(|report| report.id == id)
            .ok_or("This report could not be found.")?;
        let report = &self.reports[index];
        let community = self
            .report_community(report)
            .filter(|item| self.can_moderate(item, ModPermission::Reports))
            .map(|item| item.slug.clone())
            .ok_or("This report could not be found.")?;
        let reviewed = match value(form, "reviewed") {
            "yes" => true,
            "no" => false,
            _ => return Err("Choose whether to mark this report reviewed.".into()),
        };
        self.reports[index].reviewed = reviewed;
        if reviewed {
            self.record_moderation(&community, "Report marked reviewed.")
        } else {
            self.record_moderation(&community, "Report returned to the queue.")
        }
    }

    fn owns_community(&self, slug: &str) -> bool {
        self.communities.iter().any(|community| {
            community.slug == slug && community.owner.as_deref() == Some(self.username.as_str())
        })
    }

    fn restrict_user(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let community_index = self.moderated_community_index(form, ModPermission::Restrictions)?;
        let item = &self.communities[community_index];
        let username = restriction_handle(value(form, "username"))?;
        self.protect_community_staff(item, &username)?;
        let community = item.slug.clone();
        let kind = value(form, "kind");
        if !matches!(kind, "ban" | "mute") {
            return Err("Choose a ban or a mod-mail mute.".into());
        }
        let reason = bounded(form, "reason", "Reason", 280)?;
        let now = unix_seconds();
        let expires_at = restriction_expiration(form, now)?;
        if let Some(index) = self.restrictions.iter().position(|item| {
            item.community == community && item.username == username && item.kind == kind
        }) {
            let existing = &mut self.restrictions[index];
            if existing.is_active_at(now) {
                existing.reason = reason.into();
                existing.expires_at = expires_at;
                let message = if kind == "ban" {
                    "Community ban updated."
                } else {
                    "Mod-mail mute updated."
                };
                return self.record_moderation(&community, message);
            }
            let expired = self.restrictions.remove(index);
            self.record_expired_restriction(&expired, now)?;
        }
        let id = next_id(&mut self.next_restriction)?;
        self.restrictions.push(CommunityRestriction {
            id,
            community: community.clone(),
            username: username.clone(),
            kind: kind.into(),
            reason: reason.into(),
            created_at: now,
            expires_at,
        });
        self.notify_restriction_issued(&username, &community, kind)?;
        let message = if kind == "ban" {
            "Community ban added."
        } else {
            "Mod-mail mute added."
        };
        self.record_moderation(&community, message)
    }

    fn lift_restriction(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let community_index = self.moderated_community_index(form, ModPermission::Restrictions)?;
        let community = &self.communities[community_index];
        let id = canonical_u64(value(form, "id")).ok_or("Choose a valid restriction.")?;
        let now = unix_seconds();
        let index = self
            .restrictions
            .iter()
            .position(|item| {
                item.id == id && item.community == community.slug && item.is_active_at(now)
            })
            .ok_or("This restriction is unavailable in this community.")?;
        self.protect_community_staff(community, &self.restrictions[index].username)?;
        let item = self.restrictions.remove(index);
        self.resolve_manually_lifted_appeal(&item)?;
        let message = if item.kind == "ban" {
            "Community ban lifted."
        } else {
            "Mod-mail mute lifted."
        };
        self.record_moderation(&item.community, message)
    }

    fn send_mod_mail(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let index = self.community_index(form)?;
        let community = &self.communities[index];
        if community.owner.is_none() || !self.can_view_community(community) {
            return Err("This community does not have a moderation inbox.".into());
        }
        if self.is_mod_mail_muted(&community.slug, &self.username) {
            return Err("You are muted in this community’s moderation inbox.".into());
        }
        let subject = required(form, "subject", "Subject", 120)?;
        let body = required(form, "body", "Message", 4_000)?;
        let id = next_id(&mut self.next_mod_mail)?;
        self.mod_mail.push(ModMail {
            id,
            community: community.slug.clone(),
            sender: self.username.clone(),
            subject: subject.into(),
            body: body.into(),
            reply: String::new(),
        });
        Ok("Message sent to the local moderation inbox.")
    }

    fn reply_mod_mail(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let id = id_field(form, "id")?;
        let index = self
            .mod_mail
            .iter()
            .position(|message| message.id == id)
            .ok_or("This message could not be found.")?;
        let community = self.mod_mail[index].community.clone();
        if !self
            .communities
            .iter()
            .any(|item| item.slug == community && self.can_moderate(item, ModPermission::Mail))
        {
            return Err("This message could not be found.".into());
        }
        let reply = bounded(form, "reply", "Reply", 4_000)?;
        let reply = if reply.is_empty() {
            let response_id = value(form, "response_id")
                .parse::<u64>()
                .map_err(|_| "Write a reply or choose a saved response.")?;
            self.saved_responses
                .iter()
                .find(|item| item.id == response_id && item.community == community)
                .map(|item| item.body.clone())
                .ok_or("This saved response is unavailable for this community.")?
        } else {
            reply.into()
        };
        self.mod_mail[index].reply = reply;
        self.record_moderation(&community, "Moderation reply saved.")
    }

    fn record_moderation(
        &mut self,
        community: &str,
        message: &'static str,
    ) -> Result<&'static str, String> {
        self.record_moderation_action(community, message)?;
        Ok(message)
    }

    fn record_moderation_action(&mut self, community: &str, action: &str) -> Result<(), String> {
        self.record_moderation_action_at(community, action, unix_seconds())
    }

    fn record_moderation_action_at(
        &mut self,
        community: &str,
        action: &str,
        at: u64,
    ) -> Result<(), String> {
        let id = next_id(&mut self.next_mod_event)?;
        self.mod_log.push(ModerationEvent {
            id,
            community: community.into(),
            action: action.trim_end_matches('.').into(),
            at,
        });
        if self.mod_log.len() > MODERATION_LOG_LIMIT {
            self.mod_log.remove(0);
        }
        Ok(())
    }

    fn add_saved_response(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let index = self.moderated_community_index(form, ModPermission::Mail)?;
        let community = &self.communities[index].slug;
        if self
            .saved_responses
            .iter()
            .filter(|item| &item.community == community)
            .count()
            >= SAVED_RESPONSE_LIMIT
        {
            return Err("This community can have up to 12 saved responses.".into());
        }
        let title = required(form, "title", "Response title", 80)?;
        let body = required(form, "body", "Response text", 4_000)?;
        let id = next_id(&mut self.next_saved_response)?;
        self.saved_responses.push(SavedResponse {
            id,
            community: community.clone(),
            title: title.into(),
            body: body.into(),
        });
        Ok("Saved response added.")
    }

    fn delete_saved_response(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let id = id_field(form, "id")?;
        let index = self
            .saved_responses
            .iter()
            .position(|item| {
                item.id == id
                    && self.communities.iter().any(|community| {
                        community.slug == item.community
                            && self.can_moderate(community, ModPermission::Mail)
                    })
            })
            .ok_or("This saved response is unavailable.")?;
        self.saved_responses.remove(index);
        Ok("Saved response deleted.")
    }

    fn feed_details(
        &self,
        form: &BTreeMap<String, String>,
        name_limit: usize,
        existing: Option<&CustomFeed>,
    ) -> Result<(String, BTreeSet<String>), String> {
        let name = required(form, "name", "Feed name", name_limit)?;
        let communities: BTreeSet<_> = self
            .communities
            .iter()
            .filter(|community| {
                if self.can_view_community(community) {
                    value(form, &format!("community-{}", community.slug)) == "on"
                } else {
                    existing.is_some_and(|feed| feed.communities.contains(&community.slug))
                }
            })
            .map(|community| community.slug.clone())
            .collect();
        Ok((name.into(), communities))
    }

    fn create_feed(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let (name, communities) = self.feed_details(form, 50, None)?;
        let description = bounded(form, "description", "Feed description", 500)?;
        let (private, show_on_profile) = feed_visibility(form).unwrap_or((false, true));
        let id = next_id(&mut self.next_feed)?;
        self.custom_feeds.push(CustomFeed {
            id,
            name,
            communities,
            description: description.into(),
            private,
            show_on_profile,
        });
        Ok("Custom feed created.")
    }

    fn edit_feed(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let id = id_field(form, "id")?;
        let index = self
            .custom_feeds
            .iter()
            .position(|feed| feed.id == id)
            .ok_or("This custom feed could not be found.")?;
        let (name, communities) = self.feed_details(form, 60, self.custom_feeds.get(index))?;
        let feed = &mut self.custom_feeds[index];
        feed.name = name;
        feed.communities = communities;
        if form.contains_key("description") {
            feed.description = bounded(form, "description", "Feed description", 500)?.into();
        }
        if let Some((private, show_on_profile)) = feed_visibility(form) {
            feed.private = private;
            feed.show_on_profile = show_on_profile;
        }
        Ok("Custom feed updated.")
    }

    fn delete_feed(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let id = id_field(form, "id")?;
        let index = self
            .custom_feeds
            .iter()
            .position(|feed| feed.id == id)
            .ok_or("This custom feed could not be found.")?;
        self.custom_feeds.remove(index);
        Ok("Custom feed deleted.")
    }

    fn update_profile(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let username = checked_handle(value(form, "username"))?;
        if self.managed_profiles && username != self.username {
            return Err("Your account handle cannot be changed.".into());
        }
        if form.contains_key("display_name") {
            self.display_name = bounded(form, "display_name", "Display name", 60)?.into();
        }
        if form.contains_key("bio") {
            self.bio = bounded(form, "bio", "Bio", 280)?.into();
        }
        if username != self.username
            && (self.posts.iter().any(|post| post.author == username)
                || self
                    .comments
                    .iter()
                    .any(|comment| comment.author == username)
                || self
                    .communities
                    .iter()
                    .any(|community| community.owner.as_ref() == Some(&username)))
        {
            return Err("That username is already in use.".into());
        }
        for post in self
            .posts
            .iter_mut()
            .filter(|post| post.author == self.username)
        {
            post.author.clone_from(&username);
        }
        for comment in self
            .comments
            .iter_mut()
            .filter(|comment| comment.author == self.username)
        {
            comment.author.clone_from(&username);
        }
        for owner in self
            .communities
            .iter_mut()
            .filter_map(|community| community.owner.as_mut())
            .filter(|owner| **owner == self.username)
        {
            owner.clone_from(&username);
        }
        for message in self
            .mod_mail
            .iter_mut()
            .filter(|message| message.sender == self.username)
        {
            message.sender.clone_from(&username);
        }
        if username != self.username {
            for report in self
                .reports
                .iter_mut()
                .filter(|report| report.reporter == self.username)
            {
                report.reporter.clone_from(&username);
            }
            for message in &mut self.messages {
                if message.from == self.username {
                    message.from.clone_from(&username);
                }
                if message.to == self.username {
                    message.to.clone_from(&username);
                }
            }
            for notification in self
                .notifications
                .iter_mut()
                .filter(|item| item.recipient == self.username)
            {
                notification.recipient.clone_from(&username);
            }
            for artwork in self
                .pending_artwork
                .iter_mut()
                .filter(|artwork| artwork.creator == self.username)
            {
                artwork.creator.clone_from(&username);
            }
            for poll in self.posts.iter_mut().filter_map(|post| post.poll.as_mut()) {
                if let Some(option) = poll.votes.remove(&self.username) {
                    poll.votes.entry(username.clone()).or_insert(option);
                }
            }
            for item in self
                .restrictions
                .iter_mut()
                .filter(|item| item.username == self.username)
            {
                item.username.clone_from(&username);
            }
            let mut retained = BTreeSet::new();
            self.restrictions.retain(|item| {
                item.username != username
                    || retained.insert((item.community.clone(), item.kind.clone()))
            });
        }
        self.blocked.remove(&username);
        self.username = username;
        Ok("Profile updated.")
    }

    fn hide_profile_post(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let id = self.post_id(form, "id")?;
        if !self
            .posts
            .iter()
            .any(|post| post.id == id && post.author == self.username)
        {
            return Err("Choose one of your own posts to edit its profile visibility.".into());
        }
        if toggle(&mut self.profile_hidden_posts, id) {
            Ok("Post hidden on your profile.")
        } else {
            Ok("Post shown on your profile.")
        }
    }

    fn hide_profile_comment(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let id = id_field(form, "id")?;
        if self.is_comment_removed(id)
            || !self.comments.iter().any(|comment| {
                comment.id == id
                    && comment.author == self.username
                    && self.viewable_post_community(comment.post).is_some()
            })
        {
            return Err("Choose one of your own comments to edit its profile visibility.".into());
        }
        if toggle(&mut self.profile_hidden_comments, id) {
            Ok("Comment hidden on your profile.")
        } else {
            Ok("Comment shown on your profile.")
        }
    }

    fn add_social_link(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        if self.social_links.len() >= SOCIAL_LINK_LIMIT {
            return Err("Your profile can have up to six social links.".into());
        }
        let label = required(form, "label", "Link label", 40)?;
        let url = required(form, "url", "Link URL", 600)?;
        let authority = url
            .split_once("://")
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("https"))
            .and_then(|(_, rest)| rest.split(['/', '?', '#']).next())
            .unwrap_or_default();
        if authority.is_empty()
            || authority.starts_with(':')
            || authority.contains('@')
            || url.contains('\\')
            || url
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err("Enter a full https:// link.".into());
        }
        let id = next_id(&mut self.next_social_link)?;
        self.social_links.push(SocialLink {
            id,
            label: label.into(),
            url: url.into(),
        });
        Ok("Social link added.")
    }

    fn delete_social_link(
        &mut self,
        form: &BTreeMap<String, String>,
    ) -> Result<&'static str, String> {
        let id = id_field(form, "id")?;
        let index = self
            .social_links
            .iter()
            .position(|link| link.id == id)
            .ok_or("This social link could not be found.")?;
        self.social_links.remove(index);
        Ok("Social link removed.")
    }

    fn edit_post(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let id = self.post_id(form, "id")?;
        let index = self
            .posts
            .iter()
            .position(|post| post.id == id && post.author == self.username)
            .ok_or("You can edit posts from your own profile.")?;
        if !self.can_edit_post(&self.posts[index]) {
            return Err("This community has restricted your posting access.".into());
        }
        let source = self.posts[index]
            .repost_of
            .and_then(|id| self.repost_source(id));
        let inherited_sensitive = source.is_some_and(|source| {
            source.sensitive
                || self
                    .community_for_post(source.id)
                    .is_some_and(|community| community.mature)
        });
        let inherited_spoiler = source.is_some_and(|source| source.spoiler);
        let post = &mut self.posts[index];
        let (title, body, url) = post_content(form)?;
        let text_changed = post.title != title || post.body != body || post.url != url;
        if post.title != title {
            post.repost_title_from_source = false;
        }
        post.title = title;
        post.body = body;
        post.url = url;
        if value(form, "tags_present") == "1" {
            post.sensitive = inherited_sensitive || value(form, "sensitive") == "on";
            post.spoiler = inherited_spoiler || value(form, "spoiler") == "on";
            post.brand_affiliate = value(form, "brand_affiliate") == "on";
        }
        if form.contains_key("flair") {
            let flair = bounded(form, "flair", "Flair", 32)?;
            post.flair = if flair.is_empty() {
                "Discussion"
            } else {
                flair
            }
            .into();
        }
        let removed = text_changed && self.screen_post(id)?;
        Ok(if removed {
            AUTOMOD_REMOVED_POST
        } else {
            "Post updated."
        })
    }

    fn delete_post(&mut self, form: &BTreeMap<String, String>) -> Result<&'static str, String> {
        let id = self.post_id(form, "id")?;
        let index = self
            .posts
            .iter()
            .position(|post| post.id == id && post.author == self.username)
            .ok_or("You can delete posts from your own profile.")?;
        let post = self.posts.remove(index);
        self.locked_discussions.remove(&id);
        // Retained community reports keep the scope of the original discussion.
        for report in self
            .reports
            .iter_mut()
            .filter(|report| report.post == id && report.reason.is_some())
        {
            report.community = Some(post.community.clone());
        }
        self.content_removals
            .retain(|removal| removal.post_id != id);
        self.automod_flags.retain(|flag| flag.post_id != id);
        self.votes.remove(&id);
        self.saved.remove(&id);
        self.hidden.remove(&id);
        self.history.retain(|visited| *visited != id);
        self.profile_hidden_posts.remove(&id);
        self.notifications.retain(|entry| entry.post != Some(id));
        for profile in self.account_profiles.values_mut() {
            profile.votes.remove(&id);
            profile.saved.remove(&id);
            profile.hidden.remove(&id);
            profile.history.retain(|visited| *visited != id);
            profile.hidden_posts.remove(&id);
        }
        if let Some(community) = self
            .communities
            .iter_mut()
            .find(|community| community.slug == post.community)
        {
            community.highlighted_posts.retain(|&post_id| post_id != id);
        }
        self.comments.retain(|comment| {
            if comment.post != id {
                return true;
            }
            self.locked_comments.remove(&comment.id);
            self.comment_votes.remove(&comment.id);
            self.profile_hidden_comments.remove(&comment.id);
            for profile in self.account_profiles.values_mut() {
                profile.comment_votes.remove(&comment.id);
                profile.hidden_comments.remove(&comment.id);
            }
            false
        });
        Ok("Post deleted.")
    }

    fn read_record(&mut self, line: &str) -> Option<()> {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields.as_slice() {
            ["account-mode", active, legacy] => {
                let active = decode(active)?;
                let legacy = decode(legacy)?;
                if active != self.username {
                    return None;
                }
                self.managed_profiles = true;
                self.legacy_profile = (!legacy.is_empty()).then_some(legacy);
            }
            ["account-data", handle, sensitive, records] => {
                let handle = decode(handle)?;
                if !self.managed_profiles
                    || handle == self.username
                    || checked_handle(&handle).ok()? != handle
                {
                    return None;
                }
                let profile =
                    ProfileData::from_records(&handle, decode(sensitive)?, &decode(records)?)?;
                if self.account_profiles.insert(handle, profile).is_some() {
                    return None;
                }
            }
            ["settings", username, sensitive, next_post, next_comment, next_report, counters @ ..] =>
            {
                self.username = decode(username)?;
                self.sensitive = decode(sensitive)?;
                self.next_post = number(next_post)?;
                self.next_comment = number(next_comment)?;
                self.next_report = number(next_report)?;
                match counters {
                    [] => {}
                    [next_mod_mail, next_feed] => {
                        self.next_mod_mail = number(next_mod_mail)?;
                        self.next_feed = number(next_feed)?;
                    }
                    _ => return None,
                }
            }
            ["community", slug, name, description, guidelines, color, icon, members, owner, highlights, settings @ ..] =>
            {
                let slug = decode(slug)?;
                if self
                    .communities
                    .iter()
                    .any(|community| community.slug == slug)
                {
                    return None;
                }
                let (topic, visibility, mature) = match settings {
                    [] => (String::new(), "public".into(), false),
                    [topic] => (decode(topic)?, "public".into(), false),
                    [topic, visibility, mature] => {
                        (decode(topic)?, decode(visibility)?, number(mature)?)
                    }
                    _ => return None,
                };
                self.communities.push(Community {
                    slug,
                    name: decode(name)?,
                    description: decode(description)?,
                    guidelines: decode(guidelines)?,
                    color: decode(color)?,
                    icon: decode(icon)?,
                    icon_image: None,
                    banner_image: None,
                    members: number(members)?,
                    owner: Some(decode(owner)?),
                    highlighted_posts: highlights::read_order(highlights)?,
                    rules: Vec::new(),
                    topic,
                    visibility,
                    mature,
                });
            }
            ["community-rule", slug, id, title, description] => {
                self.read_community_rule(slug, id, title, description)?;
            }
            ["community-rule-sequence", next] => {
                let next = canonical_u64(next).filter(|&value| value > 0)?;
                self.next_community_rule = self.next_community_rule.max(next);
            }
            ["community-image", slug, kind, content_type, payload] => {
                let slug = decode(slug)?;
                let community = self
                    .communities
                    .iter_mut()
                    .find(|community| community.slug == slug)?;
                let image = checked_image(content_type, decode_image(payload)?).ok()?;
                match *kind {
                    "icon" => community.icon_image = Some(image),
                    "banner" => community.banner_image = Some(image),
                    _ => return None,
                }
            }
            ["post", id, community, author, title, body, flair, age, url, score, reply_count, sensitive, image, tags @ ..] =>
            {
                let (spoiler, brand_affiliate) = match tags {
                    [] => (false, false),
                    [spoiler, brand_affiliate] => (number(spoiler)?, number(brand_affiliate)?),
                    _ => return None,
                };
                self.posts.push(Post {
                    id: number(id)?,
                    community: decode(community)?,
                    author: decode(author)?,
                    title: decode(title)?,
                    body: decode(body)?,
                    flair: decode(flair)?,
                    age: decode(age)?,
                    created_at: None,
                    url: decode(url)?,
                    score: number(score)?,
                    reply_count: number(reply_count)?,
                    sensitive: number(sensitive)?,
                    spoiler,
                    brand_affiliate,
                    image: number(image)?,
                    poll: None,
                    repost_of: None,
                    repost_title_from_source: false,
                    media: Vec::new(),
                });
            }
            ["post-created", id, created_at] => {
                let id = number(id)?;
                let post = self.posts.iter_mut().find(|post| post.id == id)?;
                if post.created_at.is_some() {
                    return None;
                }
                post.created_at = Some(number(created_at)?);
            }
            ["discussion-lock", id] => {
                let id = number(id)?;
                if !self.posts.iter().any(|post| post.id == id) {
                    return None;
                }
                self.locked_discussions.insert(id);
            }
            ["post-poll", id, closes_at, options @ ..] if (2..=6).contains(&options.len()) => {
                let id = number(id)?;
                let post = self.posts.iter_mut().find(|post| post.id == id)?;
                post.poll = Some(Poll {
                    options: options
                        .iter()
                        .map(|option| decode(option))
                        .collect::<Option<_>>()?,
                    votes: BTreeMap::new(),
                    closes_at: number(closes_at)?,
                });
            }
            ["post-poll-vote", id, username, option] => {
                let id = number(id)?;
                let poll = self
                    .posts
                    .iter_mut()
                    .find(|post| post.id == id)?
                    .poll
                    .as_mut()?;
                let option = number(option)?;
                if option >= poll.options.len() {
                    return None;
                }
                poll.votes.insert(decode(username)?, option);
            }
            ["post-repost", id, source, derived_title] => {
                let id = number(id)?;
                let post = self.posts.iter_mut().find(|post| post.id == id)?;
                post.repost_of = Some(number(source)?);
                post.repost_title_from_source = number(derived_title)?;
            }
            ["post-media", id, key, content_type] => {
                let id = number(id)?;
                let post = self.posts.iter_mut().find(|post| post.id == id)?;
                post.media.push(PostMedia {
                    key: decode(key)?,
                    content_type: decode(content_type)?,
                });
                validate_post_media(&post.media).ok()?;
            }
            ["profile", display_name, bio, public_posts, public_comments, nsfw, flair, created, next_social_link] =>
            {
                self.display_name = decode(display_name)?;
                self.bio = decode(bio)?;
                self.public_posts = number(public_posts)?;
                self.public_comments = number(public_comments)?;
                self.profile_nsfw = number(nsfw)?;
                self.profile_flair = decode(flair)?;
                self.profile_created = number(created)?;
                self.next_social_link = number(next_social_link)?;
            }
            ["profile-image", kind, content_type, payload] => {
                let image = checked_image(content_type, decode_image(payload)?).ok()?;
                match *kind {
                    "avatar" => self.avatar = Some(image),
                    "cover" => self.cover = Some(image),
                    _ => return None,
                }
            }
            ["social-link", id, label, url] => self.social_links.push(SocialLink {
                id: number(id)?,
                label: decode(label)?,
                url: decode(url)?,
            }),
            ["draft", community, title, body, url, flair, sensitive, spoiler, brand_affiliate] => {
                self.draft = Some(PostDraft {
                    community: decode(community)?,
                    title: decode(title)?,
                    body: decode(body)?,
                    url: decode(url)?,
                    flair: decode(flair)?,
                    sensitive: number(sensitive)?,
                    spoiler: number(spoiler)?,
                    brand_affiliate: number(brand_affiliate)?,
                });
            }
            ["comment", id, post, parent, author, body, age, score, revision @ ..] => {
                let (edited_at, deleted) = match revision {
                    [] => (None, false),
                    [edited_at, deleted] => (optional_number(edited_at)?, number(deleted)?),
                    _ => return None,
                };
                self.comments.push(Comment {
                    id: number(id)?,
                    post: number(post)?,
                    parent: optional_number(parent)?,
                    author: decode(author)?,
                    body: decode(body)?,
                    age: decode(age)?,
                    score: number(score)?,
                    edited_at,
                    deleted,
                });
            }
            ["comment-lock", id] => {
                let id = number(id)?;
                if !self
                    .comments
                    .iter()
                    .any(|comment| comment.id == id && !comment.deleted)
                {
                    return None;
                }
                self.locked_comments.insert(id);
            }
            fields @ ["report", ..] => self.read_report_record(fields)?,
            ["message-sequence", next_message, next_notification] => {
                self.next_message = number(next_message)?;
                self.next_notification = number(next_notification)?;
            }
            ["direct-message", id, from, to, body, at, read] => self.messages.push(DirectMessage {
                id: number(id)?,
                from: decode(from)?,
                to: decode(to)?,
                body: decode(body)?,
                at: number(at)?,
                read: number(read)?,
            }),
            ["chat-request-permission", permission] => {
                self.chat_request_permission = ChatRequestPermission::parse(permission)?;
            }
            fields @ (["chat-request-sequence", ..]
            | ["chat-request", ..]
            | ["declined-chat-request", ..]) => {
                self.read_chat_request_record(fields)?;
            }
            ["notification-preferences", replies, messages, moderation] => {
                self.notification_preferences = NotificationPreferences {
                    replies: number(replies)?,
                    messages: number(messages)?,
                    moderation: number(moderation)?,
                };
            }
            ["notification", id, recipient, body, at, read, post, destination @ ..] => {
                let (appeal, moderation, silent) = match destination {
                    [] => (false, false, false),
                    ["appeal"] if post.is_empty() => (true, false, false),
                    ["moderation"] if post.is_empty() => (false, true, false),
                    ["appeal", "silent"] if post.is_empty() => (true, false, true),
                    ["moderation", "silent"] if post.is_empty() => (false, true, true),
                    _ => return None,
                };
                self.notifications.push(Notification {
                    id: number(id)?,
                    recipient: decode(recipient)?,
                    body: decode(body)?,
                    at: number(at)?,
                    read: number::<bool>(read)? || silent,
                    post: optional_number(post)?,
                    appeal,
                    moderation,
                    silent,
                })
            }
            fields @ (["appeal-sequence", ..] | ["appeal", ..]) => {
                self.read_appeal_record(fields)?
            }
            fields @ ["content-removal", ..] => self.read_content_removal(fields)?,
            fields @ (["automod-sequence", ..] | ["automod-rule", ..] | ["automod-flag", ..]) => {
                self.read_automod_record(fields)?
            }
            ["mod-mail", id, community, sender, subject, body, reply] => {
                self.mod_mail.push(ModMail {
                    id: number(id)?,
                    community: decode(community)?,
                    sender: decode(sender)?,
                    subject: decode(subject)?,
                    body: decode(body)?,
                    reply: decode(reply)?,
                });
            }
            ["moderation-sequence", next_event, next_response] => {
                self.next_mod_event = number(next_event)?;
                self.next_saved_response = number(next_response)?;
            }
            ["moderator-role", slug, handle, role] => {
                self.read_moderator_role(slug, handle, role)?;
            }
            ["approved-member", slug, handle] => self.read_approved_member(slug, handle)?,
            ["schedule-sequence", next_scheduled] => self.next_scheduled = number(next_scheduled)?,
            ["restriction-sequence", next_restriction] => {
                self.next_restriction = number(next_restriction)?
            }
            ["community-restriction", id, community, username, kind, reason, created_at, expiration @ ..] =>
            {
                let expires_at = match expiration {
                    [] | [""] => None,
                    [timestamp] => Some(number(timestamp)?),
                    _ => return None,
                };
                if !expires_at.is_none_or(representable_expiration) {
                    return None;
                }
                self.restrictions.push(CommunityRestriction {
                    id: number(id)?,
                    community: decode(community)?,
                    username: decode(username)?,
                    kind: decode(kind)?,
                    reason: decode(reason)?,
                    created_at: number(created_at)?,
                    expires_at,
                });
            }
            ["scheduled-item", id, community, kind, title, body, url, flair, sensitive, spoiler, brand_affiliate, publish_at, zone, event_start, event_end, status, post_id, authors @ ..] =>
            {
                let author = match authors {
                    [] | [""] => None,
                    [handle] => {
                        let handle = decode(handle)?;
                        if handle == DELETED_ACCOUNT {
                            Some(handle)
                        } else {
                            Some(checked_handle(&handle).ok()?)
                        }
                    }
                    _ => return None,
                };
                self.scheduled.push(ScheduledItem {
                    id: number(id)?,
                    community: decode(community)?,
                    author,
                    kind: decode(kind)?,
                    title: decode(title)?,
                    body: decode(body)?,
                    url: decode(url)?,
                    flair: decode(flair)?,
                    sensitive: number(sensitive)?,
                    spoiler: number(spoiler)?,
                    brand_affiliate: number(brand_affiliate)?,
                    publish_at: number(publish_at)?,
                    zone: decode(zone)?,
                    event_start: optional_number(event_start)?,
                    event_end: optional_number(event_end)?,
                    status: decode(status)?,
                    post_id: optional_number(post_id)?,
                });
            }
            ["moderation-event", id, community, action, at] => self.mod_log.push(ModerationEvent {
                id: number(id)?,
                community: decode(community)?,
                action: decode(action)?,
                at: number(at)?,
            }),
            ["saved-response", id, community, title, body] => {
                self.saved_responses.push(SavedResponse {
                    id: number(id)?,
                    community: decode(community)?,
                    title: decode(title)?,
                    body: decode(body)?,
                })
            }
            ["F", id, name, "@", description, private, show_on_profile, communities @ ..] => {
                let private: bool = number(private)?;
                let show_on_profile: bool = number(show_on_profile)?;
                self.custom_feeds.push(CustomFeed {
                    id: number(id)?,
                    name: decode(name)?,
                    communities: communities
                        .iter()
                        .map(|slug| decode(slug))
                        .collect::<Option<_>>()?,
                    description: decode(description)?,
                    private,
                    show_on_profile: !private && show_on_profile,
                });
            }
            ["F", id, name, communities @ ..] => {
                self.custom_feeds.push(CustomFeed {
                    id: number(id)?,
                    name: decode(name)?,
                    communities: communities
                        .iter()
                        .map(|slug| decode(slug))
                        .collect::<Option<_>>()?,
                    description: String::new(),
                    private: false,
                    show_on_profile: true,
                });
            }
            ["V", slug] => self.recent.push(decode(slug)?),
            ["H", id] => self.history.push(number(id)?),
            ["profile-hidden-post", id] => {
                self.profile_hidden_posts.insert(number(id)?);
            }
            ["profile-hidden-comment", id] => {
                self.profile_hidden_comments.insert(number(id)?);
            }
            ["vote", id, value] => {
                self.votes.insert(number(id)?, number(value)?);
            }
            ["comment-vote", id, value] => {
                self.comment_votes.insert(number(id)?, number(value)?);
            }
            ["saved", id] => {
                self.saved.insert(number(id)?);
            }
            ["hidden", id] => {
                self.hidden.insert(number(id)?);
            }
            ["joined", slug] => {
                self.joined.insert(decode(slug)?);
            }
            ["muted", slug] => {
                self.muted.insert(decode(slug)?);
            }
            ["blocked", author] => {
                self.blocked.insert(decode(author)?);
            }
            ["keyword", keyword] => self.keywords.push(decode(keyword)?),
            _ => return None,
        }
        Some(())
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        static TEMP_ID: AtomicU64 = AtomicU64::new(0);
        let temporary = path.with_extension(format!(
            "{}.{}.tmp",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let result = self.write_records(&mut file).and_then(|()| file.sync_all());
        drop(file);
        let result = result.and_then(|()| fs::rename(&temporary, path));
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }

    fn write_records(&self, file: &mut impl Write) -> io::Result<()> {
        writeln!(file, "{FORMAT}")?;
        writeln!(
            file,
            "settings\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            encode(&self.username),
            encode(if self.guest() {
                "blur"
            } else {
                &self.sensitive
            }),
            self.next_post,
            self.next_comment,
            self.next_report,
            self.next_mod_mail,
            self.next_feed,
        )?;
        self.write_account_records(file)?;
        writeln!(
            file,
            "community-rule-sequence\t{}",
            self.next_community_rule
        )?;
        for community in &self.communities {
            let Some(owner) = &community.owner else {
                continue;
            };
            writeln!(
                file,
                "community\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                encode(&community.slug),
                encode(&community.name),
                encode(&community.description),
                encode(&community.guidelines),
                encode(&community.color),
                encode(&community.icon),
                community.members,
                encode(owner),
                highlights::write_order(community.highlighted_posts()),
                encode(&community.topic),
                encode(&community.visibility),
                community.mature,
            )?;
            community.write_rules(file)?;
            for (kind, image) in [
                ("icon", &community.icon_image),
                ("banner", &community.banner_image),
            ] {
                if let Some(image) = image {
                    writeln!(
                        file,
                        "community-image\t{}\t{kind}\t{}\t{}",
                        encode(&community.slug),
                        image.content_type,
                        encode_image(&image.bytes)
                    )?;
                }
            }
        }
        self.write_moderator_roles(file)?;
        self.write_approved_members(file)?;
        for post in &self.posts {
            writeln!(
                file,
                "post\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                post.id,
                encode(&post.community),
                encode(&post.author),
                encode(&post.title),
                encode(&post.body),
                encode(&post.flair),
                encode(&post.age),
                encode(&post.url),
                post.score,
                post.reply_count,
                post.sensitive,
                post.image,
                post.spoiler,
                post.brand_affiliate,
            )?;
            if let Some(created_at) = post.created_at {
                writeln!(file, "post-created\t{}\t{created_at}", post.id)?;
            }
            if self.is_discussion_locked(post.id) {
                writeln!(file, "discussion-lock\t{}", post.id)?;
            }
            if let Some(poll) = &post.poll {
                write!(file, "post-poll\t{}\t{}", post.id, poll.closes_at)?;
                for option in &poll.options {
                    write!(file, "\t{}", encode(option))?;
                }
                writeln!(file)?;
                for (username, option) in &poll.votes {
                    writeln!(
                        file,
                        "post-poll-vote\t{}\t{}\t{}",
                        post.id,
                        encode(username),
                        option
                    )?;
                }
            }
            if let Some(source) = post.repost_of {
                writeln!(
                    file,
                    "post-repost\t{}\t{}\t{}",
                    post.id, source, post.repost_title_from_source
                )?;
            }
            for media in &post.media {
                writeln!(
                    file,
                    "post-media\t{}\t{}\t{}",
                    post.id,
                    encode(&media.key),
                    encode(&media.content_type)
                )?;
            }
        }
        for comment in &self.comments {
            let parent = comment.parent.map_or_else(String::new, |id| id.to_string());
            let edited_at = comment
                .edited_at
                .map_or_else(String::new, |time| time.to_string());
            writeln!(
                file,
                "comment\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{edited_at}\t{}",
                comment.id,
                comment.post,
                parent,
                encode(&comment.author),
                encode(&comment.body),
                encode(&comment.age),
                comment.score,
                comment.deleted,
            )?;
            if self.is_comment_locked(comment.id) {
                writeln!(file, "comment-lock\t{}", comment.id)?;
            }
        }
        self.write_content_removals(file)?;
        self.write_automod_records(file)?;
        for report in &self.reports {
            report.write_record(file)?;
        }
        writeln!(
            file,
            "message-sequence\t{}\t{}",
            self.next_message, self.next_notification
        )?;
        for message in &self.messages {
            writeln!(
                file,
                "direct-message\t{}\t{}\t{}\t{}\t{}\t{}",
                message.id,
                encode(&message.from),
                encode(&message.to),
                encode(&message.body),
                message.at,
                message.read
            )?;
        }
        self.write_chat_request_records(file)?;
        for notification in &self.notifications {
            let post = notification
                .post
                .map_or_else(String::new, |id| id.to_string());
            let destination = if notification.appeal {
                "\tappeal"
            } else if notification.moderation {
                "\tmoderation"
            } else {
                ""
            };
            let delivery = if notification.silent { "\tsilent" } else { "" };
            writeln!(
                file,
                "notification\t{}\t{}\t{}\t{}\t{}\t{post}{destination}{delivery}",
                notification.id,
                encode(&notification.recipient),
                encode(&notification.body),
                notification.at,
                notification.read
            )?;
        }
        self.write_appeal_records(file)?;
        for message in &self.mod_mail {
            writeln!(
                file,
                "mod-mail\t{}\t{}\t{}\t{}\t{}\t{}",
                message.id,
                encode(&message.community),
                encode(&message.sender),
                encode(&message.subject),
                encode(&message.body),
                encode(&message.reply)
            )?;
        }
        writeln!(
            file,
            "moderation-sequence\t{}\t{}",
            self.next_mod_event, self.next_saved_response
        )?;
        for event in &self.mod_log {
            writeln!(
                file,
                "moderation-event\t{}\t{}\t{}\t{}",
                event.id,
                encode(&event.community),
                encode(&event.action),
                event.at
            )?;
        }
        for response in &self.saved_responses {
            writeln!(
                file,
                "saved-response\t{}\t{}\t{}\t{}",
                response.id,
                encode(&response.community),
                encode(&response.title),
                encode(&response.body)
            )?;
        }
        writeln!(file, "schedule-sequence\t{}", self.next_scheduled)?;
        for item in &self.scheduled {
            let event_start = item
                .event_start
                .map_or_else(String::new, |time| time.to_string());
            let event_end = item
                .event_end
                .map_or_else(String::new, |time| time.to_string());
            let post_id = item.post_id.map_or_else(String::new, |id| id.to_string());
            writeln!(file, "scheduled-item\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{event_start}\t{event_end}\t{}\t{post_id}\t{}",
                item.id, encode(&item.community), encode(&item.kind), encode(&item.title), encode(&item.body),
                encode(&item.url), encode(&item.flair), item.sensitive, item.spoiler, item.brand_affiliate,
                item.publish_at, encode(&item.zone), encode(&item.status), encode(item.author.as_deref().unwrap_or_default()))?;
        }
        writeln!(file, "restriction-sequence\t{}", self.next_restriction)?;
        for item in &self.restrictions {
            write!(
                file,
                "community-restriction\t{}\t{}\t{}\t{}\t{}\t{}",
                item.id,
                encode(&item.community),
                encode(&item.username),
                encode(&item.kind),
                encode(&item.reason),
                item.created_at
            )?;
            if let Some(expires_at) = item.expires_at {
                write!(file, "\t{expires_at}")?;
            }
            writeln!(file)?;
        }
        Ok(())
    }
}

fn legacy_age_seconds(age: &str) -> u64 {
    for (suffix, seconds) in [('m', 60), ('h', 3_600), ('d', 86_400)] {
        if let Some(value) = age
            .strip_suffix(suffix)
            .and_then(|value| value.parse::<u64>().ok())
        {
            return value.saturating_mul(seconds);
        }
    }
    0
}

#[cfg(test)]
mod post_timestamp_tests {
    use super::*;

    #[test]
    fn legacy_post_age_becomes_a_stable_publication_time() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "commonline-post-time-{}-{unique}.state",
            std::process::id()
        ));
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_file(&self.0);
            }
        }
        let fixture = Fixture(path);
        let mut legacy = State::seeded();
        for post in &mut legacy.posts {
            post.created_at = None;
        }
        legacy.save(&fixture.0).unwrap();
        let modified = fs::metadata(&fixture.0)
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let migrated = State::load(&fixture.0).unwrap();
        assert_eq!(migrated.posts[0].created_at, Some(modified - 86_400));
        assert_eq!(migrated.posts[0].age_label(modified + 86_400), "2d");
        migrated.save(&fixture.0).unwrap();
        let reloaded = State::load(&fixture.0).unwrap();
        assert_eq!(reloaded.posts[0].created_at, migrated.posts[0].created_at);
    }
}

fn seed_news(first_id: u64, seeded_at: u64) -> [Post; 2] {
    [
        (first_id, "media-lab", "archivewire", "Copperfen community radio opens its archive to the public", "In this fictional local brief, the Copperfen volunteer station catalogued 400 hours of interviews, live music, and call-in shows. Listeners can browse by year or contribute episode notes.", "1h", 76),
        (first_id + 1, "public-square", "transitdesk", "Glassfern Ward opens a tool library inside its old tram depot", "In this fictional local brief, volunteers in Glassfern Ward started lending hand tools this week. Members can book a repair table on Saturdays.", "42m", 103),
    ].map(|(id, community, author, title, body, age, score)| Post {
        id, community: community.into(), author: author.into(), title: title.into(), body: body.into(),
        flair: "News".into(), age: age.into(), created_at: Some(seeded_at.saturating_sub(legacy_age_seconds(age))),
        url: String::new(), score, reply_count: 0,
        sensitive: false, spoiler: false, brand_affiliate: false, image: false,
        poll: None, repost_of: None, repost_title_from_source: false, media: Vec::new(),
    })
}

fn checked_image(content_type: &str, bytes: Vec<u8>) -> Result<ProfileAvatar, String> {
    if bytes.len() > IMAGE_LIMIT {
        return Err("Choose an image of 512 KiB or less.".into());
    }
    let content_type = match content_type.trim().to_ascii_lowercase().as_str() {
        "image/png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => "image/png",
        "image/jpeg" if bytes.starts_with(b"\xff\xd8\xff") => "image/jpeg",
        "image/webp" if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") => {
            "image/webp"
        }
        _ => return Err("Choose a PNG, JPEG, or WebP image.".into()),
    };
    Ok(ProfileAvatar {
        content_type,
        bytes,
    })
}

fn validate_post_media(media: &[PostMedia]) -> Result<(), String> {
    if media.is_empty()
        || media.len() > 6
        || media.len() > 1
            && media
                .iter()
                .any(|item| item.content_type.starts_with("video/"))
    {
        return Err("Choose up to six images or one video.".into());
    }
    if media.iter().any(|item| {
        !matches!(
            item.content_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp" | "image/gif" | "video/mp4" | "video/webm"
        ) || item.key.len() != 48
            || !item
                .key
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err("This media attachment is unavailable.".into());
    }
    Ok(())
}

fn encode_image(bytes: &[u8]) -> String {
    let hex = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        encoded.push(char::from(hex[usize::from(byte >> 4)]));
        encoded.push(char::from(hex[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn decode_image(encoded: &str) -> Option<Vec<u8>> {
    if encoded.len() > IMAGE_LIMIT * 2 || encoded.len() % 2 != 0 {
        return None;
    }
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = char::from(pair[0]).to_digit(16)?;
            let low = char::from(pair[1]).to_digit(16)?;
            Some((high * 16 + low) as u8)
        })
        .collect()
}

fn value<'a>(form: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    form.get(key).map_or("", |value| value.trim())
}

fn bounded<'a>(
    form: &'a BTreeMap<String, String>,
    key: &str,
    label: &str,
    limit: usize,
) -> Result<&'a str, String> {
    let field = value(form, key);
    if field.chars().count() > limit {
        return Err(format!("{label} can be up to {limit} characters."));
    }
    Ok(field)
}

fn required<'a>(
    form: &'a BTreeMap<String, String>,
    key: &str,
    label: &str,
    limit: usize,
) -> Result<&'a str, String> {
    let field = bounded(form, key, label, limit)?;
    if field.is_empty() {
        return Err(format!("{label} is required."));
    }
    Ok(field)
}

fn checked_slug(slug: &str) -> Result<String, String> {
    let slug = slug.to_ascii_lowercase();
    if !(2..=32).contains(&slug.len())
        || !slug
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || slug.starts_with('-')
        || slug.ends_with('-')
    {
        return Err("Use 2 to 32 letters, numbers, or hyphens for the community address. Begin and end with a letter or number.".into());
    }
    if matches!(
        slug.as_str(),
        "home"
            | "all"
            | "saved"
            | "communities"
            | "settings"
            | "policy"
            | "profile"
            | "following"
            | "popular"
            | "news"
    ) {
        return Err("That community address is reserved.".into());
    }
    Ok(slug)
}

fn community_details(form: &BTreeMap<String, String>) -> Result<(String, String, String), String> {
    let name = required(form, "name", "Community name", 60)?;
    let description = bounded(form, "description", "Description", 500)?;
    let guidelines = bounded(form, "guidelines", "Guidelines", 2_000)?;
    Ok((name.into(), description.into(), guidelines.into()))
}

fn checked_topic(topic: &str) -> Result<String, String> {
    if topic.len() > 40
        || topic.starts_with('-')
        || topic.ends_with('-')
        || !topic
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("Choose a community topic with up to 40 letters, numbers, or hyphens.".into());
    }
    Ok(topic.to_ascii_lowercase())
}

fn community_visibility(form: &BTreeMap<String, String>) -> Result<Option<(String, bool)>, String> {
    if !form.contains_key("visibility") {
        return Ok(None);
    }
    let visibility = value(form, "visibility");
    if !matches!(visibility, "public" | "restricted" | "private") {
        return Err("Choose a public, restricted, or private community.".into());
    }
    Ok(Some((visibility.into(), value(form, "mature") == "on")))
}

fn feed_visibility(form: &BTreeMap<String, String>) -> Option<(bool, bool)> {
    if value(form, "visibility_present") != "1" {
        return None;
    }
    let private = value(form, "private") == "on";
    Some((private, !private && value(form, "show_on_profile") == "on"))
}

fn post_content(form: &BTreeMap<String, String>) -> Result<(String, String, String), String> {
    let title = required(form, "title", "Title", 160)?;
    let body = bounded(form, "body", "Post text", TEXT_LIMIT)?;
    let url = bounded(form, "url", "Link", 2_000)?;
    if !url.is_empty() {
        let host = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .map_or("", |rest| rest.split(['/', '?', '#']).next().unwrap_or(""));
        if host.is_empty()
            || host.contains('@')
            || url.contains('\\')
            || url.chars().any(|ch| ch.is_whitespace() || ch.is_control())
        {
            return Err("Enter a full http:// or https:// link.".into());
        }
    }
    Ok((title.into(), body.into(), url.into()))
}

fn scheduled_item(
    form: &BTreeMap<String, String>,
    id: u64,
    community: &str,
    default_zone: &str,
    now: u64,
) -> Result<ScheduledItem, String> {
    let kind = value(form, "kind");
    if !matches!(kind, "post" | "event") {
        return Err("Choose a scheduled post or event.".into());
    }
    let (title, body, url) = post_content(form)?;
    let flair = bounded(form, "flair", "Flair", 32)?;
    let zone = match value(form, "zone") {
        "" => default_zone,
        zone => zone,
    };
    if !matches!(
        zone,
        "America/Chicago" | "UTC" | "central-standard" | "central-daylight"
    ) {
        return Err("Choose a supported time zone.".into());
    }
    let publish_at =
        canonical_u64(value(form, "publish_at")).ok_or("Choose a valid publishing time.")?;
    if publish_at <= now {
        return Err("Choose a publishing time in the future.".into());
    }
    let (event_start, event_end) = if kind == "event" {
        let start =
            canonical_u64(value(form, "event_start")).ok_or("Choose a valid event start time.")?;
        if start < publish_at {
            return Err("Choose an event start at or after the publishing time.".into());
        }
        let end = match value(form, "event_end") {
            "" => None,
            value => Some(canonical_u64(value).ok_or("Choose a valid event end time.")?),
        };
        if end.is_some_and(|end| end <= start) {
            return Err("Choose an event end after its start.".into());
        }
        (Some(start), end)
    } else {
        (None, None)
    };
    Ok(ScheduledItem {
        id,
        community: community.into(),
        author: None,
        kind: kind.into(),
        title,
        body,
        url,
        flair: flair.into(),
        sensitive: value(form, "sensitive") == "on",
        spoiler: value(form, "spoiler") == "on",
        brand_affiliate: value(form, "brand_affiliate") == "on",
        publish_at,
        zone: zone.into(),
        event_start,
        event_end,
        status: "pending".into(),
        post_id: None,
    })
}

fn canonical_u64(value: &str) -> Option<u64> {
    let number: u64 = value.parse().ok()?;
    (number.to_string() == value).then_some(number)
}

fn checked_handle(handle: &str) -> Result<String, String> {
    if !(2..=24).contains(&handle.len())
        || !handle
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err("Use a username with 2 to 24 letters, numbers, or underscores.".into());
    }
    Ok(handle.to_ascii_lowercase())
}

fn restriction_handle(handle: &str) -> Result<String, String> {
    let handle = handle.trim().to_ascii_lowercase();
    checked_handle(handle.strip_prefix("u/").unwrap_or(&handle))
}

fn id_field(form: &BTreeMap<String, String>, key: &str) -> Result<u64, String> {
    value(form, key)
        .parse()
        .map_err(|_| "Choose an existing post or comment.".into())
}

fn artwork_preview_id(form: &BTreeMap<String, String>) -> Result<u64, String> {
    value(form, "artwork_preview")
        .parse()
        .map_err(|_| "Choose a valid artwork preview.".into())
}

fn next_id(sequence: &mut u64) -> Result<u64, String> {
    let id = *sequence;
    *sequence = id
        .checked_add(1)
        .ok_or("The local prototype has reached its storage limit.")?;
    Ok(id)
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

fn direction(form: &BTreeMap<String, String>) -> Result<i8, String> {
    match value(form, "direction") {
        "1" => Ok(1),
        "-1" => Ok(-1),
        _ => Err("Choose an upvote or downvote.".into()),
    }
}

fn vote(votes: &mut BTreeMap<u64, i8>, id: u64, direction: i8) {
    if votes.get(&id) == Some(&direction) {
        votes.remove(&id);
    } else {
        votes.insert(id, direction);
    }
}

fn toggle<T: Ord>(set: &mut BTreeSet<T>, item: T) -> bool {
    if set.remove(&item) {
        return false;
    }
    set.insert(item)
}

fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '%' => encoded.push_str("%25"),
            '\t' => encoded.push_str("%09"),
            '\r' => encoded.push_str("%0D"),
            '\n' => encoded.push_str("%0A"),
            _ => encoded.push(ch),
        }
    }
    encoded
}

fn decode(value: &str) -> Option<String> {
    let mut decoded = String::with_capacity(value.len());
    let mut pieces = value.split('%');
    decoded.push_str(pieces.next()?);
    for piece in pieces {
        let (code, rest) = (piece.get(..2)?, piece.get(2..)?);
        decoded.push(match code {
            "25" => '%',
            "09" => '\t',
            "0D" => '\r',
            "0A" => '\n',
            _ => return None,
        });
        decoded.push_str(rest);
    }
    Some(decoded)
}

fn number<T: std::str::FromStr>(value: &str) -> Option<T> {
    value.parse().ok()
}

fn optional_number<T: std::str::FromStr>(value: &str) -> Option<Option<T>> {
    if value.is_empty() {
        Some(None)
    } else {
        Some(Some(number(value)?))
    }
}
