use std::collections::HashSet;
use std::ops::Range;

use roxmltree::{Document, Node};

use super::DanmakuXmlMerge;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum XmlKey {
    CommentId(String),
    Fallback { parameters: String, text: String },
}

#[derive(Clone, Debug)]
struct RawComment {
    key: XmlKey,
    parameters: String,
    text: String,
    raw: String,
    range: Range<usize>,
    namespace: Option<String>,
}

fn xml_key(parameters: &str, text: &str) -> XmlKey {
    parameters
        .split(',')
        .nth(7)
        .map(str::trim)
        .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|id| {
            let canonical = id.trim_start_matches('0');
            (!canonical.is_empty()).then(|| XmlKey::CommentId(canonical.to_owned()))
        })
        .unwrap_or_else(|| XmlKey::Fallback {
            parameters: parameters.to_owned(),
            text: text.to_owned(),
        })
}

fn invalid(message: impl Into<String>) -> crate::Error {
    crate::Error::InvalidInput(message.into())
}

fn parse_xml<'a>(xml: &'a str, label: &str) -> crate::Result<Document<'a>> {
    // roxmltree rejects actual DTD declarations by default and recognizes XML literal contexts.
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
    let root_namespace = document.root_element().tag_name().namespace();
    document
        .root_element()
        .children()
        .filter(|node| {
            if !node.is_element() || node.tag_name().name() != "d" {
                return false;
            }
            let namespace = node.tag_name().namespace();
            namespace.is_none() || namespace == root_namespace
        })
        .filter_map(|node| {
            let parameter = node.attribute("p")?;
            let text = node
                .children()
                .filter(Node::is_text)
                .filter_map(|child| child.text())
                .collect::<String>();
            let range = node.range();
            Some(RawComment {
                key: xml_key(parameter, &text),
                parameters: parameter.to_owned(),
                text,
                raw: xml[range.clone()].to_owned(),
                range,
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
        let (xml, appended_comments) = dedupe_fetched_xml(fetched_xml, &fetched_comments);
        return Ok(DanmakuXmlMerge {
            xml,
            existing_comments: 0,
            fetched_comments: fetched_comments.len(),
            appended_comments,
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

pub(crate) fn xml_to_ass_validated(xml: &str) -> crate::Result<String> {
    let document = parse_xml(xml, "fetched")?;
    root_element(&document, "fetched")?;
    let comments = xml_comments(&document, xml)
        .iter()
        .filter_map(|item| super::parse_comment_decoded(&item.parameters, &item.text))
        .collect::<Vec<_>>();
    Ok(super::render_ass(&comments))
}

fn dedupe_fetched_xml(fetched_xml: &str, comments: &[RawComment]) -> (String, usize) {
    let mut seen = HashSet::with_capacity(comments.len());
    let mut duplicates = Vec::new();
    for comment in comments {
        if !seen.insert(comment.key.clone()) {
            duplicates.push(comment.range.clone());
        }
    }
    if duplicates.is_empty() {
        return (fetched_xml.to_owned(), seen.len());
    }

    let mut xml = String::with_capacity(fetched_xml.len());
    let mut offset = 0;
    for range in duplicates {
        xml.push_str(&fetched_xml[offset..range.start]);
        offset = range.end;
    }
    xml.push_str(&fetched_xml[offset..]);
    (xml, seen.len())
}

#[cfg(test)]
mod tests {
    use super::{merge_xml_preserving, xml_to_ass_validated};

    #[test]
    fn validated_ass_renders_qualified_nodes_and_decodes_entities_once() -> crate::Result<()> {
        let xml = "<b:i xmlns:b='urn:test'><b:d p='1,1,25,16777215'>literal &amp;amp; and &amp;lt;</b:d><b:d p='2,4,32,16711680'><![CDATA[cdata &amp;amp;]]></b:d><!--<b:d p='3,1,25,0'>fake</b:d>--></b:i>";
        let ass = xml_to_ass_validated(xml)?;
        assert_eq!(ass.matches("Dialogue:").count(), 2);
        assert!(ass.contains("literal &amp; and &lt;"));
        assert!(ass.contains("cdata &amp;amp;"));
        assert!(!ass.contains("fake"));

        let ordinary =
            "<i><d p='1,1,25,16777215'>plain &amp; escaped</d><d p='2,4,32,16711680'>fixed</d></i>";
        assert_eq!(
            xml_to_ass_validated(ordinary)?,
            super::super::xml_to_ass(ordinary)
        );
        Ok(())
    }

    #[test]
    fn doctype_literals_in_comments_cdata_and_processing_instructions_are_safe() -> crate::Result<()>
    {
        let old = "<?xml version='1.0'?><i><?review <!DOCTYPE old?><!--literal <!DOCTYPE old>--><meta><![CDATA[literal <!DOCTYPE old>]]></meta><d p='1,1,25,16777215'>old</d></i>";
        let fetched = "<i><?review <!DOCTYPE fetched?><!--literal <!DOCTYPE fetched>--><meta><![CDATA[literal <!DOCTYPE fetched>]]></meta><d p='2,1,25,16777215'>new</d></i>";

        let merged = merge_xml_preserving(old, fetched)?;
        assert_eq!(merged.appended_comments, 1);
        assert!(
            merged.xml.starts_with(
                old.split_once("</i>")
                    .ok_or_else(|| {
                        crate::Error::InvalidInput("test XML is missing its root close".into())
                    })?
                    .0
            )
        );
        for literal in [
            "<?review <!DOCTYPE old?>",
            "<!--literal <!DOCTYPE old>-->",
            "<![CDATA[literal <!DOCTYPE old>]]>",
        ] {
            assert!(merged.xml.contains(literal));
        }

        let initialized = merge_xml_preserving("", fetched)?;
        assert_eq!(initialized.xml, fetched);
        let ass = xml_to_ass_validated(&merged.xml)?;
        assert_eq!(ass.matches("Dialogue:").count(), 2);
        assert!(!ass.contains("DOCTYPE"));
        Ok(())
    }

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

        let entity_dtd =
            "<!DOCTYPE i [<!ENTITY marker 'expanded'>]><i><d p='1,1,25,0'>&marker;</d></i>";
        assert!(merge_xml_preserving(entity_dtd, "<i/>").is_err());
        assert!(merge_xml_preserving("<i/>", entity_dtd).is_err());
        assert!(xml_to_ass_validated(entity_dtd).is_err());
    }

    #[test]
    fn comment_ids_dedupe_across_metadata_text_and_response_order() -> crate::Result<()> {
        let old = "<?xml version='1.0'?>\n<i marker='keep'><!--old--><d p='1,1,25,0,0,0,0,42'>old text</d><d p='2,1,25,0,0,0,0,43'>shared</d></i>";
        let fetched = "<i><d p='2,1,25,0,0,0,0,43'>shared</d><d p='9,5,40,16711680,1,1,1, 00042 '>edited text</d><d p='3,1,25,0,0,0,0,44'>shared</d></i>";

        let merged = merge_xml_preserving(old, fetched)?;
        let old_close = old.rfind("</i>").ok_or_else(|| {
            crate::Error::InvalidInput("test XML is missing its root close".into())
        })?;
        assert_eq!(merged.appended_comments, 1);
        assert!(merged.xml.starts_with(&old[..old_close]));
        assert!(
            merged.xml.contains(
                "<d p='1,1,25,0,0,0,0,42'>old text</d><d p='2,1,25,0,0,0,0,43'>shared</d>"
            )
        );
        assert!(
            merged
                .xml
                .ends_with("<d p='3,1,25,0,0,0,0,44'>shared</d></i>")
        );

        let refreshed = merge_xml_preserving(&merged.xml, fetched)?;
        assert_eq!(refreshed.appended_comments, 0);
        assert_eq!(refreshed.xml, merged.xml);
        Ok(())
    }

    #[test]
    fn unrelated_namespace_id_does_not_block_or_render_real_fetched_comment() -> crate::Result<()> {
        let old =
            "<i xmlns:x='urn:extension'><x:d p='1,1,25,16777215,0,0,0,42'>extension</x:d></i>";
        let fetched = "<i><d p='1,1,25,16777215,0,0,0,42'>real comment</d></i>";

        let merged = merge_xml_preserving(old, fetched)?;

        assert_eq!(merged.existing_comments, 0);
        assert_eq!(merged.fetched_comments, 1);
        assert_eq!(merged.appended_comments, 1);
        assert!(
            merged
                .xml
                .contains("<x:d p='1,1,25,16777215,0,0,0,42'>extension</x:d>")
        );
        assert!(
            merged
                .xml
                .contains("<d p='1,1,25,16777215,0,0,0,42'>real comment</d>")
        );
        let ass = xml_to_ass_validated(&merged.xml)?;
        assert_eq!(ass.matches("Dialogue:").count(), 1);
        assert!(ass.contains("real comment"));
        assert!(!ass.contains("extension"));
        Ok(())
    }

    #[test]
    fn fetched_extension_does_not_dedupe_unqualified_comment_or_render() -> crate::Result<()> {
        let fetched = "<i xmlns:x='urn:extension'><x:d p='1,1,25,16777215,0,0,0,42'>extension</x:d><d p='1,1,25,16777215,0,0,0,42'>real comment</d><d p='1,1,25,16777215,0,0,0,42'>duplicate real</d></i>";

        let merged = merge_xml_preserving("", fetched)?;

        assert_eq!(merged.fetched_comments, 2);
        assert_eq!(merged.appended_comments, 1);
        assert!(
            merged
                .xml
                .contains("<x:d p='1,1,25,16777215,0,0,0,42'>extension</x:d>")
        );
        assert!(
            merged
                .xml
                .contains("<d p='1,1,25,16777215,0,0,0,42'>real comment</d>")
        );
        assert!(!merged.xml.contains("duplicate real"));
        let ass = xml_to_ass_validated(&merged.xml)?;
        assert_eq!(ass.matches("Dialogue:").count(), 1);
        assert!(ass.contains("real comment"));
        assert!(!ass.contains("extension"));
        Ok(())
    }

    #[test]
    fn ass_accepts_schema_and_unqualified_comments_but_ignores_extensions() -> crate::Result<()> {
        let xml = "<b:i xmlns:b='urn:test' xmlns:x='urn:extension'><b:d p='1,1,25,16777215'>schema</b:d><d p='2,1,25,16777215'>legacy</d><x:d p='3,1,25,16777215'>extension</x:d></b:i>";

        let ass = xml_to_ass_validated(xml)?;

        assert_eq!(ass.matches("Dialogue:").count(), 2);
        assert!(ass.contains("schema"));
        assert!(ass.contains("legacy"));
        assert!(!ass.contains("extension"));
        Ok(())
    }

    #[test]
    fn comment_ids_canonicalize_large_decimal_strings_without_integer_parsing() -> crate::Result<()>
    {
        let id_with_leading_zeroes = "000184467440737095516160000000000000001";
        let canonical_id = "184467440737095516160000000000000001";
        let old = format!("<i><d p='1,1,25,0,0,0,0,{id_with_leading_zeroes}'>old</d></i>");
        let fetched = format!("<i><d p='9,5,40,16711680,1,1,1,{canonical_id}'>edited</d></i>");
        let merged = merge_xml_preserving(&old, &fetched)?;
        assert_eq!(merged.appended_comments, 0);
        assert_eq!(merged.xml, old);
        Ok(())
    }

    #[test]
    fn invalid_or_missing_comment_ids_fall_back_to_complete_parameters_and_text()
    -> crate::Result<()> {
        let old = "<i><d p='1,1,25,0,0,0,0'>missing</d><d p='2,1,25,0,0,0,0,0'>zero</d><d p='3,1,25,0,0,0,0,not-id'>invalid</d></i>";
        let fetched = "<i><d p='3,1,25,0,0,0,0,not-id'>invalid</d><d p='9,1,25,0,0,0,0'>new missing</d><d p='2,1,25,0,0,0,0,0'>zero changed</d><d p='3,1,25,0,0,0,0,not-id'>invalid changed</d><d p='1,1,25,0,0,0,0'>missing</d><d p='2,1,25,0,0,0,0,0'>zero</d></i>";
        let merged = merge_xml_preserving(old, fetched)?;
        assert_eq!(merged.appended_comments, 3);
        assert!(merged.xml.ends_with("<d p='9,1,25,0,0,0,0'>new missing</d><d p='2,1,25,0,0,0,0,0'>zero changed</d><d p='3,1,25,0,0,0,0,not-id'>invalid changed</d></i>"));
        Ok(())
    }

    #[test]
    fn empty_xml_deduplicates_fetched_ids_and_fallback_keys_without_reformatting()
    -> crate::Result<()> {
        let fetched = "<i title='keep'>prefix<!--keep--><d p='1,1,25,0,0,0,0,00099'>first</d><meta keep='yes'/><d p='8,4,42,9,9,9,9,99'>second</d><d p='5,1,25,0,0,0,0'>unknown</d><d p='5,1,25,0,0,0,0'>unknown</d></i>";
        let expected = "<i title='keep'>prefix<!--keep--><d p='1,1,25,0,0,0,0,00099'>first</d><meta keep='yes'/><d p='5,1,25,0,0,0,0'>unknown</d></i>";
        let merged = merge_xml_preserving("", fetched)?;
        assert_eq!(merged.fetched_comments, 4);
        assert_eq!(merged.appended_comments, 2);
        assert_eq!(merged.xml, expected);

        let refreshed = merge_xml_preserving(&merged.xml, fetched)?;
        assert_eq!(refreshed.appended_comments, 0);
        assert_eq!(refreshed.xml, expected);
        Ok(())
    }
}
