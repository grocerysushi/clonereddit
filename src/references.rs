pub(crate) enum Segment<'a> {
    Text(&'a str),
    Reference { label: &'a str, href: String },
}

pub(crate) fn segments(text: &str) -> impl Iterator<Item = Segment<'_>> {
    let mut segments = Vec::new();
    let mut offset = 0;
    let mut plain_start = 0;
    for part in text.split_inclusive(char::is_whitespace) {
        let token = part.trim_end_matches(char::is_whitespace);
        let start = token.trim_start_matches(['(', '[', '{', '<', '"', '\'', '“', '‘', '«', '‹']);
        let label = start.trim_end_matches([
            ')', ']', '}', '>', '"', '\'', '”', '’', '»', '›', ',', '.', '!', '?', ';', ':', '…',
        ]);
        if let Some(href) = destination(label) {
            let reference_start = offset + token.len() - start.len();
            if plain_start < reference_start {
                segments.push(Segment::Text(&text[plain_start..reference_start]));
            }
            segments.push(Segment::Reference { label, href });
            plain_start = reference_start + label.len();
        }
        offset += part.len();
    }
    if plain_start < text.len() {
        segments.push(Segment::Text(&text[plain_start..]));
    }
    segments.into_iter()
}

fn destination(label: &str) -> Option<String> {
    let path = label.strip_prefix('/').unwrap_or(label);
    let (kind, name) = path.split_once('/')?;
    let valid = match kind {
        "c" => {
            (2..=32).contains(&name.len())
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && !name.starts_with('-')
                && !name.ends_with('-')
        }
        "u" => {
            (2..=24).contains(&name.len())
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        }
        _ => false,
    };
    valid.then(|| format!("/{}", path.to_ascii_lowercase()))
}
