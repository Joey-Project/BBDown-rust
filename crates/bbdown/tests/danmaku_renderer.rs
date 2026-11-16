use bbdown_core::{merge_xml_preserving, render_danmaku_ass};

#[test]
fn public_renderer_preserves_old_and_new_multilingual_comments() -> bbdown_core::Result<()> {
    let old = concat!(
        "<i>",
        "<d p='1.0,1,25,16777215,1700000000,0,user,1'>中文旧评论</d>",
        "<d p='2.0,1,25,16777215,1700000000,0,user,2'>日本語の旧コメント</d>",
        "<d p='3.0,1,25,16777215,1700000000,0,user,3'>한국어 예전 댓글</d>",
        "</i>"
    );
    let fetched = concat!(
        "<i>",
        "<d p='1.0,1,25,16777215,1700000000,0,other,1'>provider changed old text</d>",
        "<d p='4.0,1,25,16777215,1700000000,0,user,4'>новый комментарий</d>",
        "<d p='5.0,1,25,16777215,1700000000,0,user,5'>français &amp; café</d>",
        "<d p='6.0,1,25,16777215,1700000000,0,user,6'>nuovo commento</d>",
        "</i>"
    );

    let merged = merge_xml_preserving(old, fetched)?;
    assert_eq!(merged.appended_comments, 3);
    let ass = render_danmaku_ass(&merged.xml)?;
    for exact_text in [
        "中文旧评论",
        "日本語の旧コメント",
        "한국어 예전 댓글",
        "новый комментарий",
        "français & café",
        "nuovo commento",
    ] {
        assert!(ass.contains(exact_text), "ASS should retain {exact_text:?}");
    }
    assert!(!ass.contains("provider changed old text"));
    Ok(())
}

#[test]
fn public_renderer_accepts_valid_xml_with_no_comments() -> bbdown_core::Result<()> {
    let ass = render_danmaku_ass("<?xml version='1.0'?><i><chatid>42</chatid></i>")?;
    assert!(ass.starts_with("[Script Info]\n"));
    assert!(!ass.contains("Dialogue:"));
    Ok(())
}

#[test]
fn public_renderer_handles_namespaces_and_decodes_xml_entities_once() -> bbdown_core::Result<()> {
    let xml = "<b:i xmlns:b='urn:test'><b:d p='1,1,25,16777215'>literal &amp;amp; and &amp;lt;</b:d><b:d p='2,4,32,16711680'><![CDATA[cdata &amp;amp;]]></b:d></b:i>";
    let ass = render_danmaku_ass(xml)?;
    assert_eq!(ass.matches("Dialogue:").count(), 2);
    assert!(ass.contains("literal &amp; and &lt;"));
    assert!(ass.contains("cdata &amp;amp;"));
    Ok(())
}

#[test]
fn public_renderer_rejects_malformed_doctype_and_foreign_root() {
    for xml in [
        "<i><d p='0,1,25,16777215'>truncated</i>",
        "<!DOCTYPE i [<!ENTITY comment 'expanded'>]><i><d p='0,1,25,16777215'>&comment;</d></i>",
        "<video><d p='0,1,25,16777215'>not danmaku</d></video>",
    ] {
        assert!(render_danmaku_ass(xml).is_err(), "must reject {xml:?}");
    }
}

#[test]
fn public_renderer_enforces_input_and_comment_limits() {
    let mut oversized = String::with_capacity(16 * 1024 * 1024 + 16);
    oversized.push_str("<i>");
    oversized.push_str(&" ".repeat(16 * 1024 * 1024));
    oversized.push_str("</i>");
    assert!(render_danmaku_ass(&oversized).is_err());

    let mut many_comments = String::from("<i>");
    for _ in 0..50_001 {
        many_comments.push_str("<d p='0,1,25,16777215'>x</d>");
    }
    many_comments.push_str("</i>");
    assert!(render_danmaku_ass(&many_comments).is_err());
}
