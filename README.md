# Commonline

Commonline is an original discussion website prototype. People can create communities, post and discuss lawful material, choose what appears in their own feeds, and flag content for regional legal review. The demo starts with sample communities and uses a single local profile.

## Run locally

With Rust and Cargo installed, open a terminal in the project directory and run the local web server.

```sh
cargo run --offline
```

Open [127.0.0.1:4173](http://127.0.0.1:4173/) in your browser. If the port is occupied, run `cargo run --offline -- --port 0` and open the address printed in the terminal. Stop the server with Ctrl+C.

The server stores demo changes in `commonline.state` in the working directory. Set `COMMONLINE_STATE` to another file path for a separate demo session. Choose an unused path to start with the original sample data.

## Try the demo

1. On **Home**, browse **For You** or switch to **Following** for posts from joined communities. Try **Best**, **Hot**, **New**, and **Top**. **Popular** opens with a vote-based ranking, **News** contains fictional sample news posts, and **Explore** opens the community directory.
2. Use **Start a community** to choose a topic and access level, then enter the community details. On **Details**, you can upload an icon and banner as PNG, JPEG, or WebP files up to 512 KiB each. **Review** shows the artwork with its published crop. Use **Edit details** to replace or remove an image before creating the community. Restricted communities limit posting and commenting to the local owner, and private communities limit viewing as well. The **Mature (18+)** label applies the viewer’s sensitive-content preferences to discussions.
3. Join or leave communities, revisit your **Recent** communities, or create a **Custom feed** with a name, optional description, and any number of communities. Choose whether it appears in your profile preview. Private feeds stay in your own feed list.
4. Open **New post** and choose **Text and link**, **Media**, **Poll**, or **Repost**. Media accepts up to six PNG, JPEG, WebP, or GIF images (2 MiB each), or one MP4 or WebM video (16 MiB). Polls accept two to six choices and close after one, three, or seven days. To repost, select an original discussion and a different community, or start from **Repost** in the discussion’s menu. Add topic flair and content tags when composing a text, media, or poll post. **Save draft** keeps one resumable text or link draft. Use **Creator tools** to see local vote and reply totals.
5. Open **Your profile** to browse posts, comments, saved and hidden items, voting activity, and viewing history. In **Edit profile**, update your display name, bio, flair, avatar, and cover. Avatar and cover images accept PNG, JPEG, or WebP files up to 512 KiB each. Choose which posts and comments appear, add up to six HTTPS social links, and use the public profile preview to review what is listed.
6. Use **Feed controls** to show, collapse, or hide sensitive previews or mute a keyword. The post menu also offers personal hiding, community muting, and account blocking.
7. Choose **Report a legal concern** from a post’s menu and enter the relevant region. In **Mod queue**, mark reports on your communities reviewed or reopen them. Review status is recorded locally and the discussion stays visible. Use **Message moderators** to start a conversation in **Mod mail**. **Manage communities** opens owner settings, including topic guidelines and one pinned discussion per community.
8. Open **Community settings → Appearance** to replace or remove a published icon or banner. **Preview changes** shows the proposed artwork. Select **Save appearance** to publish it, or **Cancel** to return to the current appearance.
9. Open **Mod tools** on a community you own to switch between its scoped queue, messages, discussions, rules, members, and insights. Use **Saved responses** for reusable mail replies and **Mod log** to see changes to settings, pins, appearances, and review status.
10. In **Mod tools → Scheduled posts and events**, schedule a publication and edit or cancel it under **Upcoming**. An event includes a start time and an optional end time. Times default to Central time and support explicit UTC or Central offsets when needed. The server publishes due items while running and catches up on pending items after a restart.
11. In **Mod tools → Restricted Users**, apply a ban or mod mail mute by username, with an optional moderator-only reason. A ban stops posting, editing, and replying in that community. A mute stops new mod mail there. Use **Lift ban** or **Lift mute** to restore access separately. Existing posts remain visible and legal reporting stays available.

The America/Chicago conversion follows the U.S. daylight saving schedule. Choose an explicit offset for a time that occurs twice when clocks go back. [NIST daylight saving time reference](https://www.nist.gov/pml/time-and-frequency-division/popular-links/daylight-saving-time-dst)

Uploaded post attachments are stored in `commonline.state.media` by default. The server appends `.media` to a custom state path. Keep both together when moving or backing up a local demo.

Profile curation, community access, and moderation use the single local demo profile. The public profile is a preview within the local server. Production authentication and age verification remain future work.

## Feature inventory

The long-term target is **90% parity across a defined set of Reddit-style workflows**. The table records the local prototype's scope and the work that would go into that comparison.

| Area | Current prototype | Further work |
| --- | --- | --- |
| Discovery and feeds | For You, Following, Popular, fictional News, Explore, All discussions, community and custom feeds, Recent, site-wide and community search, Best/Hot/New/Top | Saved searches, richer ranking and filters |
| Posting and discussion | Title-only, text, link, image gallery and video posts, polls, cross-community reposts, post tags, one local text/link draft, post editing and deletion, threaded replies, votes, save and hide | Comment editing and deletion, richer editing, and media transcoding |
| Creator tools | Dashboard, per-post vote and reply totals, community trends, and publishing links | Impression counts, reach, and analytics over time |
| Identity and communication | Editable profile, avatar and cover uploads, activity and history, curated profile preview, keyword mutes, account blocking, and local mod mail | Account creation, sign-in and recovery, multi-user profiles, personal chat, and notifications |
| Community operations | Guided creation, topics and visibility, mature labels, artwork uploads and previews, owner settings and pinning, scoped moderator workspace, community bans and mod mail mutes, regional reports and review status, saved responses, activity log, and scheduled posts and events | Authenticated ownership, delegated roles, case decisions, appeals, recurring schedules, and automated moderation |

### Proposed parity benchmark

Target 27 of the 30 web workflows below. Count a workflow after it passes its end-to-end desktop and mobile acceptance checks, with persistent state and distinct signed-in accounts where needed. The current site uses one local profile for creator and owner actions. Track Commonline’s regional legal review policy separately from feature parity.

| Area | Five workflows to measure |
| --- | --- |
| Feeds | Home feeds, all-discussions feed, community feed, feed sorting, custom feeds |
| Discovery | Site-wide post search, search within a community, community directory, join and leave a community, mute and unmute a community |
| Publishing | Create text and link posts, edit or delete your own post, publish media, publish a poll, repost across communities |
| Discussions | Add a top-level comment, reply to a comment, edit or delete your comment, vote on posts and comments, save and retrieve a post |
| Accounts | Sign up and sign in, recover access, view and edit a profile, receive notifications, send and receive personal messages |
| Community management | Create a community, configure community settings, delegate moderator roles, review a report in a moderation queue, communicate decisions and handle appeals |

Reddit Help documents the comparison workflows. [Posting and commenting](https://support.reddithelp.com/hc/en-us/articles/360060422572-How-do-I-post-and-comment-on-Reddit) · [Post types](https://support.reddithelp.com/hc/en-us/articles/15484546290068-Community-settings/) · [Custom feeds](https://support.reddithelp.com/hc/en-us/articles/360043043412-What-is-a-custom-feed-and-how-do-I-make-one) · [Moderation tools](https://support.reddithelp.com/hc/en-us/articles/15484384020756-Moderation-Tools-overview)

## Architecture and design

This clean-room implementation uses the Rust standard library for the local HTTP server, form handling, HTML rendering, and file-backed state. The browser renders HTML and CSS, and the prototype bundles original artwork. The plaza image was created with ImageGen. Design references were reviewed through the Awwwards MCP. [197 Historias Ilustradas](https://www.awwwards.com/sites/197-historias-ilustradas) · [SANTAL](https://www.awwwards.com/sites/santal)

The built-in ImageGen prompt for `assets/plaza.jpg` was as follows.

> Photorealistic-natural. One original editorial photograph for a civic discussion forum post about who gets to use public space. Wide horizontal frame, 16:9 or wider. An overlooked terraced public plaza, low brutalist concrete civic buildings, clean diagonal concrete bleachers in the foreground, leafy trees with deep chartreuse late afternoon foliage and a few small distant people walking. Low warm sunlight from left, documentary 35mm film with a soft grain, rich olive greens and subdued warm gray concrete. Interesting modern architecture composition with a calm broad atrium or clear open space, public library or neighborhood arts district feel. Compose so a center crop to 3:1 still shows the curved/diagonal architecture, lush greenery, sky band and 2 or 3 tiny people in the middle distance. No signs, no obvious brands, no lettering, no overlay, no border.
