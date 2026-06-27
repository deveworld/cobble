pub fn byte_offset_for_line_column(source: &str, line: usize, column: usize) -> usize {
    let target_line = line.max(1);
    let target_column = column.max(1);
    let mut offset = 0;

    for (index, source_line) in source.split_inclusive('\n').enumerate() {
        if index + 1 == target_line {
            let without_newline = source_line.strip_suffix('\n').unwrap_or(source_line);
            let without_line_ending = without_newline
                .strip_suffix('\r')
                .unwrap_or(without_newline);
            let column_offset = without_line_ending
                .char_indices()
                .nth(target_column.saturating_sub(1))
                .map(|(byte_index, _)| byte_index)
                .unwrap_or(without_line_ending.len());
            return offset + column_offset;
        }
        offset += source_line.len();
    }

    source.len()
}

pub(super) fn matching_close_paren(expression: &str, open_index: usize) -> Option<usize> {
    let bytes = expression.as_bytes();
    let mut depth = 0usize;

    for (index, byte) in bytes.iter().enumerate().skip(open_index) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }

    None
}

pub(super) fn split_top_level_args(args: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;

    for (index, byte) in args.as_bytes().iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                result.push(&args[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }

    result.push(&args[start..]);
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ArgumentSpan {
    pub(super) text: String,
    pub(super) offset: usize,
}

pub(super) fn split_top_level_arg_spans(args: &str) -> Vec<ArgumentSpan> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;

    for (index, byte) in args.as_bytes().iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                result.push(argument_span(args, start, index));
                start = index + 1;
            }
            _ => {}
        }
    }

    result.push(argument_span(args, start, args.len()));
    result
}

pub(super) fn argument_span(args: &str, start: usize, end: usize) -> ArgumentSpan {
    let text = &args[start..end];
    let trim_start = text.len() - text.trim_start().len();
    let trimmed = text.trim();

    ArgumentSpan {
        text: trimmed.to_string(),
        offset: start + trim_start,
    }
}

pub(super) fn function_name_before_open_paren(
    expression: &str,
    open_index: usize,
) -> Option<(String, usize)> {
    let bytes = expression.as_bytes();
    let mut end = open_index;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    if end == 0 {
        return None;
    }

    let mut start = end;
    while start > 0 {
        let byte = bytes[start - 1];
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' {
            start -= 1;
        } else {
            break;
        }
    }

    if start == end {
        return None;
    }

    let name = expression[start..end].trim_matches('.').to_string();
    if name.is_empty() || name.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        return None;
    }

    Some((name, start))
}

pub(super) fn single_equals_index(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut delimiter_depth = 0usize;

    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => {
                delimiter_depth += 1;
                continue;
            }
            b')' | b']' | b'}' => {
                delimiter_depth = delimiter_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }

        if delimiter_depth > 0 {
            continue;
        }

        if *byte != b'=' {
            continue;
        }
        let previous = index.checked_sub(1).and_then(|i| bytes.get(i)).copied();
        let next = bytes.get(index + 1).copied();
        if matches!(
            previous,
            Some(b'=' | b'!' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/' | b'%' | b'^')
        ) || matches!(next, Some(b'='))
        {
            continue;
        }
        return Some(index);
    }
    None
}

pub(super) fn starts_with_keyword(text: &str, keyword: &str) -> bool {
    let Some(rest) = text.strip_prefix(keyword) else {
        return false;
    };
    rest.is_empty() || rest.chars().next().is_some_and(|ch| !is_ident_char(ch))
}

pub(super) fn find_word(text: &str, word: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(offset) = text[search_from..].find(word) {
        let index = search_from + offset;
        let before = text[..index].chars().next_back();
        let after = text[index + word.len()..].chars().next();
        if before.is_none_or(|ch| !is_ident_char(ch)) && after.is_none_or(|ch| !is_ident_char(ch)) {
            return Some(index);
        }
        search_from = index + word.len();
    }
    None
}

pub(super) fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

pub(super) fn column_from_byte(line: &str, byte_index: usize) -> usize {
    line[..byte_index.min(line.len())].chars().count() + 1
}

pub(super) fn matching_close_delimiter(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => open,
    }
}

pub(super) fn triple_quote_pattern(quote: char) -> &'static str {
    match quote {
        '"' => "\"\"\"",
        '\'' => "'''",
        _ => "",
    }
}

pub(super) fn leading_triple_quote(text: &str) -> Option<char> {
    if text.starts_with("\"\"\"") {
        Some('"')
    } else if text.starts_with("'''") {
        Some('\'')
    } else {
        None
    }
}

pub(super) fn find_triple_quote(line: &str, quote: char, start: usize) -> Option<usize> {
    line.get(start..)?
        .find(triple_quote_pattern(quote))
        .map(|offset| start + offset)
}

pub(super) fn find_string_end(line: &str, quote: char, start: usize) -> Option<usize> {
    let mut index = start;
    let mut escaped = false;

    while index < line.len() {
        let ch = line[index..].chars().next().unwrap();
        let next_index = index + ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return Some(next_index);
        }
        index = next_index;
    }

    None
}

pub(super) fn mask_non_code(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut quote = None;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if let Some(active_quote) = quote {
            output.push(' ');
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                output.push('_');
            }
            '#' => {
                output.push(' ');
                for _ in chars {
                    output.push(' ');
                }
                break;
            }
            _ => output.push(ch),
        }
    }

    output
}
