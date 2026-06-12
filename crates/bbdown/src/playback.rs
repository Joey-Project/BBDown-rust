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
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
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
    pub selection_hints: PlaybackSelectionHints,
}

impl<'de> Deserialize<'de> for PlaybackVariant {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = PlaybackVariantWire::deserialize(deserializer)?;
        let selection_hints = wire.selection_hints.unwrap_or_else(|| {
            selection_hints(wire.kind, wire.video.as_ref(), wire.audio.as_ref())
        });
        Ok(Self {
            id: wire.id,
            kind: wire.kind,
            video: wire.video,
            audio: wire.audio,
            flv_segments: wire.flv_segments,
            bandwidth: wire.bandwidth,
            codecs: wire.codecs,
            mime_types: wire.mime_types,
            width: wire.width,
            height: wire.height,
            frame_rate: wire.frame_rate,
            duration_seconds: wire.duration_seconds,
            selection_hints,
        })
    }
}

#[derive(Deserialize)]
struct PlaybackVariantWire {
    id: String,
    kind: PlaybackVariantKind,
    video: Option<MediaRequestSpec>,
    audio: Option<MediaRequestSpec>,
    #[serde(default)]
    flv_segments: Vec<MediaRequestSpec>,
    bandwidth: Option<u64>,
    #[serde(default)]
    codecs: Vec<String>,
    #[serde(default)]
    mime_types: Vec<String>,
    width: Option<u32>,
    height: Option<u32>,
    frame_rate: Option<String>,
    duration_seconds: Option<u32>,
    selection_hints: Option<PlaybackSelectionHints>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackVariantKind {
    Dash,
    Flv,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaybackSelectionHints {
    pub avplayer_h264_aac: PlaybackSelectionHint,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaybackSelectionHint {
    pub playable: bool,
    pub preferred: bool,
    pub score: i32,
    pub video_codec_family: Option<PlaybackCodecFamily>,
    pub audio_codec_family: Option<PlaybackCodecFamily>,
    pub reasons: Vec<PlaybackSelectionReason>,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackCodecFamily {
    H264,
    Hevc,
    Av1,
    Vp9,
    Aac,
    Flac,
    Dolby,
    Unknown,
    Other,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackSelectionReason {
    DashContainer,
    FlvContainer,
    H264Video,
    AacAudio,
    MissingVideo,
    MissingAudio,
    UnknownVideoCodec,
    UnknownAudioCodec,
    UnsupportedVideoCodec,
    UnsupportedAudioCodec,
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
        selection_hints: selection_hints(PlaybackVariantKind::Dash, video, audio),
    }
}

fn flv_variant(entry: &DownloadEntry, request_headers: &[HttpHeaderSpec]) -> PlaybackVariant {
    let flv_segments = entry
        .streams
        .flv_segments
        .iter()
        .map(|segment| flv_segment_request(entry, segment, request_headers))
        .collect::<Vec<_>>();
    let mut mime_types = Vec::new();
    for segment in &flv_segments {
        push_unique(&mut mime_types, segment.mime_type.as_deref());
    }
    PlaybackVariant {
        id: flv_variant_id(&flv_segments),
        kind: PlaybackVariantKind::Flv,
        video: None,
        audio: None,
        bandwidth: None,
        codecs: Vec::new(),
        mime_types,
        width: None,
        height: None,
        frame_rate: None,
        duration_seconds: entry
            .streams
            .duration_seconds
            .or_else(|| flv_segments_duration_seconds(&entry.streams.flv_segments)),
        selection_hints: flv_selection_hints(),
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

fn selection_hints(
    kind: PlaybackVariantKind,
    video: Option<&MediaRequestSpec>,
    audio: Option<&MediaRequestSpec>,
) -> PlaybackSelectionHints {
    PlaybackSelectionHints {
        avplayer_h264_aac: avplayer_h264_aac_hint(kind, video, audio),
    }
}

fn flv_selection_hints() -> PlaybackSelectionHints {
    PlaybackSelectionHints {
        avplayer_h264_aac: PlaybackSelectionHint {
            playable: false,
            preferred: false,
            score: -100,
            video_codec_family: None,
            audio_codec_family: None,
            reasons: vec![PlaybackSelectionReason::FlvContainer],
        },
    }
}

fn avplayer_h264_aac_hint(
    kind: PlaybackVariantKind,
    video: Option<&MediaRequestSpec>,
    audio: Option<&MediaRequestSpec>,
) -> PlaybackSelectionHint {
    if kind != PlaybackVariantKind::Dash {
        return PlaybackSelectionHint {
            playable: false,
            preferred: false,
            score: -100,
            video_codec_family: None,
            audio_codec_family: None,
            reasons: vec![PlaybackSelectionReason::FlvContainer],
        };
    }

    let video_family = video
        .and_then(|request| request.codecs.as_deref())
        .map(codec_family);
    let audio_family = audio
        .and_then(|request| request.codecs.as_deref())
        .map(codec_family);
    let mut playable = video.is_some() || audio.is_some();
    let mut score = 0_i32;
    let mut reasons = vec![PlaybackSelectionReason::DashContainer];

    match (video, video_family) {
        (Some(_), Some(PlaybackCodecFamily::H264)) => {
            score += 60;
            reasons.push(PlaybackSelectionReason::H264Video);
        }
        (Some(_), Some(PlaybackCodecFamily::Unknown) | None) => {
            playable = false;
            score -= 30;
            reasons.push(PlaybackSelectionReason::UnknownVideoCodec);
        }
        (Some(_), Some(_)) => {
            playable = false;
            score -= 40;
            reasons.push(PlaybackSelectionReason::UnsupportedVideoCodec);
        }
        (None, _) => {
            score -= 20;
            reasons.push(PlaybackSelectionReason::MissingVideo);
        }
    }

    match (audio, audio_family) {
        (Some(_), Some(PlaybackCodecFamily::Aac)) => {
            score += 30;
            reasons.push(PlaybackSelectionReason::AacAudio);
        }
        (Some(_), Some(PlaybackCodecFamily::Unknown) | None) => {
            playable = false;
            score -= 20;
            reasons.push(PlaybackSelectionReason::UnknownAudioCodec);
        }
        (Some(_), Some(_)) => {
            playable = false;
            score -= 30;
            reasons.push(PlaybackSelectionReason::UnsupportedAudioCodec);
        }
        (None, _) => {
            score -= 10;
            reasons.push(PlaybackSelectionReason::MissingAudio);
        }
    }

    PlaybackSelectionHint {
        playable,
        preferred: matches!(video_family, Some(PlaybackCodecFamily::H264))
            && matches!(audio_family, Some(PlaybackCodecFamily::Aac)),
        score,
        video_codec_family: video_family,
        audio_codec_family: audio_family,
        reasons,
    }
}

fn codec_family(codec: &str) -> PlaybackCodecFamily {
    let normalized = codec.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return PlaybackCodecFamily::Unknown;
    }
    if normalized.starts_with("avc1") || normalized.starts_with("avc3") {
        PlaybackCodecFamily::H264
    } else if normalized.starts_with("hev1") || normalized.starts_with("hvc1") {
        PlaybackCodecFamily::Hevc
    } else if normalized.starts_with("av01") {
        PlaybackCodecFamily::Av1
    } else if normalized.starts_with("vp09") || normalized.starts_with("vp9") {
        PlaybackCodecFamily::Vp9
    } else if is_aac_mp4a_codec(&normalized) {
        PlaybackCodecFamily::Aac
    } else if normalized.starts_with("flac") {
        PlaybackCodecFamily::Flac
    } else if normalized.starts_with("ec-3")
        || normalized.starts_with("ec3")
        || normalized.starts_with("ac-3")
        || normalized.starts_with("ac3")
    {
        PlaybackCodecFamily::Dolby
    } else {
        PlaybackCodecFamily::Other
    }
}

fn is_aac_mp4a_codec(codec: &str) -> bool {
    matches!(
        codec,
        "mp4a.40.2" | "mp4a.40.5" | "mp4a.40.29" | "mp4a.40.42"
    )
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
    use super::{
        HttpHeaderSpec, MediaRequestKind, PlaybackCodecFamily, PlaybackPlan,
        PlaybackSelectionReason, PlaybackVariant, PlaybackVariantKind, codec_family, source_hash,
    };
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
        assert_preferred_avplayer_hint(variant);
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
        assert_hevc_avplayer_hint(&playback.entries[0].variants[1]);
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
        assert_eq!(variant.mime_types, ["video/x-flv"]);
        assert_flv_avplayer_hint(variant);
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

    #[test]
    fn deserializes_legacy_variant_without_selection_hints() -> anyhow::Result<()> {
        let variant: PlaybackVariant = serde_json::from_value(serde_json::json!({
            "id": "legacy",
            "kind": "dash",
            "video": legacy_request_json("video", 80, "video/mp4", "avc1.640028"),
            "audio": legacy_request_json("audio", 30280, "audio/mp4", "mp4a.40.2"),
            "flv_segments": [],
            "bandwidth": 1_328_000,
            "codecs": ["avc1.640028", "mp4a.40.2"],
            "mime_types": ["video/mp4", "audio/mp4"],
            "width": 1920,
            "height": 1080,
            "frame_rate": "60",
            "duration_seconds": 90
        }))?;

        assert_preferred_avplayer_hint(&variant);
        Ok(())
    }

    #[test]
    fn codec_family_distinguishes_mp4a_object_types() {
        assert_eq!(codec_family("mp4a.40.2"), PlaybackCodecFamily::Aac);
        assert_eq!(codec_family("mp4a.40.5"), PlaybackCodecFamily::Aac);
        assert_eq!(codec_family("mp4a.40.29"), PlaybackCodecFamily::Aac);
        assert_eq!(codec_family("mp4a.40.42"), PlaybackCodecFamily::Aac);
        assert_eq!(codec_family("mp4a.40"), PlaybackCodecFamily::Other);
        assert_eq!(codec_family("mp4a.40.34"), PlaybackCodecFamily::Other);
        assert_eq!(codec_family("mp4a.69"), PlaybackCodecFamily::Other);
        assert_eq!(codec_family("mp4a.6b"), PlaybackCodecFamily::Other);
    }

    fn legacy_request_json(
        kind: &str,
        stream_id: u32,
        mime_type: &str,
        codecs: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "kind": kind,
            "stream_id": stream_id,
            "url": format!("https://media.example/{stream_id}.m4s"),
            "backup_urls": [],
            "headers": [],
            "mime_type": mime_type,
            "codecs": codecs,
            "bandwidth": 1_000,
            "width": null,
            "height": null,
            "frame_rate": null,
            "size": 1_000,
            "duration_seconds": 90,
            "cache_key": {
                "content_id": "BV1xx411c7mD-cid2",
                "media_kind": kind,
                "stream_id": stream_id,
                "codecs": codecs,
                "source_hash": "0123456789abcdef0123456789abcdef"
            }
        })
    }

    fn assert_preferred_avplayer_hint(variant: &PlaybackVariant) {
        let hint = &variant.selection_hints.avplayer_h264_aac;
        assert!(hint.playable);
        assert!(hint.preferred);
        assert_eq!(hint.score, 90);
        assert_eq!(hint.video_codec_family, Some(PlaybackCodecFamily::H264));
        assert_eq!(hint.audio_codec_family, Some(PlaybackCodecFamily::Aac));
        assert!(hint.reasons.contains(&PlaybackSelectionReason::H264Video));
        assert!(hint.reasons.contains(&PlaybackSelectionReason::AacAudio));
    }

    fn assert_hevc_avplayer_hint(variant: &PlaybackVariant) {
        let hint = &variant.selection_hints.avplayer_h264_aac;
        assert!(!hint.playable);
        assert!(!hint.preferred);
        assert_eq!(hint.video_codec_family, Some(PlaybackCodecFamily::Hevc));
        assert!(
            hint.reasons
                .contains(&PlaybackSelectionReason::UnsupportedVideoCodec)
        );
    }

    fn assert_flv_avplayer_hint(variant: &PlaybackVariant) {
        let hint = &variant.selection_hints.avplayer_h264_aac;
        assert!(!hint.playable);
        assert!(!hint.preferred);
        assert_eq!(hint.score, -100);
        assert!(
            hint.reasons
                .contains(&PlaybackSelectionReason::FlvContainer)
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
