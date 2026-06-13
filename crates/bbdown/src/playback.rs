use crate::{
    CodecFamily, DownloadEntry, DownloadPlan, Error, FlvSegment, MediaStream, Result,
    StreamQuality, StreamSource,
};
use md5::{Digest, Md5};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize, de};
use std::{cmp::Ordering, collections::BTreeMap};
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
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PlaybackEntry {
    pub index: u32,
    pub aid: u64,
    pub bvid: Option<String>,
    pub cid: u64,
    pub epid: Option<u64>,
    pub title: String,
    pub cover_url: Option<String>,
    pub source: StreamSource,
    pub cache_key: PlaybackEntryCacheKey,
    pub qualities: Vec<StreamQuality>,
    pub duration_seconds: Option<u32>,
    pub abr: PlaybackAbrMetadata,
    pub variants: Vec<PlaybackVariant>,
}

impl<'de> Deserialize<'de> for PlaybackEntry {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = PlaybackEntryWire::deserialize(deserializer)?;
        let cache_key = wire.cache_key.unwrap_or_else(|| {
            playback_entry_cache_key_from_parts(wire.bvid.as_deref(), wire.epid, wire.aid, wire.cid)
        });
        let mut variants = wire.variants;
        let abr = apply_abr_metadata(&mut variants);
        Ok(Self {
            index: wire.index,
            aid: wire.aid,
            bvid: wire.bvid,
            cid: wire.cid,
            epid: wire.epid,
            title: wire.title,
            cover_url: wire.cover_url,
            source: wire.source,
            cache_key,
            qualities: wire.qualities,
            duration_seconds: wire.duration_seconds,
            abr,
            variants,
        })
    }
}

#[derive(Deserialize)]
struct PlaybackEntryWire {
    index: u32,
    aid: u64,
    bvid: Option<String>,
    cid: u64,
    epid: Option<u64>,
    title: String,
    cover_url: Option<String>,
    source: StreamSource,
    cache_key: Option<PlaybackEntryCacheKey>,
    qualities: Vec<StreamQuality>,
    duration_seconds: Option<u32>,
    #[serde(rename = "abr")]
    _abr: Option<PlaybackAbrMetadata>,
    variants: Vec<PlaybackVariant>,
}

impl PlaybackEntry {
    fn from_download_entry(entry: &DownloadEntry, request_headers: &[HttpHeaderSpec]) -> Self {
        let mut variants = playback_variants(entry, request_headers);
        let abr = apply_abr_metadata(&mut variants);
        Self {
            index: entry.index,
            aid: entry.aid,
            bvid: entry.bvid.clone(),
            cid: entry.cid,
            epid: entry.epid,
            title: entry.title.clone(),
            cover_url: entry.cover_url.clone(),
            source: entry.source.clone(),
            cache_key: playback_entry_cache_key(entry),
            qualities: entry.streams.qualities.clone(),
            duration_seconds: entry.streams.duration_seconds,
            abr,
            variants,
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaybackEntryCacheKey {
    pub content_id: String,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaybackAbrMetadata {
    pub groups: Vec<PlaybackAbrGroup>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaybackAbrGroup {
    pub id: String,
    pub kind: PlaybackAbrGroupKind,
    pub variant_ids: Vec<String>,
    pub level_count: u32,
    pub min_bandwidth: Option<u64>,
    pub max_bandwidth: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackAbrGroupKind {
    DashVideo,
    DashAudioOnly,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaybackAbrLevel {
    pub group_id: String,
    pub level_index: u32,
    pub level_count: u32,
    pub switchable: bool,
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
    pub cache_key: PlaybackVariantCacheKey,
    pub abr: Option<PlaybackAbrLevel>,
    pub selection_hints: PlaybackSelectionHints,
}

impl<'de> Deserialize<'de> for PlaybackVariant {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = PlaybackVariantWire::deserialize(deserializer)?;
        let selection_hints = wire.selection_hints.map_or_else(
            || selection_hints(wire.kind, wire.video.as_ref(), wire.audio.as_ref()),
            |hints| hints.into_selection_hints(wire.kind, wire.video.as_ref(), wire.audio.as_ref()),
        );
        let cache_key = wire.cache_key.unwrap_or_else(|| {
            playback_variant_cache_key(
                &wire.id,
                wire.kind,
                wire.video.as_ref(),
                wire.audio.as_ref(),
                &wire.flv_segments,
            )
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
            cache_key,
            abr: wire.abr,
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
    cache_key: Option<PlaybackVariantCacheKey>,
    abr: Option<PlaybackAbrLevel>,
    selection_hints: Option<PlaybackSelectionHintsWire>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaybackVariantCacheKey {
    pub content_id: String,
    pub variant_kind: PlaybackVariantKind,
    pub variant_id: String,
    pub media_keys: Vec<MediaCacheKey>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackVariantKind {
    Dash,
    Flv,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PlaybackSelectionHints {
    pub avplayer: PlaybackSelectionHint,
}

impl<'de> Deserialize<'de> for PlaybackSelectionHints {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = PlaybackSelectionHintsWire::deserialize(deserializer)?;
        let avplayer = wire
            .avplayer
            .ok_or_else(|| de::Error::missing_field("avplayer"))?;
        Ok(Self { avplayer })
    }
}

#[derive(Deserialize)]
struct PlaybackSelectionHintsWire {
    avplayer: Option<PlaybackSelectionHint>,
    #[serde(default, rename = "avplayer_h264_aac")]
    _avplayer_h264_aac: Option<serde_json::Value>,
}

impl PlaybackSelectionHintsWire {
    fn into_selection_hints(
        self,
        kind: PlaybackVariantKind,
        video: Option<&MediaRequestSpec>,
        audio: Option<&MediaRequestSpec>,
    ) -> PlaybackSelectionHints {
        if let Some(avplayer) = self.avplayer
            && !hint_needs_backfill(&avplayer, kind, video, audio)
        {
            return PlaybackSelectionHints { avplayer };
        }
        selection_hints(kind, video, audio)
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaybackSelectionHint {
    #[serde(default = "unknown_format_key")]
    pub format_key: String,
    #[serde(default)]
    pub video_codec: Option<String>,
    #[serde(default)]
    pub audio_codec: Option<String>,
    pub playable: bool,
    pub preferred: bool,
    pub score: i32,
    pub video_codec_family: Option<PlaybackCodecFamily>,
    pub audio_codec_family: Option<PlaybackCodecFamily>,
    pub reasons: Vec<PlaybackSelectionReason>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PlaybackCodecPreference {
    pub video_codec_families: Vec<PlaybackCodecFamily>,
    pub audio_codec_families: Vec<PlaybackCodecFamily>,
}

impl PlaybackCodecPreference {
    #[must_use]
    pub fn new(
        video_codec_families: Vec<PlaybackCodecFamily>,
        audio_codec_families: Vec<PlaybackCodecFamily>,
    ) -> Self {
        Self {
            video_codec_families,
            audio_codec_families,
        }
    }

    #[must_use]
    pub fn avplayer_default() -> Self {
        Self::new(
            vec![
                PlaybackCodecFamily::H264,
                PlaybackCodecFamily::Hevc,
                PlaybackCodecFamily::Av1,
            ],
            vec![PlaybackCodecFamily::Aac],
        )
    }

    #[must_use]
    pub fn h264_aac() -> Self {
        Self::new(
            vec![PlaybackCodecFamily::H264],
            vec![PlaybackCodecFamily::Aac],
        )
    }

    #[must_use]
    pub fn hevc_aac() -> Self {
        Self::new(
            vec![PlaybackCodecFamily::Hevc],
            vec![PlaybackCodecFamily::Aac],
        )
    }

    #[must_use]
    pub fn av1_aac() -> Self {
        Self::new(
            vec![PlaybackCodecFamily::Av1],
            vec![PlaybackCodecFamily::Aac],
        )
    }

    #[must_use]
    pub fn rank_hint(&self, hint: &PlaybackSelectionHint) -> Option<usize> {
        if !hint.playable {
            return None;
        }
        let video_rank = codec_preference_rank(
            hint.video_codec_family,
            self.video_codec_families.as_slice(),
        )?;
        let audio_rank = codec_preference_rank(
            hint.audio_codec_family,
            self.audio_codec_families.as_slice(),
        )?;
        Some(video_rank.saturating_mul(1_000).saturating_add(audio_rank))
    }
}

impl PlaybackVariant {
    #[must_use]
    pub fn codec_preference_rank(&self, preference: &PlaybackCodecPreference) -> Option<usize> {
        preference.rank_hint(&self.selection_hints.avplayer)
    }

    #[must_use]
    pub fn matches_codec_preference(&self, preference: &PlaybackCodecPreference) -> bool {
        self.codec_preference_rank(preference).is_some()
    }
}

pub type PlaybackCodecFamily = CodecFamily;

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackSelectionReason {
    DashContainer,
    FlvContainer,
    H264Video,
    HevcVideo,
    Av1Video,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec_family: Option<PlaybackCodecFamily>,
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
    let cache_key = playback_variant_cache_key(&id, PlaybackVariantKind::Dash, video, audio, &[]);
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
        cache_key,
        abr: None,
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
    let id = flv_variant_id(&flv_segments);
    let cache_key =
        playback_variant_cache_key(&id, PlaybackVariantKind::Flv, None, None, &flv_segments);
    PlaybackVariant {
        id,
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
        cache_key,
        abr: None,
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
        codec_family: stream
            .codec_family
            .or_else(|| stream.codecs.as_deref().map(codec_family)),
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
        codec_family: None,
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

fn playback_entry_cache_key(entry: &DownloadEntry) -> PlaybackEntryCacheKey {
    playback_entry_cache_key_from_parts(entry.bvid.as_deref(), entry.epid, entry.aid, entry.cid)
}

fn playback_entry_cache_key_from_parts(
    bvid: Option<&str>,
    epid: Option<u64>,
    aid: u64,
    cid: u64,
) -> PlaybackEntryCacheKey {
    PlaybackEntryCacheKey {
        content_id: entry_content_id_from_parts(bvid, epid, aid, cid),
    }
}

fn playback_variant_cache_key(
    variant_id: &str,
    kind: PlaybackVariantKind,
    video: Option<&MediaRequestSpec>,
    audio: Option<&MediaRequestSpec>,
    flv_segments: &[MediaRequestSpec],
) -> PlaybackVariantCacheKey {
    let media_keys = variant_media_keys(video, audio, flv_segments);
    PlaybackVariantCacheKey {
        content_id: media_keys
            .first()
            .map_or_else(String::new, |key| key.content_id.clone()),
        variant_kind: kind,
        variant_id: variant_id.to_owned(),
        media_keys,
    }
}

fn variant_media_keys(
    video: Option<&MediaRequestSpec>,
    audio: Option<&MediaRequestSpec>,
    flv_segments: &[MediaRequestSpec],
) -> Vec<MediaCacheKey> {
    let mut media_keys = Vec::new();
    if let Some(video) = video {
        media_keys.push(video.cache_key.clone());
    }
    if let Some(audio) = audio {
        media_keys.push(audio.cache_key.clone());
    }
    media_keys.extend(flv_segments.iter().map(|segment| segment.cache_key.clone()));
    media_keys
}

fn entry_content_id(entry: &DownloadEntry) -> String {
    entry_content_id_from_parts(entry.bvid.as_deref(), entry.epid, entry.aid, entry.cid)
}

fn entry_content_id_from_parts(
    bvid: Option<&str>,
    epid: Option<u64>,
    aid: u64,
    cid: u64,
) -> String {
    let primary = bvid.filter(|bvid| !bvid.is_empty()).map_or_else(
        || epid.map_or_else(|| format!("av{aid}"), |epid| format!("ep{epid}")),
        ToOwned::to_owned,
    );
    format!("{primary}-cid{cid}")
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

fn apply_abr_metadata(variants: &mut [PlaybackVariant]) -> PlaybackAbrMetadata {
    for variant in variants.iter_mut() {
        variant.abr = None;
    }

    let mut grouped = BTreeMap::<String, (PlaybackAbrGroupKind, Vec<usize>)>::new();
    for (index, variant) in variants.iter().enumerate() {
        if let Some((group_id, group_kind)) = abr_group_identity(variant) {
            grouped
                .entry(group_id)
                .and_modify(|(_kind, indices)| indices.push(index))
                .or_insert_with(|| (group_kind, vec![index]));
        }
    }

    let mut groups = Vec::new();
    for (group_id, (group_kind, mut indices)) in grouped {
        indices.sort_by(|left, right| abr_variant_cmp(&variants[*left], &variants[*right]));
        let level_count = u32::try_from(indices.len()).unwrap_or(u32::MAX);
        let switchable = indices.len() > 1;
        let min_bandwidth = indices
            .iter()
            .filter_map(|index| variants[*index].bandwidth)
            .min();
        let max_bandwidth = indices
            .iter()
            .filter_map(|index| variants[*index].bandwidth)
            .max();
        let variant_ids = indices
            .iter()
            .map(|index| variants[*index].id.clone())
            .collect::<Vec<_>>();

        for (level_index, variant_index) in indices.iter().enumerate() {
            variants[*variant_index].abr = Some(PlaybackAbrLevel {
                group_id: group_id.clone(),
                level_index: u32::try_from(level_index).unwrap_or(u32::MAX),
                level_count,
                switchable,
            });
        }

        groups.push(PlaybackAbrGroup {
            id: group_id,
            kind: group_kind,
            variant_ids,
            level_count,
            min_bandwidth,
            max_bandwidth,
        });
    }

    PlaybackAbrMetadata { groups }
}

fn abr_group_identity(variant: &PlaybackVariant) -> Option<(String, PlaybackAbrGroupKind)> {
    if variant.kind != PlaybackVariantKind::Dash {
        return None;
    }
    if variant.video.is_some() {
        let video_identity = variant
            .video
            .as_ref()
            .map_or_else(|| "novideo".to_owned(), media_compatibility_component);
        let audio_identity = variant
            .audio
            .as_ref()
            .map_or_else(|| "noaudio".to_owned(), media_cache_component);
        return Some((
            format!(
                "dash-video-{}-{video_identity}-{audio_identity}",
                variant.cache_key.content_id
            ),
            PlaybackAbrGroupKind::DashVideo,
        ));
    }
    if variant.audio.is_some() {
        let audio_identity = variant
            .audio
            .as_ref()
            .map_or_else(|| "noaudio".to_owned(), media_compatibility_component);
        return Some((
            format!(
                "dash-audio-{}-{audio_identity}",
                variant.cache_key.content_id
            ),
            PlaybackAbrGroupKind::DashAudioOnly,
        ));
    }
    None
}

fn media_cache_component(request: &MediaRequestSpec) -> String {
    format!(
        "{}{}-{}",
        media_kind_prefix(request.kind),
        media_id_token(request),
        request.cache_key.source_hash
    )
}

fn media_compatibility_component(request: &MediaRequestSpec) -> String {
    let codec = media_compatibility_codec_component(request);
    let mime = request
        .mime_type
        .as_deref()
        .filter(|value| !value.is_empty())
        .map_or_else(|| "unknown".to_owned(), abr_identity_part);
    format!("{codec}-{mime}")
}

fn media_compatibility_codec_component(request: &MediaRequestSpec) -> String {
    if let Some(family) = request.codec_family {
        return match family {
            PlaybackCodecFamily::Unknown => format!("unknown-{}", media_cache_component(request)),
            PlaybackCodecFamily::Other => request.codecs.as_deref().map_or_else(
                || "other".to_owned(),
                |codec| format!("other-{}", abr_identity_part(codec)),
            ),
            family => codec_family_key(family).to_owned(),
        };
    }
    request.codecs.as_deref().map_or_else(
        || format!("unknown-{}", media_cache_component(request)),
        |codec| {
            let normalized = codec.trim().to_ascii_lowercase();
            match codec_family(&normalized) {
                PlaybackCodecFamily::Unknown => {
                    format!("unknown-{}", media_cache_component(request))
                }
                PlaybackCodecFamily::Other => format!("other-{}", abr_identity_part(&normalized)),
                family => codec_family_key(family).to_owned(),
            }
        },
    )
}

fn abr_identity_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

fn media_kind_prefix(kind: MediaRequestKind) -> &'static str {
    match kind {
        MediaRequestKind::Video => "v",
        MediaRequestKind::Audio => "a",
        MediaRequestKind::FlvSegment => "s",
    }
}

fn abr_variant_cmp(left: &PlaybackVariant, right: &PlaybackVariant) -> Ordering {
    abr_bandwidth_sort_key(left)
        .cmp(&abr_bandwidth_sort_key(right))
        .then_with(|| {
            left.height
                .unwrap_or_default()
                .cmp(&right.height.unwrap_or_default())
        })
        .then_with(|| {
            left.width
                .unwrap_or_default()
                .cmp(&right.width.unwrap_or_default())
        })
        .then_with(|| left.id.cmp(&right.id))
}

fn abr_bandwidth_sort_key(variant: &PlaybackVariant) -> u64 {
    variant.bandwidth.unwrap_or(u64::MAX)
}

fn selection_hints(
    kind: PlaybackVariantKind,
    video: Option<&MediaRequestSpec>,
    audio: Option<&MediaRequestSpec>,
) -> PlaybackSelectionHints {
    PlaybackSelectionHints {
        avplayer: avplayer_hint(kind, video, audio),
    }
}

fn flv_selection_hints() -> PlaybackSelectionHints {
    PlaybackSelectionHints {
        avplayer: PlaybackSelectionHint {
            format_key: "flv".to_owned(),
            video_codec: None,
            audio_codec: None,
            playable: false,
            preferred: false,
            score: -100,
            video_codec_family: None,
            audio_codec_family: None,
            reasons: vec![PlaybackSelectionReason::FlvContainer],
        },
    }
}

fn avplayer_hint(
    kind: PlaybackVariantKind,
    video: Option<&MediaRequestSpec>,
    audio: Option<&MediaRequestSpec>,
) -> PlaybackSelectionHint {
    if kind != PlaybackVariantKind::Dash {
        return PlaybackSelectionHint {
            format_key: "flv".to_owned(),
            video_codec: None,
            audio_codec: None,
            playable: false,
            preferred: false,
            score: -100,
            video_codec_family: None,
            audio_codec_family: None,
            reasons: vec![PlaybackSelectionReason::FlvContainer],
        };
    }

    let video_family = request_codec_family(video);
    let audio_family = request_codec_family(audio);
    let video_codec = video.and_then(|request| request.codecs.clone());
    let audio_codec = audio.and_then(|request| request.codecs.clone());
    let mut playable = video.is_some() || audio.is_some();
    let mut score = 0_i32;
    let mut reasons = vec![PlaybackSelectionReason::DashContainer];
    let format_key = format_key(kind, video_family, audio_family);

    match (video, video_family) {
        (Some(_), Some(PlaybackCodecFamily::H264)) => {
            score += 60;
            reasons.push(PlaybackSelectionReason::H264Video);
        }
        (Some(_), Some(PlaybackCodecFamily::Hevc)) => {
            score += 55;
            reasons.push(PlaybackSelectionReason::HevcVideo);
        }
        (Some(_), Some(PlaybackCodecFamily::Av1)) => {
            score += 50;
            reasons.push(PlaybackSelectionReason::Av1Video);
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
        format_key,
        video_codec,
        audio_codec,
        playable,
        preferred: playable && score >= 80,
        score,
        video_codec_family: video_family,
        audio_codec_family: audio_family,
        reasons,
    }
}

fn codec_preference_rank(
    family: Option<PlaybackCodecFamily>,
    preferred: &[PlaybackCodecFamily],
) -> Option<usize> {
    if preferred.is_empty() {
        return Some(0);
    }
    let family = family?;
    preferred
        .iter()
        .position(|candidate| *candidate == family)
        .map(|rank| rank + 1)
}

fn hint_needs_backfill(
    hint: &PlaybackSelectionHint,
    kind: PlaybackVariantKind,
    video: Option<&MediaRequestSpec>,
    audio: Option<&MediaRequestSpec>,
) -> bool {
    let video_codec = request_codec(video);
    let audio_codec = request_codec(audio);
    let video_family = request_codec_family(video);
    let audio_family = request_codec_family(audio);
    let expected_format_key = format_key(kind, video_family, audio_family);
    hint.format_key != expected_format_key
        || hint.video_codec_family != video_family
        || hint.audio_codec_family != audio_family
        || video_codec.is_some() && hint.video_codec.as_deref() != video_codec
        || audio_codec.is_some() && hint.audio_codec.as_deref() != audio_codec
}

fn request_codec(request: Option<&MediaRequestSpec>) -> Option<&str> {
    request.and_then(|request| request.codecs.as_deref())
}

fn request_codec_family(request: Option<&MediaRequestSpec>) -> Option<PlaybackCodecFamily> {
    request.and_then(|request| {
        request
            .codec_family
            .or_else(|| request.codecs.as_deref().map(codec_family))
    })
}

fn format_key(
    kind: PlaybackVariantKind,
    video_family: Option<PlaybackCodecFamily>,
    audio_family: Option<PlaybackCodecFamily>,
) -> String {
    if kind == PlaybackVariantKind::Flv {
        return "flv".to_owned();
    }
    let mut parts = Vec::new();
    if let Some(family) = video_family {
        parts.push(codec_family_key(family));
    }
    if let Some(family) = audio_family {
        parts.push(codec_family_key(family));
    }
    if parts.is_empty() {
        "unknown".to_owned()
    } else {
        parts.join("+")
    }
}

fn codec_family_key(family: PlaybackCodecFamily) -> &'static str {
    match family {
        PlaybackCodecFamily::H264 => "h264",
        PlaybackCodecFamily::Hevc => "hevc",
        PlaybackCodecFamily::Av1 => "av1",
        PlaybackCodecFamily::Vp9 => "vp9",
        PlaybackCodecFamily::Aac => "aac",
        PlaybackCodecFamily::Flac => "flac",
        PlaybackCodecFamily::Dolby => "dolby",
        PlaybackCodecFamily::Unknown => "unknown",
        PlaybackCodecFamily::Other => "other",
    }
}

fn unknown_format_key() -> String {
    "unknown".to_owned()
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
    match (video, audio) {
        (Some(video), Some(audio)) => Some(video.bandwidth?.saturating_add(audio.bandwidth?)),
        (Some(video), None) => video.bandwidth,
        (None, Some(audio)) => audio.bandwidth,
        (None, None) => None,
    }
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
        HttpHeaderSpec, MediaRequestKind, PlaybackAbrGroupKind, PlaybackCodecFamily,
        PlaybackCodecPreference, PlaybackEntry, PlaybackPlan, PlaybackSelectionReason,
        PlaybackVariant, PlaybackVariantKind, codec_family, source_hash,
    };
    use crate::{
        CodecFamily, DanmakuTrack, DownloadEntry, DownloadPlan, FlvSegment, MediaStream,
        StreamDiagnostics, StreamQuality, StreamSet, StreamSource,
    };

    #[test]
    fn builds_dash_playback_variants_from_download_plan() -> anyhow::Result<()> {
        let mut primary_video = media_stream(80, "avc1.640028", 1_200_000);
        primary_video.backup_urls = vec!["https://backup.example/80.m4s".to_owned()];
        primary_video.width = Some(1920);
        primary_video.height = Some(1080);
        primary_video.frame_rate = Some("60".to_owned());
        primary_video.size = Some(10_000);
        let mut audio = audio_stream("mp4a.40.2", 128_000);
        audio.backup_urls = vec!["https://backup.example/30280.m4s".to_owned()];
        let plan = test_plan(StreamSet {
            videos: vec![
                primary_video,
                media_stream(64, "hev1.1.6.L120.90", 800_000),
                media_stream(120, "av01.0.08M.08", 600_000),
            ],
            audios: vec![audio],
            flv_segments: Vec::new(),
            accept_quality: vec![80, 64, 120],
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
        assert_eq!(
            playback.entries[0].cache_key.content_id,
            "BV1xx411c7mD-cid2"
        );
        assert_eq!(playback.entries[0].abr.groups.len(), 3);
        assert_eq!(playback.entries[0].variants.len(), 3);
        let variant = &playback.entries[0].variants[0];
        let hevc_variant = &playback.entries[0].variants[1];
        let av1_variant = &playback.entries[0].variants[2];
        assert_eq!(variant.kind, PlaybackVariantKind::Dash);
        assert_eq!(variant.bandwidth, Some(1_328_000));
        assert_eq!(variant.codecs, ["avc1.640028", "mp4a.40.2"]);
        assert_eq!(variant.mime_types, ["video/mp4", "audio/mp4"]);
        assert_eq!(variant.width, Some(1920));
        assert_eq!(variant.height, Some(1080));
        assert_eq!(variant.cache_key.content_id, "BV1xx411c7mD-cid2");
        assert_eq!(variant.cache_key.variant_kind, PlaybackVariantKind::Dash);
        assert_eq!(variant.cache_key.variant_id, variant.id);
        assert_eq!(variant.cache_key.media_keys.len(), 2);
        let abr = variant
            .abr
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing ABR level"))?;
        let abr_group = playback.entries[0]
            .abr
            .groups
            .iter()
            .find(|group| group.id == abr.group_id)
            .ok_or_else(|| anyhow::anyhow!("missing ABR group"))?;
        assert_eq!(abr_group.kind, PlaybackAbrGroupKind::DashVideo);
        assert_eq!(abr_group.level_count, 1);
        assert_eq!(abr_group.min_bandwidth, Some(1_328_000));
        assert_eq!(abr_group.max_bandwidth, Some(1_328_000));
        assert_eq!(abr_group.variant_ids, vec![variant.id.clone()]);
        assert_eq!(abr.group_id, abr_group.id);
        assert_eq!(abr.level_index, 0);
        assert_eq!(abr.level_count, 1);
        assert!(!abr.switchable);
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
        assert_hevc_avplayer_hint(hevc_variant);
        assert_av1_avplayer_hint(av1_variant);
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

        assert!(playback.entries[0].abr.groups.is_empty());
        assert_eq!(playback.entries[0].variants.len(), 1);
        let variant = &playback.entries[0].variants[0];
        assert_eq!(variant.kind, PlaybackVariantKind::Flv);
        assert_eq!(variant.mime_types, ["video/x-flv"]);
        assert!(variant.abr.is_none());
        assert_eq!(variant.cache_key.media_keys.len(), 2);
        assert_flv_avplayer_hint(variant);
        assert!(
            !variant
                .matches_codec_preference(&PlaybackCodecPreference::new(Vec::new(), Vec::new()))
        );
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
    fn deserializes_legacy_avplayer_h264_aac_hint_by_rebuilding_current_hint() -> anyhow::Result<()>
    {
        let variant: PlaybackVariant = serde_json::from_value(serde_json::json!({
            "id": "legacy-hevc",
            "kind": "dash",
            "video": legacy_request_json("video", 64, "video/mp4", "hev1.1.6.L120.90"),
            "audio": legacy_request_json("audio", 30280, "audio/mp4", "mp4a.40.2"),
            "flv_segments": [],
            "bandwidth": 928_000,
            "codecs": ["hev1.1.6.L120.90", "mp4a.40.2"],
            "mime_types": ["video/mp4", "audio/mp4"],
            "width": 1280,
            "height": 720,
            "frame_rate": "30",
            "duration_seconds": 90,
            "selection_hints": {
                "avplayer_h264_aac": {
                    "playable": false,
                    "preferred": false,
                    "score": -10,
                    "video_codec_family": "hevc",
                    "audio_codec_family": "aac"
                }
            }
        }))?;

        assert_hevc_avplayer_hint(&variant);
        Ok(())
    }

    #[test]
    fn deserializes_legacy_entry_without_cache_or_abr_metadata() -> anyhow::Result<()> {
        let entry: PlaybackEntry = serde_json::from_value(serde_json::json!({
            "index": 1,
            "aid": 170_001,
            "bvid": "BV1xx411c7mD",
            "cid": 2,
            "epid": null,
            "title": "Main",
            "cover_url": null,
            "source": "normal_web",
            "qualities": [],
            "duration_seconds": 90,
            "variants": [
                {
                    "id": "legacy-h264",
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
                },
                {
                    "id": "legacy-hevc",
                    "kind": "dash",
                    "video": legacy_request_json("video", 64, "video/mp4", "hev1.1.6.L120.90"),
                    "audio": legacy_request_json("audio", 30280, "audio/mp4", "mp4a.40.2"),
                    "flv_segments": [],
                    "bandwidth": 928_000,
                    "codecs": ["hev1.1.6.L120.90", "mp4a.40.2"],
                    "mime_types": ["video/mp4", "audio/mp4"],
                    "width": 1280,
                    "height": 720,
                    "frame_rate": "30",
                    "duration_seconds": 90
                }
            ]
        }))?;

        assert_eq!(entry.cache_key.content_id, "BV1xx411c7mD-cid2");
        assert_eq!(entry.abr.groups.len(), 2);
        assert!(entry.abr.groups.iter().all(|group| group.level_count == 1));
        assert_eq!(entry.variants[0].cache_key.media_keys.len(), 2);
        assert_eq!(
            entry.variants[0].abr.as_ref().map(|abr| abr.level_index),
            Some(0)
        );
        assert_eq!(
            entry.variants[1].abr.as_ref().map(|abr| abr.level_index),
            Some(0)
        );
        assert!(
            entry
                .variants
                .iter()
                .all(|variant| !variant.abr.as_ref().is_some_and(|abr| abr.switchable))
        );
        Ok(())
    }

    #[test]
    fn abr_order_treats_partial_combined_bandwidth_as_unknown() {
        let mut unknown_high_video = media_stream(80, "avc1.640028", 1_200_000);
        unknown_high_video.bandwidth = None;
        unknown_high_video.width = Some(1920);
        unknown_high_video.height = Some(1080);
        let known_low_video = media_stream(64, "avc1.640028", 800_000);
        let plan = test_plan(StreamSet {
            videos: vec![unknown_high_video, known_low_video],
            audios: vec![audio_stream("mp4a.40.2", 128_000)],
            flv_segments: Vec::new(),
            accept_quality: vec![80, 64],
            qualities: Vec::new(),
            duration_seconds: Some(90),
        });

        let playback = PlaybackPlan::from_download_plan(&plan, &[]);
        let variants = &playback.entries[0].variants;
        let abr_group = &playback.entries[0].abr.groups[0];

        assert_eq!(variants[0].bandwidth, None);
        assert_eq!(variants[1].bandwidth, Some(928_000));
        assert_eq!(abr_group.min_bandwidth, Some(928_000));
        assert_eq!(abr_group.max_bandwidth, Some(928_000));
        assert_eq!(
            abr_group.variant_ids,
            vec![variants[1].id.clone(), variants[0].id.clone()]
        );
        assert_eq!(abr_group.level_count, 2);
        assert_eq!(variants[0].abr.as_ref().map(|abr| abr.level_index), Some(1));
        assert_eq!(variants[1].abr.as_ref().map(|abr| abr.level_index), Some(0));
        assert!(variants[0].abr.as_ref().is_some_and(|abr| abr.switchable));
        assert!(variants[1].abr.as_ref().is_some_and(|abr| abr.switchable));
    }

    #[test]
    fn abr_groups_isolate_missing_video_codecs() {
        let mut first_unknown = media_stream(80, "avc1.640028", 1_200_000);
        first_unknown.codecs = None;
        let mut second_unknown = media_stream(64, "avc1.640028", 800_000);
        second_unknown.codecs = None;
        let plan = test_plan(StreamSet {
            videos: vec![first_unknown, second_unknown],
            audios: vec![audio_stream("mp4a.40.2", 128_000)],
            flv_segments: Vec::new(),
            accept_quality: vec![80, 64],
            qualities: Vec::new(),
            duration_seconds: Some(90),
        });

        let playback = PlaybackPlan::from_download_plan(&plan, &[]);
        let variants = &playback.entries[0].variants;

        assert_eq!(playback.entries[0].abr.groups.len(), 2);
        assert!(
            playback.entries[0]
                .abr
                .groups
                .iter()
                .all(|group| group.level_count == 1)
        );
        assert_ne!(
            variants[0].abr.as_ref().map(|abr| abr.group_id.as_str()),
            variants[1].abr.as_ref().map(|abr| abr.group_id.as_str())
        );
        assert!(
            variants
                .iter()
                .all(|variant| !variant.abr.as_ref().is_some_and(|abr| abr.switchable))
        );
    }

    #[test]
    fn codec_family_keeps_family_only_app_streams_selectable() {
        let mut hevc_video = media_stream(80, "hev1.1.6.L120.90", 1_200_000);
        hevc_video.codecs = None;
        hevc_video.codec_family = Some(CodecFamily::Hevc);
        let mut lower_hevc_video = media_stream(64, "hev1.1.6.L120.90", 800_000);
        lower_hevc_video.codecs = None;
        lower_hevc_video.codec_family = Some(CodecFamily::Hevc);
        let plan = test_plan(StreamSet {
            videos: vec![hevc_video, lower_hevc_video],
            audios: vec![audio_stream("mp4a.40.2", 128_000)],
            flv_segments: Vec::new(),
            accept_quality: vec![80, 64],
            qualities: Vec::new(),
            duration_seconds: Some(90),
        });

        let playback = PlaybackPlan::from_download_plan(&plan, &[]);
        let variants = &playback.entries[0].variants;
        let hint = &variants[0].selection_hints.avplayer;

        assert_eq!(playback.entries[0].abr.groups.len(), 1);
        assert_eq!(playback.entries[0].abr.groups[0].level_count, 2);
        assert_eq!(
            variants[0]
                .video
                .as_ref()
                .and_then(|video| video.codecs.clone()),
            None
        );
        assert_eq!(
            variants[0]
                .video
                .as_ref()
                .and_then(|video| video.codec_family),
            Some(PlaybackCodecFamily::Hevc)
        );
        assert_eq!(hint.video_codec, None);
        assert_eq!(hint.video_codec_family, Some(PlaybackCodecFamily::Hevc));
        assert_eq!(
            variants[0].codec_preference_rank(&PlaybackCodecPreference::hevc_aac()),
            Some(1_001)
        );
        assert!(variants[0].abr.as_ref().is_some_and(|abr| abr.switchable));
    }

    #[test]
    fn abr_groups_separate_other_video_codec_strings() {
        let first_other = media_stream(80, "future.1", 1_200_000);
        let second_other = media_stream(64, "future.2", 800_000);
        let plan = test_plan(StreamSet {
            videos: vec![first_other, second_other],
            audios: vec![audio_stream("mp4a.40.2", 128_000)],
            flv_segments: Vec::new(),
            accept_quality: vec![80, 64],
            qualities: Vec::new(),
            duration_seconds: Some(90),
        });

        let playback = PlaybackPlan::from_download_plan(&plan, &[]);
        let variants = &playback.entries[0].variants;

        assert_eq!(playback.entries[0].abr.groups.len(), 2);
        assert_ne!(
            variants[0].abr.as_ref().map(|abr| abr.group_id.as_str()),
            variants[1].abr.as_ref().map(|abr| abr.group_id.as_str())
        );
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

    #[test]
    fn codec_preference_can_prioritize_non_h264_variants() {
        let plan = test_plan(StreamSet {
            videos: vec![
                media_stream(80, "avc1.640028", 1_200_000),
                media_stream(64, "hev1.1.6.L120.90", 800_000),
                media_stream(120, "av01.0.08M.08", 600_000),
            ],
            audios: vec![audio_stream("mp4a.40.2", 128_000)],
            flv_segments: Vec::new(),
            accept_quality: vec![80, 64, 120],
            qualities: Vec::new(),
            duration_seconds: Some(90),
        });
        let playback = PlaybackPlan::from_download_plan(&plan, &[]);
        let variants = &playback.entries[0].variants;
        let hevc_first = PlaybackCodecPreference::new(
            vec![
                PlaybackCodecFamily::Hevc,
                PlaybackCodecFamily::H264,
                PlaybackCodecFamily::Av1,
            ],
            vec![PlaybackCodecFamily::Aac],
        );
        let av1_only = PlaybackCodecPreference::av1_aac();

        assert_eq!(variants[0].codec_preference_rank(&hevc_first), Some(2_001));
        assert_eq!(variants[1].codec_preference_rank(&hevc_first), Some(1_001));
        assert_eq!(variants[2].codec_preference_rank(&hevc_first), Some(3_001));
        assert!(!variants[0].matches_codec_preference(&av1_only));
        assert!(variants[2].matches_codec_preference(&av1_only));
        assert_eq!(
            variants[1].selection_hints.avplayer.video_codec.as_deref(),
            Some("hev1.1.6.L120.90")
        );
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
        let hint = &variant.selection_hints.avplayer;
        assert!(hint.playable);
        assert!(hint.preferred);
        assert_eq!(hint.score, 90);
        assert_eq!(hint.format_key, "h264+aac");
        assert_eq!(hint.video_codec.as_deref(), Some("avc1.640028"));
        assert_eq!(hint.audio_codec.as_deref(), Some("mp4a.40.2"));
        assert_eq!(hint.video_codec_family, Some(PlaybackCodecFamily::H264));
        assert_eq!(hint.audio_codec_family, Some(PlaybackCodecFamily::Aac));
        assert!(hint.reasons.contains(&PlaybackSelectionReason::H264Video));
        assert!(hint.reasons.contains(&PlaybackSelectionReason::AacAudio));
    }

    fn assert_hevc_avplayer_hint(variant: &PlaybackVariant) {
        let hint = &variant.selection_hints.avplayer;
        assert!(hint.playable);
        assert!(hint.preferred);
        assert_eq!(hint.score, 85);
        assert_eq!(hint.format_key, "hevc+aac");
        assert_eq!(hint.video_codec.as_deref(), Some("hev1.1.6.L120.90"));
        assert_eq!(hint.video_codec_family, Some(PlaybackCodecFamily::Hevc));
        assert!(hint.reasons.contains(&PlaybackSelectionReason::HevcVideo));
    }

    fn assert_av1_avplayer_hint(variant: &PlaybackVariant) {
        let hint = &variant.selection_hints.avplayer;
        assert!(hint.playable);
        assert!(hint.preferred);
        assert_eq!(hint.score, 80);
        assert_eq!(hint.format_key, "av1+aac");
        assert_eq!(hint.video_codec.as_deref(), Some("av01.0.08M.08"));
        assert_eq!(hint.video_codec_family, Some(PlaybackCodecFamily::Av1));
        assert!(hint.reasons.contains(&PlaybackSelectionReason::Av1Video));
    }

    fn assert_flv_avplayer_hint(variant: &PlaybackVariant) {
        let hint = &variant.selection_hints.avplayer;
        assert!(!hint.playable);
        assert!(!hint.preferred);
        assert_eq!(hint.score, -100);
        assert_eq!(hint.format_key, "flv");
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

    fn media_stream(id: u32, codecs: &str, bandwidth: u64) -> MediaStream {
        MediaStream {
            id,
            base_url: format!("https://video.example/{id}.m4s?token=secret"),
            backup_urls: Vec::new(),
            codecs: Some(codecs.to_owned()),
            codec_family: None,
            bandwidth: Some(bandwidth),
            width: Some(1280),
            height: Some(720),
            frame_rate: Some("30".to_owned()),
            mime_type: Some("video/mp4".to_owned()),
            size: Some(8_000),
        }
    }

    fn audio_stream(codecs: &str, bandwidth: u64) -> MediaStream {
        MediaStream {
            id: 30280,
            base_url: "https://audio.example/30280.m4s?token=secret".to_owned(),
            backup_urls: Vec::new(),
            codecs: Some(codecs.to_owned()),
            codec_family: None,
            bandwidth: Some(bandwidth),
            width: None,
            height: None,
            frame_rate: None,
            mime_type: Some("audio/mp4".to_owned()),
            size: Some(2_000),
        }
    }
}
