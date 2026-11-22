use std::collections::BTreeMap;

use super::field;
use crate::model::{Community, Post};

#[derive(Default)]
pub(super) struct PostSearchQuery {
    terms: Vec<Term>,
    filters: Vec<(Filter, String)>,
    error: Option<&'static str>,
}

struct Term {
    text: String,
    quoted: bool,
}

#[derive(Clone, Copy)]
enum Filter {
    Author,
    Community,
    Flair,
}

impl PostSearchQuery {
    pub(super) fn new(query: &BTreeMap<String, String>) -> Self {
        let mut search = Self::default();
        search.error = search.parse(query).err();
        search
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.error
    }

    pub(super) fn matches(&self, post: &Post, title: &str, community: Option<&Community>) -> bool {
        if self.error.is_some()
            || !self
                .filters
                .iter()
                .all(|(filter, value)| filter.matches(value, post, community))
        {
            return false;
        }
        if self.terms.is_empty() {
            return true;
        }
        let fields = [
            title,
            &post.body,
            &post.author,
            &post.community,
            &post.flair,
            community.map_or("", |item| &item.name),
        ]
        .map(str::to_lowercase);
        self.terms
            .iter()
            .all(|term| fields.iter().any(|text| term.matches(text)))
    }

    fn parse(&mut self, query: &BTreeMap<String, String>) -> Result<(), &'static str> {
        for (name, filter) in [
            ("author", Filter::Author),
            ("community", Filter::Community),
            ("post_flair", Filter::Flair),
        ] {
            let value = field(query, name).trim();
            if !value.is_empty() {
                self.filters.push((filter, filter.normalize(value)?));
            }
        }

        let text = field(query, "q").trim();
        let mut rest = text;
        let mut structured = false;
        while !rest.is_empty() {
            let word_end = rest
                .find(|character: char| character.is_whitespace() || matches!(character, ':' | '"'))
                .unwrap_or(rest.len());
            let (name, suffix) = rest.split_at(word_end);
            let operator = suffix
                .strip_prefix(':')
                .and_then(|value| Filter::from_operator(name).map(|filter| (filter, value)));
            let (filter, token) = match operator {
                Some((filter, value)) => (Some(filter), value),
                None => (None, rest),
            };
            let (value, tail, quoted) = read_term(token)?;
            if let Some(filter) = filter {
                self.filters.push((filter, filter.normalize(&value)?));
                structured = true;
            } else {
                if quoted && value.trim().is_empty() {
                    return Err("Enter a phrase inside the quotation marks.");
                }
                self.terms.push(Term {
                    text: value.to_lowercase(),
                    quoted,
                });
                structured |= quoted;
            }
            rest = tail.trim_start();
        }
        if !structured && !text.is_empty() {
            // Plain search has always matched the entire input as one substring.
            self.terms = vec![Term {
                text: text.to_lowercase(),
                quoted: false,
            }];
        }
        Ok(())
    }
}

impl Filter {
    fn from_operator(name: &str) -> Option<Self> {
        if name.eq_ignore_ascii_case("author") {
            Some(Self::Author)
        } else if name.eq_ignore_ascii_case("community") {
            Some(Self::Community)
        } else if name.eq_ignore_ascii_case("flair") {
            Some(Self::Flair)
        } else {
            None
        }
    }

    fn normalize(self, value: &str) -> Result<String, &'static str> {
        let value = value.trim().to_lowercase();
        let normalized = match self {
            Self::Author => {
                let handle = value.strip_prefix('@').unwrap_or(&value);
                handle.strip_prefix("u/").unwrap_or(handle)
            }
            Self::Community => value
                .strip_prefix("c/")
                .or_else(|| value.strip_prefix("r/"))
                .unwrap_or(&value),
            Self::Flair => &value,
        };
        if normalized.is_empty() {
            return Err(match self {
                Self::Author => "Enter a handle for the author filter.",
                Self::Community => "Enter a name for the community filter.",
                Self::Flair => "Enter a label for the flair filter.",
            });
        }
        Ok(normalized.to_owned())
    }

    fn matches(self, value: &str, post: &Post, community: Option<&Community>) -> bool {
        match self {
            Self::Author => post.author.eq_ignore_ascii_case(value),
            Self::Community => {
                post.community.eq_ignore_ascii_case(value)
                    || community.is_some_and(|item| item.name.to_lowercase() == value)
            }
            Self::Flair => post.flair.to_lowercase() == value,
        }
    }
}

impl Term {
    fn matches(&self, text: &str) -> bool {
        if !self.quoted {
            return text.contains(&self.text);
        }
        let Some(first) = self.text.chars().next() else {
            return false;
        };
        let starts_word = is_word_character(first);
        let ends_word = self.text.ends_with(is_word_character);
        text.char_indices().any(|(start, character)| {
            character == first
                && (!starts_word || !text[..start].ends_with(is_word_character))
                && text[start..].starts_with(&self.text)
                && (!ends_word || !text[start + self.text.len()..].starts_with(is_word_character))
        })
    }
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn read_term(input: &str) -> Result<(String, &str, bool), &'static str> {
    let Some(quoted) = input.strip_prefix('"') else {
        let end = input
            .find(|character: char| character.is_whitespace() || character == '"')
            .unwrap_or(input.len());
        return Ok((input[..end].to_owned(), &input[end..], false));
    };

    let mut term = String::new();
    let mut characters = quoted.char_indices().peekable();
    while let Some((index, character)) = characters.next() {
        match character {
            '"' => return Ok((term, &quoted[index + 1..], true)),
            '\\' if matches!(characters.peek(), Some((_, '"' | '\\'))) => {
                term.push(characters.next().unwrap().1);
            }
            _ => term.push(character),
        }
    }
    Err("Close the quotation marks to search for an exact phrase.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn search(fields: &[(&str, &str)]) -> PostSearchQuery {
        PostSearchQuery::new(
            &fields
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
        )
    }

    fn post() -> Post {
        Post {
            id: 1,
            community: "trail-works".into(),
            author: "riverwalker".into(),
            title: "Café Comet: a note about trail conditions".into(),
            body: "The towpath stays open. Ask if the water rises.".into(),
            flair: "Field Notes & Maps".into(),
            age: "now".into(),
            created_at: None,
            url: String::new(),
            score: 0,
            reply_count: 0,
            sensitive: false,
            spoiler: false,
            brand_affiliate: false,
            image: false,
            poll: None,
            repost_of: None,
            repost_title_from_source: false,
            media: Vec::new(),
            media_alternatives: Default::default(),
        }
    }

    #[test]
    fn combines_inline_and_form_filters_with_text() {
        let post = post();
        let query = search(&[
            ("q", r#"author:@u/RiverWalker "CAFÉ COMET" towpath"#),
            ("community", "r/Trail-Works"),
            ("post_flair", "field notes & maps"),
        ]);
        assert!(query.matches(&post, &post.title, None));
        let wrong_author = search(&[("q", "author:riverwalker"), ("author", "other")]);
        assert!(!wrong_author.matches(&post, &post.title, None));
        assert!(!search(&[("q", "author:river")]).matches(&post, &post.title, None));
        assert!(!search(&[("q", r#"flair:"field notes""#)]).matches(&post, &post.title, None));
        assert!(
            search(&[("q", r#"flair:"Field Notes & Maps" community:c/trail-works"#)]).matches(
                &post,
                &post.title,
                None
            )
        );
    }

    #[test]
    fn quoted_phrases_match_contiguously_at_word_edges_in_one_field() {
        let post = post();
        for phrase in [r#""comet: a note""#, r#""WATER RISES""#] {
            assert!(search(&[("q", phrase)]).matches(&post, &post.title, None));
        }
        for phrase in [
            r#""afé comet""#,
            r#""water rise""#,
            r#""conditions the""#,
            r#""water  rises""#,
        ] {
            assert!(!search(&[("q", phrase)]).matches(&post, &post.title, None));
        }
        assert!(search(&[("q", r#""she said \"no\"""#)]).matches(
            &post,
            r#"She said "no" last night"#,
            None
        ));
    }

    #[test]
    fn plain_queries_still_match_as_substrings_and_unknown_operators_stay_literal() {
        let post = post();
        assert!(search(&[("q", "ter rises")]).matches(&post, &post.title, None));
        assert!(!search(&[("q", "café towpath")]).matches(&post, &post.title, None));
        assert!(search(&[("q", "topic:layout")]).matches(&post, "Topic:layout notes", None));
        assert!(
            search(&[("q", "conditions"), ("flair", "unrelated feed flair")]).matches(
                &post,
                &post.title,
                None
            )
        );
    }

    #[test]
    fn incomplete_quotes_and_missing_operator_values_are_reported() {
        let post = post();
        for raw in [
            r#""missing close"#,
            r#""""#,
            "author:",
            "author: name",
            "community:r/",
        ] {
            let query = search(&[("q", raw)]);
            assert!(query.error().is_some(), "{raw}");
            assert!(!query.matches(&post, &post.title, None), "{raw}");
        }
    }
}
