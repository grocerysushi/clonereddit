use std::cell::RefCell;
use std::ops::Range;

const BODY_CAPACITY: usize = 8_000;
const IMAGE_KEY_LENGTH: usize = 48;
const IMAGE_ALT_CAPACITY: usize = 160;
const NEWLINE: u16 = b'\n' as u16;
const BACKTICK: u16 = b'`' as u16;
const SPACE: u16 = b' ' as u16;

enum Command {
    Bold,
    Italic,
    Link,
    Bullets,
    Numbered,
    Quote,
    Code,
}

impl Command {
    fn from_number(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Bold),
            1 => Some(Self::Italic),
            2 => Some(Self::Link),
            3 => Some(Self::Bullets),
            4 => Some(Self::Numbered),
            5 => Some(Self::Quote),
            6 => Some(Self::Code),
            _ => None,
        }
    }
}

struct Edit {
    range: Range<usize>,
    replacement: Vec<u16>,
    selection: Range<usize>,
}

impl Edit {
    fn new(range: Range<usize>, replacement: Vec<u16>, selection: Range<usize>) -> Self {
        let selection = range.start + selection.start..range.start + selection.end;
        Self {
            range,
            replacement,
            selection,
        }
    }
}

fn format(source: &[u16], selection: Range<usize>, command: Command) -> Option<Edit> {
    if selection.start > selection.end || selection.end > source.len() {
        return None;
    }
    if !is_character_boundary(source, selection.start)
        || !is_character_boundary(source, selection.end)
    {
        return None;
    }
    Some(match command {
        Command::Bold => emphasis(source, selection, "**", "bold text"),
        Command::Italic => emphasis(source, selection, "*", "italic text"),
        Command::Link => link(source, selection),
        Command::Bullets => format_lines(source, selection, "List item", |_| "- ".into()),
        Command::Numbered => format_lines(source, selection, "List item", |index| {
            format!("{}. ", index + 1)
        }),
        Command::Quote => format_lines(source, selection, "Quote", |_| "> ".into()),
        Command::Code => inline_code(source, selection),
    })
}

fn is_character_boundary(source: &[u16], index: usize) -> bool {
    let splits_pair = index > 0
        && index < source.len()
        && (0xD800..=0xDBFF).contains(&source[index - 1])
        && (0xDC00..=0xDFFF).contains(&source[index]);
    !splits_pair
}

fn is_whitespace(unit: &u16) -> bool {
    char::from_u32(u32::from(*unit)).is_some_and(char::is_whitespace)
}

fn trim_selection(source: &[u16], selection: Range<usize>) -> Range<usize> {
    let text = &source[selection.clone()];
    let Some(first) = text.iter().position(|unit| !is_whitespace(unit)) else {
        return selection.start..selection.start;
    };
    let last = text.iter().rposition(|unit| !is_whitespace(unit)).unwrap();
    selection.start + first..selection.start + last + 1
}

fn emphasis(source: &[u16], selection: Range<usize>, marker: &str, placeholder: &str) -> Edit {
    let range = trim_selection(source, selection);
    let mut replacement: Vec<_> = marker.encode_utf16().collect();
    let start = replacement.len();
    if range.is_empty() {
        replacement.extend(placeholder.encode_utf16());
    } else {
        replacement.extend_from_slice(&source[range.clone()]);
    }
    let end = replacement.len();
    replacement.extend(marker.encode_utf16());
    Edit::new(range, replacement, start..end)
}

fn link(source: &[u16], selection: Range<usize>) -> Edit {
    let range = trim_selection(source, selection);
    let mut replacement = vec![b'[' as u16];
    if range.is_empty() {
        replacement.extend("link text".encode_utf16());
    } else {
        for &unit in &source[range.clone()] {
            if matches!(unit, 0x5B | 0x5C | 0x5D) {
                replacement.push(b'\\' as u16);
            }
            replacement.push(if unit == NEWLINE { b' ' as u16 } else { unit });
        }
    }
    replacement.extend("](".encode_utf16());
    let start = replacement.len();
    replacement.extend("https://example.com".encode_utf16());
    let end = replacement.len();
    replacement.push(b')' as u16);
    Edit::new(range, replacement, start..end)
}

fn inline_code(source: &[u16], selection: Range<usize>) -> Edit {
    let code = &source[selection.clone()];
    let delimiter_length = code
        .split(|&unit| unit != BACKTICK)
        .map(<[u16]>::len)
        .max()
        .unwrap_or(0)
        + 1;
    // CommonMark strips one outer space from both sides unless the span is all spaces.
    let padded = code.starts_with(&[BACKTICK])
        || code.ends_with(&[BACKTICK])
        || (code.starts_with(&[SPACE])
            && code.ends_with(&[SPACE])
            && code.iter().any(|&unit| unit != SPACE));
    let mut replacement = vec![BACKTICK; delimiter_length];
    if padded {
        replacement.push(SPACE);
    }
    let start = replacement.len();
    if code.is_empty() {
        replacement.extend("code".encode_utf16());
    } else {
        replacement.extend_from_slice(code);
    }
    let end = replacement.len();
    if padded {
        replacement.push(SPACE);
    }
    replacement.resize(replacement.len() + delimiter_length, BACKTICK);
    Edit::new(selection, replacement, start..end)
}

fn format_lines(
    source: &[u16],
    selection: Range<usize>,
    placeholder: &str,
    prefix: impl Fn(usize) -> String,
) -> Edit {
    let first = source[..selection.start]
        .iter()
        .rposition(|&unit| unit == NEWLINE)
        .map_or(0, |index| index + 1);
    let end_anchor = if selection.end > selection.start && source[selection.end - 1] == NEWLINE {
        selection.end - 1
    } else {
        selection.end
    };
    let last = source[end_anchor..]
        .iter()
        .position(|&unit| unit == NEWLINE)
        .map_or(source.len(), |index| end_anchor + index);
    let mut replacement = Vec::new();
    // Prevent a paragraph outside the selection from taking part in the list or quote.
    if first > 0
        && source[..first - 1]
            .rsplit(|&unit| unit == NEWLINE)
            .next()
            .is_some_and(|line| !line.iter().all(is_whitespace))
    {
        replacement.push(NEWLINE);
    }
    let start = replacement.len();
    let lines = &source[first..last];
    let selection = if lines.iter().all(is_whitespace) {
        replacement.extend(prefix(0).encode_utf16());
        let start = replacement.len();
        replacement.extend(placeholder.encode_utf16());
        start..replacement.len()
    } else {
        for (index, line) in lines.split(|&unit| unit == NEWLINE).enumerate() {
            if index != 0 {
                replacement.push(NEWLINE);
            }
            replacement.extend(prefix(index).encode_utf16());
            replacement.extend_from_slice(line);
        }
        if selection.is_empty() {
            let prefix_length = prefix(0).encode_utf16().count();
            let caret = start + prefix_length + selection.start - first;
            caret..caret
        } else {
            start..replacement.len()
        }
    };
    if last < source.len()
        && source[last + 1..]
            .split(|&unit| unit == NEWLINE)
            .next()
            .is_some_and(|line| !line.iter().all(is_whitespace))
    {
        replacement.push(NEWLINE);
    }
    Edit::new(first..last, replacement, selection)
}

fn inline_image(source: &[u16], at: usize, key: &[u16], description: &[u16]) -> Option<Edit> {
    if at > source.len() || !is_character_boundary(source, at) {
        return None;
    }
    if key.len() != IMAGE_KEY_LENGTH
        || !key
            .iter()
            .all(|&unit| matches!(unit, 0x30..=0x39 | 0x61..=0x66))
    {
        return None;
    }
    let description = &description[trim_selection(description, 0..description.len())];
    let mut replacement = Vec::new();
    if at != 0 {
        let before = source[..at]
            .iter()
            .rev()
            .take_while(|&&unit| unit == NEWLINE)
            .count();
        replacement.extend(std::iter::repeat(NEWLINE).take(2_usize.saturating_sub(before)));
    }
    replacement.extend("![".encode_utf16());
    if description.is_empty() {
        replacement.extend("Inline image".encode_utf16());
    } else {
        for &unit in description {
            if matches!(unit, 0x21..=0x2F | 0x3A..=0x40 | 0x5B..=0x60 | 0x7B..=0x7E) {
                replacement.push(b'\\' as u16);
            }
            replacement.push(if matches!(unit, 0x0A | 0x0D) {
                b' ' as u16
            } else {
                unit
            });
        }
    }
    replacement.extend("](inline-image:".encode_utf16());
    replacement.extend_from_slice(key);
    replacement.push(b')' as u16);
    let after = source[at..]
        .iter()
        .take_while(|&&unit| unit == NEWLINE)
        .count();
    replacement.extend(std::iter::repeat(NEWLINE).take(2_usize.saturating_sub(after)));
    let end = replacement.len() + after.min(2);
    Some(Edit::new(at..at, replacement, end..end))
}

struct Buffer {
    input: [u16; BODY_CAPACITY],
    image: [u16; IMAGE_KEY_LENGTH + IMAGE_ALT_CAPACITY],
    edit: Option<Edit>,
}

thread_local! {
    static BUFFER: RefCell<Buffer> = const {
        RefCell::new(Buffer {
            input: [0; BODY_CAPACITY],
            image: [0; IMAGE_KEY_LENGTH + IMAGE_ALT_CAPACITY],
            edit: None,
        })
    };
}

#[no_mangle]
pub extern "C" fn post_format_input() -> *mut u16 {
    BUFFER.with(|buffer| buffer.borrow_mut().input.as_mut_ptr())
}

#[no_mangle]
pub extern "C" fn post_format_capacity() -> usize {
    BODY_CAPACITY
}

#[no_mangle]
pub extern "C" fn post_format_image_input() -> *mut u16 {
    BUFFER.with(|buffer| buffer.borrow_mut().image.as_mut_ptr())
}

#[no_mangle]
pub extern "C" fn post_format_apply(
    command: u32,
    length: usize,
    start: usize,
    end: usize,
    max_length: usize,
) -> u32 {
    prepare_edit(length, max_length, |_, source| {
        Command::from_number(command).and_then(|command| format(source, start..end, command))
    })
}

#[no_mangle]
pub extern "C" fn post_format_insert_image(
    length: usize,
    at: usize,
    max_length: usize,
    description_length: usize,
) -> u32 {
    prepare_edit(length, max_length, |buffer, source| {
        let (key, description) = buffer.image.split_at(IMAGE_KEY_LENGTH);
        inline_image(source, at, key, description.get(..description_length)?)
    })
}

fn prepare_edit(
    length: usize,
    max_length: usize,
    make_edit: impl FnOnce(&Buffer, &[u16]) -> Option<Edit>,
) -> u32 {
    BUFFER.with(|buffer| {
        let mut buffer = buffer.borrow_mut();
        buffer.edit = None;
        let Some(source) = buffer.input.get(..length) else {
            return 2;
        };
        let Some(edit) = make_edit(&buffer, source) else {
            return 0;
        };
        if source.len() - edit.range.len() + edit.replacement.len() > max_length.min(BODY_CAPACITY)
        {
            return 2;
        }
        buffer.edit = Some(edit);
        1
    })
}

#[no_mangle]
pub extern "C" fn post_format_part(part: u32) -> usize {
    BUFFER.with(|buffer| {
        let buffer = buffer.borrow();
        let Some(edit) = &buffer.edit else {
            return 0;
        };
        match part {
            0 => edit.replacement.as_ptr() as usize,
            1 => edit.replacement.len(),
            2 => edit.range.start,
            3 => edit.range.end,
            4 => edit.selection.start,
            5 => edit.selection.end,
            _ => 0,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edited(text: &str, selection: Range<usize>, command: Command) -> (String, String) {
        let mut document: Vec<_> = text.encode_utf16().collect();
        let edit = format(&document, selection, command).unwrap();
        document.splice(edit.range, edit.replacement);
        let selected = String::from_utf16(&document[edit.selection]).unwrap();
        (String::from_utf16(&document).unwrap(), selected)
    }

    #[test]
    fn wraps_unicode_and_preserves_surrounding_whitespace() {
        assert_eq!(
            edited("a 🐟 café z", 1..10, Command::Bold),
            ("a **🐟 café** z".to_owned(), "🐟 café".to_owned())
        );
        let fish: Vec<_> = "🐟".encode_utf16().collect();
        assert!(format(&fish, 1..2, Command::Italic).is_none());
        assert_eq!(
            edited("", 0..0, Command::Italic),
            ("*italic text*".to_owned(), "italic text".to_owned())
        );
    }

    #[test]
    fn links_escape_brackets_and_select_the_address() {
        assert_eq!(
            edited("a [fish]", 2..8, Command::Link),
            (
                "a [\\[fish\\]](https://example.com)".to_owned(),
                "https://example.com".to_owned()
            )
        );
    }

    #[test]
    fn inline_code_keeps_unicode_spaces_and_literal_backticks_selected() {
        for (source, markdown) in [
            ("λ + 🐟", "`λ + 🐟`"),
            ("`literal``", "``` `literal`` ```"),
            (" padded ", "`  padded  `"),
            ("   ", "`   `"),
        ] {
            assert_eq!(
                edited(source, 0..source.encode_utf16().count(), Command::Code),
                (markdown.to_owned(), source.to_owned())
            );
        }
        assert_eq!(
            edited("", 0..0, Command::Code),
            ("`code`".to_owned(), "code".to_owned())
        );
    }

    #[test]
    fn blocks_format_only_the_targeted_lines() {
        let source = "Before\nfish\nbird\nAfter";
        assert_eq!(
            edited(source, 7..17, Command::Numbered),
            (
                "Before\n\n1. fish\n2. bird\n\nAfter".to_owned(),
                "1. fish\n2. bird".to_owned()
            )
        );
        assert_eq!(
            edited(source, 9..9, Command::Quote),
            ("Before\n\n> fish\n\nbird\nAfter".to_owned(), String::new())
        );
        assert_eq!(
            edited("", 0..0, Command::Bullets),
            ("- List item".to_owned(), "List item".to_owned())
        );
    }

    #[test]
    fn formatter_rejects_invalid_ranges_and_edits_over_maximum_length() {
        assert_eq!(post_format_apply(0, 0, 0, 0, 5), 2);
        assert_eq!(post_format_part(1), 0);
        assert_eq!(post_format_apply(0, 0, 0, 1, BODY_CAPACITY), 0);
        assert_eq!(post_format_apply(0, 0, 0, 0, BODY_CAPACITY), 1);
        assert_eq!(post_format_part(1), "**bold text**".len());
    }

    #[test]
    fn image_insertions_keep_paragraphs_and_unicode_at_the_caret() {
        let first = "Trail 🥾";
        let source = format!("{first}\n\nSecond paragraph");
        let mut document: Vec<_> = source.encode_utf16().collect();
        let key = "a1".repeat(24);
        let key_units: Vec<_> = key.encode_utf16().collect();
        let alt: Vec<_> = " Ridge [east] 🌲 ".encode_utf16().collect();
        let at = first.encode_utf16().count() + 1;
        let edit = inline_image(&document, at, &key_units, &alt).unwrap();
        assert!(edit.selection.is_empty());
        document.splice(edit.range, edit.replacement);
        assert_eq!(
            String::from_utf16(&document).unwrap(),
            format!("{first}\n\n![Ridge \\[east\\] 🌲](inline-image:{key})\n\nSecond paragraph")
        );
        assert!(inline_image(&document, 7, &key_units, &alt).is_none());
    }

    #[test]
    fn image_insertions_validate_keys_and_default_empty_descriptions() {
        let invalid = [b'x' as u16; IMAGE_KEY_LENGTH];
        assert!(inline_image(&[], 0, &invalid, &[]).is_none());
        let key = [b'3' as u16; IMAGE_KEY_LENGTH];
        let edit = inline_image(&[], 0, &key, &[]).unwrap();
        assert_eq!(
            String::from_utf16(&edit.replacement).unwrap(),
            format!(
                "![Inline image](inline-image:{})\n\n",
                "3".repeat(IMAGE_KEY_LENGTH)
            )
        );
    }
}
