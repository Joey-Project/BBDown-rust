use std::collections::HashSet;
use std::fmt::Write as _;
use std::time::Duration;

const DANMAKU_FORMAT_ORDER: [DanmakuFormat; 2] = [DanmakuFormat::Xml, DanmakuFormat::Ass];
const PLAY_RES_X: u32 = 1920;
const PLAY_RES_Y: u32 = 1080;
const DEFAULT_FONT_SIZE: f64 = 42.0;
const SCROLL_DURATION_SECONDS: f64 = 8.0;
const FIXED_DURATION_SECONDS: f64 = 4.0;
const TOP_MARGIN: f64 = 40.0;
const LINE_HEIGHT: f64 = 52.0;
const SCROLL_LANES: usize = 15;
const FIXED_LANES: usize = 8;

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DanmakuFormat {
    #[default]
    Xml,
    Ass,
}

impl DanmakuFormat {
    pub(crate) const fn archive_key_token(self) -> &'static str {
        match self {
            Self::Xml => "xml",
            Self::Ass => "ass",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DanmakuFormats {
    formats: Vec<DanmakuFormat>,
}

impl Default for DanmakuFormats {
    fn default() -> Self {
        Self {
            formats: vec![DanmakuFormat::Xml],
        }
    }
}

impl DanmakuFormats {
    #[must_use]
    pub fn new(formats: impl IntoIterator<Item = DanmakuFormat>) -> Self {
        let requested = formats.into_iter().collect::<Vec<_>>();
        let mut normalized = DANMAKU_FORMAT_ORDER
            .into_iter()
            .filter(|format| requested.contains(format))
            .collect::<Vec<_>>();
        if normalized.is_empty() {
            normalized.push(DanmakuFormat::Xml);
        }
        Self {
            formats: normalized,
        }
    }

    #[must_use]
    pub fn as_slice(&self) -> &[DanmakuFormat] {
        &self.formats
    }

    #[must_use]
    pub fn contains(&self, format: DanmakuFormat) -> bool {
        self.formats.contains(&format)
    }

    pub(crate) fn archive_key_token(&self) -> String {
        self.formats
            .iter()
            .map(|format| format.archive_key_token())
            .collect::<Vec<_>>()
            .join("+")
    }

    pub(crate) fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub(crate) fn non_default_combinations() -> Vec<Self> {
        let mut combinations = vec![Self {
            formats: Vec::new(),
        }];
        for format in DANMAKU_FORMAT_ORDER {
            let additions = combinations
                .iter()
                .map(|combination| {
                    let mut formats = combination.formats.clone();
                    formats.push(format);
                    Self { formats }
                })
                .collect::<Vec<_>>();
            combinations.extend(additions);
        }
        combinations
            .into_iter()
            .filter(|combination| {
                !combination.formats.is_empty() && *combination != Self::default()
            })
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DanmakuXmlMerge {
    pub xml: String,
    pub existing_comments: usize,
    pub fetched_comments: usize,
    pub appended_comments: usize,
}

#[must_use]
pub fn merge_xml_append_only(existing_xml: &str, fetched_xml: &str) -> DanmakuXmlMerge {
    let existing_comments = xml_comment_blocks(existing_xml);
    let fetched_comments = xml_comment_blocks(fetched_xml);
    if existing_xml.trim().is_empty() || existing_comments.is_empty() {
        return DanmakuXmlMerge {
            xml: fetched_xml.to_owned(),
            existing_comments: existing_comments.len(),
            fetched_comments: fetched_comments.len(),
            appended_comments: fetched_comments.len(),
        };
    }

    let mut seen = existing_comments
        .iter()
        .map(|comment| comment.key.clone())
        .collect::<HashSet<_>>();
    let appended = fetched_comments
        .iter()
        .filter(|comment| seen.insert(comment.key.clone()))
        .map(|comment| comment.block.clone())
        .collect::<Vec<_>>();
    let appended_comments = appended.len();
    let last_complete_comment_end = existing_comments.last().map(|comment| comment.end);
    let xml = append_comment_blocks(existing_xml, &appended, last_complete_comment_end);
    DanmakuXmlMerge {
        xml,
        existing_comments: existing_comments.len(),
        fetched_comments: fetched_comments.len(),
        appended_comments,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DanmakuXmlCommentBlock {
    key: String,
    block: String,
    end: usize,
}

fn xml_comment_blocks(xml: &str) -> Vec<DanmakuXmlCommentBlock> {
    let mut comments = Vec::new();
    let mut offset = 0;
    while let Some(tag_start_relative) = xml[offset..].find("<d") {
        let tag_start = offset + tag_start_relative;
        let name_tail = tag_start + 2;
        let Some(next) = xml[name_tail..].chars().next() else {
            break;
        };
        if !(next.is_ascii_whitespace() || matches!(next, '>' | '/')) {
            offset = name_tail;
            continue;
        }
        let Some(tag_end_relative) = xml[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + tag_end_relative;
        let content_start = tag_end + 1;
        let Some(close_relative) = xml[content_start..].find("</d>") else {
            break;
        };
        let content_end = content_start + close_relative;
        offset = content_end + "</d>".len();

        let tag = &xml[tag_start..tag_end];
        let parameters = attribute_value(tag, "p").unwrap_or_default();
        let text = &xml[content_start..content_end];
        comments.push(DanmakuXmlCommentBlock {
            key: format!("{parameters}\0{}", xml_unescape(text)),
            block: xml[tag_start..offset].to_owned(),
            end: offset,
        });
    }
    comments
}

fn append_comment_blocks(
    existing_xml: &str,
    blocks: &[String],
    last_complete_comment_end: Option<usize>,
) -> String {
    if blocks.is_empty() {
        return existing_xml.to_owned();
    }
    let insertion_at = existing_xml
        .rfind("</i>")
        .or(last_complete_comment_end)
        .unwrap_or(existing_xml.len());
    let mut output = String::with_capacity(
        existing_xml.len() + blocks.iter().map(String::len).sum::<usize>() + blocks.len() + 1,
    );
    output.push_str(&existing_xml[..insertion_at]);
    if !output.ends_with('\n') {
        output.push('\n');
    }
    for block in blocks {
        output.push_str(block);
        output.push('\n');
    }
    output.push_str(&existing_xml[insertion_at..]);
    output
}

#[derive(Clone, Debug, PartialEq)]
struct DanmakuComment {
    start_seconds: f64,
    mode: u8,
    font_size: f64,
    color: u32,
    text: String,
}

pub(crate) fn xml_to_ass(xml: &str) -> String {
    let comments = parse_comments(xml);
    render_ass(&comments)
}

fn parse_comments(xml: &str) -> Vec<DanmakuComment> {
    let mut comments = Vec::new();
    let mut offset = 0;
    while let Some(tag_start_relative) = xml[offset..].find("<d") {
        let tag_start = offset + tag_start_relative;
        let name_tail = tag_start + 2;
        let Some(next) = xml[name_tail..].chars().next() else {
            break;
        };
        if !(next.is_ascii_whitespace() || matches!(next, '>' | '/')) {
            offset = name_tail;
            continue;
        }
        let Some(tag_end_relative) = xml[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + tag_end_relative;
        let content_start = tag_end + 1;
        let Some(close_relative) = xml[content_start..].find("</d>") else {
            break;
        };
        let content_end = content_start + close_relative;
        offset = content_end + "</d>".len();

        let Some(parameters) = attribute_value(&xml[tag_start..tag_end], "p") else {
            continue;
        };
        if let Some(comment) = parse_comment(&parameters, &xml[content_start..content_end]) {
            comments.push(comment);
        }
    }
    comments
}

fn parse_comment(parameters: &str, raw_text: &str) -> Option<DanmakuComment> {
    let mut parts = parameters.split(',');
    let start_seconds = parts.next()?.parse::<f64>().ok()?;
    if !start_seconds.is_finite() {
        return None;
    }
    let mode = parts.next()?.parse::<u8>().ok()?;
    let raw_font_size = parts
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(DEFAULT_FONT_SIZE);
    let font_size = if raw_font_size.is_finite() {
        raw_font_size.clamp(12.0, 96.0)
    } else {
        DEFAULT_FONT_SIZE
    };
    let color = parts
        .next()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0xFF_FF_FF)
        & 0xFF_FF_FF;
    let text = xml_unescape(raw_text);
    if text.is_empty() {
        return None;
    }
    Some(DanmakuComment {
        start_seconds: start_seconds.max(0.0),
        mode,
        font_size,
        color,
        text,
    })
}

fn attribute_value(tag: &str, name: &str) -> Option<String> {
    let bytes = tag.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        while index < bytes.len() && !is_attribute_name_char(bytes[index]) {
            index += 1;
        }
        let name_start = index;
        while index < bytes.len() && is_attribute_name_char(bytes[index]) {
            index += 1;
        }
        if name_start == index {
            continue;
        }
        let attr_name = &tag[name_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if bytes.get(index) != Some(&b'=') {
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let quote = bytes.get(index).copied()?;
        if !matches!(quote, b'\'' | b'"') {
            continue;
        }
        index += 1;
        let value_start = index;
        while index < bytes.len() && bytes[index] != quote {
            index += 1;
        }
        if index >= bytes.len() {
            return None;
        }
        if attr_name == name {
            return Some(xml_unescape(&tag[value_start..index]));
        }
        index += 1;
    }
    None
}

const fn is_attribute_name_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b':')
}

fn render_ass(comments: &[DanmakuComment]) -> String {
    let mut output = String::new();
    output.push_str("[Script Info]\n");
    output.push_str("ScriptType: v4.00+\n");
    output.push_str("WrapStyle: 2\n");
    output.push_str("ScaledBorderAndShadow: yes\n");
    let _ = writeln!(output, "PlayResX: {PLAY_RES_X}");
    let _ = writeln!(output, "PlayResY: {PLAY_RES_Y}");
    output.push('\n');
    output.push_str("[V4+ Styles]\n");
    output.push_str("Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n");
    output.push_str("Style: Danmaku,Arial,42,&H00FFFFFF,&H000000FF,&H00000000,&H64000000,0,0,0,0,100,100,0,0,1,2,0,7,20,20,20,1\n");
    output.push('\n');
    output.push_str("[Events]\n");
    output.push_str(
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
    );
    for (index, comment) in comments.iter().enumerate() {
        if let Some(line) = render_dialogue(comment, index) {
            output.push_str(&line);
        }
    }
    output
}

fn render_dialogue(comment: &DanmakuComment, index: usize) -> Option<String> {
    if matches!(comment.mode, 7 | 8) {
        return None;
    }
    let text = ass_escape(&comment.text);
    if text.is_empty() {
        return None;
    }
    let duration = if matches!(comment.mode, 4 | 5) {
        FIXED_DURATION_SECONDS
    } else {
        SCROLL_DURATION_SECONDS
    };
    let start = ass_timestamp(comment.start_seconds);
    let end = ass_timestamp(comment.start_seconds + duration);
    let font_size = comment.font_size.round();
    let color = ass_color(comment.color);
    let override_block = match comment.mode {
        4 => {
            let y = bottom_lane_y(index);
            format!(
                "{{\\an2\\pos({},{y:.0})\\fs{font_size:.0}\\c{color}}}",
                PLAY_RES_X / 2
            )
        }
        5 => {
            let y = top_lane_y(index, FIXED_LANES);
            format!(
                "{{\\an8\\pos({},{y:.0})\\fs{font_size:.0}\\c{color}}}",
                PLAY_RES_X / 2
            )
        }
        6 => {
            let y = top_lane_y(index, SCROLL_LANES);
            let width = estimated_text_width(&comment.text, font_size);
            format!(
                "{{\\move({:.0},{y:.0},{:.0},{y:.0})\\fs{font_size:.0}\\c{color}}}",
                -width,
                f64::from(PLAY_RES_X) + width
            )
        }
        _ => {
            let y = top_lane_y(index, SCROLL_LANES);
            let width = estimated_text_width(&comment.text, font_size);
            format!(
                "{{\\move({:.0},{y:.0},{:.0},{y:.0})\\fs{font_size:.0}\\c{color}}}",
                f64::from(PLAY_RES_X) + width,
                -width
            )
        }
    };
    Some(format!(
        "Dialogue: 0,{start},{end},Danmaku,,0,0,0,,{override_block}{text}\n"
    ))
}

fn top_lane_y(index: usize, lane_count: usize) -> f64 {
    TOP_MARGIN + (lane_index(index, lane_count) * LINE_HEIGHT)
}

fn bottom_lane_y(index: usize) -> f64 {
    f64::from(PLAY_RES_Y) - TOP_MARGIN - (lane_index(index, FIXED_LANES) * LINE_HEIGHT)
}

fn lane_index(index: usize, lane_count: usize) -> f64 {
    u32::try_from(index % lane_count).map_or(0.0, f64::from)
}

fn estimated_text_width(text: &str, font_size: f64) -> f64 {
    let chars = u32::try_from(text.chars().count().max(1)).map_or(f64::from(u32::MAX), f64::from);
    (chars * font_size * 0.72).max(font_size * 2.0)
}

fn ass_timestamp(seconds: f64) -> String {
    let safe_seconds = if seconds.is_finite() {
        seconds.clamp(0.0, 3_599_999.0)
    } else {
        0.0
    };
    let centiseconds = Duration::from_secs_f64(safe_seconds + 0.005).as_millis() / 10;
    let total_seconds = centiseconds / 100;
    let centisecond = centiseconds % 100;
    let second = total_seconds % 60;
    let minute = (total_seconds / 60) % 60;
    let hour = total_seconds / 3600;
    format!("{hour}:{minute:02}:{second:02}.{centisecond:02}")
}

fn ass_color(color: u32) -> String {
    let red = (color >> 16) & 0xFF;
    let green = (color >> 8) & 0xFF;
    let blue = color & 0xFF;
    format!("&H{blue:02X}{green:02X}{red:02X}&")
}

fn ass_escape(text: &str) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut output = String::with_capacity(normalized.len());
    for character in normalized.chars() {
        match character {
            '\n' => output.push_str("\\N"),
            '\\' => output.push_str("\\\\"),
            '{' => output.push_str("\\{"),
            '}' => output.push_str("\\}"),
            character if character.is_control() => {}
            character => output.push(character),
        }
    }
    output
}

fn xml_unescape(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut index = 0;
    while let Some(ampersand_relative) = text[index..].find('&') {
        let ampersand = index + ampersand_relative;
        output.push_str(&text[index..ampersand]);
        let entity_start = ampersand + 1;
        let Some(semicolon_relative) = text[entity_start..].find(';') else {
            output.push_str(&text[ampersand..]);
            return output;
        };
        let semicolon = entity_start + semicolon_relative;
        let entity = &text[entity_start..semicolon];
        if let Some(decoded) = decode_xml_entity(entity) {
            output.push(decoded);
            index = semicolon + 1;
        } else {
            output.push('&');
            index = entity_start;
        }
    }
    output.push_str(&text[index..]);
    output
}

fn decode_xml_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        entity if entity.starts_with("#x") || entity.starts_with("#X") => {
            u32::from_str_radix(&entity[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        entity if entity.starts_with('#') => {
            entity[1..].parse::<u32>().ok().and_then(char::from_u32)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{merge_xml_append_only, xml_to_ass};

    #[test]
    fn converts_common_xml_comments_to_ass_dialogues() {
        let ass = xml_to_ass(
            r#"<i>
                <d p="1.25,1,25,16711680,0,0,0,0">hello &amp; {world}</d>
                <d p="2,5,30,255,0,0,0,0">top</d>
                <d p="3,4,18,65280,0,0,0,0">bottom</d>
                <d p="4,7,25,16777215,0,0,0,0">special</d>
            </i>"#,
        );

        assert!(ass.contains("[Script Info]"));
        assert!(ass.contains("PlayResX: 1920"));
        assert_eq!(ass.matches("Dialogue:").count(), 3);
        assert!(ass.contains("0:00:01.25"));
        assert!(ass.contains("\\move"));
        assert!(ass.contains("\\an8"));
        assert!(ass.contains("\\an2"));
        assert!(ass.contains("&H0000FF&"));
        assert!(ass.contains(r"hello & \{world\}"));
        assert!(!ass.contains("special"));
    }

    #[test]
    fn decodes_numeric_entities_and_single_quoted_attributes() {
        let ass = xml_to_ass(r"<i><d p='0,6,25,0,0,0,0,0'>&#x5f39;&#24149;&#10;left</d></i>");

        assert_eq!(ass.matches("Dialogue:").count(), 1);
        assert!(ass.contains("弹幕\\Nleft"));
        assert!(ass.contains("&H000000&"));
    }

    #[test]
    fn merge_xml_append_only_adds_new_comments_before_root_close() {
        let merged = merge_xml_append_only(
            r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#,
            r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#,
        );

        assert_eq!(merged.existing_comments, 1);
        assert_eq!(merged.fetched_comments, 2);
        assert_eq!(merged.appended_comments, 1);
        assert_eq!(merged.xml.matches("<d ").count(), 2);
        assert!(
            merged
                .xml
                .contains("old</d>\n<d p=\"2,1,25,0,0,0,0,0\">new</d>\n</i>")
        );
    }

    #[test]
    fn merge_xml_append_only_keeps_existing_when_no_new_comments() {
        let existing = r#"<i><d p="1,1,25,0,0,0,0,0">old</d></i>"#;
        let merged = merge_xml_append_only(existing, existing);

        assert_eq!(merged.appended_comments, 0);
        assert_eq!(merged.xml, existing);
    }

    #[test]
    fn merge_xml_append_only_deduplicates_equivalent_entities() {
        let merged = merge_xml_append_only(
            r#"<i><d p="1,1,25,0,0,0,0,0">a&amp;b</d></i>"#,
            r#"<i><d p="1,1,25,0,0,0,0,0">a&#38;b</d></i>"#,
        );

        assert_eq!(merged.existing_comments, 1);
        assert_eq!(merged.fetched_comments, 1);
        assert_eq!(merged.appended_comments, 0);
        assert_eq!(merged.xml.matches("<d ").count(), 1);
    }

    #[test]
    fn merge_xml_append_only_uses_fetched_xml_when_existing_is_empty() {
        let fetched = r#"<i><d p="1,1,25,0,0,0,0,0">first</d></i>"#;
        let merged = merge_xml_append_only("", fetched);

        assert_eq!(merged.existing_comments, 0);
        assert_eq!(merged.fetched_comments, 1);
        assert_eq!(merged.appended_comments, 1);
        assert_eq!(merged.xml, fetched);
    }

    #[test]
    fn merge_xml_append_only_uses_fetched_xml_when_existing_root_is_self_closing() {
        let fetched = r#"<i><d p="1,1,25,0,0,0,0,0">first</d></i>"#;
        let merged = merge_xml_append_only("<i/>", fetched);

        assert_eq!(merged.existing_comments, 0);
        assert_eq!(merged.fetched_comments, 1);
        assert_eq!(merged.appended_comments, 1);
        assert_eq!(merged.xml, fetched);
    }

    #[test]
    fn merge_xml_append_only_does_not_insert_comments_into_chatserver() {
        let merged = merge_xml_append_only(
            r#"<i><chatserver>chat.example</chatserver><d p="1,1,25,0,0,0,0,0">old</d>"#,
            r#"<i><chatserver>chat.example</chatserver><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#,
        );

        assert_eq!(merged.existing_comments, 1);
        assert_eq!(merged.fetched_comments, 2);
        assert_eq!(merged.appended_comments, 1);
        assert!(
            merged
                .xml
                .contains("<chatserver>chat.example</chatserver><d")
        );
        assert!(!merged.xml.contains("new</d>\n</chatserver>"));
        assert!(
            merged
                .xml
                .ends_with("old</d>\n<d p=\"2,1,25,0,0,0,0,0\">new</d>\n")
        );
    }

    #[test]
    fn merge_xml_append_only_inserts_after_last_complete_comment_when_existing_has_dangling_tail() {
        let merged = merge_xml_append_only(
            r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="99,1,25,0,0,0,0,0">partial"#,
            r#"<i><d p="1,1,25,0,0,0,0,0">old</d><d p="2,1,25,0,0,0,0,0">new</d></i>"#,
        );
        let ass = xml_to_ass(&merged.xml);

        assert_eq!(merged.existing_comments, 1);
        assert_eq!(merged.fetched_comments, 2);
        assert_eq!(merged.appended_comments, 1);
        assert!(merged.xml.contains(
            "old</d>\n<d p=\"2,1,25,0,0,0,0,0\">new</d>\n<d p=\"99,1,25,0,0,0,0,0\">partial"
        ));
        assert_eq!(ass.matches("Dialogue:").count(), 2);
        assert!(ass.contains("new"));
        assert!(!ass.contains("partial"));
    }
}
