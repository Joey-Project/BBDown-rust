use crate::{
    DownloadEntry, DownloadPlan, Error, FlvSegment, MediaStream, Result, StreamQuality,
    StreamSource,
};
use md5::{Digest, Md5};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use url::Url;

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaybackPlan {
    pub title: String,
    pub entries: Vec<PlaybackEntry>,
}

impl PlaybackPlan {
    #[must_use]
    pub fn from_download_plan(plan: &DownloadPlan, request_headers: &[HttpHeaderSpec]) -> Self {
        Self {
            title: plan.title.clone(),
            entries: plan
                .entries
                .iter()
                .map(|entry| PlaybackEntry::from_download_entry(entry, request_headers))
                .collect(),
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaybackEntry {
    pub index: u32,
    pub aid: u64,
    pub bvid: Option<String>,
    pub cid: u64,
    pub epid: Option<u64>,
    pub title: String,
    pub cover_url: Option<String>,
    pub source: StreamSource,
    pub qualities: Vec<StreamQuality>,
    pub duration_seconds: Option<u32>,
    pub variants: Vec<PlaybackVariant>,
}

impl PlaybackEntry {
    fn from_download_entry(entry: &DownloadEntry, request_headers: &[HttpHeaderSpec]) -> Self {
        let variants = playback_variants(entry, request_headers);
        Self {
            index: entry.index,
            aid: entry.aid,
            bvid: entry.bvid.clone(),
            cid: entry.cid,
            epid: entry.epid,
            title: entry.title.clone(),
            cover_url: entry.cover_url.clone(),
            source: entry.source.clone(),
            qualities: entry.streams.qualities.clone(),
            duration_seconds: entry.streams.duration_seconds,
            variants,
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaybackVariant {
    pub id: String,
    pub kind: PlaybackVariantKind,
    pub video: Option<MediaRequestSpec>,
    pub audio: Option<MediaRequestSpec>,
    pub flv_segments: Vec<MediaRequestSpec>,
    pub bandwidth: Option<u64>,
    pub codecs: Vec<String>,
    pub mime_types: Vec<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<String>,
    pub duration_seconds: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackVariantKind {
    Dash,
    Flv,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MediaRequestSpec {
    pub kind: MediaRequestKind,
    pub stream_id: Option<u32>,
    pub url: String,
    pub backup_urls: Vec<String>,
    pub headers: Vec<HttpHeaderSpec>,
    pub mime_type: Option<String>,
    pub codecs: Option<String>,
    pub bandwidth: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<String>,
    pub size: Option<u64>,
    pub duration_seconds: Option<u32>,
    pub cache_key: MediaCacheKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaRequestKind {
    Video,
    Audio,
    FlvSegment,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MediaCacheKey {
    pub content_id: String,
    pub media_kind: MediaRequestKind,
    pub stream_id: Option<u32>,
    pub codecs: Option<String>,
    pub source_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HttpHeaderSpec {
    pub name: String,
    pub value: String,
}

pub(crate) fn header_specs_from_map(headers: &HeaderMap) -> Result<Vec<HttpHeaderSpec>> {
    let mut specs = Vec::new();
    for (name, value) in headers {
        let value = value.to_str().map_err(|_| {
            Error::InvalidInput(format!(
                "media request header `{}` is not valid UTF-8",
                name.as_str()
            ))
        })?;
        specs.push(HttpHeaderSpec {
            name: name.as_str().to_owned(),
            value: value.to_owned(),
        });
    }
    specs.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(specs)
}

fn playback_variants(
    entry: &DownloadEntry,
    request_headers: &[HttpHeaderSpec],
) -> Vec<PlaybackVariant> {
    let video_requests = entry
        .streams
        .videos
        .iter()
        .map(|stream| media_stream_request(entry, stream, MediaRequestKind::Video, request_headers))
        .collect::<Vec<_>>();
    let audio_requests = entry
        .streams
        .audios
        .iter()
        .map(|stream| media_stream_request(entry, stream, MediaRequestKind::Audio, request_headers))
        .collect::<Vec<_>>();
    let mut variants = Vec::new();
    if video_requests.is_empty() {
        variants.extend(
            audio_requests
                .iter()
                .map(|audio| dash_variant(entry, None, Some(audio))),
        );
    } else if audio_requests.is_empty() {
        variants.extend(
            video_requests
                .iter()
                .map(|video| dash_variant(entry, Some(video), None)),
        );
    } else {
        for video in &video_requests {
            variants.extend(
                audio_requests
                    .iter()
                    .map(|audio| dash_variant(entry, Some(video), Some(audio))),
            );
        }
    }
    if !entry.streams.flv_segments.is_empty() {
        variants.push(flv_variant(entry, request_headers));
    }
    variants
}

fn dash_variant(
    entry: &DownloadEntry,
    video: Option<&MediaRequestSpec>,
    audio: Option<&MediaRequestSpec>,
) -> PlaybackVariant {
    let id = dash_variant_id(video, audio);
    let mut codecs = Vec::new();
    let mut mime_types = Vec::new();
    push_unique(
        &mut codecs,
        video.and_then(|request| request.codecs.as_deref()),
    );
    push_unique(
        &mut codecs,
        audio.and_then(|request| request.codecs.as_deref()),
    );
    push_unique(
        &mut mime_types,
        video.and_then(|request| request.mime_type.as_deref()),
    );
    push_unique(
        &mut mime_types,
        audio.and_then(|request| request.mime_type.as_deref()),
    );
    PlaybackVariant {
        id,
        kind: PlaybackVariantKind::Dash,
        video: video.cloned(),
        audio: audio.cloned(),
        flv_segments: Vec::new(),
        bandwidth: combined_bandwidth(video, audio),
        codecs,
        mime_types,
        width: video.and_then(|request| request.width),
        height: video.and_then(|request| request.height),
        frame_rate: video.and_then(|request| request.frame_rate.clone()),
        duration_seconds: entry.streams.duration_seconds,
    }
}

fn flv_variant(entry: &DownloadEntry, request_headers: &[HttpHeaderSpec]) -> PlaybackVariant {
    let flv_segments = entry
        .streams
        .flv_segments
        .iter()
        .map(|segment| flv_segment_request(entry, segment, request_headers))
        .collect::<Vec<_>>();
    PlaybackVariant {
        id: flv_variant_id(&flv_segments),
        kind: PlaybackVariantKind::Flv,
        video: None,
        audio: None,
        bandwidth: None,
        codecs: Vec::new(),
        mime_types: Vec::new(),
        width: None,
        height: None,
        frame_rate: None,
        duration_seconds: entry
            .streams
            .duration_seconds
            .or_else(|| flv_segments_duration_seconds(&entry.streams.flv_segments)),
        flv_segments,
    }
}

fn media_stream_request(
    entry: &DownloadEntry,
    stream: &MediaStream,
    kind: MediaRequestKind,
    request_headers: &[HttpHeaderSpec],
) -> MediaRequestSpec {
    MediaRequestSpec {
        kind,
        stream_id: Some(stream.id),
        url: stream.base_url.clone(),
        backup_urls: stream.backup_urls.clone(),
        headers: request_headers.to_vec(),
        mime_type: stream.mime_type.clone(),
        codecs: stream.codecs.clone(),
        bandwidth: stream.bandwidth,
        width: stream.width,
        height: stream.height,
        frame_rate: stream.frame_rate.clone(),
        size: stream.size,
        duration_seconds: entry.streams.duration_seconds,
        cache_key: media_cache_key(
            entry,
            kind,
            Some(stream.id),
            stream.codecs.as_deref(),
            &stream.base_url,
        ),
    }
}

fn flv_segment_request(
    entry: &DownloadEntry,
    segment: &FlvSegment,
    request_headers: &[HttpHeaderSpec],
) -> MediaRequestSpec {
    MediaRequestSpec {
        kind: MediaRequestKind::FlvSegment,
        stream_id: Some(segment.order),
        url: segment.url.clone(),
        backup_urls: segment.backup_urls.clone(),
        headers: request_headers.to_vec(),
        mime_type: Some("video/x-flv".to_owned()),
        codecs: None,
        bandwidth: None,
        width: None,
        height: None,
        frame_rate: None,
        size: segment.size,
        duration_seconds: segment.length_ms.and_then(ms_to_seconds_ceil_u32),
        cache_key: media_cache_key(
            entry,
            MediaRequestKind::FlvSegment,
            Some(segment.order),
            None,
            &segment.url,
        ),
    }
}

fn media_cache_key(
    entry: &DownloadEntry,
    kind: MediaRequestKind,
    stream_id: Option<u32>,
    codecs: Option<&str>,
    url: &str,
) -> MediaCacheKey {
    MediaCacheKey {
        content_id: entry_content_id(entry),
        media_kind: kind,
        stream_id,
        codecs: codecs.map(ToOwned::to_owned),
        source_hash: source_hash(url),
    }
}

fn entry_content_id(entry: &DownloadEntry) -> String {
    let primary = entry
        .bvid
        .as_deref()
        .filter(|bvid| !bvid.is_empty())
        .map_or_else(
            || {
                entry
                    .epid
                    .map_or_else(|| format!("av{}", entry.aid), |epid| format!("ep{epid}"))
            },
            ToOwned::to_owned,
        );
    format!("{primary}-cid{}", entry.cid)
}

fn source_hash(url: &str) -> String {
    let source = source_url_identity(url);
    let digest = Md5::digest(source.as_bytes());
    format!("{digest:x}")
}

fn source_url_identity(url: &str) -> String {
    Url::parse(url).map_or_else(
        |_| {
            url.split_once('#').map_or_else(
                || url.to_owned(),
                |(before_fragment, _fragment)| before_fragment.to_owned(),
            )
        },
        |mut parsed| {
            parsed.set_fragment(None);
            parsed.to_string()
        },
    )
}

fn dash_variant_id(video: Option<&MediaRequestSpec>, audio: Option<&MediaRequestSpec>) -> String {
    match (video, audio) {
        (Some(video), Some(audio)) => {
            format!(
                "dash-v{}-{}-a{}-{}",
                media_id_token(video),
                short_source_hash(video),
                media_id_token(audio),
                short_source_hash(audio)
            )
        }
        (Some(video), None) => {
            format!(
                "dash-v{}-{}",
                media_id_token(video),
                short_source_hash(video)
            )
        }
        (None, Some(audio)) => {
            format!(
                "dash-a{}-{}",
                media_id_token(audio),
                short_source_hash(audio)
            )
        }
        (None, None) => "dash-empty".to_owned(),
    }
}

fn flv_variant_id(segments: &[MediaRequestSpec]) -> String {
    segments.first().map_or_else(
        || "flv-empty".to_owned(),
        |segment| {
            format!(
                "flv-s{}-{}",
                media_id_token(segment),
                short_source_hash(segment)
            )
        },
    )
}

fn media_id_token(request: &MediaRequestSpec) -> String {
    request
        .stream_id
        .map_or_else(|| "unknown".to_owned(), |id| id.to_string())
}

fn short_source_hash(request: &MediaRequestSpec) -> String {
    request.cache_key.source_hash.chars().take(8).collect()
}

fn push_unique(values: &mut Vec<String>, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty())
        && !values.iter().any(|existing| existing == value)
    {
        values.push(value.to_owned());
    }
}

fn combined_bandwidth(
    video: Option<&MediaRequestSpec>,
    audio: Option<&MediaRequestSpec>,
) -> Option<u64> {
    let mut total = 0_u64;
    let mut has_value = false;
    for bandwidth in [
        video.and_then(|request| request.bandwidth),
        audio.and_then(|request| request.bandwidth),
    ]
    .into_iter()
    .flatten()
    {
        total = total.saturating_add(bandwidth);
        has_value = true;
    }
    has_value.then_some(total)
}

fn flv_segments_duration_seconds(segments: &[FlvSegment]) -> Option<u32> {
    if segments.is_empty() {
        return None;
    }
    let mut total_ms = 0_u64;
    for segment in segments {
        total_ms = total_ms.checked_add(segment.length_ms?)?;
    }
    ms_to_seconds_ceil_u32(total_ms)
}

fn ms_to_seconds_ceil_u32(milliseconds: u64) -> Option<u32> {
    let seconds = milliseconds.saturating_add(999) / 1000;
    u32::try_from(seconds).ok()
}

#[cfg(test)]
mod tests {
    use super::{HttpHeaderSpec, MediaRequestKind, PlaybackPlan, PlaybackVariantKind, source_hash};
    use crate::{
        DanmakuTrack, DownloadEntry, DownloadPlan, FlvSegment, MediaStream, StreamDiagnostics,
        StreamQuality, StreamSet, StreamSource,
    };

    #[test]
    fn builds_dash_playback_variants_from_download_plan() -> anyhow::Result<()> {
        let plan = test_plan(StreamSet {
            videos: vec![
                MediaStream {
                    id: 80,
                    base_url: "https://video.example/80.m4s?token=secret".to_owned(),
                    backup_urls: vec!["https://backup.example/80.m4s".to_owned()],
                    codecs: Some("avc1.640028".to_owned()),
                    bandwidth: Some(1_200_000),
                    width: Some(1920),
                    height: Some(1080),
                    frame_rate: Some("60".to_owned()),
                    mime_type: Some("video/mp4".to_owned()),
                    size: Some(10_000),
                },
                MediaStream {
                    id: 64,
                    base_url: "https://video.example/64.m4s?token=secret".to_owned(),
                    backup_urls: Vec::new(),
                    codecs: Some("hev1.1.6.L120.90".to_owned()),
                    bandwidth: Some(800_000),
                    width: Some(1280),
                    height: Some(720),
                    frame_rate: Some("30".to_owned()),
                    mime_type: Some("video/mp4".to_owned()),
                    size: Some(8_000),
                },
            ],
            audios: vec![MediaStream {
                id: 30280,
                base_url: "https://audio.example/30280.m4s?token=secret".to_owned(),
                backup_urls: vec!["https://backup.example/30280.m4s".to_owned()],
                codecs: Some("mp4a.40.2".to_owned()),
                bandwidth: Some(128_000),
                width: None,
                height: None,
                frame_rate: None,
                mime_type: Some("audio/mp4".to_owned()),
                size: Some(2_000),
            }],
            flv_segments: Vec::new(),
            accept_quality: vec![80, 64],
            qualities: vec![StreamQuality {
                id: 80,
                description: Some("1080P".to_owned()),
            }],
            duration_seconds: Some(90),
        });
        let headers = vec![
            HttpHeaderSpec {
                name: "referer".to_owned(),
                value: "https://www.bilibili.com/".to_owned(),
            },
            HttpHeaderSpec {
                name: "user-agent".to_owned(),
                value: "bbdown-rs/0.1".to_owned(),
            },
        ];

        let playback = PlaybackPlan::from_download_plan(&plan, &headers);

        assert_eq!(playback.title, "Mock video");
        assert_eq!(playback.entries[0].variants.len(), 2);
        let variant = &playback.entries[0].variants[0];
        assert_eq!(variant.kind, PlaybackVariantKind::Dash);
        assert_eq!(variant.bandwidth, Some(1_328_000));
        assert_eq!(variant.codecs, ["avc1.640028", "mp4a.40.2"]);
        assert_eq!(variant.mime_types, ["video/mp4", "audio/mp4"]);
        assert_eq!(variant.width, Some(1920));
        assert_eq!(variant.height, Some(1080));
        let video = variant
            .video
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing video request"))?;
        assert_eq!(video.url, "https://video.example/80.m4s?token=secret");
        assert_eq!(video.backup_urls, ["https://backup.example/80.m4s"]);
        assert_eq!(video.headers, headers);
        assert_eq!(video.cache_key.media_kind, MediaRequestKind::Video);
        assert_eq!(video.cache_key.content_id, "BV1xx411c7mD-cid2");
        assert_eq!(video.cache_key.stream_id, Some(80));
        assert_eq!(
            video.cache_key.source_hash,
            source_hash("https://video.example/80.m4s?token=secret")
        );
        let audio = variant
            .audio
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing audio request"))?;
        assert_eq!(audio.cache_key.media_kind, MediaRequestKind::Audio);
        assert!(variant.id.starts_with("dash-v80-"));
        Ok(())
    }

    #[test]
    fn builds_flv_playback_variant_with_segment_specs() {
        let plan = test_plan(StreamSet {
            videos: Vec::new(),
            audios: Vec::new(),
            flv_segments: vec![
                FlvSegment {
                    order: 1,
                    url: "https://flv.example/1.flv?token=secret".to_owned(),
                    backup_urls: vec!["https://backup.example/1.flv".to_owned()],
                    size: Some(1_000),
                    length_ms: Some(1_500),
                },
                FlvSegment {
                    order: 2,
                    url: "https://flv.example/2.flv?token=secret".to_owned(),
                    backup_urls: Vec::new(),
                    size: Some(2_000),
                    length_ms: Some(2_500),
                },
            ],
            accept_quality: Vec::new(),
            qualities: Vec::new(),
            duration_seconds: None,
        });

        let playback = PlaybackPlan::from_download_plan(&plan, &[]);

        assert_eq!(playback.entries[0].variants.len(), 1);
        let variant = &playback.entries[0].variants[0];
        assert_eq!(variant.kind, PlaybackVariantKind::Flv);
        assert_eq!(variant.duration_seconds, Some(4));
        assert_eq!(variant.flv_segments.len(), 2);
        assert_eq!(variant.flv_segments[0].stream_id, Some(1));
        assert_eq!(variant.flv_segments[0].duration_seconds, Some(2));
        assert_eq!(
            variant.flv_segments[0].mime_type.as_deref(),
            Some("video/x-flv")
        );
        assert_eq!(
            variant.flv_segments[0].cache_key.source_hash,
            source_hash("https://flv.example/1.flv?token=secret")
        );
    }

    #[test]
    fn source_hash_preserves_query_identity_and_ignores_fragments() {
        assert_ne!(
            source_hash("https://proxy.example/media?u=one"),
            source_hash("https://proxy.example/media?u=two")
        );
        assert_eq!(
            source_hash("https://video.example/80.m4s?token=secret#frag"),
            source_hash("https://video.example/80.m4s?token=secret")
        );
    }

    fn test_plan(streams: StreamSet) -> DownloadPlan {
        DownloadPlan {
            title: "Mock video".to_owned(),
            entries: vec![DownloadEntry {
                index: 1,
                aid: 170_001,
                bvid: Some("BV1xx411c7mD".to_owned()),
                cid: 2,
                epid: None,
                title: "Main".to_owned(),
                cover_url: Some("https://example.invalid/cover.jpg".to_owned()),
                source: StreamSource::NormalWeb,
                streams,
                diagnostics: StreamDiagnostics::default(),
                subtitles: Vec::new(),
                danmaku: DanmakuTrack {
                    cid: 2,
                    xml_url: "https://comment.example/2.xml".to_owned(),
                },
            }],
        }
    }
}
