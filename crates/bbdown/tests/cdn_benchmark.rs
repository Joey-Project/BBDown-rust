//! Explicit, opt-in live CDN benchmark. Run with `--ignored`; normal test and CI runs skip it.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    env,
    fs::File,
    future::Future,
    io::{Read, Write as _},
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};

use bbdown_core::{
    BiliClient, ClientConfig, DownloadCancellationToken, DownloadMode, DownloadOptions,
    DownloadPlan, DownloadProgressEvent, DownloadReport, DownloadTransferDiagnostic,
    DownloadTransferPhase, DownloadTransferRequestOutcome, Error, MediaHostOptions, MediaStream,
    MuxOptions, RetryPolicy, StreamSelection, probe_media_cdns_with_progress,
};
use httpmock::{Method::GET, MockServer};
use serde::Serialize;
use sha2::{Digest, Sha256};
use url::Url;

const DEFAULT_URL: &str = "https://www.bilibili.com/video/BV1QtjA6BEB8/";
const MAX_REPETITIONS: usize = 3;
const MAX_MEDIA_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MULTI_HOSTS: usize = 4;
const MAX_SIZE_PROBE_SAMPLE_BYTES: u64 = 64 * 1024;
const SIZE_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const DOWNLOAD_REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(5);
const TOTAL_RUN_TIMEOUT: std::time::Duration = std::time::Duration::from_mins(45);
const MAX_TOTAL_CONSUMED_BODY_BYTES: u64 = 3 * 1024 * 1024 * 1024;
const MAX_OBSERVED_REQUESTS: u64 = 16_384;
const CLEANUP_GRACE: std::time::Duration = std::time::Duration::from_secs(5);
const DOWNLOAD_TIMEOUT_ERROR: &str = "download request timeout elapsed";
const BUDGET_CANCEL_REASON: &str = "CDN benchmark budget exhausted";
const GROUPS: [Group; 3] = [
    Group::FixedHostBaseline,
    Group::FixedHostRange4,
    Group::MultiHostRange4,
];

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
enum Group {
    FixedHostBaseline,
    FixedHostRange4,
    MultiHostRange4,
}

#[derive(Debug, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "This is the flat benchmark JSON schema; nesting its status flags would change the output contract."
)]
struct Record {
    sample: String,
    entry_index: u32,
    repetition: usize,
    order: usize,
    order_offset: usize,
    started_at_epoch_ms: u128,
    group: Group,
    fixed_host: String,
    candidate_hosts: Vec<String>,
    request_timeout_secs: u64,
    quality: u32,
    codecs: Option<String>,
    declared_size: u64,
    size_source: &'static str,
    size_probe_phase: Option<&'static str>,
    size_probe_sample_bytes: u64,
    size_probe_elapsed_ms: u128,
    size_probe_observation_scope: &'static str,
    size_probe_transfer_bytes_by_host_phase: Vec<HostPhaseBytes>,
    size_probe_transfer_activity_by_host_phase: Vec<HostPhaseActivity>,
    size_probe_transfer_consumed_body_bytes: u64,
    size_probe_observed_request_ids: Vec<u64>,
    size_probe_observed_request_count_lower_bound: u64,
    size_probe_request_outcomes: RequestOutcomeCounts,
    size_probe_transfer_diagnostics: Vec<TransferDiagnosticObservation>,
    size_probe_included_in_group_totals: bool,
    configured_cdn_parallelism: usize,
    success: bool,
    elapsed_ms: u128,
    benchmark_elapsed_ms: u128,
    size: Option<u64>,
    sha256: Option<String>,
    shard_bytes_by_host: BTreeMap<String, u64>,
    whole_file_fallback: Option<bool>,
    transfer_bytes_by_host_phase: Vec<HostPhaseBytes>,
    transfer_activity_by_host_phase: Vec<HostPhaseActivity>,
    observed_request_ids: Vec<u64>,
    observed_request_count_lower_bound: u64,
    request_outcomes: RequestOutcomeCounts,
    transfer_diagnostics: Vec<TransferDiagnosticObservation>,
    transfer_total_consumed_body_bytes: u64,
    preflight_consumed_body_bytes: u64,
    media_body_consumed_bytes: u64,
    budget_exhausted: Option<&'static str>,
    budget_body_bytes_limit: u64,
    budget_observed_body_bytes: u64,
    budget_body_overshoot_bytes: u64,
    budget_observed_request_limit: u64,
    budget_observed_request_count_lower_bound: u64,
    cleanup_timed_out: bool,
    total_run_timeout_secs: u64,
    cleanup_grace_secs: u64,
    request_id_scope: &'static str,
    request_count_semantics: &'static str,
    transfer_activity_semantics: &'static str,
    body_bytes_scope: &'static str,
    body_measurement: &'static str,
    body_budget_semantics: &'static str,
    api_response_body_bytes_observed: bool,
    wire_bytes_observed: bool,
    wire_completeness_observed: bool,
    valid_three_way_sample: bool,
    validity_reasons: Vec<&'static str>,
    error_class: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
struct HostPhaseBytes {
    host: Option<String>,
    phase: DownloadTransferPhase,
    bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
struct HostPhaseActivity {
    host: Option<String>,
    phase: DownloadTransferPhase,
    first_event_elapsed_ms: u128,
    last_event_elapsed_ms: u128,
    observed_activity_elapsed_ms: u128,
}

#[derive(Clone, Debug, Default, Serialize)]
struct RequestOutcomeCounts {
    completed: u64,
    failed: u64,
    cancelled: u64,
}

#[derive(Clone, Debug, Serialize)]
struct TransferDiagnosticObservation {
    request_id: Option<u64>,
    phase: Option<DownloadTransferPhase>,
    host: Option<String>,
    observed_at_elapsed_ms: u128,
    diagnostic: DownloadTransferDiagnostic,
}

#[derive(Clone, Debug)]
struct TransferEventCollector {
    started_at: Instant,
    bytes_by_host_phase: Vec<HostPhaseBytes>,
    activity_by_host_phase: Vec<HostPhaseActivity>,
    request_ids: BTreeSet<u64>,
    request_outcomes: RequestOutcomeCounts,
    diagnostics: Vec<TransferDiagnosticObservation>,
}

impl Default for TransferEventCollector {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            bytes_by_host_phase: Vec::new(),
            activity_by_host_phase: Vec::new(),
            request_ids: BTreeSet::new(),
            request_outcomes: RequestOutcomeCounts::default(),
            diagnostics: Vec::new(),
        }
    }
}

#[derive(Debug, Default)]
struct RunBudgetState {
    consumed_body_bytes: u64,
    observed_request_count: u64,
    exhausted_reason: Option<&'static str>,
}

#[derive(Debug)]
struct RunBudget {
    deadline: tokio::time::Instant,
    timeout: std::time::Duration,
    max_consumed_body_bytes: u64,
    max_observed_requests: u64,
    state: Mutex<RunBudgetState>,
}

#[derive(Clone, Copy, Debug)]
struct RunBudgetSnapshot {
    consumed_body_bytes: u64,
    observed_request_count: u64,
    exhausted_reason: Option<&'static str>,
    max_consumed_body_bytes: u64,
    max_observed_requests: u64,
    timeout_secs: u64,
}

#[derive(Debug)]
struct BenchmarkSettings {
    raw_url: String,
    sample: String,
    entry_index: u32,
    repetitions: usize,
    order_offset: usize,
}

#[derive(Clone, Debug)]
struct SizeDiscovery {
    size: u64,
    source: &'static str,
    sample_bytes: u64,
    elapsed_ms: u128,
    transfer_events: TransferEventCollector,
}

struct BenchmarkCase {
    plan: DownloadPlan,
    selected: MediaStream,
    size: SizeDiscovery,
    fixed_host: String,
    candidate_hosts: Vec<String>,
}

#[derive(Clone, Copy)]
struct RunMetadata<'a> {
    sample: &'a str,
    entry_index: u32,
    repetition: usize,
    order: usize,
    order_offset: usize,
    started_at_epoch_ms: u128,
    group: Group,
    case: &'a BenchmarkCase,
}

struct RunObservation {
    elapsed_ms: u128,
    result: bbdown_core::Result<DownloadReport>,
    shards: BTreeMap<String, u64>,
    transfer_events: TransferEventCollector,
    budget: RunBudgetSnapshot,
    cleanup_timed_out: bool,
    error_class: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "This is the flat setup-failure JSON schema; nesting its status flags would change the output contract."
)]
struct SetupFailureRecord {
    record_type: &'static str,
    sample: String,
    entry_index: u32,
    started_at_epoch_ms: u128,
    elapsed_ms: u128,
    success: bool,
    valid_three_way_sample: bool,
    validity_reasons: Vec<&'static str>,
    failure_class: &'static str,
    cleanup_timed_out: bool,
    budget_exhausted: Option<&'static str>,
    budget_body_bytes_limit: u64,
    budget_observed_body_bytes: u64,
    budget_body_overshoot_bytes: u64,
    budget_observed_request_limit: u64,
    budget_observed_request_count_lower_bound: u64,
    total_run_timeout_secs: u64,
    cleanup_grace_secs: u64,
    api_response_body_bytes_observed: bool,
    wire_bytes_observed: bool,
    wire_completeness_observed: bool,
}

#[path = "support/cdn_benchmark_scenarios.rs"]
mod scenarios;

#[tokio::test]
#[ignore = "opt-in live network benchmark; run explicitly with --ignored"]
async fn cdn_benchmark_harness() -> anyhow::Result<()> {
    let settings = BenchmarkSettings::from_env()?;
    let started_at_epoch_ms = epoch_millis();
    let benchmark_started = Instant::now();
    let budget = RunBudget::live();
    let (temp_root, root) = output_root()?;
    let client =
        BiliClient::new(ClientConfig::default().with_request_timeout(DOWNLOAD_REQUEST_TIMEOUT));
    let case_result = resolve_case_with_budget(&client, &settings, &budget).await;
    let case = match case_result {
        Some(Ok(case)) if budget.exhaustion_reason().is_none() => case,
        _ => {
            return emit_setup_failure(
                &settings,
                started_at_epoch_ms,
                benchmark_started.elapsed().as_millis(),
                &budget,
            );
        }
    };
    let mut records = run_repetitions(&client, &case, &settings, &root, &budget).await?;
    let budget_snapshot = budget.snapshot();
    let final_elapsed_ms = benchmark_started.elapsed().as_millis();
    finalize_records(&mut records, &case, budget_snapshot, final_elapsed_ms);
    emit_records(&records)?;
    retain_output_if_requested(temp_root);
    anyhow::ensure!(
        records.iter().all(|record| record.valid_three_way_sample),
        "CDN benchmark sample is invalid; inspect JSON validity_reasons"
    );
    Ok(())
}

async fn resolve_case_with_budget(
    client: &BiliClient,
    settings: &BenchmarkSettings,
    budget: &RunBudget,
) -> Option<anyhow::Result<BenchmarkCase>> {
    run_until_deadline(resolve_case(client, settings, budget), budget).await
}

async fn run_until_deadline<F, T>(operation: F, budget: &RunBudget) -> Option<T>
where
    F: Future<Output = T>,
{
    tokio::pin!(operation);
    tokio::select! {
        biased;
        () = tokio::time::sleep_until(budget.deadline) => {
            budget.exhaust_deadline();
            None
        }
        result = &mut operation => Some(result),
    }
}

fn emit_setup_failure(
    settings: &BenchmarkSettings,
    started_at_epoch_ms: u128,
    elapsed_ms: u128,
    budget: &RunBudget,
) -> anyhow::Result<()> {
    let budget_snapshot = budget.snapshot();
    let mut validity_reasons = vec!["case_resolution_failed"];
    if budget_snapshot.exhausted_reason.is_some() {
        validity_reasons.push("benchmark_budget_exhausted");
    }
    let failure = SetupFailureRecord {
        record_type: "harness_setup_failure",
        sample: settings.sample.clone(),
        entry_index: settings.entry_index,
        started_at_epoch_ms,
        elapsed_ms,
        success: false,
        valid_three_way_sample: false,
        validity_reasons,
        failure_class: if budget_snapshot.exhausted_reason.is_some() {
            "benchmark_budget_exhausted"
        } else {
            "case_resolution_failed"
        },
        cleanup_timed_out: false,
        budget_exhausted: budget_snapshot.exhausted_reason,
        budget_body_bytes_limit: budget_snapshot.max_consumed_body_bytes,
        budget_observed_body_bytes: budget_snapshot.consumed_body_bytes,
        budget_body_overshoot_bytes: budget_snapshot
            .consumed_body_bytes
            .saturating_sub(budget_snapshot.max_consumed_body_bytes),
        budget_observed_request_limit: budget_snapshot.max_observed_requests,
        budget_observed_request_count_lower_bound: budget_snapshot.observed_request_count,
        total_run_timeout_secs: budget_snapshot.timeout_secs,
        cleanup_grace_secs: CLEANUP_GRACE.as_secs(),
        api_response_body_bytes_observed: false,
        wire_bytes_observed: false,
        wire_completeness_observed: false,
    };
    println!("{}", serde_json::to_string(&failure)?);
    std::io::stdout().flush()?;
    anyhow::bail!("CDN benchmark setup failed; inspect its safe JSON failure record")
}

async fn run_repetitions(
    client: &BiliClient,
    case: &BenchmarkCase,
    settings: &BenchmarkSettings,
    root: &Path,
    budget: &RunBudget,
) -> anyhow::Result<Vec<Record>> {
    let mut records = Vec::with_capacity(settings.repetitions.saturating_mul(GROUPS.len()));
    for repetition in 0..settings.repetitions {
        records.extend(run_repetition(client, case, settings, root, repetition, budget).await?);
    }
    Ok(records)
}

fn finalize_records(
    records: &mut [Record],
    case: &BenchmarkCase,
    budget: RunBudgetSnapshot,
    final_elapsed_ms: u128,
) {
    let default_port = Url::parse(&case.selected.base_url)
        .ok()
        .and_then(|source| scheme_default_port(&source));
    apply_run_budget_status(
        records,
        &case.fixed_host,
        default_port,
        budget.exhausted_reason,
    );
    for record in records.iter_mut() {
        record.benchmark_elapsed_ms = final_elapsed_ms;
        record.budget_observed_body_bytes = budget.consumed_body_bytes;
        record.budget_body_overshoot_bytes = budget
            .consumed_body_bytes
            .saturating_sub(budget.max_consumed_body_bytes);
        record.budget_observed_request_count_lower_bound = budget.observed_request_count;
    }
}

fn apply_run_budget_status(
    records: &mut [Record],
    fixed_host: &str,
    default_port: Option<u16>,
    exhausted_reason: Option<&'static str>,
) {
    if let Some(reason) = exhausted_reason {
        for record in records.iter_mut() {
            record.budget_exhausted = Some(reason);
        }
        for repetition_records in records.chunks_mut(GROUPS.len()) {
            mark_valid_samples_with_default(repetition_records, fixed_host, default_port);
        }
    }
}

fn emit_records(records: &[Record]) -> anyhow::Result<()> {
    for record in records {
        println!("{}", serde_json::to_string(record)?);
    }
    std::io::stdout().flush()?;
    Ok(())
}

fn retain_output_if_requested(temp_root: tempfile::TempDir) {
    if env::var_os("BBDOWN_CDN_BENCHMARK_KEEP_OUTPUT").is_some() {
        let retained_path = temp_root.keep();
        eprintln!(
            "cdn_benchmark_output_retained path={}",
            retained_path.display()
        );
    }
}

impl BenchmarkSettings {
    fn for_test() -> Self {
        Self {
            raw_url: "http://127.0.0.1/fixture".to_owned(),
            sample: "local-scenario".to_owned(),
            entry_index: 1,
            repetitions: 1,
            order_offset: 0,
        }
    }

    fn from_env() -> anyhow::Result<Self> {
        let sample = env::var("BBDOWN_CDN_BENCHMARK_SAMPLE")
            .unwrap_or_else(|_| "normal-playlist-video".to_owned());
        anyhow::ensure!(
            !sample.is_empty()
                && sample.len() <= 64
                && sample
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)),
            "sample label must be at most 64 ASCII letters, digits, dots, underscores, or hyphens"
        );
        let repetitions = parse_env("BBDOWN_CDN_BENCHMARK_REPETITIONS", 1_usize)?;
        anyhow::ensure!(
            (1..=MAX_REPETITIONS).contains(&repetitions),
            "repetitions must be between 1 and {MAX_REPETITIONS}"
        );
        let order_offset = parse_env("BBDOWN_CDN_BENCHMARK_ORDER_OFFSET", 0_usize)?;
        anyhow::ensure!(
            order_offset < GROUPS.len(),
            "order offset must be between 0 and 2"
        );
        Ok(Self {
            raw_url: env::var("BBDOWN_CDN_BENCHMARK_URL")
                .unwrap_or_else(|_| DEFAULT_URL.to_owned()),
            sample,
            entry_index: parse_env("BBDOWN_CDN_BENCHMARK_ENTRY", 1_u32)?,
            repetitions,
            order_offset,
        })
    }
}

impl RunBudget {
    fn live() -> Self {
        Self::with_limits(
            TOTAL_RUN_TIMEOUT,
            MAX_TOTAL_CONSUMED_BODY_BYTES,
            MAX_OBSERVED_REQUESTS,
        )
    }

    fn for_test(
        deadline: std::time::Duration,
        max_consumed_body_bytes: u64,
        max_observed_requests: u64,
    ) -> Self {
        Self::with_limits(deadline, max_consumed_body_bytes, max_observed_requests)
    }

    fn with_limits(
        timeout: std::time::Duration,
        max_consumed_body_bytes: u64,
        max_observed_requests: u64,
    ) -> Self {
        Self {
            deadline: tokio::time::Instant::now() + timeout,
            timeout,
            max_consumed_body_bytes,
            max_observed_requests,
            state: Mutex::new(RunBudgetState::default()),
        }
    }

    fn observe_body_bytes(&self, bytes: u64) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.consumed_body_bytes = state.consumed_body_bytes.saturating_add(bytes);
        if state.exhausted_reason.is_none()
            && state.consumed_body_bytes > self.max_consumed_body_bytes
        {
            state.exhausted_reason = Some("cumulative_body_bytes");
        }
        state.exhausted_reason.is_some()
    }

    fn observe_request_id(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.observed_request_count = state.observed_request_count.saturating_add(1);
        if state.exhausted_reason.is_none()
            && state.observed_request_count > self.max_observed_requests
        {
            state.exhausted_reason = Some("observed_request_threshold");
        }
        state.exhausted_reason.is_some()
    }

    fn exhaustion_reason(&self) -> Option<&'static str> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.exhausted_reason.is_none() && tokio::time::Instant::now() >= self.deadline {
            state.exhausted_reason = Some("total_run_deadline");
        }
        state.exhausted_reason
    }

    fn exhaust_deadline(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.exhausted_reason.is_none() {
            state.exhausted_reason = Some("total_run_deadline");
        }
    }

    fn snapshot(&self) -> RunBudgetSnapshot {
        let exhausted_reason = self.exhaustion_reason();
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        RunBudgetSnapshot {
            consumed_body_bytes: state.consumed_body_bytes,
            observed_request_count: state.observed_request_count,
            exhausted_reason,
            max_consumed_body_bytes: self.max_consumed_body_bytes,
            max_observed_requests: self.max_observed_requests,
            timeout_secs: self.timeout.as_secs(),
        }
    }
}

impl TransferEventCollector {
    fn observe(
        &mut self,
        event: &DownloadProgressEvent,
        budget: &RunBudget,
        cancellation: &DownloadCancellationToken,
    ) {
        match event {
            DownloadProgressEvent::TransferBytesReceived {
                request_id,
                phase,
                host,
                bytes_delta,
                ..
            } => {
                self.observe_request_id(*request_id, budget, cancellation);
                self.add_bytes(host.clone(), *phase, *bytes_delta);
                let observed_at_elapsed_ms = self.elapsed_ms();
                self.note_activity(host.clone(), *phase, observed_at_elapsed_ms);
                if budget.observe_body_bytes(*bytes_delta) {
                    cancellation.cancel_with_reason(BUDGET_CANCEL_REASON);
                }
            }
            DownloadProgressEvent::TransferDiagnostic {
                request_id,
                phase,
                host,
                diagnostic,
                ..
            } => {
                let observed_at_elapsed_ms = self.elapsed_ms();
                if let Some(request_id) = request_id {
                    self.observe_request_id(*request_id, budget, cancellation);
                }
                if let Some(phase) = phase {
                    self.note_activity(host.clone(), *phase, observed_at_elapsed_ms);
                }
                if let DownloadTransferDiagnostic::RequestFinished { outcome, .. } = diagnostic {
                    match outcome {
                        DownloadTransferRequestOutcome::Succeeded => {
                            self.request_outcomes.completed =
                                self.request_outcomes.completed.saturating_add(1);
                        }
                        DownloadTransferRequestOutcome::Failed { .. } => {
                            self.request_outcomes.failed =
                                self.request_outcomes.failed.saturating_add(1);
                        }
                        DownloadTransferRequestOutcome::Cancelled => {
                            self.request_outcomes.cancelled =
                                self.request_outcomes.cancelled.saturating_add(1);
                        }
                        _ => {}
                    }
                }
                self.diagnostics.push(TransferDiagnosticObservation {
                    request_id: *request_id,
                    phase: *phase,
                    host: host.clone(),
                    observed_at_elapsed_ms,
                    diagnostic: *diagnostic,
                });
            }
            _ => {}
        }
    }

    fn observe_request_id(
        &mut self,
        request_id: u64,
        budget: &RunBudget,
        cancellation: &DownloadCancellationToken,
    ) {
        if self.request_ids.insert(request_id) && budget.observe_request_id() {
            cancellation.cancel_with_reason(BUDGET_CANCEL_REASON);
        }
    }

    fn add_bytes(&mut self, host: Option<String>, phase: DownloadTransferPhase, bytes: u64) {
        if let Some(tally) = self
            .bytes_by_host_phase
            .iter_mut()
            .find(|tally| tally.host == host && tally.phase == phase)
        {
            tally.bytes = tally.bytes.saturating_add(bytes);
        } else {
            self.bytes_by_host_phase
                .push(HostPhaseBytes { host, phase, bytes });
        }
    }

    fn note_activity(
        &mut self,
        host: Option<String>,
        phase: DownloadTransferPhase,
        elapsed_ms: u128,
    ) {
        if let Some(activity) = self
            .activity_by_host_phase
            .iter_mut()
            .find(|activity| activity.host == host && activity.phase == phase)
        {
            activity.first_event_elapsed_ms = activity.first_event_elapsed_ms.min(elapsed_ms);
            activity.last_event_elapsed_ms = activity.last_event_elapsed_ms.max(elapsed_ms);
            activity.observed_activity_elapsed_ms = activity
                .last_event_elapsed_ms
                .saturating_sub(activity.first_event_elapsed_ms);
        } else {
            self.activity_by_host_phase.push(HostPhaseActivity {
                host,
                phase,
                first_event_elapsed_ms: elapsed_ms,
                last_event_elapsed_ms: elapsed_ms,
                observed_activity_elapsed_ms: 0,
            });
        }
    }

    fn elapsed_ms(&self) -> u128 {
        self.started_at.elapsed().as_millis()
    }

    fn sort(&mut self) {
        self.bytes_by_host_phase.sort_by(|left, right| {
            left.host
                .cmp(&right.host)
                .then_with(|| phase_name(left.phase).cmp(phase_name(right.phase)))
        });
        self.activity_by_host_phase.sort_by(|left, right| {
            left.host
                .cmp(&right.host)
                .then_with(|| phase_name(left.phase).cmp(phase_name(right.phase)))
        });
    }

    fn total_bytes(&self) -> u64 {
        self.bytes_by_host_phase
            .iter()
            .fold(0_u64, |sum, tally| sum.saturating_add(tally.bytes))
    }

    fn preflight_bytes(&self) -> u64 {
        self.bytes_by_host_phase
            .iter()
            .filter(|tally| {
                matches!(
                    tally.phase,
                    DownloadTransferPhase::AutomaticProbe | DownloadTransferPhase::ShardProbe
                )
            })
            .fold(0_u64, |sum, tally| sum.saturating_add(tally.bytes))
    }

    fn media_body_bytes(&self) -> u64 {
        self.bytes_by_host_phase
            .iter()
            .filter(|tally| {
                matches!(
                    tally.phase,
                    DownloadTransferPhase::RangeChunk | DownloadTransferPhase::WholeFile
                )
            })
            .fold(0_u64, |sum, tally| sum.saturating_add(tally.bytes))
    }
}

fn phase_name(phase: DownloadTransferPhase) -> &'static str {
    match phase {
        DownloadTransferPhase::AutomaticProbe => "automatic_probe",
        DownloadTransferPhase::ShardProbe => "shard_probe",
        DownloadTransferPhase::RangeChunk => "range_chunk",
        DownloadTransferPhase::WholeFile => "whole_file",
        DownloadTransferPhase::StandaloneProbe => "standalone_probe",
        _ => "unknown",
    }
}

async fn resolve_case(
    client: &BiliClient,
    settings: &BenchmarkSettings,
    budget: &RunBudget,
) -> anyhow::Result<BenchmarkCase> {
    let mut plan = client
        .plan_download_with_mode(&settings.raw_url, None, DownloadMode::VideoOnly)
        .await?;
    let entry_pos = plan
        .entries
        .iter()
        .position(|entry| entry.index == settings.entry_index)
        .ok_or_else(|| anyhow::anyhow!("entry index is absent from the resolved plan"))?;
    let mut entry = plan.entries.remove(entry_pos);
    let mut selected = entry
        .streams
        .videos
        .first()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("selected entry has no video representation"))?;
    let size = discover_selected_size(client, &selected, budget).await?;
    anyhow::ensure!(
        size.size <= MAX_MEDIA_BYTES,
        "selected representation exceeds the sample limit"
    );
    selected.size = Some(size.size);
    let donor = Url::parse(&selected.base_url)?;
    let fixed_host_input =
        env::var("BBDOWN_CDN_BENCHMARK_FIXED_HOST").unwrap_or_else(|_| authority_label(&donor));
    let fixed_host = canonical_authority_for_source(&fixed_host_input, &donor)
        .ok_or_else(|| anyhow::anyhow!("fixed host must be a valid hostname and optional port"))?;
    let candidate_hosts = read_hosts(&selected, &fixed_host)?;
    entry.streams.videos = vec![selected.clone()];
    entry.streams.audios.clear();
    entry.streams.flv_segments.clear();
    entry.subtitles.clear();
    plan.entries = vec![entry];
    Ok(BenchmarkCase {
        plan,
        selected,
        size,
        fixed_host,
        candidate_hosts,
    })
}

async fn run_repetition(
    client: &BiliClient,
    case: &BenchmarkCase,
    settings: &BenchmarkSettings,
    root: &Path,
    repetition: usize,
    budget: &RunBudget,
) -> anyhow::Result<Vec<Record>> {
    let mut records = Vec::with_capacity(GROUPS.len());
    for order in 0..GROUPS.len() {
        let group = GROUPS[(order + settings.order_offset + repetition) % GROUPS.len()];
        records.push(
            run_group(
                client, case, settings, root, repetition, order, group, budget,
            )
            .await?,
        );
    }
    let default_port = Url::parse(&case.selected.base_url)
        .ok()
        .and_then(|source| scheme_default_port(&source));
    mark_valid_samples_with_default(&mut records, &case.fixed_host, default_port);
    Ok(records)
}

#[allow(
    clippy::too_many_arguments,
    reason = "The deterministic scenario module calls this private seam with independent case, settings, output, ordering, and budget inputs."
)]
async fn run_group(
    client: &BiliClient,
    case: &BenchmarkCase,
    settings: &BenchmarkSettings,
    root: &Path,
    repetition: usize,
    order: usize,
    group: Group,
    budget: &RunBudget,
) -> anyhow::Result<Record> {
    let output_dir = root.join(format!("run-{repetition:02}-{order:02}"));
    std::fs::create_dir(&output_dir)?;
    let plan = plan_for_group(&case.plan, group, &case.fixed_host, &case.candidate_hosts)?;
    let shard_bytes: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
    let transfer_events = Mutex::new(TransferEventCollector::default());
    let cancellation = DownloadCancellationToken::new();
    let progress = |event: &DownloadProgressEvent| {
        if let DownloadProgressEvent::CdnShardCompleted { host, bytes, .. } = event {
            let mut totals = shard_bytes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let total = totals.entry(host.clone()).or_default();
            *total = total.saturating_add(*bytes);
        }
        transfer_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observe(event, budget, &cancellation);
    };
    let options = download_options(&output_dir, case, group);
    let started_at_epoch_ms = epoch_millis();
    eprintln!(
        "cdn_benchmark_start sample={} group={} repetition={} at_epoch_ms={}",
        settings.sample,
        group_name(group),
        repetition + 1,
        started_at_epoch_ms
    );
    let began = Instant::now();
    let (result, cleanup_timed_out) = if budget.exhaustion_reason().is_some() {
        cancellation.cancel_with_reason(BUDGET_CANCEL_REASON);
        (Err(cancellation.cancelled_error()), false)
    } else {
        download_with_budget(client, &plan, options, &progress, &cancellation, budget).await
    };
    let elapsed_ms = began.elapsed().as_millis();
    let budget_snapshot = budget.snapshot();
    let error_class = group_error_class(
        budget_snapshot.exhausted_reason,
        cleanup_timed_out,
        result.as_ref().err().map(classify_error),
    );
    eprintln!(
        "cdn_benchmark_end sample={} group={} repetition={} at_epoch_ms={} status={} error_class={}",
        settings.sample,
        group_name(group),
        repetition + 1,
        epoch_millis(),
        if result.is_ok() { "success" } else { "failed" },
        error_class.unwrap_or("none")
    );
    let shards = shard_bytes
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut transfer_events = transfer_events
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    transfer_events.sort();
    Ok(make_record(
        RunMetadata {
            sample: &settings.sample,
            entry_index: settings.entry_index,
            repetition: repetition + 1,
            order: order + 1,
            order_offset: settings.order_offset,
            started_at_epoch_ms,
            group,
            case,
        },
        RunObservation {
            elapsed_ms,
            result,
            shards,
            transfer_events,
            budget: budget_snapshot,
            cleanup_timed_out,
            error_class,
        },
    ))
}

async fn download_with_budget<P>(
    client: &BiliClient,
    plan: &DownloadPlan,
    options: DownloadOptions,
    progress: &P,
    cancellation: &DownloadCancellationToken,
    budget: &RunBudget,
) -> (bbdown_core::Result<DownloadReport>, bool)
where
    P: bbdown_core::DownloadProgressSink + ?Sized,
{
    let download =
        client.download_plan_with_progress_and_cancellation(plan, options, progress, cancellation);
    tokio::pin!(download);
    tokio::select! {
        biased;
        result = &mut download => (result, false),
        () = tokio::time::sleep_until(budget.deadline) => {
            budget.exhaust_deadline();
            cancellation.cancel_with_reason(BUDGET_CANCEL_REASON);
            match tokio::time::timeout(CLEANUP_GRACE, &mut download).await {
                Ok(result) => (result, false),
                Err(_) => (
                    Err(Error::InvalidInput(DOWNLOAD_TIMEOUT_ERROR.to_owned())),
                    true,
                ),
            }
        }
        () = cancellation.cancelled() => {
            match tokio::time::timeout(CLEANUP_GRACE, &mut download).await {
                Ok(result) => (result, false),
                Err(_) => (
                    Err(Error::InvalidInput(DOWNLOAD_TIMEOUT_ERROR.to_owned())),
                    true,
                ),
            }
        }
    }
}

fn download_options(output_dir: &Path, case: &BenchmarkCase, group: Group) -> DownloadOptions {
    DownloadOptions::new(output_dir)
        .with_retry_policy(RetryPolicy::single_attempt())
        .with_download_mode(DownloadMode::VideoOnly)
        .with_stream_selection(StreamSelection::video(case.selected.id))
        .with_resume(false)
        .with_subtitles(false)
        .with_danmaku(false)
        .with_cover(false)
        .with_mux(MuxOptions::Disabled)
        .with_cdn_parallelism(configured_cdn_parallelism(group))
}

fn configured_cdn_parallelism(group: Group) -> usize {
    if matches!(group, Group::FixedHostBaseline) {
        1
    } else {
        4
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "This assembles the flat serialized benchmark record so every JSON field remains auditable beside the schema."
)]
fn make_record(metadata: RunMetadata<'_>, observation: RunObservation) -> Record {
    let RunMetadata {
        sample,
        entry_index,
        repetition,
        order,
        order_offset,
        started_at_epoch_ms,
        group,
        case,
    } = metadata;
    let RunObservation {
        elapsed_ms,
        result,
        shards,
        transfer_events,
        budget,
        cleanup_timed_out,
        error_class,
    } = observation;
    let result = result.ok();
    let media = result.and_then(|report| {
        report
            .entries
            .into_iter()
            .flat_map(|entry| entry.files)
            .find(|file| file.kind == bbdown_core::DownloadFileKind::Video)
    });
    let (size, sha256) = media
        .and_then(|file| {
            let (size, digest) = hash_file(&file.path).ok()?;
            Some((Some(size), Some(digest)))
        })
        .unwrap_or((None, None));
    let budget_exhausted = budget.exhausted_reason;
    let success = size == Some(case.size.size)
        && sha256.is_some()
        && budget_exhausted.is_none()
        && !cleanup_timed_out;
    let shard_total = shards.values().copied().sum::<u64>();
    let whole_file_fallback = if !success {
        None
    } else if matches!(group, Group::FixedHostBaseline) {
        Some(false)
    } else {
        Some(shard_total == 0)
    };
    let observed_request_count_lower_bound =
        u64::try_from(transfer_events.request_ids.len()).unwrap_or(u64::MAX);
    let transfer_total_consumed_body_bytes = transfer_events.total_bytes();
    let preflight_consumed_body_bytes = transfer_events.preflight_bytes();
    let media_body_consumed_bytes = transfer_events.media_body_bytes();
    Record {
        sample: sample.to_owned(),
        entry_index,
        repetition,
        order,
        order_offset,
        started_at_epoch_ms,
        group,
        fixed_host: case.fixed_host.clone(),
        candidate_hosts: case.candidate_hosts.clone(),
        request_timeout_secs: DOWNLOAD_REQUEST_TIMEOUT.as_secs(),
        quality: case.selected.id,
        codecs: case.selected.codecs.clone(),
        declared_size: case.size.size,
        size_source: case.size.source,
        size_probe_phase: (case.size.source == "probe").then_some("standalone_probe"),
        size_probe_sample_bytes: case.size.sample_bytes,
        size_probe_elapsed_ms: case.size.elapsed_ms,
        size_probe_observation_scope: "one shared case setup; repeated metadata only; excluded from each group transfer total",
        size_probe_transfer_bytes_by_host_phase: case
            .size
            .transfer_events
            .bytes_by_host_phase
            .clone(),
        size_probe_transfer_activity_by_host_phase: case
            .size
            .transfer_events
            .activity_by_host_phase
            .clone(),
        size_probe_transfer_consumed_body_bytes: case.size.transfer_events.total_bytes(),
        size_probe_observed_request_ids: case
            .size
            .transfer_events
            .request_ids
            .iter()
            .copied()
            .collect(),
        size_probe_observed_request_count_lower_bound: u64::try_from(
            case.size.transfer_events.request_ids.len(),
        )
        .unwrap_or(u64::MAX),
        size_probe_request_outcomes: case.size.transfer_events.request_outcomes.clone(),
        size_probe_transfer_diagnostics: case.size.transfer_events.diagnostics.clone(),
        size_probe_included_in_group_totals: false,
        configured_cdn_parallelism: configured_cdn_parallelism(group),
        success,
        elapsed_ms,
        benchmark_elapsed_ms: elapsed_ms,
        size,
        sha256,
        shard_bytes_by_host: shards,
        whole_file_fallback,
        transfer_bytes_by_host_phase: transfer_events.bytes_by_host_phase,
        transfer_activity_by_host_phase: transfer_events.activity_by_host_phase,
        observed_request_ids: transfer_events.request_ids.into_iter().collect(),
        observed_request_count_lower_bound,
        request_outcomes: transfer_events.request_outcomes,
        transfer_diagnostics: transfer_events.diagnostics,
        transfer_total_consumed_body_bytes,
        preflight_consumed_body_bytes,
        media_body_consumed_bytes,
        budget_exhausted,
        budget_body_bytes_limit: budget.max_consumed_body_bytes,
        budget_observed_body_bytes: budget.consumed_body_bytes,
        budget_body_overshoot_bytes: budget
            .consumed_body_bytes
            .saturating_sub(budget.max_consumed_body_bytes),
        budget_observed_request_limit: budget.max_observed_requests,
        budget_observed_request_count_lower_bound: budget.observed_request_count,
        cleanup_timed_out,
        total_run_timeout_secs: budget.timeout_secs,
        cleanup_grace_secs: CLEANUP_GRACE.as_secs(),
        request_id_scope: "IDs are local to each progress-reporter call; shared standalone_probe IDs are recorded separately and are not globally unique",
        request_count_semantics: "unique media-transfer request IDs seen in body or terminal/retry diagnostics; lower bound because no request-start event is emitted; API/auth requests are excluded",
        transfer_activity_semantics: "first/last times are relative to the group or standalone probe collector; includes body or phase-tagged diagnostic events only, omits connection and response-header wait, and is neither full request latency nor additive concurrency time",
        body_bytes_scope: "decoded media response bodies only",
        body_measurement: "response body chunks after transparent gzip decoding; excludes API/auth responses; not wire bytes",
        body_budget_semantics: "cancel after the first observed crossing chunk/request; the crossing chunk and in-flight decoded chunks may overshoot; not a wire cap",
        api_response_body_bytes_observed: false,
        wire_bytes_observed: false,
        wire_completeness_observed: false,
        valid_three_way_sample: false,
        validity_reasons: Vec::new(),
        error_class,
    }
}

async fn discover_selected_size(
    client: &BiliClient,
    selected: &MediaStream,
    budget: &RunBudget,
) -> anyhow::Result<SizeDiscovery> {
    if let Some(size) = selected.size.filter(|size| *size > 0) {
        return Ok(SizeDiscovery {
            size,
            source: "plan",
            sample_bytes: 0,
            elapsed_ms: 0,
            transfer_events: TransferEventCollector::default(),
        });
    }

    let mut donor_only = selected.clone();
    donor_only.backup_urls.clear();
    let transfer_events = Mutex::new(TransferEventCollector::default());
    let cancellation = DownloadCancellationToken::new();
    let progress = |event: &DownloadProgressEvent| {
        transfer_events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .observe(event, budget, &cancellation);
    };
    let began = Instant::now();
    let results = tokio::time::timeout(
        SIZE_PROBE_TIMEOUT,
        probe_media_cdns_with_progress(
            client,
            &donor_only,
            &MediaHostOptions::default(),
            &progress,
        ),
    )
    .await
    .map_err(|_| anyhow::anyhow!("bounded donor size probe timed out"))?
    .map_err(|_| anyhow::anyhow!("bounded donor size probe failed"))?;
    let result = results
        .into_iter()
        .find(|result| result.ok && result.total_size.is_some())
        .ok_or_else(|| anyhow::anyhow!("bounded donor size probe returned no size"))?;
    let size = result
        .total_size
        .filter(|size| *size > 0)
        .ok_or_else(|| anyhow::anyhow!("bounded donor size probe returned an invalid size"))?;
    anyhow::ensure!(
        result.bytes > 0 && result.bytes <= MAX_SIZE_PROBE_SAMPLE_BYTES,
        "bounded donor size probe exceeded its sample limit"
    );
    let mut transfer_events = transfer_events
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    transfer_events.sort();
    Ok(SizeDiscovery {
        size,
        source: "probe",
        sample_bytes: result.bytes,
        elapsed_ms: began.elapsed().as_millis(),
        transfer_events,
    })
}

fn mark_valid_samples(records: &mut [Record], fixed_host: &str) {
    mark_valid_samples_with_default(records, fixed_host, None);
}

fn mark_valid_samples_with_default(
    records: &mut [Record],
    fixed_host: &str,
    default_port: Option<u16>,
) {
    let fingerprints = records
        .iter()
        .filter_map(|record| Some((record.size?, record.sha256.as_ref()?)))
        .collect::<Vec<_>>();
    let baseline = records
        .iter()
        .find(|record| matches!(record.group, Group::FixedHostBaseline));
    let same_host = records
        .iter()
        .find(|record| matches!(record.group, Group::FixedHostRange4));
    let multi_host = records
        .iter()
        .find(|record| matches!(record.group, Group::MultiHostRange4));
    let mut reasons = Vec::new();
    if records.len() != 3 || baseline.is_none() || same_host.is_none() || multi_host.is_none() {
        reasons.push("missing_group");
    }
    if records.iter().any(|record| !record.success) {
        reasons.push("download_failed");
    }
    if records
        .iter()
        .any(|record| record.budget_exhausted.is_some())
    {
        reasons.push("benchmark_budget_exhausted");
    }
    if records.iter().any(|record| record.cleanup_timed_out) {
        reasons.push("bounded_cleanup_timed_out");
    }
    if fingerprints.len() != 3
        || fingerprints
            .iter()
            .any(|fingerprint| *fingerprint != fingerprints[0])
    {
        reasons.push("size_or_sha256_mismatch");
    }
    if let Some(record) = baseline
        && !record.shard_bytes_by_host.is_empty()
    {
        reasons.push("baseline_reported_shards");
    }
    if let Some(record) = same_host {
        let hosts = record
            .shard_bytes_by_host
            .iter()
            .filter(|(_, bytes)| **bytes > 0)
            .map(|(host, _)| host)
            .collect::<Vec<_>>();
        if record.whole_file_fallback != Some(false)
            || record.shard_bytes_by_host.values().sum::<u64>() != record.declared_size
        {
            reasons.push("same_host_shard_incomplete_or_fallback");
        }
        if hosts.len() != 1 || !host_matches_with_default(hosts[0], fixed_host, default_port) {
            reasons.push("same_host_shard_host_mismatch");
        }
    }
    if let Some(record) = multi_host {
        let hosts = record
            .shard_bytes_by_host
            .iter()
            .filter(|(_, bytes)| **bytes > 0)
            .count();
        if record.whole_file_fallback != Some(false)
            || record.shard_bytes_by_host.values().sum::<u64>() != record.declared_size
        {
            reasons.push("multi_host_shard_incomplete_or_fallback");
        }
        if hosts < 2 {
            reasons.push("fewer_than_two_successful_shard_hosts");
        }
    }
    reasons.sort_unstable();
    reasons.dedup();
    let valid = reasons.is_empty();
    for record in records {
        record.valid_three_way_sample = valid;
        record.validity_reasons.clone_from(&reasons);
    }
}

#[test]
fn validity_requires_complete_range_shards_and_multiple_multi_hosts() {
    let mut records = validity_fixture();
    mark_valid_samples(&mut records, "mirror.example");
    assert!(records.iter().all(|record| record.valid_three_way_sample));

    let mut fallback = validity_fixture();
    fallback[1].whole_file_fallback = Some(true);
    mark_valid_samples(&mut fallback, "mirror.example");
    assert!(!fallback[1].valid_three_way_sample);
    assert!(
        fallback[1]
            .validity_reasons
            .contains(&"same_host_shard_incomplete_or_fallback")
    );

    let mut one_multi_host = validity_fixture();
    one_multi_host[2].shard_bytes_by_host = BTreeMap::from([("mirror.example".to_owned(), 100)]);
    mark_valid_samples(&mut one_multi_host, "mirror.example");
    assert!(!one_multi_host[2].valid_three_way_sample);
    assert!(
        one_multi_host[2]
            .validity_reasons
            .contains(&"fewer_than_two_successful_shard_hosts")
    );

    let mut exhausted = validity_fixture();
    exhausted[0].budget_exhausted = Some("cumulative_body_bytes");
    mark_valid_samples(&mut exhausted, "mirror.example");
    assert!(
        exhausted[0]
            .validity_reasons
            .contains(&"benchmark_budget_exhausted")
    );
}

#[test]
fn run_budget_invalidates_the_experiment_without_erasing_group_outcomes() {
    let mut records = validity_fixture();
    records[1].success = false;
    records[1].error_class = Some("http_error");

    apply_run_budget_status(
        &mut records,
        "mirror.example",
        None,
        Some("cumulative_body_bytes"),
    );

    assert!(records[0].success);
    assert_eq!(records[0].error_class, None);
    assert!(!records[1].success);
    assert_eq!(records[1].error_class, Some("http_error"));
    assert!(records.iter().all(|record| {
        record.budget_exhausted == Some("cumulative_body_bytes")
            && !record.valid_three_way_sample
            && record
                .validity_reasons
                .contains(&"benchmark_budget_exhausted")
    }));
}

#[test]
fn cleanup_timeout_error_class_takes_priority_over_budget_exhaustion() {
    assert_eq!(
        group_error_class(Some("cumulative_body_bytes"), true, Some("http_error")),
        Some("bounded_cleanup_timeout")
    );
    assert_eq!(
        group_error_class(Some("cumulative_body_bytes"), false, Some("http_error")),
        Some("benchmark_budget_exhausted")
    );
    assert_eq!(
        group_error_class(None, false, Some("http_error")),
        Some("http_error")
    );
}

#[tokio::test]
async fn setup_deadline_drops_resolution_before_later_request_stages() {
    let later_stage_requests = std::cell::Cell::new(0_u8);
    let setup = async {
        std::future::pending::<()>().await;
        later_stage_requests.set(later_stage_requests.get().saturating_add(1));
    };
    let budget = RunBudget::for_test(std::time::Duration::from_millis(10), u64::MAX, u64::MAX);

    let result = run_until_deadline(setup, &budget).await;

    assert_eq!(result, None);
    assert_eq!(later_stage_requests.get(), 0);
    assert_eq!(budget.exhaustion_reason(), Some("total_run_deadline"));
}

fn validity_fixture() -> Vec<Record> {
    let mut baseline = validity_record(Group::FixedHostBaseline);
    baseline.whole_file_fallback = Some(false);
    let mut same_host = validity_record(Group::FixedHostRange4);
    same_host.shard_bytes_by_host = BTreeMap::from([("mirror.example".to_owned(), 100)]);
    same_host.whole_file_fallback = Some(false);
    let mut multi_host = validity_record(Group::MultiHostRange4);
    multi_host.shard_bytes_by_host = BTreeMap::from([
        ("mirror.example".to_owned(), 50),
        ("edge.example".to_owned(), 50),
    ]);
    multi_host.whole_file_fallback = Some(false);
    vec![baseline, same_host, multi_host]
}

fn validity_record(group: Group) -> Record {
    Record {
        sample: "fixture".to_owned(),
        entry_index: 1,
        repetition: 1,
        order: 1,
        order_offset: 0,
        started_at_epoch_ms: 1,
        group,
        fixed_host: "mirror.example".to_owned(),
        candidate_hosts: vec!["mirror.example".to_owned(), "edge.example".to_owned()],
        request_timeout_secs: DOWNLOAD_REQUEST_TIMEOUT.as_secs(),
        quality: 80,
        codecs: Some("avc".to_owned()),
        declared_size: 100,
        size_source: "plan",
        size_probe_phase: None,
        size_probe_sample_bytes: 0,
        size_probe_elapsed_ms: 0,
        size_probe_observation_scope: "one shared case setup; repeated metadata only; excluded from each group transfer total",
        size_probe_transfer_bytes_by_host_phase: Vec::new(),
        size_probe_transfer_activity_by_host_phase: Vec::new(),
        size_probe_transfer_consumed_body_bytes: 0,
        size_probe_observed_request_ids: Vec::new(),
        size_probe_observed_request_count_lower_bound: 0,
        size_probe_request_outcomes: RequestOutcomeCounts::default(),
        size_probe_transfer_diagnostics: Vec::new(),
        size_probe_included_in_group_totals: false,
        configured_cdn_parallelism: configured_cdn_parallelism(group),
        success: true,
        elapsed_ms: 100,
        benchmark_elapsed_ms: 100,
        size: Some(100),
        sha256: Some("same-digest".to_owned()),
        shard_bytes_by_host: BTreeMap::new(),
        whole_file_fallback: None,
        transfer_bytes_by_host_phase: Vec::new(),
        transfer_activity_by_host_phase: Vec::new(),
        observed_request_ids: Vec::new(),
        observed_request_count_lower_bound: 0,
        request_outcomes: RequestOutcomeCounts::default(),
        transfer_diagnostics: Vec::new(),
        transfer_total_consumed_body_bytes: 0,
        preflight_consumed_body_bytes: 0,
        media_body_consumed_bytes: 0,
        budget_exhausted: None,
        budget_body_bytes_limit: MAX_TOTAL_CONSUMED_BODY_BYTES,
        budget_observed_body_bytes: 0,
        budget_body_overshoot_bytes: 0,
        budget_observed_request_limit: MAX_OBSERVED_REQUESTS,
        budget_observed_request_count_lower_bound: 0,
        cleanup_timed_out: false,
        total_run_timeout_secs: TOTAL_RUN_TIMEOUT.as_secs(),
        cleanup_grace_secs: CLEANUP_GRACE.as_secs(),
        request_id_scope: "IDs are local to each progress-reporter call; shared standalone_probe IDs are recorded separately and are not globally unique",
        request_count_semantics: "unique media-transfer request IDs seen in body or terminal/retry diagnostics; lower bound because no request-start event is emitted; API/auth requests are excluded",
        transfer_activity_semantics: "first/last times are relative to the group or standalone probe collector; includes body or phase-tagged diagnostic events only, omits connection and response-header wait, and is neither full request latency nor additive concurrency time",
        body_bytes_scope: "decoded media response bodies only",
        body_measurement: "response body chunks after transparent gzip decoding; excludes API/auth responses; not wire bytes",
        body_budget_semantics: "cancel after the first observed crossing chunk/request; the crossing chunk and in-flight decoded chunks may overshoot; not a wire cap",
        api_response_body_bytes_observed: false,
        wire_bytes_observed: false,
        wire_completeness_observed: false,
        valid_three_way_sample: false,
        validity_reasons: Vec::new(),
        error_class: None,
    }
}

fn group_name(group: Group) -> &'static str {
    match group {
        Group::FixedHostBaseline => "fixed_host_baseline",
        Group::FixedHostRange4 => "fixed_host_range4",
        Group::MultiHostRange4 => "multi_host_range4",
    }
}

fn host_matches(candidate: &str, fixed_host: &str) -> bool {
    host_matches_with_default(candidate, fixed_host, None)
}

fn host_matches_with_default(candidate: &str, fixed_host: &str, default_port: Option<u16>) -> bool {
    match (parse_authority(candidate), parse_authority(fixed_host)) {
        (Some(candidate), Some(fixed)) => {
            let hosts_match = candidate
                .host_str()
                .zip(fixed.host_str())
                .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right));
            // A missing event port stands only for the media URL's scheme default.
            let ports_match = match (candidate.port(), fixed.port()) {
                (left, right) if left == right => true,
                (None, Some(port)) | (Some(port), None) => Some(port) == default_port,
                _ => false,
            };
            hosts_match && ports_match
        }
        _ => false,
    }
}

fn scheme_default_port(source: &Url) -> Option<u16> {
    // Event host labels omit only the default port for the source scheme.
    let mut without_explicit_port = source.clone();
    without_explicit_port.set_port(None).ok()?;
    without_explicit_port.port_or_known_default()
}

fn parse_authority(authority: &str) -> Option<Url> {
    if authority.is_empty()
        || authority.ends_with(':')
        || authority.bytes().any(|byte| b"/@?#\\".contains(&byte))
    {
        return None;
    }
    let url = Url::parse(&format!("bbdown-cdn://{authority}/")).ok()?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none_or(str::is_empty)
        || url.port() == Some(0)
    {
        return None;
    }
    Some(url)
}

fn authority_label(url: &Url) -> String {
    authority_label_with_port(url, url.port())
}

fn authority_label_with_port(url: &Url, port: Option<u16>) -> String {
    let host_label = url.host().map_or_else(String::new, |host| host.to_string());
    match port {
        Some(port) => format!("{host_label}:{port}"),
        None => host_label,
    }
}

fn canonical_authority_for_source(authority: &str, source: &Url) -> Option<String> {
    let authority = parse_authority(authority)?;
    // Preserve an explicit override port even when the source scheme treats it as default.
    let effective_port = authority.port().or(source.port_or_known_default());
    Some(authority_label_with_port(&authority, effective_port))
}

#[test]
fn candidate_authorities_validate_host_and_port_and_preserve_the_media_target() -> anyhow::Result<()>
{
    let source = Url::parse("https://cdn.example:8443/video/part.m4s?token=fixture")?;
    let replaced = Url::parse(&replace_host(&source, "127.0.0.1:43127")?)?;

    assert_eq!(replaced.scheme(), "https");
    assert_eq!(replaced.host_str(), Some("127.0.0.1"));
    assert_eq!(replaced.port(), Some(43127));
    assert_eq!(replaced.path(), "/video/part.m4s");
    assert_eq!(replaced.query(), Some("token=fixture"));
    let explicit_default_port = Url::parse(&replace_host(&source, "mirror.example:80")?)?;
    assert_eq!(explicit_default_port.port(), Some(80));
    let explicit_https_default = Url::parse(&replace_host(&source, "mirror.example:443")?)?;
    assert_eq!(explicit_https_default.port(), None);
    assert_eq!(
        explicit_https_default.as_str(),
        "https://mirror.example/video/part.m4s?token=fixture"
    );
    let omitted_port = Url::parse(&replace_host(&source, "mirror.example")?)?;
    assert_eq!(omitted_port.port(), source.port());
    assert_eq!(
        canonical_authority_for_source("mirror.example:443", &source).as_deref(),
        Some("mirror.example:443")
    );
    assert_eq!(
        canonical_authority_for_source("mirror.example", &source).as_deref(),
        Some("mirror.example:8443")
    );
    assert_eq!(
        canonical_authority_for_source("mirror.example:80", &source).as_deref(),
        Some("mirror.example:80")
    );
    let http_source = Url::parse("http://cdn.example:8080/video/part.m4s?token=fixture")?;
    assert_eq!(
        canonical_authority_for_source("mirror.example:80", &http_source).as_deref(),
        Some("mirror.example:80")
    );
    assert_eq!(
        replace_host(&http_source, "mirror.example:80")?,
        "http://mirror.example/video/part.m4s?token=fixture"
    );
    let ipv6_override = canonical_authority_for_source("[::1]:443", &source)
        .ok_or_else(|| anyhow::anyhow!("IPv6 override was rejected"))?;
    assert_eq!(ipv6_override, "[::1]:443");
    assert_eq!(
        replace_host(&source, &ipv6_override)?,
        "https://[::1]/video/part.m4s?token=fixture"
    );
    assert!(host_matches("127.0.0.1:43127", "127.0.0.1:43127"));
    assert!(!host_matches("127.0.0.1:43127", "127.0.0.1:43128"));
    let secure_default_port = scheme_default_port(&source);
    let cleartext_default_port = scheme_default_port(&http_source);
    assert_eq!(secure_default_port, Some(443));
    assert_eq!(cleartext_default_port, Some(80));
    assert!(host_matches_with_default(
        "mirror.example:443",
        "mirror.example",
        secure_default_port
    ));
    assert!(!host_matches_with_default(
        "mirror.example:80",
        "mirror.example",
        secure_default_port
    ));
    assert!(host_matches_with_default(
        "mirror.example:80",
        "mirror.example",
        cleartext_default_port
    ));
    assert!(!host_matches_with_default(
        "mirror.example:443",
        "mirror.example",
        cleartext_default_port
    ));
    assert!(!host_matches("mirror.example:8443", "mirror.example:8444"));

    for invalid in [
        "user@cdn.example",
        "cdn.example/path",
        "cdn.example?token=fixture",
        "cdn.example#fragment",
        "cdn.example:",
        "cdn.example:0",
        "cdn.example:65536",
    ] {
        assert!(
            replace_host(&source, invalid).is_err(),
            "accepted {invalid}"
        );
    }
    Ok(())
}

fn hash_file(path: &std::path::Path) -> std::io::Result<(u64, String)> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    let mut size = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        size = size.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
    }
    Ok((size, format!("{:x}", hasher.finalize())))
}

fn epoch_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn classify_error(error: &Error) -> &'static str {
    match error {
        Error::InvalidInput(message) if message == DOWNLOAD_TIMEOUT_ERROR => "download_timeout",
        Error::InvalidInput(_) => "invalid_input",
        Error::SelectionRequired { .. } => "selection_required",
        Error::Unsupported(_) => "unsupported",
        Error::Api { .. } => "api_error",
        Error::AccessRestricted(_) => "access_restricted",
        Error::MissingField(_) => "missing_field",
        Error::Url(_) => "url_error",
        Error::Http(_) => "http_error",
        Error::Json(_) => "parse_error",
        Error::Io(_) => "io_error",
        Error::MuxFailed { .. } => "mux_error",
        Error::Cancelled { .. } => "cancelled",
    }
}

fn group_error_class(
    budget_exhausted: Option<&'static str>,
    cleanup_timed_out: bool,
    result_error_class: Option<&'static str>,
) -> Option<&'static str> {
    if cleanup_timed_out {
        Some("bounded_cleanup_timeout")
    } else if budget_exhausted.is_some() {
        Some("benchmark_budget_exhausted")
    } else {
        result_error_class
    }
}

#[test]
fn classifies_core_download_timeout_without_exposing_error_text() {
    assert_eq!(
        classify_error(&Error::InvalidInput(DOWNLOAD_TIMEOUT_ERROR.to_owned())),
        "download_timeout"
    );
    assert_eq!(
        classify_error(&Error::InvalidInput("other static detail".to_owned())),
        "invalid_input"
    );
}

fn plan_for_group(
    plan: &DownloadPlan,
    group: Group,
    fixed_host: &str,
    multi_hosts: &[String],
) -> anyhow::Result<DownloadPlan> {
    let mut plan = plan.clone();
    let stream = &mut plan.entries[0].streams.videos[0];
    let source = Url::parse(&stream.base_url)?;
    let hosts = match group {
        Group::FixedHostBaseline | Group::FixedHostRange4 => vec![fixed_host.to_owned()],
        Group::MultiHostRange4 => multi_hosts.to_vec(),
    };
    let mut urls = hosts
        .iter()
        .map(|host| replace_host(&source, host))
        .collect::<anyhow::Result<Vec<_>>>()?;
    stream.base_url = urls.remove(0);
    stream.backup_urls = urls;
    Ok(plan)
}

fn replace_host(source: &Url, host: &str) -> anyhow::Result<String> {
    let authority = parse_authority(host).ok_or_else(|| {
        anyhow::anyhow!("candidate host must be a valid hostname and optional port")
    })?;
    let host = authority
        .host()
        .map(|host| host.to_string())
        .ok_or_else(|| anyhow::anyhow!("candidate host is missing"))?;
    let mut url = source.clone();
    url.set_host(Some(&host))
        .map_err(|_| anyhow::anyhow!("candidate host could not be applied"))?;
    url.set_port(authority.port().or(source.port()))
        .map_err(|()| anyhow::anyhow!("candidate port could not be applied"))?;
    Ok(url.into())
}

fn read_hosts(selected: &MediaStream, fixed_host: &str) -> anyhow::Result<Vec<String>> {
    let requested = env::var("BBDOWN_CDN_BENCHMARK_HOSTS").ok();
    let hosts = requested.map_or_else(
        || {
            std::iter::once(fixed_host.to_owned())
                .chain(
                    selected
                        .backup_urls
                        .iter()
                        .filter_map(|url| Url::parse(url).ok().map(|url| authority_label(&url))),
                )
                .collect()
        },
        |value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|host| !host.is_empty())
                .map(str::to_owned)
                .collect::<Vec<_>>()
        },
    );
    let source = Url::parse(&selected.base_url)?;
    let mut normalized_hosts = Vec::with_capacity(hosts.len());
    let mut seen_hosts = HashSet::new();
    for host in hosts {
        let host = canonical_authority_for_source(&host, &source).ok_or_else(|| {
            anyhow::anyhow!("candidate host must be a valid hostname and optional port")
        })?;
        if seen_hosts.insert(host.clone()) {
            normalized_hosts.push(host);
        }
    }
    let hosts = normalized_hosts;
    anyhow::ensure!(
        (2..=MAX_MULTI_HOSTS).contains(&hosts.len()),
        "multi-host group requires between 2 and {MAX_MULTI_HOSTS} distinct hosts; set BBDOWN_CDN_BENCHMARK_HOSTS"
    );
    anyhow::ensure!(
        hosts.iter().any(|host| host_matches(host, fixed_host)),
        "multi-host list must include the fixed host"
    );
    // Keep all candidate URLs on the selected representation's exact path and query.
    let mut urls = hosts
        .iter()
        .map(|host| replace_host(&Url::parse(&selected.base_url)?, host))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let first = Url::parse(&urls.remove(0))?;
    anyhow::ensure!(
        urls.iter()
            .all(|url| Url::parse(url)
                .is_ok_and(|candidate| candidate.path() == first.path()
                    && candidate.query() == first.query())),
        "multi-host candidates do not share a media path and query"
    );
    Ok(hosts)
}

fn output_root() -> anyhow::Result<(tempfile::TempDir, PathBuf)> {
    if let Some(path) = env::var_os("BBDOWN_CDN_BENCHMARK_OUTPUT_DIR") {
        let path = PathBuf::from(path);
        std::fs::create_dir_all(&path)?;
        let temp = tempfile::Builder::new()
            .prefix("bbdown-cdn-benchmark-")
            .tempdir_in(path)?;
        let root = temp.path().to_path_buf();
        return Ok((temp, root));
    }
    let temp = tempfile::tempdir()?;
    let path = temp.path().to_path_buf();
    Ok((temp, path))
}

fn parse_env<T: std::str::FromStr>(name: &str, default: T) -> anyhow::Result<T>
where
    T::Err: std::fmt::Display,
{
    env::var(name).map_or(Ok(default), |value| {
        value
            .parse()
            .map_err(|error| anyhow::anyhow!("invalid {name}: {error}"))
    })
}

#[tokio::test]
async fn discovers_unknown_size_with_a_bounded_single_donor_sample() -> anyhow::Result<()> {
    let server = MockServer::start();
    let discovery = server.mock(|when, then| {
        when.method(GET).path("/media").header("range", "bytes=0-0");
        then.status(206)
            .header("Content-Range", "bytes 0-0/100000")
            .header("Content-Length", "1")
            .body("x");
    });
    let sample = server.mock(|when, then| {
        when.method(GET)
            .path("/media")
            .header("range", "bytes=1-65535");
        then.status(206)
            .header("Content-Range", "bytes 1-65535/100000")
            .header("Content-Length", "65535")
            .body(vec![b'x'; 65_535]);
    });
    let stream: MediaStream = serde_json::from_value(serde_json::json!({
        "id": 80,
        "base_url": server.url("/media?fixture=1"),
        "backup_urls": [],
        "language": null,
        "language_doc": null,
        "codecs": "avc1.640028",
        "codec_family": "h264",
        "bandwidth": 1_000_000,
        "width": 1920,
        "height": 1080,
        "frame_rate": "30",
        "mime_type": "video/mp4",
        "size": null
    }))?;

    let budget = RunBudget::for_test(std::time::Duration::from_secs(10), u64::MAX, u64::MAX);
    let discovered =
        discover_selected_size(&BiliClient::new(ClientConfig::default()), &stream, &budget).await?;

    assert_eq!(discovered.size, 100_000);
    assert_eq!(discovered.source, "probe");
    assert_eq!(discovered.sample_bytes, MAX_SIZE_PROBE_SAMPLE_BYTES);
    assert_eq!(
        discovered.transfer_events.total_bytes(),
        MAX_SIZE_PROBE_SAMPLE_BYTES
    );
    assert_eq!(discovery.calls(), 1);
    assert_eq!(sample.calls(), 1);
    Ok(())
}
