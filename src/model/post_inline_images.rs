use std::io::{self, Write};
use std::path::Path;

use pulldown_cmark::{Event, Parser, Tag};

use super::{canonical_u64, checked_handle, unix_seconds, PostMedia, State};

const INLINE_IMAGE_PREFIX: &str = "inline-image:";
const IMAGES_PER_POST: usize = 6;
const STAGED_UPLOAD_LIMIT: usize = 24;
const STAGED_UPLOAD_LIFETIME: u64 = 24 * 60 * 60;
const IMAGE_UNAVAILABLE: &str =
    "An inline image is unavailable. Remove its marker or upload the image again.";

pub fn inline_image_key(destination: &str) -> Option<&str> {
    let key = destination.strip_prefix(INLINE_IMAGE_PREFIX)?;
    valid_image_key(key).then_some(key)
}

fn valid_image_key(key: &str) -> bool {
    key.len() == 48
        && key
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn valid_image_type(content_type: &str) -> bool {
    matches!(
        content_type,
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    )
}

#[derive(Clone, Copy)]
pub(super) enum InlineTarget {
    Draft(u64),
    Post(u64),
}

#[derive(Clone, Eq, PartialEq)]
enum Placement {
    Staged { owner: String, expires: u64 },
    Draft { owner: String, id: u64 },
    Post(u64),
}

impl Placement {
    fn target(target: InlineTarget, owner: &str) -> Self {
        match target {
            InlineTarget::Draft(id) => Self::Draft {
                owner: owner.to_owned(),
                id,
            },
            InlineTarget::Post(id) => Self::Post(id),
        }
    }

    fn private_owner(&self) -> Option<&str> {
        match self {
            Self::Staged { owner, .. } | Self::Draft { owner, .. } => Some(owner),
            Self::Post(_) => None,
        }
    }

    fn expired(&self, now: u64) -> bool {
        matches!(self, Self::Staged { expires, .. } if *expires <= now)
    }
}

#[derive(Clone)]
pub(super) struct InlineImage {
    pub(super) asset: PostMedia,
    placement: Placement,
}

impl State {
    pub fn stage_inline_image(&mut self, asset: PostMedia, path: &Path) -> Result<(), String> {
        let owner =
            checked_handle(&self.username).map_err(|_| "Sign in to upload an inline image.")?;
        if !valid_image_key(&asset.key) || !valid_image_type(&asset.content_type) {
            return Err("Choose a PNG, JPEG, WebP, or GIF image.".into());
        }

        let mut updated = self.clone();
        let now = unix_seconds();
        updated.remove_expired_inline_uploads(now);
        let pending = updated
            .inline_images
            .values()
            .filter(|item| matches!(&item.placement, Placement::Staged { owner: uploader, .. } if uploader == &owner))
            .count();
        if pending >= STAGED_UPLOAD_LIMIT {
            return Err("Save or publish your current uploads before adding more images.".into());
        }
        let placement = Placement::Staged {
            owner,
            expires: now.saturating_add(STAGED_UPLOAD_LIFETIME),
        };
        if updated
            .inline_images
            .insert(asset.key.clone(), InlineImage { asset, placement })
            .is_some()
        {
            return Err("Could not register this image upload.".into());
        }
        self.commit(updated, path)
    }

    pub fn expire_inline_uploads(&mut self, path: &Path, now: u64) -> Result<(), String> {
        if !self
            .inline_images
            .values()
            .any(|image| image.placement.expired(now))
        {
            return Ok(());
        }
        let mut updated = self.clone();
        updated.remove_expired_inline_uploads(now);
        self.commit(updated, path)
    }

    fn remove_expired_inline_uploads(&mut self, now: u64) {
        self.inline_images
            .retain(|_, image| !image.placement.expired(now));
    }

    pub fn inline_upload_preview(&self, key: &str) -> Option<&PostMedia> {
        let image = self.inline_images.get(key)?;
        (image.placement.private_owner() == Some(self.username.as_str())
            && !image.placement.expired(unix_seconds()))
        .then_some(&image.asset)
    }

    pub fn post_inline_image(&self, post_id: u64, key: &str) -> Option<&PostMedia> {
        let image = self.inline_images.get(key)?;
        (image.placement == Placement::Post(post_id)).then_some(&image.asset)
    }

    pub fn post_has_inline_images(&self, post_id: u64) -> bool {
        self.inline_images
            .values()
            .any(|image| image.placement == Placement::Post(post_id))
    }

    pub(super) fn place_inline_images(
        &mut self,
        body: &str,
        target: InlineTarget,
        source: Option<InlineTarget>,
    ) -> Result<(), String> {
        let target = Placement::target(target, &self.username);
        let source = source.map(|target| Placement::target(target, &self.username));
        let now = unix_seconds();
        let mut keys = Vec::new();
        if body.contains("![") {
            for event in Parser::new(body) {
                let Event::Start(Tag::Image { dest_url, .. }) = event else {
                    continue;
                };
                if !dest_url.starts_with(INLINE_IMAGE_PREFIX) {
                    continue;
                }
                if keys.len() == IMAGES_PER_POST {
                    return Err("Use up to six inline images per post.".into());
                }
                let key = inline_image_key(&dest_url).ok_or(IMAGE_UNAVAILABLE)?;
                let image = self.inline_images.get(key).ok_or(IMAGE_UNAVAILABLE)?;
                let staged = matches!(&image.placement, Placement::Staged { owner, .. } if owner == &self.username)
                    && !image.placement.expired(now);
                if !staged && source.as_ref() != Some(&image.placement) {
                    return Err(IMAGE_UNAVAILABLE.into());
                }
                keys.push(key.to_owned());
            }
        }
        for key in &keys {
            if let Some(image) = self.inline_images.get_mut(key) {
                image.placement = target.clone();
            }
        }
        if let Some(source) = source {
            self.inline_images
                .retain(|key, image| image.placement != source || keys.contains(key));
        }
        Ok(())
    }

    pub(super) fn remove_inline_images(&mut self, target: InlineTarget) {
        let placement = Placement::target(target, &self.username);
        self.inline_images
            .retain(|_, image| image.placement != placement);
    }

    pub(super) fn remove_private_inline_images(&mut self, handle: &str) {
        self.inline_images
            .retain(|_, image| image.placement.private_owner() != Some(handle));
    }

    pub(super) fn read_inline_image(
        &mut self,
        key: &str,
        content_type: &str,
        parts: &[&str],
    ) -> Option<()> {
        if !valid_image_key(key)
            || !valid_image_type(content_type)
            || self.inline_images.contains_key(key)
        {
            return None;
        }
        let placement = match parts {
            ["staged", owner, expires] if checked_handle(owner).ok()?.as_str() == *owner => {
                Placement::Staged {
                    owner: (*owner).to_owned(),
                    expires: canonical_u64(expires)?,
                }
            }
            ["draft", owner, id] if checked_handle(owner).ok()?.as_str() == *owner => {
                Placement::Draft {
                    owner: (*owner).to_owned(),
                    id: canonical_u64(id)?,
                }
            }
            ["post", id] => {
                let id = canonical_u64(id)?;
                if !self.posts.iter().any(|post| post.id == id) {
                    return None;
                }
                Placement::Post(id)
            }
            _ => return None,
        };
        self.inline_images.insert(
            key.to_owned(),
            InlineImage {
                asset: PostMedia {
                    key: key.to_owned(),
                    content_type: content_type.to_owned(),
                },
                placement,
            },
        );
        Some(())
    }

    pub(super) fn write_inline_images(&self, file: &mut impl Write) -> io::Result<()> {
        for image in self.inline_images.values() {
            write!(
                file,
                "inline-image\t{}\t{}\t",
                image.asset.key, image.asset.content_type
            )?;
            match &image.placement {
                Placement::Staged { owner, expires } => {
                    writeln!(file, "staged\t{owner}\t{expires}")?
                }
                Placement::Draft { owner, id } => writeln!(file, "draft\t{owner}\t{id}")?,
                Placement::Post(id) => writeln!(file, "post\t{id}")?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    const AUTHOR: &str = "inline_author";
    const OTHER: &str = "inline_other";

    struct Fixture {
        directory: PathBuf,
        state: PathBuf,
        media: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
            let directory = std::env::temp_dir().join(format!(
                "commonline-inline-images-{}-{}",
                std::process::id(),
                now.as_nanos()
            ));
            let state = directory.join("board.state");
            let media = directory.join("board.state.media");
            fs::create_dir_all(&media).unwrap();
            Self {
                directory,
                state,
                media,
            }
        }

        fn stage(&self, state: &mut State, digit: char) -> String {
            let key = digit.to_string().repeat(48);
            fs::write(self.media.join(&key), b"inline image fixture").unwrap();
            state
                .stage_inline_image(
                    PostMedia {
                        key: key.clone(),
                        content_type: "image/gif".into(),
                    },
                    &self.state,
                )
                .unwrap();
            key
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn form(body: &str) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("community".into(), "open-debate".into()),
            ("title".into(), "Inline image discussion".into()),
            ("body".into(), body.into()),
        ])
    }

    fn marker(key: &str) -> String {
        format!("![A sample image](inline-image:{key})")
    }

    #[test]
    fn inline_images_stay_scoped_to_their_draft_and_published_post() {
        let fixture = Fixture::new();
        let mut state = State::seeded();
        state.activate_profile(Some(AUTHOR));
        let first = fixture.stage(&mut state, 'a');
        let second = fixture.stage(&mut state, 'b');
        let body = format!(
            "Before\n\n{}\n\nAfter\n\n{}",
            marker(&first).replace("inline-image:", r"inline\-image:"),
            marker(&second)
        );
        let mut submission = form(&body);

        state.activate_profile(Some(OTHER));
        assert!(state.inline_upload_preview(&first).is_none());
        assert!(state
            .apply("save-draft", &submission, &fixture.state)
            .is_err());
        assert!(state.apply("create", &submission, &fixture.state).is_err());
        // Code containing the marker is text, even when the asset is private.
        assert!(state
            .apply(
                "create",
                &form(&format!("\x60{}\x60", marker(&first))),
                &fixture.state
            )
            .is_ok());
        assert!(!state.post_has_inline_images(state.posts.last().unwrap().id));

        state.activate_profile(Some(AUTHOR));
        assert!(state
            .apply(
                "save-draft",
                &form(&marker(&first).repeat(7)),
                &fixture.state
            )
            .is_err());
        state
            .apply("save-draft", &submission, &fixture.state)
            .unwrap();
        let draft = state.drafts.last().unwrap().id;
        let mut state = State::load(&fixture.state).unwrap();
        state.activate_profile(Some(AUTHOR));
        assert!(state.inline_upload_preview(&first).is_some());
        assert_eq!(state.draft(draft).unwrap().body, body);
        assert!(state.apply("create", &submission, &fixture.state).is_err());

        submission.insert("draft_id".into(), draft.to_string());
        state.apply("create", &submission, &fixture.state).unwrap();
        let post = state.posts.last().unwrap().id;
        assert!(state.draft(draft).is_none());
        assert!(state.inline_upload_preview(&first).is_none());
        assert!(state.post_has_inline_images(post));
        assert!(state.post_inline_image(post, &first).is_some());
        assert!(state.post_inline_image(post - 1, &first).is_none());
        assert!(state
            .apply("create", &form(&marker(&first)), &fixture.state)
            .is_err());

        let mut edit = form(&marker(&second));
        edit.insert("id".into(), post.to_string());
        let unusable_path = fixture.directory.join("existing-directory");
        fs::create_dir(&unusable_path).unwrap();
        assert!(state.apply("edit", &edit, &unusable_path).is_err());
        assert!(state.post_inline_image(post, &first).is_some());
        assert!(fixture.media.join(&first).is_file());
        state.apply("edit", &edit, &fixture.state).unwrap();
        assert!(state.post_inline_image(post, &first).is_none());
        assert!(!fixture.media.join(&first).exists());
        assert!(fixture.media.join(&second).is_file());
        state.apply("delete", &edit, &fixture.state).unwrap();
        assert!(!state.post_has_inline_images(post));
        assert!(!fixture.media.join(&second).exists());
    }

    #[test]
    fn expiry_and_account_deletion_remove_private_uploads_and_keep_published_images() {
        let fixture = Fixture::new();
        let mut state = State::seeded();
        state.activate_profile(Some(AUTHOR));
        let expiring = fixture.stage(&mut state, 'c');
        state
            .expire_inline_uploads(
                &fixture.state,
                unix_seconds().saturating_add(STAGED_UPLOAD_LIFETIME),
            )
            .unwrap();
        assert!(state.inline_upload_preview(&expiring).is_none());
        assert!(!fixture.media.join(&expiring).exists());

        let discarded = fixture.stage(&mut state, 'd');
        state
            .apply("save-draft", &form(&marker(&discarded)), &fixture.state)
            .unwrap();
        let draft = state.drafts.last().unwrap().id;
        state
            .apply(
                "delete-draft",
                &BTreeMap::from([("draft_id".into(), draft.to_string())]),
                &fixture.state,
            )
            .unwrap();
        assert!(!fixture.media.join(&discarded).exists());

        let published = fixture.stage(&mut state, 'e');
        state
            .apply("create", &form(&marker(&published)), &fixture.state)
            .unwrap();
        let post = state.posts.last().unwrap().id;
        let drafted = fixture.stage(&mut state, '1');
        state
            .apply("save-draft", &form(&marker(&drafted)), &fixture.state)
            .unwrap();
        let staged = fixture.stage(&mut state, '2');
        state
            .delete_account_profiles(&[AUTHOR.into()], &fixture.state)
            .unwrap();

        let state = State::load(&fixture.state).unwrap();
        assert!(state.post_inline_image(post, &published).is_some());
        assert!(fixture.media.join(&published).is_file());
        assert!(!fixture.media.join(&staged).exists());
        assert!(!fixture.media.join(&drafted).exists());
    }
}
