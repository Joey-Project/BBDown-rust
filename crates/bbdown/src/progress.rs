use crate::DownloadFileKind;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DownloadProgressEvent {
    PlanStarted {
        title: String,
        output_dir: PathBuf,
        entry_count: usize,
    },
    EntryStarted {
        index: u32,
        title: String,
        directory: PathBuf,
    },
    FileStarted {
        entry_index: u32,
        entry_title: String,
        kind: DownloadFileKind,
        path: PathBuf,
        resumed_from: u64,
        expected_size: Option<u64>,
        attempt: u32,
        max_attempts: u32,
    },
    FileProgress {
        entry_index: u32,
        entry_title: String,
        kind: DownloadFileKind,
        path: PathBuf,
        bytes_delta: u64,
        bytes_written: u64,
        resumed_from: u64,
        expected_size: Option<u64>,
    },
    FileCompleted {
        entry_index: u32,
        entry_title: String,
        kind: DownloadFileKind,
        path: PathBuf,
        bytes_written: u64,
        resumed_from: u64,
        total_bytes: u64,
    },
    FileFailed {
        entry_index: u32,
        entry_title: String,
        kind: DownloadFileKind,
        path: PathBuf,
        attempt: u32,
        max_attempts: u32,
        error: String,
    },
    MuxStarted {
        entry_index: u32,
        entry_title: String,
        output_path: PathBuf,
        command: Vec<String>,
    },
    MuxCompleted {
        entry_index: u32,
        entry_title: String,
        output_path: PathBuf,
    },
    MuxFailed {
        entry_index: u32,
        entry_title: String,
        output_path: PathBuf,
        command: Vec<String>,
        error: String,
    },
    EntryCompleted {
        index: u32,
        title: String,
        directory: PathBuf,
        file_count: usize,
        mux_output: Option<PathBuf>,
    },
    EntryFailed {
        index: u32,
        title: String,
        directory: PathBuf,
        error: String,
    },
    PlanCompleted {
        title: String,
        output_dir: PathBuf,
        entry_count: usize,
    },
    PlanFailed {
        title: String,
        output_dir: PathBuf,
        completed_entries: usize,
        error: String,
    },
    PlanCancelled {
        title: String,
        output_dir: PathBuf,
        completed_entries: usize,
        error: String,
    },
}

pub trait DownloadProgressSink: Send + Sync {
    fn on_download_progress(&self, event: &DownloadProgressEvent);
}

impl<F> DownloadProgressSink for F
where
    F: Fn(&DownloadProgressEvent) + Send + Sync,
{
    fn on_download_progress(&self, event: &DownloadProgressEvent) {
        self(event);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoopDownloadProgress;

impl DownloadProgressSink for NoopDownloadProgress {
    fn on_download_progress(&self, _event: &DownloadProgressEvent) {}
}
