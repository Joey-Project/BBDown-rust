use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedContent {
    Video(VideoMetadata),
    Season(SeasonResolution),
    Collection(VideoCollectionResolution),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SeasonResolution {
    pub season: SeasonMetadata,
    pub selected_episodes: Vec<EpisodeMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VideoCollectionResolution {
    pub collection: VideoCollectionMetadata,
    pub selected_items: Vec<VideoCollectionItem>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UgcCollectionKind {
    Collection,
    Series,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UgcCollectionReference {
    pub id: u64,
    pub kind: UgcCollectionKind,
    pub owner_mid: u64,
    pub title: String,
    pub description: String,
    pub cover_url: Option<String>,
    pub item_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VideoCollectionMetadata {
    pub id: Option<u64>,
    pub kind: VideoCollectionKind,
    pub title: String,
    pub description: String,
    pub cover_url: Option<String>,
    pub pub_time: Option<i64>,
    pub owner: Option<Owner>,
    pub items: Vec<VideoCollectionItem>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoCollectionKind {
    Space,
    Favorite,
    Collection,
    Series,
    Recommendation,
    RelatedRecommendation,
    Search,
    Following,
    SpaceDynamic,
    History,
    WatchLater,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VideoCollectionItem {
    pub index: u32,
    pub aid: u64,
    pub bvid: Option<String>,
    pub cid: u64,
    pub title: String,
    pub cover_url: Option<String>,
    pub description: String,
    pub pub_time: Option<i64>,
    pub owner: Option<Owner>,
    pub duration_seconds: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct VideoMetadata {
    pub aid: u64,
    pub bvid: Option<String>,
    pub title: String,
    pub description: String,
    pub cover_url: Option<String>,
    pub pub_time: Option<i64>,
    pub owner: Option<Owner>,
    pub tags: Vec<Tag>,
    pub pages: Vec<PageMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Owner {
    pub mid: u64,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PageMetadata {
    pub index: u32,
    pub aid: u64,
    pub cid: u64,
    pub epid: Option<u64>,
    pub title: String,
    pub duration_seconds: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    pub id: u64,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SeasonMetadata {
    pub season_id: Option<u64>,
    pub media_id: Option<u64>,
    pub title: String,
    pub description: String,
    pub cover_url: Option<String>,
    pub main_episode_count: usize,
    pub areas: Vec<String>,
    pub tags: Vec<String>,
    pub episodes: Vec<EpisodeMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub episode_groups: Vec<SeasonEpisodeGroup>,
}

/// Describes one upstream episode group within a season.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SeasonEpisodeGroup {
    pub kind: SeasonEpisodeGroupKind,
    /// The upstream group identifier, when it is present and supported.
    pub source_id: Option<String>,
    /// The upstream title, preserving missing titles separately from empty titles.
    pub title: Option<String>,
    /// Zero-based offsets into [`SeasonMetadata::episodes`], not EP IDs or display indexes.
    pub episode_positions: Vec<usize>,
}

/// Identifies the source role of a season episode group.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeasonEpisodeGroupKind {
    Main,
    Section,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EpisodeMetadata {
    pub index: u32,
    pub aid: u64,
    pub bvid: Option<String>,
    pub cid: u64,
    pub epid: u64,
    pub title: String,
    pub long_title: Option<String>,
    pub pub_time: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadPlan {
    pub title: String,
    pub entries: Vec<DownloadEntry>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DownloadEntry {
    pub index: u32,
    pub aid: u64,
    pub bvid: Option<String>,
    pub cid: u64,
    pub epid: Option<u64>,
    pub title: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    pub source: StreamSource,
    pub streams: StreamSet,
    #[serde(default, skip_serializing_if = "StreamDiagnostics::is_empty")]
    pub diagnostics: StreamDiagnostics,
    pub subtitles: Vec<SubtitleTrack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<ChapterTrack>,
    pub danmaku: DanmakuTrack,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamSource {
    NormalWeb,
    NormalTv,
    NormalApp,
    PgcWeb,
    PgcTv,
    PgcApp,
    PgcProxy,
    PugvWeb,
    IntlWeb,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct StreamDiagnostics {
    pub attempts: Vec<StreamResolverAttempt>,
}

impl StreamDiagnostics {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.attempts.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StreamResolverAttempt {
    pub source: StreamSource,
    pub outcome: StreamResolverOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamResolverOutcome {
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct StreamSet {
    pub videos: Vec<MediaStream>,
    pub audios: Vec<MediaStream>,
    pub flv_segments: Vec<FlvSegment>,
    pub accept_quality: Vec<u32>,
    #[serde(default)]
    pub qualities: Vec<StreamQuality>,
    pub duration_seconds: Option<u32>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct StreamQuality {
    pub id: u32,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MediaStream {
    pub id: u32,
    pub base_url: String,
    pub backup_urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_doc: Option<String>,
    pub codecs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec_family: Option<CodecFamily>,
    pub bandwidth: Option<u64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<u64>,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodecFamily {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FlvSegment {
    pub order: u32,
    pub url: String,
    pub backup_urls: Vec<String>,
    pub size: Option<u64>,
    pub length_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SubtitleTrack {
    pub language: String,
    pub language_doc: Option<String>,
    #[serde(default)]
    pub is_ai_generated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_type: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai_status: Option<i64>,
    pub url: String,
    pub format: SubtitleFormat,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChapterTrack {
    pub title: String,
    pub start_seconds: u64,
    pub end_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleFormat {
    Json,
    Ass,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DanmakuTrack {
    pub cid: u64,
    pub xml_url: String,
}

#[cfg(test)]
mod tests {
    use super::{
        DownloadEntry, SeasonEpisodeGroup, SeasonEpisodeGroupKind, SeasonMetadata, StreamSource,
    };

    #[test]
    fn download_entry_defaults_missing_cover_url() -> anyhow::Result<()> {
        let entry: DownloadEntry = serde_json::from_value(serde_json::json!({
            "index": 1,
            "aid": 170_001,
            "bvid": "BV1xx411c7mD",
            "cid": 2,
            "epid": null,
            "title": "Main",
            "source": "normal_web",
            "streams": {
                "videos": [],
                "audios": [],
                "flv_segments": [],
                "accept_quality": [],
                "duration_seconds": null
            },
            "subtitles": [],
            "danmaku": {
                "cid": 2,
                "xml_url": "https://comment.example/2.xml"
            }
        }))?;

        assert_eq!(entry.cover_url, None);
        assert_eq!(entry.chapters.len(), 0);
        assert_eq!(entry.source, StreamSource::NormalWeb);
        Ok(())
    }

    #[test]
    fn season_episode_groups_default_and_use_snake_case() -> anyhow::Result<()> {
        let season: SeasonMetadata = serde_json::from_value(serde_json::json!({
            "season_id": null,
            "media_id": null,
            "title": "Season",
            "description": "",
            "cover_url": null,
            "main_episode_count": 0,
            "areas": [],
            "tags": [],
            "episodes": []
        }))?;
        assert_eq!(season.episode_groups, [] as [SeasonEpisodeGroup; 0]);
        let serialized = serde_json::to_value(&season)?;
        assert!(serialized.get("episode_groups").is_none());

        let group = SeasonEpisodeGroup {
            kind: SeasonEpisodeGroupKind::Section,
            source_id: Some("42".to_owned()),
            title: Some(String::new()),
            episode_positions: vec![0, 2],
        };
        let value = serde_json::to_value(&group)?;
        assert_eq!(value["kind"], "section");
        assert_eq!(value["source_id"], "42");
        assert_eq!(value["title"], "");
        assert_eq!(value["episode_positions"], serde_json::json!([0, 2]));
        assert_eq!(serde_json::from_value::<SeasonEpisodeGroup>(value)?, group);
        Ok(())
    }
}
