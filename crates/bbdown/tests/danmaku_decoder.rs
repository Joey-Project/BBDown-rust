use std::error::Error as StdError;
use std::io::Write as _;

use bbdown_core::decode_danmaku_xml;
use flate2::Compression;
use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};

const VALID_XML: &str = "<i><d p='1,1,25,16777215'>中文コメント</d></i>";
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

#[test]
fn public_decoder_accepts_plain_gzip_zlib_and_raw_deflate_unicode() -> Result<(), Box<dyn StdError>>
{
    assert_eq!(decode_danmaku_xml(VALID_XML.as_bytes())?, VALID_XML);

    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(VALID_XML.as_bytes())?;
    assert_eq!(decode_danmaku_xml(&gzip.finish()?)?, VALID_XML);

    let mut zlib = ZlibEncoder::new(Vec::new(), Compression::default());
    zlib.write_all(VALID_XML.as_bytes())?;
    assert_eq!(decode_danmaku_xml(&zlib.finish()?)?, VALID_XML);

    let mut deflate = DeflateEncoder::new(Vec::new(), Compression::default());
    deflate.write_all(VALID_XML.as_bytes())?;
    assert_eq!(decode_danmaku_xml(&deflate.finish()?)?, VALID_XML);
    Ok(())
}

#[test]
fn public_decoder_rejects_malformed_unsafe_and_foreign_xml() {
    for payload in [
        b"not XML".as_slice(),
        b"<i><d>truncated</i>".as_slice(),
        b"<!DOCTYPE i [<!ENTITY x 'bad'>]><i>&x;</i>".as_slice(),
        b"<video><d p='0,1,25,16777215'>foreign</d></video>".as_slice(),
    ] {
        assert!(decode_danmaku_xml(payload).is_err());
    }
}

#[test]
fn public_decoder_rejects_truncated_and_trailing_deflate_streams() -> Result<(), Box<dyn StdError>>
{
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(VALID_XML.as_bytes())?;
    let compressed_bytes = encoder.finish()?;

    assert!(decode_danmaku_xml(&compressed_bytes[..compressed_bytes.len() - 1]).is_err());
    let mut trailing = compressed_bytes;
    trailing.push(0);
    assert!(decode_danmaku_xml(&trailing).is_err());
    Ok(())
}

#[test]
fn public_decoder_enforces_raw_and_decompressed_size_limits() -> Result<(), Box<dyn StdError>> {
    let oversized_raw = vec![b'x'; MAX_BODY_BYTES + 1];
    assert!(decode_danmaku_xml(&oversized_raw).is_err());

    let mut oversized_xml = String::with_capacity(MAX_BODY_BYTES + 16);
    oversized_xml.push_str("<i>");
    oversized_xml.push_str(&" ".repeat(MAX_BODY_BYTES));
    oversized_xml.push_str("</i>");
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(oversized_xml.as_bytes())?;
    let oversized_compressed = encoder.finish()?;
    assert!(oversized_compressed.len() < MAX_BODY_BYTES);
    assert!(decode_danmaku_xml(&oversized_compressed).is_err());
    Ok(())
}
