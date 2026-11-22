use std::collections::BTreeMap;

use super::{bounded, decode, value, PostMedia};

pub const MEDIA_DESCRIPTION_LIMIT: usize = 240;
pub const MEDIA_TRANSCRIPT_LIMIT: usize = 10_000;

#[derive(Clone, Default)]
pub struct MediaAlternatives {
    pub descriptions: Vec<String>,
    pub transcript: String,
}

impl MediaAlternatives {
    pub(super) fn from_form(
        form: &BTreeMap<String, String>,
        media: &[PostMedia],
    ) -> Result<Self, String> {
        let mut descriptions: Vec<_> = value(form, "media_descriptions")
            .lines()
            .map(|line| line.trim().to_owned())
            .collect();
        while descriptions.last().is_some_and(String::is_empty) {
            descriptions.pop();
        }
        let transcript = bounded(
            form,
            "media_transcript",
            "Video transcript",
            MEDIA_TRANSCRIPT_LIMIT,
        )?;
        let alternatives = Self {
            descriptions,
            transcript: transcript.to_owned(),
        };
        alternatives.validate(media)?;
        Ok(alternatives)
    }

    pub(super) fn from_record(
        transcript: &str,
        descriptions: &[&str],
        media: &[PostMedia],
    ) -> Option<Self> {
        let alternatives = Self {
            descriptions: descriptions
                .iter()
                .map(|description| decode(description))
                .collect::<Option<Vec<_>>>()?,
            transcript: decode(transcript)?,
        };
        alternatives.validate(media).ok()?;
        Some(alternatives)
    }

    fn validate(&self, media: &[PostMedia]) -> Result<(), String> {
        if self.descriptions.len() > media.len() {
            return Err("Enter at most one description per attached file.".into());
        }
        if self.descriptions.iter().any(|description| {
            description.chars().count() > MEDIA_DESCRIPTION_LIMIT
                || description.chars().any(char::is_control)
        }) {
            return Err(format!(
                "Each attachment description must be on one line and at most {MEDIA_DESCRIPTION_LIMIT} characters."
            ));
        }
        if self.transcript.chars().count() > MEDIA_TRANSCRIPT_LIMIT {
            return Err(format!(
                "The video transcript must be at most {MEDIA_TRANSCRIPT_LIMIT} characters."
            ));
        }
        if !self.transcript.is_empty()
            && !media
                .first()
                .is_some_and(|asset| asset.content_type.starts_with("video/"))
        {
            return Err("Add a video to include a video transcript.".into());
        }
        Ok(())
    }

    pub fn description(&self, index: usize) -> Option<&str> {
        self.descriptions
            .get(index)
            .filter(|description| !description.is_empty())
            .map(String::as_str)
    }
}
