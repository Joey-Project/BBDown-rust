use crate::DownloadFileKind;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

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
    CdnShardCompleted {
        entry_index: u32,
        entry_title: String,
        kind: DownloadFileKind,
        host: String,
        bytes: u64,
    },
    TransferBytesReceived {
        request_id: u64,
        entry_index: Option<u32>,
        entry_title: Option<String>,
        kind: Option<DownloadFileKind>,
        path: Option<PathBuf>,
        phase: DownloadTransferPhase,
        host: Option<String>,
        bytes_delta: u64,
        bytes_received: u64,
    },
    TransferDiagnostic {
        request_id: Option<u64>,
        entry_index: Option<u32>,
        entry_title: Option<String>,
        kind: Option<DownloadFileKind>,
        path: Option<PathBuf>,
        phase: Option<DownloadTransferPhase>,
        host: Option<String>,
        diagnostic: DownloadTransferDiagnostic,
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

/// HTTP response-body request phase reported by transfer diagnostics.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadTransferPhase {
    AutomaticProbe,
    ShardProbe,
    RangeChunk,
    WholeFile,
    StandaloneProbe,
}

/// Stable, non-sensitive reason why a CDN candidate was excluded from a transfer group.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CdnCandidateExclusionReason {
    ProbeFailed,
    SizeMismatch,
    SampleMismatch,
    PathQueryMismatch,
    SchemeMismatch,
    OutsideWinningGroup,
}

/// Stable failure category used by retry and request-completion diagnostics.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadTransferFailureReason {
    RequestFailed,
    InvalidResponse,
    BodyReadFailed,
    BodyStalled,
    TimedOut,
    IncompleteBody,
    OversizedBody,
    WriteFailed,
    Other,
}

/// Stable reason that a sharded attempt yielded to the whole-file downloader.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadTransferFallbackReason {
    RangeTransferFailed,
    NoCompatibleCandidates,
    OutputNotPublished,
}

/// Final status for one observed HTTP response-body request.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DownloadTransferRequestOutcome {
    Succeeded,
    Failed {
        reason: DownloadTransferFailureReason,
    },
    Cancelled,
}

/// Typed transfer diagnostic. It intentionally excludes URLs and upstream error text.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum DownloadTransferDiagnostic {
    CandidateSelected,
    CandidateExcluded {
        reason: CdnCandidateExclusionReason,
    },
    RetryScheduled {
        reason: DownloadTransferFailureReason,
    },
    WholeFileFallback {
        reason: DownloadTransferFallbackReason,
    },
    RequestFinished {
        outcome: DownloadTransferRequestOutcome,
        bytes_received: u64,
    },
}

#[derive(Clone, Debug, Default)]
struct TransferEventContext {
    entry_index: Option<u32>,
    entry_title: Option<String>,
    kind: Option<DownloadFileKind>,
    path: Option<PathBuf>,
}

/// Request-local observer shared by the media probes, range downloader, and file downloader.
pub(crate) struct TransferReporter<'a, P: DownloadProgressSink + ?Sized> {
    progress: &'a P,
    context: TransferEventContext,
    enabled: bool,
    next_request_id: AtomicU64,
}

impl<'a, P: DownloadProgressSink + ?Sized> TransferReporter<'a, P> {
    pub(crate) fn new(
        progress: &'a P,
        entry_index: Option<u32>,
        entry_title: Option<&str>,
        kind: Option<&DownloadFileKind>,
        path: Option<&Path>,
    ) -> Self {
        let enabled = progress.wants_transfer_diagnostics();
        let context = if enabled {
            TransferEventContext {
                entry_index,
                entry_title: entry_title.map(str::to_owned),
                kind: kind.cloned(),
                path: path.map(Path::to_path_buf),
            }
        } else {
            TransferEventContext::default()
        };
        Self {
            progress,
            context,
            enabled,
            next_request_id: AtomicU64::new(0),
        }
    }

    pub(crate) fn begin_request(
        &self,
        url: &str,
        phase: DownloadTransferPhase,
        retry_reason: Option<DownloadTransferFailureReason>,
    ) -> Option<TransferRequestReporter<'a, P>> {
        if !self.enabled {
            return None;
        }
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed) + 1;
        let reporter = TransferRequestReporter {
            progress: self.progress,
            context: self.context.clone(),
            request_id,
            phase,
            host: transfer_host_label(url),
            bytes_received: AtomicU64::new(0),
            body_read_failed: AtomicBool::new(false),
        };
        if let Some(reason) = retry_reason {
            reporter.diagnostic(DownloadTransferDiagnostic::RetryScheduled { reason });
        }
        Some(reporter)
    }

    pub(crate) fn diagnostic(
        &self,
        phase: Option<DownloadTransferPhase>,
        diagnostic: DownloadTransferDiagnostic,
    ) {
        if !self.enabled {
            return;
        }
        emit_transfer_diagnostic(self.progress, &self.context, None, phase, None, diagnostic);
    }

    pub(crate) fn diagnostic_for_url(
        &self,
        source_url: &str,
        phase: Option<DownloadTransferPhase>,
        diagnostic: DownloadTransferDiagnostic,
    ) {
        if !self.enabled {
            return;
        }
        emit_transfer_diagnostic(
            self.progress,
            &self.context,
            None,
            phase,
            transfer_host_label(source_url),
            diagnostic,
        );
    }
}

pub(crate) struct TransferRequestReporter<'a, P: DownloadProgressSink + ?Sized> {
    progress: &'a P,
    context: TransferEventContext,
    request_id: u64,
    phase: DownloadTransferPhase,
    host: Option<String>,
    bytes_received: AtomicU64,
    body_read_failed: AtomicBool,
}

impl<P: DownloadProgressSink + ?Sized> TransferRequestReporter<'_, P> {
    pub(crate) fn bytes_received(&self) -> u64 {
        self.bytes_received.load(Ordering::Relaxed)
    }

    pub(crate) fn mark_body_read_failed(&self) {
        self.body_read_failed.store(true, Ordering::Relaxed);
    }

    pub(crate) fn body_read_failed(&self) -> bool {
        self.body_read_failed.load(Ordering::Relaxed)
    }

    pub(crate) fn body_chunk(&self, bytes: usize) {
        let bytes_delta = u64::try_from(bytes).unwrap_or(u64::MAX);
        let bytes_received = self
            .bytes_received
            .fetch_add(bytes_delta, Ordering::Relaxed)
            .saturating_add(bytes_delta);
        self.progress
            .on_download_progress(&DownloadProgressEvent::TransferBytesReceived {
                request_id: self.request_id,
                entry_index: self.context.entry_index,
                entry_title: self.context.entry_title.clone(),
                kind: self.context.kind.clone(),
                path: self.context.path.clone(),
                phase: self.phase,
                host: self.host.clone(),
                bytes_delta,
                bytes_received,
            });
    }

    pub(crate) fn finish(&self, outcome: DownloadTransferRequestOutcome) {
        self.diagnostic(DownloadTransferDiagnostic::RequestFinished {
            outcome,
            bytes_received: self.bytes_received(),
        });
    }

    pub(crate) fn diagnostic(&self, diagnostic: DownloadTransferDiagnostic) {
        emit_transfer_diagnostic(
            self.progress,
            &self.context,
            Some(self.request_id),
            Some(self.phase),
            self.host.clone(),
            diagnostic,
        );
    }
}

fn transfer_host_label(source_url: &str) -> Option<String> {
    let url = url::Url::parse(source_url).ok()?;
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    })
}

fn emit_transfer_diagnostic<P: DownloadProgressSink + ?Sized>(
    progress: &P,
    context: &TransferEventContext,
    request_id: Option<u64>,
    phase: Option<DownloadTransferPhase>,
    host: Option<String>,
    diagnostic: DownloadTransferDiagnostic,
) {
    progress.on_download_progress(&DownloadProgressEvent::TransferDiagnostic {
        request_id,
        entry_index: context.entry_index,
        entry_title: context.entry_title.clone(),
        kind: context.kind.clone(),
        path: context.path.clone(),
        phase,
        host,
        diagnostic,
    });
}

pub trait DownloadProgressSink: Send + Sync {
    fn on_download_progress(&self, event: &DownloadProgressEvent);

    fn wants_transfer_diagnostics(&self) -> bool {
        true
    }
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

    fn wants_transfer_diagnostics(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{
        DownloadProgressEvent, DownloadTransferDiagnostic, DownloadTransferFailureReason,
        DownloadTransferPhase, DownloadTransferRequestOutcome, NoopDownloadProgress,
        TransferReporter,
    };

    #[test]
    fn transfer_events_are_request_scoped_and_redact_url_credentials() -> anyhow::Result<()> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = Arc::clone(&events);
        let sink = move |event: &DownloadProgressEvent| match sink_events.lock() {
            Ok(mut events) => events.push(event.clone()),
            Err(poisoned) => poisoned.into_inner().push(event.clone()),
        };
        let reporter = TransferReporter::new(&sink, None, None, None, None);
        let request = reporter.begin_request(
            "https://cdn.example:8443/asset.m4s?token=codex_synth_v1_bearer_a",
            DownloadTransferPhase::StandaloneProbe,
            None,
        );
        let Some(request) = request else {
            return Err(anyhow::anyhow!(
                "progress closure should enable transfer diagnostics"
            ));
        };
        request.body_chunk(4);
        request.body_chunk(3);
        request.finish(DownloadTransferRequestOutcome::Failed {
            reason: DownloadTransferFailureReason::IncompleteBody,
        });

        let events = match events.lock() {
            Ok(events) => events.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        assert_eq!(events.len(), 3);
        let bytes = events
            .iter()
            .filter_map(|event| match event {
                DownloadProgressEvent::TransferBytesReceived {
                    request_id,
                    entry_index,
                    entry_title,
                    kind,
                    path,
                    phase,
                    host,
                    bytes_delta,
                    bytes_received,
                } => Some((
                    *request_id,
                    *entry_index,
                    entry_title,
                    kind,
                    path,
                    *phase,
                    host.as_deref(),
                    *bytes_delta,
                    *bytes_received,
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(bytes.len(), 2);
        assert_eq!(
            bytes[0],
            (
                1,
                None,
                &None,
                &None,
                &None,
                DownloadTransferPhase::StandaloneProbe,
                Some("cdn.example:8443"),
                4,
                4
            )
        );
        assert_eq!(bytes[1].0, 1);
        assert_eq!(bytes[1].7, 3);
        assert_eq!(bytes[1].8, 7);
        assert!(matches!(
            events.last(),
            Some(DownloadProgressEvent::TransferDiagnostic {
                request_id: Some(1),
                entry_index: None,
                path: None,
                phase: Some(DownloadTransferPhase::StandaloneProbe),
                host: Some(host),
                diagnostic: DownloadTransferDiagnostic::RequestFinished {
                    bytes_received: 7,
                    outcome: DownloadTransferRequestOutcome::Failed {
                        reason: DownloadTransferFailureReason::IncompleteBody,
                    },
                },
                ..
            }) if host == "cdn.example:8443"
        ));
        let serialized = serde_json::to_string(&events)?;
        assert!(!serialized.contains("codex_synth_v1_bearer_a"));
        assert!(!serialized.contains("/asset.m4s"));
        Ok(())
    }

    #[test]
    fn noop_progress_does_not_allocate_transfer_events() {
        let reporter = TransferReporter::new(&NoopDownloadProgress, None, None, None, None);
        assert!(
            reporter
                .begin_request(
                    "https://cdn.example/asset.m4s",
                    DownloadTransferPhase::WholeFile,
                    None,
                )
                .is_none()
        );
    }
}
