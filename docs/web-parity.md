# Commonline web parity benchmark

The target is **99 of 100 workflows verified**. This benchmark covers common signed-in experiences in a desktop or mobile browser, including publishing, account management, communication, and community moderation. It excludes native apps, paid products, advertising, business products, and Reddit’s internal systems. Commonline can meet a workflow with its own interface. Regional legal review policy is tracked separately.

This extends the [original 30-workflow benchmark](../README.md#original-core-workflows). Its denominator stays fixed at 100. Changing a workflow or its acceptance criteria requires revisiting both the baseline and the target percentage.

## Acceptance and baseline

Each row is worth one point. Count it once the expected result passes end to end on the same Commonline revision in desktop and mobile browsers. The baseline uses Chromium at widths of 1440 and 390 CSS pixels. Use separate signed-in browser profiles when another user’s access, role, or account state matters. For a stateful workflow, restart the server with the same storage and check the outcome again. Test a denied action as well as a permitted one when permissions or visibility are involved. Record the revision, browser and width, observed result, and any restart or account checks per row.

**Implemented** means the published baseline has the main workflow at the scope stated in the row. **Partial** means a smaller workflow exists. **Missing** means the required workflow is absent. These describe the code. A check in the verified subset records a completed test. A source review, a unit test, or a pass on only one viewport does not earn a point on its own.

The starting revision, `c748fcb`, had **53 implemented, 23 partial, and 24 missing** workflows, with **5 verified** in the baseline acceptance record. The tables preserve those baseline classifications. The release update below records changes to both implementation and verification.

### Feeds and discovery

| ID | Workflow to verify | Baseline |
| --- | --- | --- |
| 001 | Open Home and see a personalized mix of joined communities and public discussions. | Implemented. Membership and feed settings influence a local ranking. |
| 002 | Switch to Following and see posts only from communities that account joined. | Implemented. |
| 003 | Browse an All feed spanning public communities while respecting personal filters. | Implemented. |
| 004 | Open a community’s feed and browse only discussions from that community. | Implemented. |
| 005 | Sort by Top, switch time windows, and see vote-ranked posts within the selected window. | Partial. Top uses votes with a single all-time pool. |
| 006 | Sort by Hot and see recency and engagement affect rank as posts age. | Partial. Hot uses stored age labels that do not advance. |
| 007 | Sort by New and see newly published posts ahead of older posts. | Implemented. |
| 008 | Open Popular and see a public feed ordered by aggregate votes. | Implemented. |
| 009 | Search within a community and verify a matching post in another community stays outside the results. | Implemented. |
| 010 | Search site-wide for a term and open a matching public post. | Implemented. Basic substring matching. |
| 011 | Find public communities in the directory with a name or topic filter. | Implemented. |
| 012 | Switch between Posts and Media results, keeping the same query and restricting Media to posts with media. | Partial. Post search exists as the default, without result-type navigation. |
| 013 | Search comment text site-wide and open a matching comment in its discussion. | Missing. |
| 014 | Select Communities in the global search and open a matching community. | Partial. Community discovery has a separate directory search. |
| 015 | Search for a person by handle or display name and open the matching public profile. | Missing. |
| 016 | Narrow post search by structured fields such as author, community, flair, or quoted phrase. | Missing. |
| 017 | Create and edit a custom feed with posts from the selected communities. | Implemented. |
| 018 | Share a custom feed publicly or keep it private, then check what a second user can open. | Implemented. |
| 019 | Join, leave, and rejoin a community, then check the account’s membership and Following feed. | Implemented. |
| 020 | Mute and unmute a community in broad feeds while keeping its direct page available. | Implemented. |
| 021 | Revisit recently opened communities from account-scoped navigation. | Implemented. |
| 022 | Choose how sensitive posts appear in personal feeds and see previews follow that preference. | Implemented. This is a display preference. |
| 023 | Switch between compact and card-style feed layouts and retain the choice. | Missing. |
| 024 | Turn off recommendations from unjoined communities on Home and retain the setting. | Missing. Following offers a separate joined-only feed. |

### Publishing and creator tools

| ID | Workflow to verify | Baseline |
| --- | --- | --- |
| 025 | Publish a text post from the composer and open its discussion in the chosen community. | Implemented. |
| 026 | Publish a link post and open the correct external destination from its card. | Implemented. |
| 027 | Publish a multi-image gallery and open each attached image in the discussion. | Implemented. Up to six supported images. |
| 028 | Upload a supported native video and play or seek through it in the discussion. | Implemented. MP4 and WebM with file limits. |
| 029 | Publish a poll, vote from another account, and see persisted results and deadline behavior. | Implemented. |
| 030 | Crosspost into another community and open the original when it is still available. | Implemented. |
| 031 | Edit an owned post through the normal composer and retain the same discussion. | Implemented. |
| 032 | Delete an owned post and verify the old URL and attachments are no longer publicly available. | Implemented. |
| 033 | Save, select, resume, or discard multiple unpublished drafts without losing other drafts. | Partial. One resumable text or link draft per account. |
| 034 | Format post text with supported lists, links, quotes, and emphasis, then read it as published. | Partial. Rendering supports a small inline formatting set. |
| 035 | Apply spoiler and mature-content labels to a post and see the corresponding preview treatment. | Implemented. |
| 036 | Apply a brand-affiliate disclosure and see the label on the published post. | Implemented. |
| 037 | Select a community-defined post flair and have any required flair enforced when publishing. | Partial. Authors can enter free-text post flair. |
| 038 | Insert an image inline in the body of a text post and preserve its placement after publishing. | Missing. Image attachments use the media post flow. |
| 039 | Open a post’s creator insights and see real views and engagement measurements over time. | Partial. Creator pages show local vote and reply totals. |

### Discussion and voting

| ID | Workflow to verify | Baseline |
| --- | --- | --- |
| 040 | Add a top-level comment to a discussion from a second account. | Implemented. |
| 041 | Reply to a comment and open the nested reply in the right branch. | Implemented. |
| 042 | Edit an owned comment in the same thread and see an edited indicator. | Implemented. |
| 043 | Delete an owned comment and verify surviving replies remain readable. | Implemented. |
| 044 | Switch between discussion-level comment sorts and see sibling order change without flattening threads. | Partial. Siblings are ordered by vote score with no user-facing sort control. |
| 045 | Collapse and re-expand a branch without hiding unrelated replies. | Missing. |
| 046 | Copy a direct link to a comment and have another account open it in context. | Partial. Comment anchors exist, but the comment menu lacks a share action. |
| 047 | Upvote, downvote, and clear a vote on an accessible comment while preserving other accounts’ votes. | Implemented. |
| 048 | Upvote, downvote, and clear a vote on a post while preserving other accounts’ votes. | Implemented. |
| 049 | Save a comment and revisit it from the account’s saved items. | Missing. |
| 050 | Find comments matching a phrase within the currently open discussion. | Missing. |
| 051 | Attach a permitted image or GIF to a comment and view it in the thread. | Missing. |
| 052 | Format a comment with links, code, or emphasis and see the formatting rendered. | Missing. Comments are escaped plain text. |

### Accounts, engagement, and communication

| ID | Workflow to verify | Baseline |
| --- | --- | --- |
| 053 | Create an independent account with a username and password, then open its profile. | Implemented. |
| 054 | Sign in and out from a browser and verify protected pages require sign-in afterward. | Implemented. |
| 055 | Add or change an account email and verify ownership of the new address. | Missing. |
| 056 | Request a password reset through a verified mailbox while signed out, then verify prior sessions end. | Partial. A saved one-time code can reset the password and revoke sessions. Email reset is absent. |
| 057 | Change the current password from account settings and verify subsequent sign-in uses it. | Missing. One-time recovery is a separate flow. |
| 058 | Enable a second authentication factor, then complete the extra challenge when signing in on desktop and mobile. | Missing. |
| 059 | Inspect authenticated sessions and revoke a different browser session from account settings. | Missing. Reset and sign-out can revoke sessions, but the user has no session list. |
| 060 | Start account deletion with confirmation and verify the deleted account can no longer sign in. | Missing. |
| 061 | Edit the public profile’s display name, bio, and social links and inspect them from another account. | Implemented. |
| 062 | Upload a profile avatar and cover and verify the public profile uses the saved assets. | Implemented. |
| 063 | Control which profile activity a second account sees while retaining access to one’s own activity. | Implemented. |
| 064 | Follow or unfollow another profile and discover their eligible activity through Following. | Missing. |
| 065 | Block or unblock another account and verify the feed and message restrictions update. | Implemented. |
| 066 | Save a post and retrieve it from the account’s saved list without altering another account’s list. | Implemented. |
| 067 | Hide or unhide a post and verify only that account’s feed changes. | Implemented. |
| 068 | Open a discussion and revisit it later through account-scoped browsing history. | Implemented. |
| 069 | Receive account notifications for replies and mentions and open the relevant content. | Partial. Reply notifications exist. Mentions are absent. |
| 070 | See an unread notification count and mark the account’s notifications read. | Implemented. |
| 071 | Exchange private messages with another account and receive new messages in an open conversation. | Partial. Persistent inbox and unread states exist. Open conversations do not refresh live. |
| 072 | Control who may send chat requests and accept or decline a request before a conversation starts. | Missing. |
| 073 | Change notification categories or delivery preferences and verify later alerts follow the setting. | Missing. |

### Community and moderation

| ID | Workflow to verify | Baseline |
| --- | --- | --- |
| 074 | Create a community with a topic and public, restricted, or private access. | Implemented. |
| 075 | Update community settings and verify that the public description and access behavior follow the saved choices. | Implemented. |
| 076 | Preview, upload, replace, or remove the community icon and banner from authorized settings. | Implemented. |
| 077 | Approve and revoke a member, then check restricted posting and private visibility from that account. | Implemented. |
| 078 | Delegate a moderator role with specific tool permissions and revoke it without changing ownership. | Partial. Fixed Moderator and Appeals Reviewer roles exist. Individual permissions and invitation acceptance are absent. |
| 079 | Apply a permanent or timed community ban and verify lifting or expiry restores eligible access. | Partial. Bans and manual lifts exist. Expiry is absent. |
| 080 | Apply a timed mod mail mute and verify it can expire or be lifted separately from a ban. | Partial. Mutes can be lifted manually. Expiry is absent. |
| 081 | Publish individual community rules and make them selectable reasons when reporting content. | Partial. Topic guidelines are editable. Individual reportable rules are absent. |
| 082 | Report an individual post or comment with a reason and observe the submitted report’s status. | Partial. Current intake is regional legal reporting for posts. |
| 083 | Triage human and automated reports in moderation queues with relevant content-type and status filters. | Partial. Legal review status and AutoMod flags exist. Broader queue categories and filters are absent. |
| 084 | Remove and restore a post without losing its discussion, attachments, or votes. | Implemented. |
| 085 | Remove and restore a comment while keeping eligible descendants readable. | Implemented. |
| 086 | Give an author a private removal reason and notify the author when content is restored. | Implemented. |
| 087 | Submit a community restriction appeal and receive a written decision with the appropriate access change. | Implemented. Commonline uses a dedicated appeal form. |
| 088 | Lock and unlock a discussion and verify ordinary users cannot reply while it is locked. | Missing. |
| 089 | Highlight and reorder multiple discussions on the community’s main page. | Partial. Owners can pin one discussion. |
| 090 | Exchange multiple messages in a moderator mailbox thread and archive the conversation when resolved. | Partial. Mod mail supports an incoming message and one moderator reply. |
| 091 | Open the moderation log and trace when a moderator changed a role or removed content. | Implemented. |
| 092 | Schedule a one-off post or event, edit or cancel pending work, and publish the remaining item once. | Implemented. |
| 093 | Schedule a recurring post, edit its cadence, and stop future occurrences. | Missing. |
| 094 | Enable or pause an AutoMod rule that flags or removes matching posts and comments. | Implemented. Literal phrases, flag review, and reversible removal. |
| 095 | Apply an automated rule that combines content checks with author, domain, or community thresholds. | Missing. |
| 096 | Create, edit, and publish a public community wiki page with editor permissions. | Missing. |

### Accessibility and interface preferences

| ID | Workflow to verify | Baseline |
| --- | --- | --- |
| 097 | Navigate the feed, composer, discussion, inbox, and moderation workspace on mobile without horizontal overflow. | Implemented. Responsive layouts exist. Full representative-route verification is pending. |
| 098 | Complete navigation, publishing, and discussion actions with only a keyboard and visible focus. | Partial. Native controls and focus styling exist. The complete keyboard paths are unverified. |
| 099 | Read and operate core pages with a screen reader, including status messages and alternatives for user media. | Partial. Landmarks and control labels exist. User-supplied media alternatives and full assisted checks are absent. |
| 100 | Switch between light and dark appearance and retain the preference on another visit. | Missing. |

## Verified on the published baseline

| Workflow IDs | What passed at 1440 and 390 CSS pixels |
| --- | --- |
| 002, 019 | Two independent accounts joined different communities, saw the correct posts under Following, left, and rejoined. Membership and each Following feed were checked after a server restart. |
| 053, 054 | Each browser registered its own credentials through the normal account form, opened its own profile, signed out, encountered the sign-in gate, and signed in again. Both accounts signed in successfully after restart. |
| 066 | The two browsers saved different posts through their discussion pages and opened distinct saved lists. The lists stayed distinct and retained the posts after restart. |

Existing browser smoke suites cover other parts of account recovery, profile isolation, moderator roles, approved members, appeals, content removal, and AutoMod. Release and model tests also cover persistence for individual operations. These results are useful regression evidence. They earn a benchmark point only once the full row is checked and recorded at both browser sizes with the relevant account and restart checks.

## Search, feed ranking, and discussion update

This release brings the implementation count to **61 implemented, 18 partial, and 21 missing**. Eight checks moved to Implemented.

| Workflow IDs | Implemented in this release |
| --- | --- |
| 005, 006 | Top has time windows. Hot uses elapsed time from stored publication timestamps, with engagement and recency both affecting order. New posts store their actual publication time. Older state files estimate publication time from the stored age and the snapshot's modification time, then preserve that estimate. |
| 012–015 | Search tabs keep the query across result types. Media keeps feed filtering, and comments, communities, and people results link to the corresponding discussion, community, or public profile. |
| 044, 045 | Discussions offer Best, Top, New, and Old sorts and independently collapsible reply branches. Sort changes preserve the threaded hierarchy. |

**16 of 100 workflows passed the release acceptance checks.** The following checks ran on the release build in Chromium at 1440 and 390 CSS pixels.

| Workflow IDs | What passed |
| --- | --- |
| 002–004, 019 | Separate accounts joined and left communities, saw their own Following and All results, and could open each community directly with the appropriate scope. Membership, muting, and feed visibility were rechecked after restarting the server. |
| 005–007 | Native controls switched between Hot, New, and Top. A recent post outranked an older post on Hot, New showed chronological order, and Top applied the day and week limits before ranking by votes. Publication timestamps survived a restart. |
| 012–015 | Native tabs kept the query when switching between Posts and Media. Comment results opened the exact reply, community results opened the matching community, and People found profiles by handle and display name. The searches ran after restarting with the same data. Separate visibility checks covered private, muted, blocked, and removed content. |
| 044, 045 | All four sorts changed sibling order while preserving the reply tree. Collapsing or expanding one branch left unrelated replies visible. Posting under the chosen sort kept it selected and anchored the new reply. |
| 053, 054, 066 | Distinct accounts registered, signed out and back in, and retrieved different saved posts. Sign-in and both saved lists were checked after a restart. |

## Account security update

This release brings the implementation count to **65 implemented, 17 partial, and 18 missing**.

| Workflow ID | Implemented in this release |
| --- | --- |
| 055 | Add or change a recovery email in Security and prove ownership with a one-time emailed code. The previously verified address remains usable while verification is pending. |
| 056 | Request an emailed reset code using the username or verified address. Completing the reset invalidates other sessions and requires the authenticator code when enabled. |
| 057 | Change the password with the current password and any enrolled authenticator. The change issues a new recovery code and invalidates other sessions. |
| 058 | Enroll a time-based authenticator and complete the second sign-in step. Previously accepted codes cannot be replayed after a restart. |

Independent Chromium profiles at 1440 and 390 CSS pixels each verified an address, reset a password by email, changed the password in settings, and enrolled and used an authenticator. Both profiles rejected old passwords and invalid authenticator codes. Verified addresses and enrolled authenticators persisted after restarting the server. An isolated SMTP test confirmed that an emailed code worked after restart, failed on reuse, and ended prior sessions when redeemed. It also checked that resetting an enrolled account requires an authenticator code. Rust model tests cover replacing a verified address and expiring email codes.

## Session management update

Workflow **059** is implemented. Account Security shows active sessions, marks the current browser, and supports signing out one session or every other session. Older sessions remain manageable. This brings the implementation count to **66 implemented, 17 partial, and 17 missing**.

Workflow **059** passed at 1440 and 390 CSS pixels in independent Chromium profiles. Individual sign-out ended the selected session, and bulk sign-out preserved the current browser and a separate account. Revoked sessions stayed signed out after restarting the server. A separate upgrade check verified that sessions from the prior release stayed manageable.

## Remaining work

1. Expand the release acceptance record. Run the remaining implemented workflows at both sizes against one revision and capture the state and access checks for each ID.
2. Add account deletion, notification preferences, and chat request controls.
3. Improve discovery and publishing. Add structured search filters, extend post drafts, add community-defined flair, and expand sharing and discussion controls. Verify that private and removed content stays unavailable through search, attachments, profiles, and links.
4. Expand community operations. Add structured rules and reporting, granular role permissions, timed restrictions, discussion locking, recurring schedules, and richer rule conditions. Preserve moderation decisions and access state across restarts.
5. Complete accessibility and interface preferences. Run keyboard and screen-reader checks, add user-media alternatives, and verify the theme choice and mobile layout alongside the main workflows.

## Reddit Help references

- Search types, filters, and sorts. [How Reddit search works](https://support.reddithelp.com/hc/en-us/articles/19695647891988-How-does-Reddit-search-work) · [Search filters and sorts](https://support.reddithelp.com/hc/en-us/articles/19695706914196-What-filters-and-sorts-are-available)
- Publishing, comments, and drafts. [Posting and commenting](https://support.reddithelp.com/hc/en-us/articles/360060422572-How-do-I-post-and-comment-on-Reddit) · [Draft management](https://support.reddithelp.com/hc/en-us/articles/360043483111-How-do-I-delete-a-draft-of-a-post)
- Account controls. [Email verification](https://support.reddithelp.com/hc/en-us/articles/360043047552-Why-should-I-verify-my-Reddit-account-with-an-email-address) · [Password reset](https://support.reddithelp.com/hc/en-us/articles/205240005-How-do-I-log-in-to-Reddit-if-I-forgot-my-password) · [Two-factor authentication](https://support.reddithelp.com/hc/en-us/articles/360043470031-What-is-two-factor-authentication-and-how-do-I-set-it-up) · [Account deletion](https://support.reddithelp.com/hc/en-us/articles/204579509-How-do-I-delete-my-account)
- Community tooling and permission scope. [Moderation tools overview](https://support.reddithelp.com/hc/en-us/articles/15484384020756-Moderation-Tools-overview) · [Moderator permissions](https://support.reddithelp.com/hc/en-us/articles/15484498369428-User-Management-moderators-and-permissions) · [Moderation queue](https://support.reddithelp.com/hc/en-us/articles/15484440494356-Moderation-Queue)
- Restrictions and moderator conversations. [Bans, mutes, and appeals](https://support.reddithelp.com/hc/en-us/articles/15484464549524-User-Management-banning-and-muting) · [Mod mail folders](https://support.reddithelp.com/hc/en-us/articles/15484158762260-Mod-mail-folders)
- Recurrence, highlights, and automation. [Scheduled and recurring posts](https://support.reddithelp.com/hc/en-us/articles/15484443169556-Scheduled-and-Recurring-Posts) · [Community highlights](https://support.reddithelp.com/hc/en-us/articles/15484641176724-Community-highlights-sticky-posts) · [Post and comment guidance](https://support.reddithelp.com/hc/en-us/articles/17625458521748-Automations-Post-Comment-Guidance-Set-Up)
