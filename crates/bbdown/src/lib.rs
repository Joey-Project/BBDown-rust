#![allow(clippy::missing_errors_doc)]
#![allow(clippy::module_name_repetitions)]

mod bv;
mod client;
mod credentials;
mod error;
mod input;
mod models;
mod selection;

pub use client::{BiliClient, ClientConfig, EndpointConfig};
pub use credentials::{CredentialSource, CredentialStore, Credentials};
pub use error::{Error, Result};
pub use input::Input;
pub use models::{
    DanmakuTrack, DownloadEntry, DownloadPlan, EpisodeMetadata, FlvSegment, MediaStream, Owner,
    PageMetadata, ResolvedContent, SeasonMetadata, SeasonResolution, StreamSet, StreamSource,
    SubtitleFormat, SubtitleTrack, Tag, VideoMetadata,
};
pub use selection::Selection;
