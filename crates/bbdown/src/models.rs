use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedContent {
    Video(VideoMetadata),
    Season(SeasonResolution),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SeasonResolution {
    pub season: SeasonMetadata,
    pub selected_episodes: Vec<EpisodeMetadata>,
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
