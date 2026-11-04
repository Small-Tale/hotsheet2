//! The FEEDBACK NEEDED choice grammar (HS2-83YCDP, HS2-3MHSBS, HS2-EWF0FT).
//!
//! A feedback prompt offers explicit choices as choice groups:
//!
//! - an uppercase `CHOICE` / `CHOICE:` header (optionally `CHOICE: <question>`) followed by a
//!   Markdown list, where each item's first line is its label and its lazy continuation lines
//!   and indented paragraphs (nested lists included) are its details;
//! - the same header followed by lettered blocks, or a headerless run of at least two lettered
//!   blocks: paragraphs led by `CHOICE A:`, `Option B —`, or a bare `C)` / `C.` marker,
//!   optionally bold, in sequence from A (or 1 after a keyword).
//!
//! This is a port of `clients/web/src/feedback-choices.ts`. Both implementations read the shared
//! corpus `tests/fixtures/feedback-choices.json`, so change them together. Offsets are byte
//! offsets into the source (the web client's are UTF-16 offsets into the same text).

/// One selectable choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackChoice {
    /// Stable id: `choice-N` in the first group, `group-G-choice-N` after it.
    pub id: String,
    /// The label Markdown: a list item's first line or a lettered block's header text.
    pub label: String,
    /// The rest of the choice's Markdown, if any.
    pub details: Option<String>,
}

/// One choice group within a prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackChoiceGroup {
    /// One-based position of the group within its note.
    pub index: usize,
    /// Byte offset where the group (its header, or its first lettered block) starts.
    pub start: usize,
    /// Byte offset just past the group's last line, including its line break.
    pub end: usize,
    /// The question on the `CHOICE:` line, else the last prose line before the group.
    pub question: Option<String>,
    /// The question was written on the `CHOICE:` line itself.
    pub inline_question: bool,
    pub choices: Vec<FeedbackChoice>,
}

/// Prose around the groups, with its byte offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackChoiceRegion {
    pub source: String,
    pub source_start: usize,
}

/// A prompt split into prose regions and choice groups: `regions[i]` precedes `groups[i]`, and
/// one trailing region follows the last group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedbackChoiceDocument {
    pub groups: Vec<FeedbackChoiceGroup>,
    pub regions: Vec<FeedbackChoiceRegion>,
}

#[derive(Clone, Copy)]
struct Line<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

fn source_lines(source: &str) -> Vec<Line<'_>> {
    let mut lines = Vec::new();
    let mut start = 0;
    while start < source.len() {
        match source[start..].find('\n') {
            Some(offset) => {
                let newline = start + offset;
                lines.push(Line {
                    start,
                    end: newline + 1,
                    text: &source[start..newline],
                });
                start = newline + 1;
            }
            None => {
                lines.push(Line {
                    start,
                    end: source.len(),
                    text: &source[start..],
                });
                break;
            }
        }
    }
    lines
}

fn strip_bold(text: &str) -> Option<&str> {
    text.strip_prefix("**").or_else(|| text.strip_prefix("__"))
}

/// `**x**` or `__x__` (at least four characters) without its delimiters.
fn unwrap_bold(text: &str) -> &str {
    for delimiter in ["**", "__"] {
        if text.len() >= 2 * delimiter.len()
            && text.starts_with(delimiter)
            && text.ends_with(delimiter)
        {
            return &text[delimiter.len()..text.len() - delimiter.len()];
        }
    }
    text
}

/// `Some(question)` for a choice group header line; text after a colon-less `CHOICE` is prose.
fn choice_header(text: &str) -> Option<Option<String>> {
    let rest = text.trim_start();
    let rest = strip_bold(rest).unwrap_or(rest);
    let rest = rest.strip_prefix("CHOICE")?;
    let (colon, rest) = match rest.strip_prefix(':') {
        Some(rest) => (true, rest),
        None => (false, rest),
    };
    let rest = strip_bold(rest).unwrap_or(rest);
    let inline = rest.trim();
    if inline.is_empty() {
        return Some(None);
    }
    if !rest.starts_with(char::is_whitespace) || !colon {
        return None;
    }
    let question = unwrap_bold(inline).trim();
    Some((!question.is_empty()).then(|| question.to_owned()))
}

/// The opening run of a fenced code block: its character and length.
fn fence(text: &str) -> Option<(char, usize)> {
    let rest = text.trim_start();
    let marker = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let run = rest.chars().take_while(|c| *c == marker).count();
    (run >= 3).then_some((marker, run))
}

/// Leading spaces when there are at most three of them.
fn block_indent(text: &str) -> Option<&str> {
    let spaces = text.len() - text.trim_start_matches(' ').len();
    (spaces <= 3).then(|| &text[spaces..])
}

fn is_heading(text: &str) -> bool {
    let Some(rest) = block_indent(text) else {
        return false;
    };
    let hashes = rest.len() - rest.trim_start_matches('#').len();
    (1..=6).contains(&hashes)
        && rest[hashes..]
            .chars()
            .next()
            .is_none_or(char::is_whitespace)
}

fn is_blockquote(text: &str) -> bool {
    block_indent(text).is_some_and(|rest| rest.starts_with('>'))
}

fn is_thematic_break(text: &str) -> bool {
    let Some(rest) = block_indent(text) else {
        return false;
    };
    let Some(marker) = rest.chars().next().filter(|c| matches!(c, '-' | '*' | '_')) else {
        return false;
    };
    let tail = rest[1..].trim_end();
    tail.chars().all(|c| c == marker || c == ' ' || c == '\t')
        && tail.chars().filter(|c| *c == marker).count() >= 2
}

/// Leading indentation in columns; a tab counts as four.
fn indent_of(text: &str) -> usize {
    let mut columns = 0;
    for c in text.chars() {
        match c {
            ' ' => columns += 1,
            '\t' => columns += 4,
            _ => break,
        }
    }
    columns
}

/// Remove up to `columns` columns of leading indentation.
fn dedent(text: &str, columns: usize) -> &str {
    let mut removed = 0;
    let mut index = 0;
    for c in text.chars() {
        if removed >= columns {
            break;
        }
        match c {
            ' ' => removed += 1,
            '\t' => removed += 4,
            _ => break,
        }
        index += 1;
    }
    &text[index..]
}

fn is_blank(text: &str) -> bool {
    text.trim().is_empty()
}

struct ListItemLine<'a> {
    indent: usize,
    ordered: bool,
    content_column: usize,
    label: &'a str,
}

/// A Markdown bullet (`-`, `+`, `*`) or numbered (`1.`, `1)`) list item line.
fn list_item(text: &str) -> Option<ListItemLine<'_>> {
    let rest = text.trim_start_matches([' ', '\t']);
    let indent = indent_of(&text[..text.len() - rest.len()]);
    let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    let marker_len = if digits > 0 {
        matches!(rest[digits..].chars().next(), Some('.' | ')')).then_some(digits + 1)?
    } else {
        matches!(rest.chars().next(), Some('-' | '+' | '*')).then_some(1)?
    };
    let after = &rest[marker_len..];
    let content = after.trim_start_matches([' ', '\t']);
    if content.len() == after.len() {
        return None;
    }
    let label = content.trim();
    if label.is_empty() {
        return None;
    }
    Some(ListItemLine {
        indent,
        ordered: digits > 0,
        content_column: indent + marker_len + indent_of(&after[..after.len() - content.len()]),
        label,
    })
}

struct LetteredHeader {
    /// Marker family and ordinal kind; one group never mixes styles.
    style: String,
    ordinal: u32,
    indent: usize,
    label: String,
    /// Text after a bold lead that carries its own label.
    details: Option<String>,
}

/// `CHOICE A`, `Option 2`, ... with an optional separator: (style, ordinal, marker length, separated).
fn lettered_keyword(body: &str) -> Option<(String, u32, usize, bool)> {
    let (keyword, rest) = ["CHOICE", "Option", "OPTION"]
        .iter()
        .find_map(|keyword| body.strip_prefix(keyword).map(|rest| (*keyword, rest)))?;
    let value = rest.trim_start_matches([' ', '\t']);
    if value.len() == rest.len() {
        return None;
    }
    let first = value.chars().next()?;
    let (kind, ordinal, value_len) = if first.is_ascii_uppercase() {
        ("letter", u32::from(first) - 64, 1)
    } else if ('1'..='9').contains(&first) {
        let len = if value[1..].starts_with(|c: char| c.is_ascii_digit()) {
            2
        } else {
            1
        };
        ("number", value[..len].parse().ok()?, len)
    } else {
        return None;
    };
    let after = &value[value_len..];
    if after.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return None;
    }
    let spaced = after.trim_start_matches([' ', '\t']);
    let separator = spaced
        .chars()
        .next()
        .filter(|c| matches!(c, ':' | '.' | ')' | '—' | '–' | '-'));
    let tail = match separator {
        Some(c) => &spaced[c.len_utf8()..],
        None => spaced,
    };
    let style = format!("{}-{kind}", keyword.to_lowercase());
    Some((style, ordinal, body.len() - tail.len(), separator.is_some()))
}

/// A lettered choice block header: `CHOICE A: ...`, `Option B — ...`, or a bare `C) ...` lead.
fn lettered_header(text: &str) -> Option<LetteredHeader> {
    let spaces = (text.len() - text.trim_start_matches(' ').len()).min(3);
    let after_indent = &text[spaces..];
    let bold = ["**", "__"]
        .into_iter()
        .find(|bold| after_indent.starts_with(bold));
    let body = &after_indent[bold.map_or(0, str::len)..];
    let (style, ordinal, marker_len, separated) = match lettered_keyword(body) {
        Some(found) => found,
        None => {
            let mut chars = body.chars();
            let letter = chars.next().filter(char::is_ascii_uppercase)?;
            chars.next().filter(|c| matches!(c, '.' | ')' | ':'))?;
            ("bare-letter".to_owned(), u32::from(letter) - 64, 2, true)
        }
    };
    let rest = &body[marker_len..];
    let header = |label: String, details: Option<String>| LetteredHeader {
        style: style.clone(),
        ordinal,
        indent: spaces,
        label,
        details,
    };
    if let Some(bold) = bold {
        let close = rest.find(bold)?;
        let inner = &rest[..close];
        let after = rest[close + bold.len()..].trim();
        let lead = body[..marker_len + close].trim();
        if !separated && !inner.trim().is_empty() {
            return None;
        }
        if !inner.is_empty() && !inner.starts_with(char::is_whitespace) {
            return None;
        }
        if !inner.trim().is_empty() {
            return Some(header(
                lead.to_owned(),
                (!after.is_empty()).then(|| after.to_owned()),
            ));
        }
        let label = if after.is_empty() {
            lead.to_owned()
        } else {
            format!("{lead} {after}")
        };
        return Some(header(label, None));
    }
    if !separated || (!rest.is_empty() && !rest.starts_with(char::is_whitespace)) {
        return None;
    }
    Some(header(after_indent.trim().to_owned(), None))
}

/// A line that cannot lazily continue the paragraph before it.
fn interrupts_paragraph(text: &str) -> bool {
    fence(text).is_some()
        || is_heading(text)
        || is_blockquote(text)
        || is_thematic_break(text)
        || choice_header(text).is_some()
}

/// Unique, stable choice id; group 1 keeps the original `choice-N` form.
#[must_use]
pub fn feedback_choice_id(group_index: usize, choice_index: usize) -> String {
    if group_index == 1 {
        format!("choice-{choice_index}")
    } else {
        format!("group-{group_index}-choice-{choice_index}")
    }
}

fn question_line(prose: &str) -> Option<String> {
    let line = prose
        .split('\n')
        .map(str::trim)
        .rfind(|line| !line.is_empty())?;
    let hashes = line.len() - line.trim_start_matches('#').len();
    let line = if (1..=6).contains(&hashes) && line[hashes..].starts_with(char::is_whitespace) {
        line[hashes..].trim_start()
    } else {
        line
    };
    let text = unwrap_bold(line).trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn region(source: &str, from: usize, to: usize) -> FeedbackChoiceRegion {
    let raw = &source[from..to];
    let body = raw.trim_start_matches('\n');
    FeedbackChoiceRegion {
        source: body.trim_end_matches('\n').to_owned(),
        source_start: from + raw.len() - body.len(),
    }
}

struct PendingChoice {
    line: usize,
    label: String,
    first_details: Option<String>,
    /// Columns removed from detail lines; `None` removes their common indentation.
    dedent: Option<usize>,
    last_line: usize,
}

fn choice_details(lines: &[Line<'_>], choice: &PendingChoice) -> Option<String> {
    let raw: Vec<&str> = lines[choice.line + 1..=choice.last_line]
        .iter()
        .map(|line| line.text)
        .collect();
    let columns = choice.dedent.unwrap_or_else(|| {
        raw.iter()
            .filter(|text| !is_blank(text))
            .map(|text| indent_of(text))
            .min()
            .unwrap_or(0)
    });
    let mut body: Vec<&str> = raw
        .iter()
        .map(|text| dedent(text, columns).trim_end())
        .collect();
    if let Some(first) = &choice.first_details {
        body.insert(0, first);
    }
    let start = body.iter().position(|text| !text.is_empty())?;
    let end = body.iter().rposition(|text| !text.is_empty())?;
    Some(body[start..=end].join("\n"))
}

struct ScannedGroup {
    choices: Vec<PendingChoice>,
    last_line: usize,
}

/// A Markdown list; see the module documentation.
fn scan_list(lines: &[Line<'_>], first: usize) -> ScannedGroup {
    let ordered = list_item(lines[first].text).is_some_and(|item| item.ordered);
    let mut choices: Vec<PendingChoice> = Vec::new();
    let mut current_indent: Option<usize> = None;
    let mut last_line = first;
    let mut blank_before = false;
    for (index, line) in lines.iter().enumerate().skip(first) {
        let text = line.text;
        if is_blank(text) {
            blank_before = true;
            continue;
        }
        if let (Some(indent), Some(current)) = (current_indent, choices.last_mut()) {
            if indent_of(text) >= indent + 2 {
                current.last_line = index;
                last_line = index;
                blank_before = false;
                continue;
            }
        }
        if let Some(item) = list_item(text) {
            if item.ordered != ordered {
                break;
            }
            choices.push(PendingChoice {
                line: index,
                label: item.label.to_owned(),
                first_details: None,
                dedent: Some(item.content_column),
                last_line: index,
            });
            current_indent = Some(item.indent);
            last_line = index;
            blank_before = false;
            continue;
        }
        if let Some(current) = choices.last_mut() {
            if !blank_before && !interrupts_paragraph(text) {
                current.last_line = index;
                last_line = index;
                continue;
            }
        }
        break;
    }
    ScannedGroup { choices, last_line }
}

/// End of the fenced block that opens at `index` (its closing line, or the last line).
fn fence_end(lines: &[Line<'_>], index: usize) -> usize {
    let Some((marker, length)) = fence(lines[index].text) else {
        return index;
    };
    lines
        .iter()
        .enumerate()
        .skip(index + 1)
        .find(|(_, line)| fence(line.text).is_some_and(|(c, len)| c == marker && len >= length))
        .map_or(lines.len() - 1, |(next, _)| next)
}

fn continues_paragraph(text: &str) -> bool {
    !is_blank(text) && !interrupts_paragraph(text) && lettered_header(text).is_none()
}

/// A run of lettered blocks; see the module documentation and `scanLettered` in the web client.
fn scan_lettered(lines: &[Line<'_>], first: usize) -> ScannedGroup {
    let style = lettered_header(lines[first].text)
        .map(|header| header.style)
        .unwrap_or_default();
    let mut choices: Vec<PendingChoice> = Vec::new();
    let is_next = |index: usize, count: usize| {
        lettered_header(lines[index].text).is_some_and(|next| {
            next.style == style && usize::try_from(next.ordinal).is_ok_and(|n| n == count + 1)
        })
    };
    let mut index = first;
    while index < lines.len() && is_next(index, choices.len()) {
        let Some(block) = lettered_header(lines[index].text) else {
            break;
        };
        let mut current = PendingChoice {
            line: index,
            label: block.label,
            first_details: block.details,
            dedent: None,
            last_line: index,
        };
        index += 1;
        // The header's own paragraph.
        while index < lines.len() && continues_paragraph(lines[index].text) {
            current.last_line = index;
            index += 1;
        }
        // Following paragraphs: kept when indented, or when the next header follows them.
        let header_only = current.last_line == current.line && current.first_details.is_none();
        let mut probe = index;
        let mut pending_end: Option<usize> = None;
        let mut first_unit_end: Option<usize> = None;
        let mut found_next = false;
        loop {
            while probe < lines.len() && is_blank(lines[probe].text) {
                probe += 1;
            }
            if probe >= lines.len() {
                break;
            }
            if is_next(probe, choices.len() + 1) {
                if let Some(end) = pending_end {
                    current.last_line = end;
                }
                found_next = true;
                break;
            }
            let text = lines[probe].text;
            if is_heading(text) || choice_header(text).is_some() || lettered_header(text).is_some()
            {
                break;
            }
            let mut unit_end = probe;
            if fence(text).is_some() {
                unit_end = fence_end(lines, probe);
            } else {
                while unit_end + 1 < lines.len() && continues_paragraph(lines[unit_end + 1].text) {
                    unit_end += 1;
                }
            }
            if pending_end.is_none() && indent_of(text) >= block.indent + 2 {
                current.last_line = unit_end;
            } else {
                pending_end = Some(unit_end);
            }
            first_unit_end.get_or_insert(unit_end);
            probe = unit_end + 1;
        }
        // A last block written as a bare header takes the next paragraph as its details when an
        // earlier block in the run has details; one-line runs keep trailing prose as prose.
        if !found_next && header_only && current.last_line == current.line {
            if let Some(end) = first_unit_end {
                if choices
                    .iter()
                    .any(|choice| choice.last_line > choice.line || choice.first_details.is_some())
                {
                    current.last_line = end;
                }
            }
        }
        choices.push(current);
        index = if found_next { probe } else { lines.len() };
    }
    let last_line = choices.last().map_or(first, |choice| choice.last_line);
    ScannedGroup { choices, last_line }
}

/// The group a header (or a headerless lettered run) opens at `first`, if any.
fn scan_group(lines: &[Line<'_>], first: usize, explicit_header: bool) -> Option<ScannedGroup> {
    let line = lines.get(first)?;
    if explicit_header && list_item(line.text).is_some() {
        return Some(scan_list(lines, first));
    }
    if lettered_header(line.text)?.ordinal != 1 {
        return None;
    }
    let group = scan_lettered(lines, first);
    // Without a CHOICE header, one lettered paragraph is ordinary prose.
    (explicit_header || group.choices.len() >= 2).then_some(group)
}

/// Parse every choice group in a FEEDBACK NEEDED prompt; `None` when it has none. Headers inside
/// fenced code are ignored.
#[must_use]
pub fn parse_feedback_choices(source: &str) -> Option<FeedbackChoiceDocument> {
    let lines = source_lines(source);
    let mut groups: Vec<FeedbackChoiceGroup> = Vec::new();
    let mut regions = Vec::new();
    let mut open_fence: Option<(char, usize)> = None;
    let mut prose_start = 0;
    let mut line_index = 0;
    while line_index < lines.len() {
        let text = lines[line_index].text;
        let fence_match = fence(text);
        if let Some((marker, length)) = open_fence {
            if fence_match.is_some_and(|(c, len)| c == marker && len >= length) {
                open_fence = None;
            }
            line_index += 1;
            continue;
        }
        if fence_match.is_some() {
            open_fence = fence_match;
            line_index += 1;
            continue;
        }
        let header = choice_header(text);
        let scanned = if header.is_some() {
            let mut first = line_index + 1;
            while first < lines.len() && is_blank(lines[first].text) {
                first += 1;
            }
            scan_group(&lines, first, true)
        } else {
            scan_group(&lines, line_index, false)
        };
        let Some(scanned) = scanned.filter(|group| !group.choices.is_empty()) else {
            line_index += 1;
            continue;
        };
        let group_index = groups.len() + 1;
        let start = lines[line_index].start;
        let end = lines[scanned.last_line].end;
        let before = region(source, prose_start, start);
        let inline = header.clone().flatten();
        let question = inline.clone().or_else(|| question_line(&before.source));
        regions.push(before);
        groups.push(FeedbackChoiceGroup {
            index: group_index,
            start,
            end,
            question,
            inline_question: inline.is_some(),
            choices: scanned
                .choices
                .iter()
                .enumerate()
                .map(|(index, choice)| FeedbackChoice {
                    id: feedback_choice_id(group_index, index + 1),
                    label: choice.label.clone(),
                    details: choice_details(&lines, choice),
                })
                .collect(),
        });
        prose_start = end;
        line_index = scanned.last_line + 1;
    }
    if groups.is_empty() {
        return None;
    }
    regions.push(region(source, prose_start, source.len()));
    Some(FeedbackChoiceDocument { groups, regions })
}

fn choice_bullets(choices: &[&FeedbackChoice]) -> String {
    choices
        .iter()
        .map(|choice| format!("- {}", choice.label.replace('\n', "\n  ")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The selected choices of one group, in the backward-compatible `Selected choice(s):` form.
#[must_use]
pub fn selected_feedback_group_markdown(group: &FeedbackChoiceGroup, selected: &[&str]) -> String {
    let choices: Vec<&FeedbackChoice> = group
        .choices
        .iter()
        .filter(|choice| selected.contains(&choice.id.as_str()))
        .collect();
    if choices.is_empty() {
        return String::new();
    }
    let plural = if choices.len() == 1 { "" } else { "s" };
    format!("Selected choice{plural}:\n{}", choice_bullets(&choices))
}

/// Markdown listing the selected choices: a single-group prompt keeps the `Selected choice(s):`
/// form; a multi-group prompt lists each answered group under its bold question line.
#[must_use]
pub fn selected_feedback_choices_markdown(source: &str, selected: &[&str]) -> String {
    let Some(document) = parse_feedback_choices(source) else {
        return String::new();
    };
    if let [group] = document.groups.as_slice() {
        return selected_feedback_group_markdown(group, selected);
    }
    document
        .groups
        .iter()
        .filter_map(|group| {
            let choices: Vec<&FeedbackChoice> = group
                .choices
                .iter()
                .filter(|choice| selected.contains(&choice.id.as_str()))
                .collect();
            if choices.is_empty() {
                return None;
            }
            let question = group
                .question
                .clone()
                .unwrap_or_else(|| format!("Choice {}", group.index));
            Some(format!("**{question}**\n{}", choice_bullets(&choices)))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
