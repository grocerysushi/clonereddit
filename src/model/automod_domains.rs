use std::collections::BTreeSet;

use pulldown_cmark::{utils::TextMergeStream, Event, Parser, Tag, TagEnd};

const MAX_DOMAINS: usize = 20;
const MAX_INPUT_BYTES: usize = 4096;

pub(super) fn parse_list(input: &str) -> Result<Vec<String>, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("The link domain list is too long.".into());
    }
    let mut domains = BTreeSet::new();
    let invalid = "Enter bare domain names, such as example.com, separated by commas or spaces.";
    for domain in input
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|part| !part.is_empty())
    {
        domains.insert(normalize_host(domain).ok_or(invalid)?);
        if domains.len() > MAX_DOMAINS {
            return Err(format!("Enter up to {MAX_DOMAINS} link domains."));
        }
    }
    if domains.is_empty() && !input.trim().is_empty() {
        return Err(invalid.into());
    }
    Ok(domains.into_iter().collect())
}

pub(super) fn matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

pub(super) fn linked_hosts(
    direct_url: &str,
    title: &str,
    body: &str,
    markdown: bool,
) -> BTreeSet<String> {
    let mut hosts = BTreeSet::new();
    hosts.extend(host_from_url(direct_url.trim()));
    hosts_from_text(title, &mut hosts);
    if markdown {
        hosts_from_markdown(body, &mut hosts);
    } else {
        hosts_from_text(body, &mut hosts);
    }
    hosts
}

fn hosts_from_markdown(body: &str, hosts: &mut BTreeSet<String>) {
    let mut link_depth = 0;
    let mut in_code_block = false;
    let mut image_depth = 0;
    for event in TextMergeStream::new(Parser::new(body)) {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                link_depth += 1;
                if image_depth == 0 {
                    hosts.extend(host_from_url(&dest_url));
                }
            }
            Event::End(TagEnd::Link) => link_depth -= 1,
            Event::Start(Tag::CodeBlock(_)) => in_code_block = true,
            Event::End(TagEnd::CodeBlock) => in_code_block = false,
            Event::Start(Tag::Image { .. }) => image_depth += 1,
            Event::End(TagEnd::Image) => image_depth -= 1,
            Event::Text(text) if link_depth == 0 && !in_code_block && image_depth == 0 => {
                hosts_from_text(&text, hosts);
            }
            _ => {}
        }
    }
}

fn hosts_from_text(text: &str, hosts: &mut BTreeSet<String>) {
    for token in text.split_whitespace() {
        let url = token
            .trim_start_matches(['<', '[', '(', '{', '"', '\'', '“', '‘'])
            .trim_end_matches([
                '>', ']', ')', '}', '"', '\'', '”', '’', ',', '.', ':', '!', ';',
            ]);
        hosts.extend(host_from_url(url));
    }
}

fn host_from_url(url: &str) -> Option<String> {
    let (scheme, address) = url.split_once("://")?;
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || url.contains('\\')
        || url.chars().any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return None;
    }
    let authority = address.split(['/', '?', '#']).next()?;
    let host_and_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = if let Some((host, port)) = host_and_port.rsplit_once(':') {
        if port.is_empty()
            || !port.bytes().all(|byte| byte.is_ascii_digit())
            || port.parse::<u16>().is_err()
        {
            return None;
        }
        host
    } else {
        host_and_port
    };
    normalize_host(host)
}

fn normalize_host(host: &str) -> Option<String> {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.len() > 253
        || !host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_normalizes_and_deduplicates_valid_hosts() {
        assert!(parse_list(" \n\t ").unwrap().is_empty());
        assert_eq!(
            parse_list("ExAmPlE.Com.\nsub.example.com, example.com  xn--bcher-kva.test").unwrap(),
            ["example.com", "sub.example.com", "xn--bcher-kva.test"]
        );
        for invalid in [
            ",,",
            "https://example.com",
            "example.com:80",
            "example.com/path",
            "*.example.com",
            "example..com",
            "example.com..",
            "-example.com",
            "example-.com",
            "host_name.test",
            "bücher.test",
        ] {
            assert!(parse_list(invalid).is_err(), "{invalid}");
        }
        assert!(parse_list(&format!("{}.test", "a".repeat(64))).is_err());
        assert!(parse_list(&" ".repeat(MAX_INPUT_BYTES + 1)).is_err());
        let domains = (0..=MAX_DOMAINS)
            .map(|index| format!("example{index}.test"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(parse_list(&domains).is_err());
    }

    #[test]
    fn matches_only_exact_and_real_subdomain_hosts() {
        assert!(matches("example.test", "example.test"));
        assert!(matches("x.sub.example.test", "example.test"));
        assert!(!matches("otherexample.test", "example.test"));
        assert!(!matches("example.test.other.test", "example.test"));
        assert!(!matches("x.example.other.test", "example.test"));
    }

    #[test]
    fn extracts_true_authority_from_direct_and_plain_links() {
        let hosts = linked_hosts(
            "HTTPS://Direct.Test.:443/path?url=https://query.test",
            "Title (https://title.test/), keyword path.test",
            "See ‘http://Body.Test/a’. https://decoy.test@actual.test/ \
                https://actual.test/?next=https://query.test http://user.test/decoy.test \
                ftp://file.test https://invalid.test:70000 path.test \
                https:///no-authority.test https://broken.example\\@false.test",
            false,
        );
        assert_eq!(
            hosts.into_iter().collect::<Vec<_>>(),
            [
                "actual.test",
                "body.test",
                "direct.test",
                "title.test",
                "user.test"
            ]
        );
    }

    #[test]
    fn uses_markdown_destinations_while_ignoring_examples_and_non_links() {
        let hosts = linked_hosts(
            "",
            "",
            "See [https://label.test](https://destination.test/path) [reference][id] \
                 and <https://automatic.test> or https://plain.test.

![https://alt.test](https://image.test/picture.png) `https://inline-code.test`

```text
https://code-block.test
```

<img src=\"https://raw-html.test\">

[id]: https://reference.test/path",
            true,
        );
        assert_eq!(
            hosts.into_iter().collect::<Vec<_>>(),
            [
                "automatic.test",
                "destination.test",
                "plain.test",
                "reference.test"
            ]
        );
    }
}
