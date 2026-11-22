use std::cell::RefCell;
use std::ops::Range;

const BODY_CAPACITY: usize = 8_000;
const NEWLINE: u16 = b'\n' as u16;

enum Command {
    Bold,
    Italic,
    Link,
    Bullets,
    Numbered,
    Quote,
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

struct Buffer {
    input: [u16; BODY_CAPACITY],
    edit: Option<Edit>,
}

thread_local! {
    static BUFFER: RefCell<Buffer> = const {
        RefCell::new(Buffer {
            input: [0; BODY_CAPACITY],
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
pub extern "C" fn post_format_apply(
    command: u32,
    length: usize,
    start: usize,
    end: usize,
    max_length: usize,
) -> u32 {
    BUFFER.with(|buffer| {
        let mut buffer = buffer.borrow_mut();
        buffer.edit = None;
        let Some(source) = buffer.input.get(..length) else {
            return 2;
        };
        let Some(edit) =
            Command::from_number(command).and_then(|command| format(source, start..end, command))
        else {
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
}
