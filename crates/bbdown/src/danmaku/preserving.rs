use std::collections::HashSet;
use std::time::Duration;

use roxmltree::{Document, Node};

use super::{DanmakuComment, DanmakuXmlMerge, ass_timestamp};

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DanmakuAssMerge {
    pub ass: String,
    pub existing_events: usize,
    pub appended_events: usize,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct XmlKey(String, String);

#[derive(Clone, Debug)]
struct RawComment {
    key: XmlKey,
    raw: String,
    namespace: Option<String>,
}

fn invalid(message: impl Into<String>) -> crate::Error {
    crate::Error::InvalidInput(message.into())
}

fn parse_xml<'a>(xml: &'a str, label: &str) -> crate::Result<Document<'a>> {
    if xml.to_ascii_lowercase().contains("<!doctype") {
        return Err(invalid(format!("{label} XML contains a forbidden DTD")));
    }
    Document::parse(xml).map_err(|error| invalid(format!("invalid {label} XML: {error}")))
}

fn root_element<'a>(document: &'a Document<'a>, label: &str) -> crate::Result<Node<'a, 'a>> {
    let root = document.root_element();
    if root.tag_name().name() != "i" {
        return Err(invalid(format!("{label} XML root must be <i>")));
    }
    Ok(root)
}

fn root_self_closing_slash(source: &str) -> crate::Result<Option<usize>> {
    let bytes = source.as_bytes();
    let mut quote = None;
    for (index, byte) in bytes.iter().copied().enumerate().skip(1) {
        if let Some(quote_byte) = quote {
            if byte == quote_byte {
                quote = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'>' => {
                let mut previous = index;
                while previous > 0 && bytes[previous - 1].is_ascii_whitespace() {
                    previous -= 1;
                }
                let slash = previous
                    .checked_sub(1)
                    .filter(|offset| bytes[*offset] == b'/');
                return Ok(slash);
            }
            _ => {}
        }
    }
    Err(invalid("existing XML root opening tag is incomplete"))
}

fn root_qualified_name(source: &str) -> crate::Result<&str> {
    let opening = source
        .strip_prefix('<')
        .ok_or_else(|| invalid("existing XML root opening tag is missing"))?;
    let end = opening
        .find(|character: char| character.is_ascii_whitespace() || matches!(character, '/' | '>'))
        .ok_or_else(|| invalid("existing XML root opening tag is incomplete"))?;
    let name = &opening[..end];
    if name.is_empty() {
        return Err(invalid("existing XML root opening tag has no name"));
    }
    Ok(name)
}

fn xml_comments(document: &Document<'_>, xml: &str) -> Vec<RawComment> {
    document
        .root_element()
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "d")
        .filter_map(|node| {
            let parameter = node.attribute("p")?;
            let text = node
                .children()
                .filter(Node::is_text)
                .filter_map(|child| child.text())
                .collect::<String>();
            let range = node.range();
            Some(RawComment {
                key: XmlKey(parameter.to_owned(), text.clone()),
                raw: xml[range].to_owned(),
                namespace: node.tag_name().namespace().map(str::to_owned),
            })
        })
        .collect()
}

/// Append fetched `<d>` elements while preserving every byte of the existing document.
pub fn merge_xml_preserving(
    existing_xml: &str,
    fetched_xml: &str,
) -> crate::Result<DanmakuXmlMerge> {
    let fetched_document = parse_xml(fetched_xml, "fetched")?;
    let fetched_root = root_element(&fetched_document, "fetched")?;
    let fetched_comments = xml_comments(&fetched_document, fetched_xml);

    if existing_xml.trim().is_empty() {
        return Ok(DanmakuXmlMerge {
            xml: fetched_xml.to_owned(),
            existing_comments: 0,
            fetched_comments: fetched_comments.len(),
            appended_comments: fetched_comments.len(),
        });
    }

    let old_document = parse_xml(existing_xml, "existing")?;
    let old_root = root_element(&old_document, "existing")?;
    let old_comments = xml_comments(&old_document, existing_xml);
    let mut seen = old_comments
        .iter()
        .map(|comment| comment.key.clone())
        .collect::<HashSet<_>>();
    let append = fetched_comments
        .iter()
        .filter(|comment| seen.insert(comment.key.clone()))
        .collect::<Vec<_>>();
    let appended_comments = append.len();
    if append.is_empty() {
        return Ok(DanmakuXmlMerge {
            xml: existing_xml.to_owned(),
            existing_comments: old_comments.len(),
            fetched_comments: fetched_comments.len(),
            appended_comments: 0,
        });
    }

    let root_range = old_root.range();
    let root_source = &existing_xml[root_range.clone()];
    let self_closing_slash = root_self_closing_slash(root_source)?;
    let root_name = root_qualified_name(root_source)?;
    let (insertion, appended_ranges) = if let Some(slash) = self_closing_slash {
        let before_slash = root_range.start + slash;
        let mut result = String::with_capacity(
            existing_xml.len() + append.iter().map(|c| c.raw.len()).sum::<usize>() + 16,
        );
        result.push_str(&existing_xml[..before_slash]);
        result.push('>');
        let mut ranges = Vec::with_capacity(append.len());
        for comment in &append {
            ranges.push(result.len());
            result.push_str(&comment.raw);
        }
        result.push_str("</");
        result.push_str(root_name);
        result.push('>');
        result.push_str(&existing_xml[root_range.end..]);
        (result, ranges)
    } else {
        let close_tag = format!("</{root_name}");
        let root_close_rel = root_source
            .rfind(&close_tag)
            .ok_or_else(|| invalid("existing XML root closing tag is missing"))?;
        let insert_at = root_range.start + root_close_rel;
        let mut result = String::with_capacity(
            existing_xml.len() + append.iter().map(|c| c.raw.len()).sum::<usize>() + append.len(),
        );
        result.push_str(&existing_xml[..insert_at]);
        let mut ranges = Vec::with_capacity(append.len());
        for comment in &append {
            ranges.push(result.len());
            result.push_str(&comment.raw);
        }
        result.push_str(&existing_xml[insert_at..]);
        (result, ranges)
    };
    // Parse the splice as a complete document so namespace declarations and context are checked.
    let merged_document = parse_xml(&insertion, "merged")?;
    root_element(&merged_document, "merged")?;
    {
        for (source, at) in append.iter().zip(appended_ranges) {
            let node = merged_document
                .descendants()
                .find(|node| node.is_element() && node.range().start == at)
                .ok_or_else(|| invalid("an appended XML comment could not be located"))?;
            if source.namespace.as_deref() != node.tag_name().namespace() {
                return Err(invalid(
                    "fetched XML comment namespace is incompatible with existing document",
                ));
            }
        }
    }
    let _ = fetched_root;

    Ok(DanmakuXmlMerge {
        xml: insertion,
        existing_comments: old_comments.len(),
        fetched_comments: fetched_comments.len(),
        appended_comments,
    })
}

fn ass_section_lines(ass: &str) -> crate::Result<(usize, usize, Vec<&str>)> {
    let lines = ass.split_inclusive('\n').collect::<Vec<_>>();
    let mut events_start = None;
    let mut events_end = lines.len();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if events_start.is_some() {
                events_end = index;
                break;
            }
            if trimmed.eq_ignore_ascii_case("[Events]") {
                events_start = Some(index);
            }
        }
    }
    let start = events_start.ok_or_else(|| invalid("existing ASS is missing [Events]"))?;
    Ok((start, events_end, lines))
}

struct AssSection {
    name: String,
    start: usize,
    content_start: usize,
    end: usize,
}

fn ass_sections(ass: &str) -> Vec<AssSection> {
    let mut sections: Vec<AssSection> = Vec::new();
    let mut offset = 0;
    for line in ass.split_inclusive('\n') {
        let header = line.trim();
        if header.starts_with('[') && header.ends_with(']') {
            let name = &header[1..header.len() - 1];
            if !name.contains('[') && !name.contains(']') {
                if let Some(previous) = sections.last_mut() {
                    previous.end = offset;
                }
                sections.push(AssSection {
                    name: name.to_owned(),
                    start: offset,
                    content_start: offset + line.len(),
                    end: ass.len(),
                });
            }
        }
        offset += line.len();
    }
    sections
}

fn event_fields(line: &str, column_count: usize) -> Option<Vec<&str>> {
    let mut fields = Vec::with_capacity(column_count);
    let mut rest = line.trim_start().split_once(':')?.1.trim_start();
    for _ in 1..column_count {
        let comma = rest.find(',')?;
        fields.push(&rest[..comma]);
        rest = &rest[comma + 1..];
    }
    fields.push(rest.trim_end_matches(['\r', '\n']));
    Some(fields)
}

fn ass_unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('N' | 'n') => out.push('\n'),
                Some('h') => out.push('\u{00a0}'),
                Some(next @ ('\\' | '{' | '}')) => out.push(next),
                Some(next) => {
                    out.push('\\');
                    out.push(next);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn visible_text(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            result.push(ch);
            if let Some(next) = chars.next() {
                result.push(next);
            }
        } else if ch == '{' {
            let mut depth = 1;
            let mut block = String::new();
            while let Some(next) = chars.next() {
                if next == '\\' {
                    block.push(next);
                    if let Some(escaped) = chars.next() {
                        block.push(escaped);
                    }
                } else if next == '{' {
                    depth += 1;
                    block.push(next);
                } else if next == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    block.push(next);
                } else {
                    block.push(next);
                }
            }
            if depth != 0 {
                result.push('{');
                result.push_str(&block);
            }
        } else {
            result.push(ch);
        }
    }
    ass_unescape(&result)
}

fn normalized_time(time: &str) -> Option<String> {
    let (hour, tail) = time.split_once(':')?;
    let (minute, tail) = tail.split_once(':')?;
    let (second, centi) = tail.split_once('.')?;
    let h = hour.parse::<u64>().ok()?;
    let m = minute.parse::<u64>().ok()?;
    let s = second.parse::<u64>().ok()?;
    if centi.is_empty() || !centi.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let whole_seconds = h
        .checked_mul(3600)?
        .checked_add(m.checked_mul(60)?)?
        .checked_add(s)?;
    let fraction = format!("0.{centi}").parse::<f64>().ok()?;
    Some(ass_timestamp(
        Duration::from_secs(whole_seconds).as_secs_f64() + fraction,
    ))
}

fn dialogue_identity(start: &str, raw_text: &str) -> (String, String) {
    (
        normalized_time(start).unwrap_or_else(|| start.to_owned()),
        visible_text(raw_text),
    )
}

fn renderer_key(comment: &DanmakuComment) -> (String, String, u8, String, u32) {
    (
        ass_timestamp(comment.start_seconds),
        comment.text.clone(),
        comment.mode,
        format!("{:.0}", comment.font_size.round()),
        comment.color,
    )
}

fn ass_canvas(ass: &str) -> crate::Result<(u32, u32)> {
    let mut width = 1920;
    let mut height = 1080;
    let mut in_script_info = false;
    for line in ass.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_script_info = trimmed.eq_ignore_ascii_case("[Script Info]");
            continue;
        }
        if !in_script_info {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            if key.trim().eq_ignore_ascii_case("PlayResX") {
                width = value
                    .trim()
                    .parse()
                    .map_err(|_| invalid("ASS PlayResX is invalid"))?;
            } else if key.trim().eq_ignore_ascii_case("PlayResY") {
                height = value
                    .trim()
                    .parse()
                    .map_err(|_| invalid("ASS PlayResY is invalid"))?;
            }
        }
    }
    if width == 0 || height == 0 {
        return Err(invalid("ASS PlayResX and PlayResY must be positive"));
    }
    Ok((width, height))
}

fn event_line(
    comment: &DanmakuComment,
    index: usize,
    columns: &[&str],
    width: u32,
    height: u32,
) -> Option<String> {
    let text = super::ass_escape(&comment.text);
    let start = ass_timestamp(comment.start_seconds);
    let duration = if matches!(comment.mode, 4 | 5) {
        super::FIXED_DURATION_SECONDS
    } else {
        super::SCROLL_DURATION_SECONDS
    };
    let end = ass_timestamp(comment.start_seconds + duration);
    let font_size = comment.font_size.round();
    let color = super::ass_color(comment.color);
    let xmid = width / 2;
    let override_block = match comment.mode {
        4 => format!(
            "{{\\an2\\pos({xmid},{:.0})\\fs{font_size:.0}\\c{color}}}",
            f64::from(height)
                - super::TOP_MARGIN
                - super::lane_index(index, super::FIXED_LANES) * super::LINE_HEIGHT
        ),
        5 => format!(
            "{{\\an8\\pos({xmid},{:.0})\\fs{font_size:.0}\\c{color}}}",
            super::TOP_MARGIN + super::lane_index(index, super::FIXED_LANES) * super::LINE_HEIGHT
        ),
        6 => {
            let y = super::TOP_MARGIN
                + super::lane_index(index, super::SCROLL_LANES) * super::LINE_HEIGHT;
            let w = super::estimated_text_width(&comment.text, font_size);
            format!(
                "{{\\move({:.0},{y:.0},{:.0},{y:.0})\\fs{font_size:.0}\\c{color}}}",
                -w,
                f64::from(width) + w
            )
        }
        7 | 8 => return None,
        _ => {
            let y = super::TOP_MARGIN
                + super::lane_index(index, super::SCROLL_LANES) * super::LINE_HEIGHT;
            let w = super::estimated_text_width(&comment.text, font_size);
            format!(
                "{{\\move({:.0},{y:.0},{:.0},{y:.0})\\fs{font_size:.0}\\c{color}}}",
                f64::from(width) + w,
                -w
            )
        }
    };
    let values = columns
        .iter()
        .map(|column| match column.trim().to_ascii_lowercase().as_str() {
            "layer" | "marked" | "marginl" | "marginr" | "marginv" => "0".to_owned(),
            "start" => start.clone(),
            "end" => end.clone(),
            "style" => "Danmaku".to_owned(),
            "text" => format!("{override_block}{text}"),
            _ => String::new(),
        })
        .collect::<Vec<_>>();
    Some(format!("Dialogue: {}\n", values.join(",")))
}

fn style_insertion(ass: &str) -> crate::Result<Option<(usize, String)>> {
    let sections = ass_sections(ass);
    let style_section = sections.iter().find(|section| {
        section.name.eq_ignore_ascii_case("V4+ Styles")
            || section.name.eq_ignore_ascii_case("V4 Styles")
    });
    let Some(section) = style_section else {
        let events = sections
            .iter()
            .find(|section| section.name.eq_ignore_ascii_case("Events"))
            .ok_or_else(|| invalid("existing ASS is missing [Events]"))?;
        return Ok(Some((events.start, "[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Danmaku,Arial,42,&H00FFFFFF,&H000000FF,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,2,0,7,20,20,20,1\n\n".to_owned())));
    };
    let body = &ass[section.content_start..section.end];
    let format = body
        .lines()
        .find_map(|line| {
            let (name, value) = line.trim().split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("Format")
                .then_some(value.trim())
        })
        .ok_or_else(|| invalid("existing ASS style section is missing Format"))?;
    let fields = format.split(',').map(str::trim).collect::<Vec<_>>();
    let name_index = fields
        .iter()
        .position(|field| field.eq_ignore_ascii_case("Name"))
        .ok_or_else(|| invalid("existing ASS style Format has no Name column"))?;
    if body.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Style:"))
            && trimmed[6..]
                .split(',')
                .nth(name_index)
                .is_some_and(|name| name.trim().eq_ignore_ascii_case("Danmaku"))
    }) {
        return Ok(None);
    }
    let style = fields
        .iter()
        .map(|field| match field.to_ascii_lowercase().as_str() {
            "name" => "Danmaku",
            "fontname" => "Arial",
            "fontsize" => "42",
            "primarycolour" => "&H00FFFFFF",
            "secondarycolour" => "&H000000FF",
            "outlinecolour" => "&H00000000",
            "backcolour" => "&H64000000",
            "bold" | "italic" | "underline" | "strikeout" | "spacing" | "angle" | "shadow" => "0",
            "scalex" | "scaley" => "100",
            "borderstyle" | "encoding" => "1",
            "outline" => "2",
            "alignment" => "7",
            "marginl" | "marginr" | "marginv" => "20",
            _ => "",
        })
        .collect::<Vec<_>>()
        .join(",");
    let line = format!("Style: {style}\n");
    let at = body
        .rfind('\n')
        .map_or(section.end, |index| section.content_start + index + 1);
    Ok(Some((at, line)))
}

fn parsed_renderer_key(start: &str, text: &str) -> Option<(String, String, u8, String, u32)> {
    let time = normalized_time(start)?;
    let visible = visible_text(text);
    let overrides = text.split_once('}')?.0.strip_prefix('{')?;
    let (mode, after_geometry) = if let Some(rest) = overrides.strip_prefix("\\an2\\pos(") {
        (4, rest.split_once(')')?.1)
    } else if let Some(rest) = overrides.strip_prefix("\\an8\\pos(") {
        (5, rest.split_once(')')?.1)
    } else {
        let rest = overrides.strip_prefix("\\move(")?;
        let (coordinates, suffix) = rest.split_once(')')?;
        let coordinates = coordinates.split(',').collect::<Vec<_>>();
        if coordinates.len() != 4 || coordinates.iter().any(|part| part.parse::<f64>().is_err()) {
            return None;
        }
        let x1 = coordinates[0].parse::<f64>().ok()?;
        let x2 = coordinates[2].parse::<f64>().ok()?;
        (if x1 < x2 { 6 } else { 1 }, suffix)
    };
    let after_size = after_geometry.strip_prefix("\\fs")?;
    let (font_text, color_text) = after_size.split_once("\\c&H")?;
    if !color_text.ends_with('&') || color_text.len() != 7 {
        return None;
    }
    let fontsize = font_text.parse::<u32>().ok()?.to_string();
    let bgr = u32::from_str_radix(&color_text[..6], 16).ok()?;
    let color = ((bgr & 0xff) << 16) | (bgr & 0xff00) | ((bgr >> 16) & 0xff);
    Some((time, visible, mode, fontsize, color))
}

#[derive(Default)]
struct ExistingAssEvents {
    event_count: usize,
    dialogue_count: usize,
    custom_keys: HashSet<(String, String)>,
    renderer_keys: HashSet<(String, String, u8, String, u32)>,
}

fn event_columns<'a>(
    lines: &'a [&str],
    start: usize,
    end: usize,
) -> crate::Result<(Vec<&'a str>, usize, usize)> {
    let format_line = lines[start + 1..end]
        .iter()
        .find(|line| {
            line.trim_start()
                .to_ascii_lowercase()
                .starts_with("format:")
        })
        .ok_or_else(|| invalid("existing ASS [Events] is missing Format"))?;
    let format = format_line
        .split_once(':')
        .map(|(_, value)| value.trim())
        .ok_or_else(|| invalid("invalid ASS Events Format"))?;
    let columns = format.split(',').map(str::trim).collect::<Vec<_>>();
    let text_index = columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case("Text"))
        .ok_or_else(|| invalid("ASS Events Format has no Text column"))?;
    let start_index = columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case("Start"))
        .ok_or_else(|| invalid("ASS Events Format has no Start column"))?;
    if text_index != columns.len() - 1 {
        return Err(invalid(
            "ASS Events Format must place Text last for reliable comma parsing",
        ));
    }
    Ok((columns, start_index, text_index))
}

fn existing_ass_events(
    lines: &[&str],
    section_start: usize,
    section_end: usize,
    column_count: usize,
    start_index: usize,
    text_index: usize,
) -> crate::Result<ExistingAssEvents> {
    let mut events = ExistingAssEvents::default();
    for line in &lines[section_start + 1..section_end] {
        let trimmed = line.trim_start();
        if trimmed.to_ascii_lowercase().starts_with("comment:") {
            events.event_count += 1;
        }
        if !trimmed.to_ascii_lowercase().starts_with("dialogue:") {
            continue;
        }
        events.event_count += 1;
        events.dialogue_count += 1;
        let fields = event_fields(line, column_count)
            .ok_or_else(|| invalid("malformed ASS Dialogue event"))?;
        let start = fields
            .get(start_index)
            .ok_or_else(|| invalid("ASS Dialogue has no Start field"))?
            .trim();
        let text = fields
            .get(text_index)
            .ok_or_else(|| invalid("ASS Dialogue has no Text field"))?;
        let identity = dialogue_identity(start, text);
        if let Some(key) = parsed_renderer_key(start, text) {
            events.renderer_keys.insert(key);
        } else {
            events.custom_keys.insert(identity);
        }
    }
    Ok(events)
}

fn baseline_keys(baseline: Option<&str>) -> crate::Result<HashSet<XmlKey>> {
    let Some(xml) = baseline else {
        return Ok(HashSet::new());
    };
    let document = parse_xml(xml, "baseline")?;
    root_element(&document, "baseline")?;
    Ok(xml_comments(&document, xml)
        .into_iter()
        .map(|item| item.key)
        .collect())
}

fn append_new_ass_events(
    incoming: &[(&RawComment, DanmakuComment)],
    baseline: Option<&str>,
    old_keys: &HashSet<XmlKey>,
    existing: &ExistingAssEvents,
    columns: &[&str],
    width: u32,
    height: u32,
) -> (String, usize) {
    let mut xml_seen = HashSet::new();
    let mut append_lines = String::new();
    let mut appended = 0;
    let mut seen = HashSet::new();
    for (item, comment) in incoming {
        if matches!(comment.mode, 7 | 8) {
            continue;
        }
        if baseline.is_some()
            && (old_keys.contains(&item.key) || !xml_seen.insert(item.key.clone()))
        {
            continue;
        }
        let identity = (ass_timestamp(comment.start_seconds), comment.text.clone());
        let rendered = renderer_key(comment);
        if baseline.is_none()
            && (existing.renderer_keys.contains(&rendered)
                || existing.custom_keys.contains(&identity))
        {
            continue;
        }
        let key = if baseline.is_some() {
            (item.key.0.clone(), item.key.1.clone(), 0, String::new(), 0)
        } else {
            rendered
        };
        if !seen.insert(key) {
            continue;
        }
        if let Some(line) = event_line(
            comment,
            existing.dialogue_count + appended,
            columns,
            width,
            height,
        ) {
            append_lines.push_str(&line);
            appended += 1;
        }
    }
    (append_lines, appended)
}

fn insert_ass_events(existing_ass: &str, append_lines: &str) -> crate::Result<String> {
    let mut ass_result = existing_ass.to_owned();
    if let Some((style_at, style_line)) = style_insertion(existing_ass)? {
        ass_result.insert_str(style_at, &style_line);
    }
    let (_, events_end, lines) = ass_section_lines(&ass_result)?;
    let insertion_at = if events_end < lines.len() {
        lines[..events_end]
            .iter()
            .map(|line| line.len())
            .sum::<usize>()
    } else {
        ass_result.len()
    };
    let mut ass = String::with_capacity(ass_result.len() + append_lines.len());
    ass.push_str(&ass_result[..insertion_at]);
    if insertion_at > 0 && !ass.ends_with('\n') {
        ass.push('\n');
    }
    ass.push_str(append_lines);
    ass.push_str(&ass_result[insertion_at..]);
    Ok(ass)
}

/// Preserve an existing ASS document and append newly fetched renderer events.
pub fn merge_ass_preserving(
    existing_ass: &str,
    fetched_xml: &str,
    baseline_xml: Option<&str>,
) -> crate::Result<DanmakuAssMerge> {
    let incoming_document = parse_xml(fetched_xml, "fetched")?;
    root_element(&incoming_document, "fetched")?;
    let incoming_items = xml_comments(&incoming_document, fetched_xml);
    let incoming = incoming_items
        .iter()
        .filter_map(|item| {
            super::parse_comment(&item.key.0, &item.key.1).map(|comment| (item, comment))
        })
        .collect::<Vec<_>>();
    let (events_start, events_end, lines) = if existing_ass.trim().is_empty() {
        let initialized = super::xml_to_ass(fetched_xml);
        let count = initialized.matches("Dialogue:").count();
        return Ok(DanmakuAssMerge {
            ass: initialized,
            existing_events: 0,
            appended_events: count,
        });
    } else {
        ass_section_lines(existing_ass)?
    };
    let baseline = baseline_xml.filter(|xml| !xml.trim().is_empty());
    let (columns, start_index, text_index) = event_columns(&lines, events_start, events_end)?;
    let existing = existing_ass_events(
        &lines,
        events_start,
        events_end,
        columns.len(),
        start_index,
        text_index,
    )?;
    let old_keys = baseline_keys(baseline)?;
    let (play_res_x, play_res_y) = ass_canvas(existing_ass)?;
    let (append_lines, appended_events) = append_new_ass_events(
        &incoming, baseline, &old_keys, &existing, &columns, play_res_x, play_res_y,
    );
    if appended_events == 0 {
        return Ok(DanmakuAssMerge {
            ass: existing_ass.to_owned(),
            existing_events: existing.event_count,
            appended_events: 0,
        });
    }
    let ass = insert_ass_events(existing_ass, &append_lines)?;
    Ok(DanmakuAssMerge {
        ass,
        existing_events: existing.event_count,
        appended_events,
    })
}

#[cfg(test)]
mod tests {
    use super::{merge_ass_preserving, merge_xml_preserving};

    #[test]
    fn xml_preserves_raw_nodes_duplicates_and_entities() -> crate::Result<()> {
        let old = "<?xml version='1.0'?>\n<i foo='x&amp;y'>\n<!--keep--><meta a=\"1\">&lt;ok&gt;<![CDATA[z]]></meta><d p='1,1,25,0'>x&amp;y</d><d p='1,1,25,0'>x&amp;y</d></i>";
        let fetched = "<i><unknown/><d p=\"1,1,25,0\">x&#38;y</d><d p='2,1,25,0'>new<![CDATA[!]]></d><d p='2,1,25,0'>new<![CDATA[!]]></d></i>";
        let merged = merge_xml_preserving(old, fetched)?;
        assert_eq!(merged.existing_comments, 2);
        assert_eq!(merged.fetched_comments, 3);
        assert_eq!(merged.appended_comments, 1);
        assert!(merged.xml.starts_with("<?xml version='1.0'?>\n<i foo='x&amp;y'>\n<!--keep--><meta a=\"1\">&lt;ok&gt;<![CDATA[z]]></meta>"));
        assert!(
            merged
                .xml
                .contains("<d p='1,1,25,0'>x&amp;y</d><d p='1,1,25,0'>x&amp;y</d>")
        );
        assert!(
            merged
                .xml
                .ends_with("<d p='2,1,25,0'>new<![CDATA[!]]></d></i>")
        );
        Ok(())
    }

    #[test]
    fn xml_noop_is_byte_identical_and_self_closing_root_expands() -> crate::Result<()> {
        let old = "<i a=\"x\"><d p='1,1,25,0'>same</d></i>";
        assert_eq!(merge_xml_preserving(old, old)?.xml, old);
        let merged = merge_xml_preserving("<i a='keep' />", "<i><d p='2,1,25,0'>new</d></i>")?;
        assert!(merged.xml.contains("<i a='keep' >"));
        assert!(merged.xml.contains("<d p='2,1,25,0'>new</d></i>"));
        Ok(())
    }

    #[test]
    fn xml_root_attributes_with_terminator_characters_are_not_self_closing() -> crate::Result<()> {
        let old = "<i marker=\"/>\"><meta>keep</meta></i>";
        let fetched = "<i><d p='2,1,25,0'>new</d></i>";
        let merged = merge_xml_preserving(old, fetched)?;
        assert_eq!(
            merged.xml,
            "<i marker=\"/>\"><meta>keep</meta><d p='2,1,25,0'>new</d></i>"
        );

        let self_closing = merge_xml_preserving("<i marker=\"/>\" />", fetched)?;
        assert_eq!(
            self_closing.xml,
            "<i marker=\"/>\" ><d p='2,1,25,0'>new</d></i>"
        );
        Ok(())
    }

    #[test]
    fn xml_preserves_prefixed_root_names_for_normal_and_self_closing_roots() -> crate::Result<()> {
        let fetched = "<i xmlns='urn:test'><d p='2,1,25,0'>new</d></i>";
        let old = "<b:i xmlns:b='urn:test' xmlns='urn:test'><b:meta>keep</b:meta></b:i>";
        let merged = merge_xml_preserving(old, fetched)?;
        assert_eq!(merged.appended_comments, 1);
        assert!(merged.xml.ends_with("<d p='2,1,25,0'>new</d></b:i>"));

        let unqualified_fetched = "<b:i xmlns:b='urn:test'><d p='3,1,25,0'>plain</d></b:i>";
        let unqualified_old = "<b:i xmlns:b='urn:test'><b:meta>keep</b:meta></b:i>";
        let merged = merge_xml_preserving(unqualified_old, unqualified_fetched)?;
        assert_eq!(merged.appended_comments, 1);
        assert!(merged.xml.ends_with("<d p='3,1,25,0'>plain</d></b:i>"));

        let self_closing = merge_xml_preserving("<b:i xmlns:b='urn:test' />", unqualified_fetched)?;
        assert_eq!(self_closing.appended_comments, 1);
        assert!(
            self_closing
                .xml
                .ends_with("<d p='3,1,25,0'>plain</d></b:i>")
        );
        Ok(())
    }

    #[test]
    fn xml_namespace_validation_uses_appended_byte_ranges() -> crate::Result<()> {
        let fetched = "<i><d p='2,1,25,0'>new</d></i>";
        let shadowed_in_comment = "<i xmlns='urn:old'><!--<d p='2,1,25,0'>new</d>--></i>";
        let shadowed_in_cdata = "<i xmlns='urn:old'><![CDATA[<d p='2,1,25,0'>new</d>]]></i>";
        assert!(merge_xml_preserving(shadowed_in_comment, fetched).is_err());
        assert!(merge_xml_preserving(shadowed_in_cdata, fetched).is_err());

        let shadowed_later_comment = "<i xmlns='urn:old'><!--<d p='3,1,25,0'>later</d>--></i>";
        let multiple_fetched = "<i><d xmlns='' p='2,1,25,0'>first</d><d p='3,1,25,0'>later</d></i>";
        assert!(merge_xml_preserving(shadowed_later_comment, multiple_fetched).is_err());

        let compatible_old = "<i xmlns='urn:test'></i>";
        let compatible_fetched = "<i xmlns='urn:test'><d p='2,1,25,0'>new</d></i>";
        assert_eq!(
            merge_xml_preserving(compatible_old, compatible_fetched)?.appended_comments,
            1
        );
        Ok(())
    }

    #[test]
    fn xml_rejects_malformed_and_incompatible_documents() {
        assert!(merge_xml_preserving("<i><d>", "<i/>").is_err());
        assert!(merge_xml_preserving("<i/>", "<i><d>").is_err());
        assert!(
            merge_xml_preserving(
                "<i><d p='1'>a</d></i>",
                "<i xmlns='urn:x'><d p='2'>b</d></i>"
            )
            .is_err()
        );
        assert!(merge_xml_preserving("<!DOCTYPE i><i/>", "<i/>").is_err());
    }

    #[test]
    fn ass_preserves_custom_data_reorders_columns_and_uses_old_canvas() -> crate::Result<()> {
        let old = "[Script Info]\nTitle: keep\nPlayResX: 1280\nPlayResY: 720\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour\nStyle: Custom,Arial,20,&H00FFFFFF\n\n[Events]\nFormat: Start, Layer, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nComment: 0:00:00.00,0,0:00:01.00,Custom,,0,0,0,,note, with comma\nDialogue: 0:00:01.00,0,0:00:09.00,Custom,,0,0,0,,old\n[Custom]\nKeep: exact\n";
        let fetched = "<i><d p='1,1,25,0'>old</d><d p='2,4,25,0'>new, with comma\nline</d></i>";
        let merged = merge_ass_preserving(old, fetched, None)?;
        assert_eq!(merged.existing_events, 2);
        assert_eq!(merged.appended_events, 1);
        assert!(merged.ass.contains("Style: Custom,Arial,20,&H00FFFFFF\n"));
        assert!(merged.ass.contains("Style: Danmaku,Arial,42,&H00FFFFFF\n"));
        assert!(merged.ass.contains("[Custom]\nKeep: exact\n"));
        assert!(
            merged
                .ass
                .contains("Dialogue: 0:00:02.00,0,0:00:06.00,Danmaku,,0,0,0,,")
        );
        assert!(merged.ass.contains("new, with comma\\Nline"));
        assert!(merged.ass.contains("\\pos(640,628)"));
        Ok(())
    }

    #[test]
    fn ass_section_comment_with_brackets_does_not_end_styles_section() -> crate::Result<()> {
        let old = "[Script Info]\n\n[V4+ Styles]\n; [custom note]\nFormat: Name, Fontname, Fontsize, PrimaryColour\nStyle: Custom,Arial,20,&H00FFFFFF\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n";
        let fetched = "<i><d p='3,1,25,0'>new</d></i>";
        let merged = merge_ass_preserving(old, fetched, None)?;
        assert_eq!(merged.appended_events, 1);
        assert!(
            merged
                .ass
                .contains("[V4+ Styles]\n; [custom note]\nFormat:")
        );
        assert!(merged.ass.contains("Style: Custom,Arial,20,&H00FFFFFF\n"));
        assert!(merged.ass.contains("Style: Danmaku,Arial,42,&H00FFFFFF\n"));
        assert!(merged.ass.contains("[Events]\nFormat:"));
        Ok(())
    }

    #[test]
    fn ass_only_refresh_is_idempotent_but_distinct_modes_append() -> crate::Result<()> {
        let old = "[Script Info]\nPlayResX: 1920\nPlayResY: 1080\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:09.00,Danmaku,,0,0,0,,{\\move(1500,40,-100,40)\\fs25\\c&HFFFFFF&}same\n";
        let fetched = "<i><d p='1,1,25,16777215'>same</d><d p='1,4,25,16777215'>same</d></i>";
        let merged = merge_ass_preserving(old, fetched, None)?;
        assert_eq!(merged.appended_events, 1);
        assert!(merged.ass.contains("\\an2\\pos"));
        let refreshed = merge_ass_preserving(&merged.ass, fetched, None)?;
        assert_eq!(refreshed.appended_events, 0);
        assert_eq!(refreshed.ass, merged.ass);
        Ok(())
    }

    #[test]
    fn ass_only_refresh_appends_same_text_when_font_size_or_color_differs() -> crate::Result<()> {
        let cases = [
            ("24", "FFFFFF", "1,1,25,16777215"),
            ("25", "FFFFFF", "1,1,25,16711680"),
        ];
        for (font_size, color, parameters) in cases {
            let existing = format!(
                "[Script Info]\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.00,0:00:09.00,Danmaku,,0,0,0,,{{\\move(1500,40,-100,40)\\fs{font_size}\\c&H{color}&}}same\n"
            );
            let fetched = format!("<i><d p='{parameters}'>same</d></i>");
            let merged = merge_ass_preserving(&existing, &fetched, None)?;
            assert_eq!(merged.appended_events, 1, "font={font_size}, color={color}");
        }
        Ok(())
    }

    #[test]
    fn xml_baseline_distinct_key_is_not_ass_visual_deduped() -> crate::Result<()> {
        let old_xml = "<i><d p='1,1,25,0'>same</d></i>";
        let fetched = "<i><d p='1,1,25,0'>same</d><d p='1,1,25,0,id=other'>same</d></i>";
        let old_ass = "[Script Info]\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n";
        let merged = merge_ass_preserving(old_ass, fetched, Some(old_xml))?;
        assert_eq!(merged.appended_events, 1);
        assert_eq!(merge_xml_preserving(old_xml, fetched)?.appended_comments, 1);
        Ok(())
    }

    #[test]
    fn invalid_ass_is_an_error() {
        assert!(merge_ass_preserving("[Script Info]\nTitle: old\n", "<i/>", None).is_err());
    }
}
