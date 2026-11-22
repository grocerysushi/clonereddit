# ThreadPorch

ThreadPorch is an original discussion website prototype. People can create communities, post and discuss lawful material, choose what appears in their own feeds, and flag content for regional legal review. Browse the sample communities as a guest or create an account to participate.

## Run locally

With Rust and Cargo installed, open a terminal in the project directory and run the local web server.

```sh
cargo run --locked
```

Open [127.0.0.1:4173](http://127.0.0.1:4173/) in your browser. If the port is occupied, run `cargo run --locked -- --port 0` and open the address printed in the terminal. Stop the server with Ctrl+C. Cargo needs access to the locked Rust dependencies on the first build. Add `--offline` after Cargo caches them.

The server stores changes in `commonline.state` and credentials in `commonline.state.accounts` in the working directory. Set `COMMONLINE_STATE` to another file path for a separate demo and the server will append `.accounts` to it. Choose an unused path to start with the original sample data. Run one server per state path.

ThreadPorch keeps the original `commonline.state` filename and `COMMONLINE_*` settings so existing installations can upgrade using their current files and configuration.

The browser chat and writing controls are written in Rust and distributed as WebAssembly assets. Small browser bindings connect them to the page. After editing either Rust client, rebuild its asset from the project directory.

```sh
rustup target add wasm32-unknown-unknown
rustc --target wasm32-unknown-unknown --edition 2021 --crate-type cdylib -C opt-level=z -C panic=abort -C strip=symbols -o assets/chat-live.wasm client/chat_live.rs
rustc --target wasm32-unknown-unknown --edition 2021 --crate-type cdylib -C opt-level=z -C panic=abort -C strip=symbols -o assets/post-format.wasm client/post_format.rs
```

## Accounts and recovery

Choose **Create account** and save the one-time recovery code. From **Your profile → Security**, add a recovery email, enter the verification code from your inbox, or change your password. Your current verified address can still receive reset codes while a replacement address is pending. Verification and emailed reset codes expire after 15 minutes and work once.

If you forget your password, select **Reset by email** on the sign-in page and enter your username or verified email. Follow the email to the reset page and paste the code. You can also select **Use a recovery code** and enter the code you saved at registration or after your last password change. A password reset ends your other sessions and issues a replacement recovery code.

In **Security → Two-factor authentication**, use your password to generate a setup key, add it to a time-based authenticator app, and enter the app’s six-digit code. Once enabled, sign-in and both password reset methods require an authenticator code. Store your authenticator backup securely so you can recover access if your device is lost. Turning off two-factor authentication requires your password and a fresh app code.

In **Security → Active sessions**, review each session’s browser, operating system, and sign-in time when available. Select **Sign out** beside a session to end it, or **Sign out everywhere else** to keep only the current browser signed in. Older sessions stay in the list even when their device and sign-in time are unavailable. ThreadPorch uses a general browser and operating-system label without storing the full browser identifier or an IP address for this feature.

In **Security → Delete account**, confirm permanent deletion with your current password and a fresh authenticator code if two-factor authentication is enabled. Deletion revokes every session, removes credentials, profile data, drafts, and personal saves, and reserves the retired handle. Existing posts and comments stay in their threads with a deleted byline. Recipient copies of messages and moderation records can remain. Communities keep their existing access settings and moderators, and owner-only settings become unavailable.

Open **Your profile** to sign out or edit your public profile. Guests can see your public profile and activity you choose to share. Saved posts, drafts, history, and private messages are scoped to your account.

The **Messages** and **Notifications** buttons in the header show unread counts. In **Messages**, choose a registered recipient or open **Message** from their profile. An existing conversation opens directly. For a new conversation, send a chat request with an opening message. The recipient can read it in **Messages → Requests**, then accept to open the conversation or decline to discard the request. Declining prevents repeat requests from that sender. The recipient can still initiate a request later. For a local two-account check, use two separate browser profiles or one regular window and one private window.

In **Account Settings → Chat requests**, choose **Anyone with an account**, **Accounts with verified email**, or **Nobody**. By default, anyone with an account can send a request. Changes apply to new requests. Existing conversations and pending requests remain available according to their existing access and block settings.

Open a conversation in **Messages** to receive new messages automatically. Incoming messages leave the composer and its draft in place. The transcript follows new messages when you are at the bottom, and you can mark the conversation read when you are ready.

Open **Your profile → Account settings** or **Notifications → Settings** to choose which new in-app alerts you receive for replies, direct messages, and moderation updates. All three are on by default. Changing a preference applies to future alerts. Messages still arrive in your inbox, and private moderation notices and reasons remain available under **Notifications → Moderation updates**.

To claim a profile from a version that predates accounts, stop the server, keep `COMMONLINE_STATE` set to the same path, and run `cargo run --locked -- --legacy-recovery` from this directory. The command prints the existing username and a one-time claim code. Restart the server, choose **Use a recovery code**, and enter that username, the claim code, and a new password. Save the replacement recovery code displayed after the reset.

### Email delivery

For local testing, set `COMMONLINE_MAIL_OUTBOX` to a private directory. ThreadPorch writes the generated messages as `.eml` files that you can open in a mail client.

```powershell
$env:COMMONLINE_MAIL_OUTBOX = Join-Path (Get-Location) 'mail-outbox'
cargo run --locked
```

For delivery to an inbox, run a trusted SMTP relay on the same machine. Set `COMMONLINE_SMTP_ADDR` to its loopback socket and `COMMONLINE_MAIL_FROM` to a sender the relay accepts. Use one delivery mode per server. The relay is responsible for the upstream connection, authentication, and TLS.

```powershell
Remove-Item Env:COMMONLINE_MAIL_OUTBOX -ErrorAction SilentlyContinue
$env:COMMONLINE_SMTP_ADDR = '127.0.0.1:2525'
$env:COMMONLINE_MAIL_FROM = 'notifications@example.com'
cargo run --locked
```

Emails link back to the same localhost server. Keep local outbox files private because they contain working one-time codes. Without either delivery mode, saved recovery codes are still available.

## Try the demo

1. On **Home**, browse **For You** or switch to **Following** for posts from joined communities and people you follow. Try **Best**, **Hot**, **New**, and **Top**. **Hot** weighs activity against time since publication. **Top** offers time windows from the past hour through all time. **Popular** opens with a vote-based ranking, **News** contains fictional sample news posts, and **Explore** opens the community directory. Search from the header, then switch between posts, comments, communities, people, and media.
2. Use **Start a community** to choose a topic and access level, then enter the community details. On **Details**, you can upload an icon and banner as PNG, JPEG, or WebP files up to 512 KiB each. **Review** shows the artwork with its published crop. Use **Edit details** to replace or remove an image before creating the community. Anyone can read a restricted community, while approved members and moderators can also post and comment. Private communities give viewing and posting access to approved members and moderators. The **Mature (18+)** label applies the viewer’s sensitive-content preferences to discussions.
3. Join or leave communities, revisit your **Recent** communities, or create a **Custom feed** with a name, optional description, and any number of communities. Choose whether it appears in your profile preview. Private feeds stay in your own feed list.
4. Open **New post** and choose **Text and link**, **Media**, **Poll**, or **Repost**. Media accepts up to six PNG, JPEG, WebP, or GIF images (2 MiB each), or one MP4 or WebM video (16 MiB). Polls accept two to six choices and close after one, three, or seven days. To repost, select an original discussion and a different community, or start from **Repost** in the discussion’s menu. Choose a post flair from the destination community’s options when available. **Save draft** saves a text or link draft, and **Drafts** lets you resume or discard one. Publishing a saved draft removes that draft from the list. Use **Creator tools** to see local vote and reply totals.
5. Open **Your profile** to browse posts, comments, saved and hidden items, voting activity, and viewing history. In **Edit profile**, update your display name, bio, flair, avatar, and cover. Avatar and cover images accept PNG, JPEG, or WebP files up to 512 KiB each. Choose which posts and comments appear, add up to six HTTPS social links, and use **Share profile** to open your public profile.
6. Use **Feed controls** to show, collapse, or hide sensitive previews or mute a keyword. Under **Home feed**, clear **Recommend posts from communities you haven’t joined** and select **Save preferences** to make **For You** show only communities you’ve joined. The post menu also offers personal hiding, community muting, and account blocking.
7. Choose **Report post** or **Report comment** from its menu, select a reason, and optionally add details. Check progress in **Your reports**. Posts also have a separate **Report a legal concern** option that takes a country or region. In **Mod tools → Queue**, filter by content type and review status, then mark reports reviewed or reopen them. Reporting sends the item to review while preserving its current visibility. Use **Message moderators** to start a conversation in **Mod mail**. **Manage communities** opens owner settings, including topic guidelines.
8. Open **Community settings → Appearance** to replace or remove a published icon or banner. **Preview changes** shows the proposed artwork. Select **Save appearance** to publish it, or **Cancel** to return to the current appearance.
9. Open **Mod tools** from a community or **Manage communities** to see the tools available to your role. Owners assign, change, and revoke roles in **Moderator roles**. A **Moderator** can review reports, remove and restore content, manage highlights and AutoMod rules, answer mod mail, manage member approvals and restrictions, schedule posts, and decide appeals. An **Appeals reviewer** can handle the community’s appeals. Owners control community settings and role assignments. Use **Mod log** to see moderation and role changes.
10. In **Mod tools → Scheduled posts and events**, schedule a publication and edit or cancel it under **Upcoming**. An event includes a start time and an optional end time. Times default to Central time and support explicit UTC or Central offsets when needed. The server publishes due items while running and catches up on pending items after a restart.
11. In **Mod tools → Restricted Users**, apply a ban or mod mail mute by username, with an optional moderator-only reason. Set a duration in minutes, hours, or days, or leave it blank for a permanent restriction. Active restrictions show their expiry in Central time. Each timed ban or mute lifts independently at its deadline. Use **Lift ban** or **Lift mute** to end it early. A ban stops posting, editing, and replying in that community. A mute stops new mod mail there. Existing posts remain visible and legal reporting stays available.
12. In a discussion, sort comments by **Best**, **Top**, **New**, or **Old** and collapse individual reply branches. Open **Comment actions** on your own reply to edit it in place or delete it after confirming. The thread keeps replies beneath a deleted comment and hides deleted comments that have no remaining replies.
13. When a community bans you or mutes your mod mail, use **Your appeals** from the sidebar or restriction notification. Each active restriction can be appealed once, including restrictions from private communities. Moderators and Appeals reviewers use **Mod tools → Appeals** to approve or deny an appeal with a written decision. Approval lifts the specific ban or mute. Expiry or manual lift closes its pending appeal and adds a private moderation update.
14. In **Mod tools → Approved members**, enter a registered username and select **Approve member**. If the community is private or restricted, it appears under **Your communities** for that account. Approval grants member access independently of joining and moderator roles. Select **Revoke approval** next to the account to withdraw that access. Community bans and mod mail mutes continue to apply according to their own rules.
15. In **Mod tools → Posts & comments**, open an item, choose a community rule or enter a custom reason, and select **Remove content**. When you choose a rule, you can add an optional note to the author. The author receives the reason and any note privately in **Notifications**. A removed post and its attachments are hidden during removal. Replies below a removed comment stay in the thread. Open the **Removed** list and select **Restore content** to return the original item and notify its author. Both actions appear in **Mod log**.
16. In **Mod tools → AutoMod**, create or edit a rule for posts, comments, or both. Combine a case-insensitive keyword or phrase, an account-age limit, and one or more link domains. Choose **All conditions** to require every condition you enter, or **Any condition** to require at least one. New rules and rules saved before this setting default to **All conditions**. Within the domain condition, any listed host or its subdomain counts. Choose **Flag for review** or **Remove**, and enter a reason. Flagged items stay visible in **Queue → AutoMod flags**. Removed items appear under **Posts & comments → Removed**, and the author receives the reason privately. If multiple rules match, removal takes priority over flagging. Rule changes apply to future submissions and edits.

17. In **Mod tools → Discussions** or a post’s detail view under **Posts & comments**, select **Lock discussion** to stop new comments and replies. Readers can still browse the thread, and authors can manage their existing comments under the usual permissions. Select **Unlock discussion** to reopen it. Both changes appear in **Mod log**.
18. In **Mod tools → Posts & comments → Comments**, open a comment and select **Lock replies**. Existing replies remain visible, and people can still reply elsewhere in the discussion, including to an existing reply. Select **Unlock replies** to reopen direct replies to that comment. Both changes appear in **Mod log**.
19. In **Mod tools → Community Highlights**, choose a post and select **Pin post**. Use **Move up**, **Move down**, and **Unpin** to manage the list. New pins go at the end. The community feed shows pinned posts in that order above its sorted discussions. The previous single pin carries over, and changes appear in **Mod log**.
20. In **Mod tools → Rules**, enter a rule title and an optional description. Use **Edit rule**, **Move up**, and **Move down** to maintain the list, or **Delete rule** to remove a rule from the community page and new report forms. A submitted report keeps the version of the rule selected when it was filed.
21. When your post or comment is removed, select **Appeal removal** in the removal notification and explain why the team should reconsider. You can also start an appeal for an active removal from **Your appeals**. In **Mod tools → Appeals**, search by username or exact appeal ID, such as `#42`. Combine the search with **Appeal type** in **Pending** or **Resolved**, and with **Decision** in **Resolved**. **Closed** includes lifted or expired restrictions, content restored outside the appeal, and unavailable content. Reviewers can approve and restore the content or deny the appeal, with a written reason either way. The author receives a private update and can read the decision in **Your appeals**.
22. In **Mod tools → Post flair**, create, rename, or remove labels, and enable **Require post flair** to require a current label when publishing. Posts keep their original label after it is renamed or removed. Drafts can be saved before choosing a label. Pending scheduled items that need a different flair stay in **Upcoming**, where moderators can edit or cancel them.
23. In a community feed, open **Flair** next to the sort controls and choose a label. The filter applies to posts and Community Highlights. Select **Clear flair** to see all labels again while keeping your current sort and time range.
24. On the **Posts** or **Media** search tab, combine **Author**, **Community**, and **Post flair**. Enter a handle, a community address or name, and the full flair label. The fields also work on their own. Use double quotes around an exact phrase, or type filters directly, for example `author:u/irisarchive community:c/culture flair:Question "public archive"`. **Clear fields** removes the three field filters and keeps the search terms.
25. Choose **Compact** or **Card** next to the feed sort controls. Compact shows titles and post actions in a denser list. Open a discussion to see its full content. ThreadPorch saves the choice to your account when signed in and to your browser when browsing as a guest.
26. Select text in the post composer and use **Bold**, **Italic**, **Link**, **Bulleted list**, **Numbered list**, or **Block quote**. The toolbar inserts Markdown, which renders when you publish. For a link, replace the selected example URL with your own HTTP or HTTPS address. **Formatting help** shows the syntax for typing it yourself.
27. In **Text and link**, put the cursor where the image should appear. Choose a PNG, JPEG, WebP, or GIF, add a description for screen readers, and select **Insert image**. Move the image’s Markdown line to change its position. A post can have up to six inline images of 2 MiB each. Images saved to a draft carry over when you resume it.
28. In **Creator tools → Performance**, open **Insights** beside one of your posts. Switch between **Last 7 days** and **Last 30 days** to see discussion views, net vote changes, and comments added by UTC day. Tracking starts when the updated server first runs. Past activity is unavailable for earlier days.
29. In a discussion, open a comment’s **Comment actions → Share** and select **Copy link**. The link opens the discussion at that comment for anyone who already has access. You can also select the URL and copy it manually.
30. Open a comment’s **Comment actions → Save** to bookmark it. Find it under **Saved comments** in your profile’s **Saved** tab and open it to jump back to the discussion. Select **Unsave** in either place to remove the bookmark.
31. Enter a word or phrase in **Search comments** inside a discussion. Select a result to jump to that comment in the thread, or select **Clear** to remove the results.
32. While writing a comment or reply, open **Attach an image or GIF** and choose a PNG, JPEG, WebP, or GIF up to 2 MiB. You can add an image description for screen readers and post it with or without comment text. The attachment appears inline in the discussion.
33. Type `c/technology` to link to a community, or `u/username` to link to a profile. The shorthand becomes clickable in published post text and comments. You can also open or share the short paths `/c/technology` and `/u/username` directly.
34. In a comment or reply, select text and use **Bold**, **Italic**, **Link**, or **Code**. For a link, replace the selected example URL with your own HTTP or HTTPS address. **Formatting help** shows the Markdown syntax. Older comments keep their plain text until you choose Markdown or apply formatting when editing them.
35. Open another user’s profile and select **Follow**. Posts they show on their public profile appear in **Following** alongside posts from your joined communities. Choose **Unfollow** from their profile to remove them as a source. Your community memberships and feed preferences still apply.
36. In **Mod Tools → Scheduled posts and events**, choose **Post**, set a publish time, and select **Daily**, **Weekly**, or **Monthly** under **Repeat**. **Edit** changes the next post and future schedule. **Cancel future posts** ends the series and keeps its published entries in **History**.

Recurring posts follow the selected time zone. Monthly schedules use the last day of shorter months and return to the original day when possible. In America/Chicago, a skipped hour moves that occurrence one hour later, and a repeated hour uses the earlier time. If the server misses several runs, it publishes one overdue post and advances to the next future slot.

The America/Chicago conversion follows the U.S. daylight saving schedule. Choose an explicit offset for a time that occurs twice when clocks go back. [NIST daylight saving time reference](https://www.nist.gov/pml/time-and-frequency-division/popular-links/daylight-saving-time-dst)

Uploaded post and comment attachments are stored in `commonline.state.media` by default. The server appends `.media` to a custom state path. Move or back up the state file, accounts file, and media directory together. Keep those private local files out of source control.

The prototype serves accounts over a loopback connection. Passwords use salted Argon2id hashes, and the account file stores hashes of session tokens, recovery codes, and emailed codes. Authenticator keys are stored in the private local account file and rely on its filesystem permissions. A public deployment would require HTTPS and hosting controls, and age labels currently follow the viewer’s feed preferences.

## Feature inventory

The website target is **99 of 100 accepted workflows** on desktop and mobile web. The expanded benchmark lists each check, its current implementation status, and the evidence needed to count it. [Web parity benchmark](docs/web-parity.md)

| Area | Current prototype | Further work |
| --- | --- | --- |
| Discovery and feeds | For You, Following, Popular, fictional News, Explore, All discussions, community and custom feeds, Recent, site-wide and community search, Best/Hot/New/Top | Saved searches, richer ranking and filters |
| Posting and discussion | Title-only, text, link, image gallery and video posts, polls, cross-community reposts, community-defined post flair with a per-community requirement, post tags, multiple text/link drafts, post and comment editing and deletion, threaded replies, votes, save and hide | Richer editing and media transcoding |
| Creator tools | Dashboard, per-post vote and reply totals, daily post insights, community trends, and publishing links | Feed impressions and unique reach |
| Identity and communication | Account creation and deletion, verified recovery email, emailed password resets, one-time recovery codes, password changes, authenticator-app two-factor sign-in, active sessions with individual and bulk sign-out, editable public profiles, profile following, avatar and cover uploads, private activity and history, keyword mutes, account blocking, notifications with per-category preferences, chat requests with sender permissions and recipient approval, private messages with live conversation updates, and mod mail | Mention alerts |
| Community operations | Guided creation, topics and visibility, approved-member access, mature labels, artwork uploads and previews, authenticated owner settings, ordered Community Highlights, scoped moderator workspace and roles, post and comment removal and restoration with author notices, compound AutoMod rules, timed community bans and mod mail mutes, restriction appeals and decisions, post and comment reports with selectable community rules, regional legal reporting, filterable queues, saved responses, activity log, scheduled posts and events, and recurring posts | Granular moderator permissions and community wikis |

### Original core workflows

These 30 workflows defined the original 90% milestone. The expanded benchmark breaks them down and adds acceptance checks for the rest of the website scope. Track ThreadPorch’s regional legal review policy separately from feature parity.

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

This clean-room implementation uses Rust for the local HTTP server, form handling, HTML rendering, and file-backed state. Account security uses RustCrypto libraries and operating-system randomness through `getrandom`. The browser renders HTML and CSS, and the prototype bundles original artwork. The plaza image was created with ImageGen. Design references were reviewed through the Awwwards MCP. [197 Historias Ilustradas](https://www.awwwards.com/sites/197-historias-ilustradas) · [SANTAL](https://www.awwwards.com/sites/santal)

The built-in ImageGen prompt for `assets/plaza.jpg` was as follows.

> Photorealistic-natural. One original editorial photograph for a civic discussion forum post about who gets to use public space. Wide horizontal frame, 16:9 or wider. An overlooked terraced public plaza, low brutalist concrete civic buildings, clean diagonal concrete bleachers in the foreground, leafy trees with deep chartreuse late afternoon foliage and a few small distant people walking. Low warm sunlight from left, documentary 35mm film with a soft grain, rich olive greens and subdued warm gray concrete. Interesting modern architecture composition with a calm broad atrium or clear open space, public library or neighborhood arts district feel. Compose so a center crop to 3:1 still shows the curved/diagonal architecture, lush greenery, sky band and 2 or 3 tiny people in the middle distance. No signs, no obvious brands, no lettering, no overlay, no border.
