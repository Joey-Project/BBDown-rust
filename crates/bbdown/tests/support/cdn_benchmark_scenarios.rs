//! Deterministic local-server scenarios for the opt-in CDN benchmark harness.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use bbdown_core::{
    CdnCandidateExclusionReason, ClientConfig, DownloadTransferDiagnostic,
    DownloadTransferFailureReason, DownloadTransferFallbackReason, DownloadTransferPhase,
    DownloadTransferRequestOutcome, MediaStream,
};
use sha2::{Digest, Sha256};

use super::*;

const SMALL_MEDIA_BYTES: usize = 64 * 1024;
const LARGE_MEDIA_BYTES: usize = 4 * 1024 * 1024 + 733;
const RETRY_MEDIA_BYTES: usize = 2 * 1024 * 1024 + 19;
const WHOLE_FILE_PROBE_BYTES: usize = 64 * 1024;
const SHARD_PROBE_BYTES: usize = 16 * 1024;
const RANGE_CHUNK_BYTES: u64 = 1024 * 1024;
const SERVER_READ_TIMEOUT: Duration = Duration::from_secs(3);
const SERVER_WRITE_TIMEOUT: Duration = Duration::from_secs(3);
const OVERLAP_GATE_TIMEOUT: Duration = Duration::from_secs(2);

#[tokio::test]
async fn fixed_baseline_and_range_scenarios_validate_small_and_large_media() -> anyhow::Result<()> {
    let small_body = fixture_body(SMALL_MEDIA_BYTES);
    let small_cluster = FixtureCluster::start(4, &small_body, Behavior::Success, true)?;
    let small_case = benchmark_case(&small_cluster, &small_body)?;
    let (mut small_records, _) = run_scenario_groups(
        &small_body,
        &small_cluster,
        &small_case,
        Duration::from_secs(10),
    )
    .await?;
    assert_small_media_results(&mut small_records, &small_body, &small_case.fixed_host)?;

    let large_body = fixture_body(LARGE_MEDIA_BYTES);
    let large_cluster = FixtureCluster::start_with_delays(
        4,
        &large_body,
        Behavior::Success,
        true,
        &[
            Duration::ZERO,
            Duration::from_millis(3),
            Duration::from_millis(1),
            Duration::from_millis(5),
        ],
    )?;
    let large_case = benchmark_case(&large_cluster, &large_body)?;
    let (mut large_records, large_overlaps) = run_scenario_groups(
        &large_body,
        &large_cluster,
        &large_case,
        Duration::from_secs(15),
    )
    .await?;
    assert_large_media_results(
        &mut large_records,
        &large_overlaps,
        &large_body,
        &large_case.fixed_host,
    );
    Ok(())
}

async fn run_scenario_groups(
    body: &[u8],
    cluster: &FixtureCluster,
    case: &BenchmarkCase,
    deadline: Duration,
) -> anyhow::Result<(Vec<Record>, Vec<OverlapSnapshot>)> {
    let settings = BenchmarkSettings::for_test();
    let root = tempfile::tempdir()?;
    let mut records = Vec::with_capacity(GROUPS.len());
    let mut overlaps = Vec::with_capacity(GROUPS.len());
    for (order, group) in GROUPS.into_iter().enumerate() {
        cluster.reset_observations();
        let record = run_fixture_group(
            case,
            &settings,
            root.path(),
            order,
            group,
            &test_budget(deadline, u64::MAX, u64::MAX),
        )
        .await?;
        assert_valid_file_on_servers(&record, body, cluster);
        assert_server_bytes_match_record(&record, cluster);
        records.push(record);
        overlaps.push(cluster.overlap_snapshot());
    }
    Ok((records, overlaps))
}

fn assert_small_media_results(
    records: &mut [Record],
    body: &[u8],
    fixed_host: &str,
) -> anyhow::Result<()> {
    for record in records.iter() {
        match record.group {
            Group::FixedHostBaseline => {
                assert_eq!(record.configured_cdn_parallelism, 1);
                assert_eq!(record.shard_bytes_by_host.len(), 0);
            }
            Group::FixedHostRange4 | Group::MultiHostRange4 => {
                assert_eq!(record.configured_cdn_parallelism, 4);
                assert_eq!(
                    record.shard_bytes_by_host.values().sum::<u64>(),
                    body.len() as u64
                );
                assert_eq!(
                    finished_request_count(record, DownloadTransferPhase::RangeChunk),
                    1,
                    "one small media object cannot use four range lanes"
                );
            }
        }
    }
    mark_valid_samples(records, fixed_host);
    let small_multi = records
        .iter()
        .find(|record| matches!(record.group, Group::MultiHostRange4))
        .ok_or_else(|| anyhow::anyhow!("small sample omitted the multi-host group"))?;
    assert!(!small_multi.valid_three_way_sample);
    assert!(
        small_multi
            .validity_reasons
            .contains(&"fewer_than_two_successful_shard_hosts")
    );
    assert_eq!(small_multi.shard_bytes_by_host.len(), 1);
    Ok(())
}

fn assert_large_media_results(
    records: &mut [Record],
    overlaps: &[OverlapSnapshot],
    body: &[u8],
    fixed_host: &str,
) {
    for (record, overlap) in records.iter().zip(overlaps) {
        match record.group {
            Group::FixedHostBaseline => {
                assert_eq!(record.configured_cdn_parallelism, 1);
                assert_eq!(record.shard_bytes_by_host.len(), 0);
                assert_eq!(overlap.max_active_range_chunks, 0);
            }
            Group::FixedHostRange4 => {
                assert_eq!(record.configured_cdn_parallelism, 4);
                assert_eq!(record.shard_bytes_by_host.len(), 1);
                assert_eq!(
                    record.shard_bytes_by_host.values().sum::<u64>(),
                    body.len() as u64
                );
                assert!(overlap.max_active_range_chunks >= 2);
                assert_eq!(overlap.hosts_at_overlap.len(), 1);
            }
            Group::MultiHostRange4 => {
                assert_eq!(record.configured_cdn_parallelism, 4);
                assert_eq!(
                    record.shard_bytes_by_host.values().sum::<u64>(),
                    body.len() as u64
                );
                assert!(record.shard_bytes_by_host.len() >= 2);
                assert!(overlap.max_active_range_chunks >= 2);
                assert!(overlap.hosts_at_overlap.len() >= 2);
            }
        }
    }
    mark_valid_samples(records, fixed_host);
    assert!(records.iter().all(|record| record.valid_three_way_sample));
}

#[tokio::test]
async fn excludes_mismatched_candidate_from_shards() -> anyhow::Result<()> {
    let body = fixture_body(RETRY_MEDIA_BYTES);
    let mut mismatched_body = body.clone();
    mismatched_body[0] ^= 0xff;
    let mismatch_cluster = FixtureCluster::start_with_bodies(
        vec![body.clone(), mismatched_body],
        vec![Behavior::Success, Behavior::Success],
        true,
        vec![Duration::ZERO, Duration::from_millis(2)],
    )?;
    let mismatch_case = benchmark_case(&mismatch_cluster, &body)?;
    let settings = BenchmarkSettings::for_test();
    let root = tempfile::tempdir()?;

    mismatch_cluster.reset_observations();
    let excluded = run_fixture_group(
        &mismatch_case,
        &settings,
        root.path(),
        0,
        Group::MultiHostRange4,
        &test_budget(Duration::from_secs(12), u64::MAX, u64::MAX),
    )
    .await?;
    assert_valid_file_on_servers(&excluded, &body, &mismatch_cluster);
    assert!(excluded.transfer_diagnostics.iter().any(|event| {
        event.phase == Some(DownloadTransferPhase::ShardProbe)
            && matches!(
                &event.diagnostic,
                DownloadTransferDiagnostic::CandidateExcluded {
                    reason: CdnCandidateExclusionReason::SampleMismatch
                }
            )
    }));
    assert_server_bytes_match_record(&excluded, &mismatch_cluster);
    assert_eq!(
        excluded.shard_bytes_by_host.values().sum::<u64>(),
        body.len() as u64
    );
    Ok(())
}

#[tokio::test]
async fn retries_partial_chunk_on_another_candidate_and_counts_partial_bytes() -> anyhow::Result<()>
{
    let body = fixture_body(RETRY_MEDIA_BYTES);
    let settings = BenchmarkSettings::for_test();
    let root = tempfile::tempdir()?;
    let partial_once = Arc::new(AtomicBool::new(false));
    let retry_cluster = FixtureCluster::start_with_faults(
        2,
        &body,
        vec![
            Behavior::PartialOnce {
                start: 0,
                end: RANGE_CHUNK_BYTES - 1,
                prefix_bytes: 7,
                fired: Arc::clone(&partial_once),
            },
            Behavior::PartialOnce {
                start: 0,
                end: RANGE_CHUNK_BYTES - 1,
                prefix_bytes: 7,
                fired: partial_once,
            },
        ],
        true,
        vec![Duration::ZERO, Duration::from_millis(2)],
    )?;
    let retry_case = benchmark_case(&retry_cluster, &body)?;
    retry_cluster.reset_observations();
    let retried = run_fixture_group(
        &retry_case,
        &settings,
        root.path(),
        1,
        Group::MultiHostRange4,
        &test_budget(Duration::from_secs(12), u64::MAX, u64::MAX),
    )
    .await?;
    assert_valid_file_on_servers(&retried, &body, &retry_cluster);
    assert_server_bytes_match_record(&retried, &retry_cluster);
    let failed_host = retried.transfer_diagnostics.iter().find_map(|event| {
        (event.phase == Some(DownloadTransferPhase::RangeChunk)
            && matches!(
                &event.diagnostic,
                DownloadTransferDiagnostic::RequestFinished {
                    outcome: DownloadTransferRequestOutcome::Failed {
                        reason: DownloadTransferFailureReason::BodyReadFailed
                    },
                    bytes_received: 7
                }
            ))
        .then(|| event.host.clone())
        .flatten()
    });
    assert!(
        failed_host.is_some(),
        "the incomplete chunk's seven consumed bytes are retained"
    );
    assert!(retried.transfer_diagnostics.iter().any(|event| {
        event.host.is_some()
            && event.host != failed_host
            && event.phase == Some(DownloadTransferPhase::RangeChunk)
            && matches!(
                &event.diagnostic,
                DownloadTransferDiagnostic::RetryScheduled {
                    reason: DownloadTransferFailureReason::BodyReadFailed
                }
            )
    }));
    assert!(
        bytes_for(
            &retried,
            failed_host.as_deref(),
            DownloadTransferPhase::RangeChunk
        ) >= 7
    );
    Ok(())
}

#[tokio::test]
async fn exhausted_ranges_fall_back_with_partial_bytes_but_no_published_shards()
-> anyhow::Result<()> {
    let body = fixture_body(RETRY_MEDIA_BYTES);
    let cluster = FixtureCluster::start_with_faults(
        2,
        &body,
        vec![
            Behavior::PartialEveryRangeChunk { prefix_bytes: 7 },
            Behavior::PartialEveryRangeChunk { prefix_bytes: 7 },
        ],
        false,
        vec![Duration::ZERO, Duration::ZERO],
    )?;
    let case = benchmark_case(&cluster, &body)?;
    let settings = BenchmarkSettings::for_test();
    let root = tempfile::tempdir()?;
    let budget = test_budget(Duration::from_secs(12), u64::MAX, u64::MAX);
    cluster.reset_observations();

    let record = run_fixture_group(
        &case,
        &settings,
        root.path(),
        0,
        Group::MultiHostRange4,
        &budget,
    )
    .await?;

    assert_valid_file_on_servers(&record, &body, &cluster);
    assert_eq!(record.whole_file_fallback, Some(true));
    assert_eq!(record.shard_bytes_by_host.len(), 0);
    assert!(record.media_body_consumed_bytes > body.len() as u64);
    assert!(record.request_outcomes.failed > 0);
    assert!(record.transfer_diagnostics.iter().any(|event| {
        event.phase == Some(DownloadTransferPhase::RangeChunk)
            && matches!(
                &event.diagnostic,
                DownloadTransferDiagnostic::WholeFileFallback {
                    reason: DownloadTransferFallbackReason::RangeTransferFailed
                }
            )
    }));
    assert!(record.transfer_diagnostics.iter().any(|event| {
        event.phase == Some(DownloadTransferPhase::RangeChunk)
            && matches!(
                &event.diagnostic,
                DownloadTransferDiagnostic::RequestFinished {
                    outcome: DownloadTransferRequestOutcome::Failed {
                        reason: DownloadTransferFailureReason::BodyReadFailed
                    },
                    bytes_received: 7
                }
            )
    }));
    assert_server_phases_match_record(
        &record,
        &cluster,
        &[
            DownloadTransferPhase::AutomaticProbe,
            DownloadTransferPhase::ShardProbe,
            DownloadTransferPhase::WholeFile,
        ],
    );
    let whole_file_bytes = record
        .transfer_bytes_by_host_phase
        .iter()
        .filter(|event| event.phase == DownloadTransferPhase::WholeFile)
        .map(|event| event.bytes)
        .sum::<u64>();
    assert_eq!(whole_file_bytes, body.len() as u64);
    let range_bytes = record
        .transfer_bytes_by_host_phase
        .iter()
        .filter(|event| event.phase == DownloadTransferPhase::RangeChunk)
        .map(|event| event.bytes)
        .sum::<u64>();
    assert!(
        range_bytes >= 7,
        "failed range body bytes remain in consumed totals"
    );
    Ok(())
}

#[tokio::test]
async fn every_run_budget_gate_cancels_without_a_successful_sample() -> anyhow::Result<()> {
    let scenarios = [
        (
            "total_run_deadline",
            Behavior::Success,
            Duration::from_millis(40),
            u64::MAX,
            u64::MAX,
        ),
        (
            "cumulative_body_bytes",
            Behavior::Success,
            Duration::from_secs(5),
            1,
            u64::MAX,
        ),
        (
            "observed_request_threshold",
            Behavior::Success,
            Duration::from_secs(5),
            u64::MAX,
            1,
        ),
    ];

    for (index, (reason, behavior, deadline, max_body_bytes, max_requests)) in
        scenarios.into_iter().enumerate()
    {
        let body = fixture_body(LARGE_MEDIA_BYTES);
        let cluster = FixtureCluster::start_with_delays(
            4,
            &body,
            behavior,
            false,
            &[
                Duration::from_millis(250),
                Duration::from_millis(250),
                Duration::from_millis(250),
                Duration::from_millis(250),
            ],
        )?;
        let case = benchmark_case(&cluster, &body)?;
        let settings = BenchmarkSettings::for_test();
        let root = tempfile::tempdir()?;
        let budget = test_budget(deadline, max_body_bytes, max_requests);
        cluster.reset_observations();

        let record = run_fixture_group(
            &case,
            &settings,
            root.path(),
            index,
            Group::MultiHostRange4,
            &budget,
        )
        .await?;

        assert!(
            !record.success,
            "budget exhaustion must not produce a sample"
        );
        assert!(!record.valid_three_way_sample);
        assert_eq!(record.budget_exhausted, Some(reason));
        assert_eq!(record.error_class, Some("benchmark_budget_exhausted"));
        assert_eq!(record.budget_body_bytes_limit, max_body_bytes);
        assert_eq!(record.budget_observed_request_limit, max_requests);
    }
    Ok(())
}

#[tokio::test]
async fn cumulative_budget_counts_restarted_request_ids_across_groups() -> anyhow::Result<()> {
    let body = fixture_body(SMALL_MEDIA_BYTES);
    let cluster = FixtureCluster::start(4, &body, Behavior::Success, false)?;
    let case = benchmark_case(&cluster, &body)?;
    let settings = BenchmarkSettings::for_test();
    let root = tempfile::tempdir()?;
    let budget = test_budget(Duration::from_secs(10), body.len() as u64 + 1, u64::MAX);

    cluster.reset_observations();
    let baseline = run_fixture_group(
        &case,
        &settings,
        root.path(),
        0,
        Group::FixedHostBaseline,
        &budget,
    )
    .await?;
    assert_valid_file(&baseline, &body);
    assert_eq!(baseline.budget_exhausted, None);
    assert_eq!(
        baseline.transfer_total_consumed_body_bytes,
        body.len() as u64
    );
    assert_eq!(
        baseline.budget_observed_body_bytes,
        baseline.transfer_total_consumed_body_bytes
    );
    assert_server_bytes_match_record(&baseline, &cluster);

    cluster.reset_observations();
    let range = run_fixture_group(
        &case,
        &settings,
        root.path(),
        1,
        Group::FixedHostRange4,
        &budget,
    )
    .await?;
    assert!(!range.success);
    assert!(!range.valid_three_way_sample);
    assert_eq!(range.budget_exhausted, Some("cumulative_body_bytes"));
    assert!(range.budget_observed_body_bytes > range.budget_body_bytes_limit);
    assert!(range.budget_body_overshoot_bytes > 0);
    assert!(
        baseline
            .observed_request_ids
            .iter()
            .any(|request_id| range.observed_request_ids.contains(request_id)),
        "request IDs restart within each group's independent observation scope"
    );
    assert_eq!(range.observed_request_count_lower_bound, 1);
    assert_eq!(
        range.budget_observed_request_count_lower_bound,
        baseline.budget_observed_request_count_lower_bound
            + range.observed_request_count_lower_bound,
        "the same local request ID is charged again in the shared run budget"
    );
    assert_server_bytes_match_record(&range, &cluster);
    let requests_after_exhaustion = cluster
        .servers
        .iter()
        .map(|server| server.snapshot().request_count)
        .sum::<u64>();

    let later = run_fixture_group(
        &case,
        &settings,
        root.path(),
        2,
        Group::MultiHostRange4,
        &budget,
    )
    .await?;
    assert!(!later.success);
    assert_eq!(later.budget_exhausted, Some("cumulative_body_bytes"));
    assert_eq!(later.observed_request_ids, Vec::<u64>::new());
    assert_eq!(later.transfer_total_consumed_body_bytes, 0);
    assert_eq!(
        cluster
            .servers
            .iter()
            .map(|server| server.snapshot().request_count)
            .sum::<u64>(),
        requests_after_exhaustion,
        "a later group must not issue more media requests after budget exhaustion"
    );
    Ok(())
}

fn test_budget(deadline: Duration, max_body_bytes: u64, max_requests: u64) -> RunBudget {
    RunBudget::for_test(deadline, max_body_bytes, max_requests)
}

async fn run_fixture_group(
    case: &BenchmarkCase,
    settings: &BenchmarkSettings,
    root: &Path,
    order: usize,
    group: Group,
    budget: &RunBudget,
) -> anyhow::Result<Record> {
    let client =
        BiliClient::new(ClientConfig::default().with_request_timeout(Duration::from_secs(4)));
    run_group(&client, case, settings, root, 0, order, group, budget).await
}

fn benchmark_case(cluster: &FixtureCluster, body: &[u8]) -> anyhow::Result<BenchmarkCase> {
    let urls = cluster
        .servers
        .iter()
        .map(LocalServer::url)
        .collect::<Vec<_>>();
    let hosts = cluster
        .servers
        .iter()
        .map(LocalServer::host_label)
        .collect::<Vec<_>>();
    let selected: MediaStream = serde_json::from_value(serde_json::json!({
        "id": 80,
        "base_url": urls[0],
        "backup_urls": urls.iter().skip(1).collect::<Vec<_>>(),
        "language": null,
        "language_doc": null,
        "codecs": "avc1.640028",
        "codec_family": "h264",
        "bandwidth": 1_000_000,
        "width": 1280,
        "height": 720,
        "frame_rate": "30",
        "mime_type": "video/mp4",
        "size": body.len()
    }))?;
    let plan: DownloadPlan = serde_json::from_value(serde_json::json!({
        "title": "CDN benchmark fixture",
        "entries": [{
            "index": 1,
            "aid": 1,
            "bvid": null,
            "cid": 1,
            "epid": null,
            "title": "Synthetic media",
            "cover_url": null,
            "source": "normal_web",
            "streams": {
                "videos": [selected],
                "audios": [],
                "flv_segments": [],
                "accept_quality": [80],
                "qualities": [],
                "duration_seconds": 1
            },
            "diagnostics": {"attempts": []},
            "subtitles": [],
            "chapters": [],
            "danmaku": {"cid": 1, "xml_url": "http://127.0.0.1/danmaku.xml"}
        }]
    }))?;

    Ok(BenchmarkCase {
        plan,
        selected,
        size: SizeDiscovery {
            size: body.len() as u64,
            source: "fixture",
            sample_bytes: 0,
            elapsed_ms: 0,
            transfer_events: TransferEventCollector::default(),
        },
        fixed_host: hosts[0].clone(),
        candidate_hosts: hosts,
    })
}

fn assert_valid_file(record: &Record, body: &[u8]) {
    assert!(
        record.success,
        "download failed: error_class={:?}, transfer_bytes={:?}, diagnostics={:?}",
        record.error_class, record.transfer_bytes_by_host_phase, record.transfer_diagnostics
    );
    assert_valid_output(record, body);
}

fn assert_valid_file_on_servers(record: &Record, body: &[u8], cluster: &FixtureCluster) {
    assert!(
        record.success,
        "download failed: error_class={:?}, transfer_bytes={:?}, diagnostics={:?}, local_servers={}",
        record.error_class,
        record.transfer_bytes_by_host_phase,
        record.transfer_diagnostics,
        server_request_summary(cluster)
    );
    assert_valid_output(record, body);
}

fn assert_valid_output(record: &Record, body: &[u8]) {
    assert_eq!(record.size, Some(body.len() as u64));
    let expected_sha256 = sha256(body);
    assert_eq!(record.sha256.as_deref(), Some(expected_sha256.as_str()));
    assert!(record.transfer_total_consumed_body_bytes > 0);
}

fn server_request_summary(cluster: &FixtureCluster) -> String {
    cluster
        .servers
        .iter()
        .map(|server| {
            let snapshot = server.snapshot();
            let requests = snapshot
                .requests
                .iter()
                .map(|request| {
                    format!(
                        "phase={:?},range={:?},status={}",
                        request.phase, request.range, request.response_status
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            let write_errors = snapshot
                .write_errors
                .iter()
                .map(|error| {
                    format!(
                        "phase={:?},bytes_written={},kind={:?}",
                        error.phase, error.bytes_written, error.kind
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            format!(
                "host={} requests=[{}] served_phase_bytes={:?} write_errors=[{}]",
                server.host_label(),
                requests,
                snapshot.bytes_by_phase,
                write_errors
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn sha256(body: &[u8]) -> String {
    format!("{:x}", Sha256::digest(body))
}

fn finished_request_count(record: &Record, phase: DownloadTransferPhase) -> usize {
    record
        .transfer_diagnostics
        .iter()
        .filter(|event| {
            event.phase == Some(phase)
                && matches!(
                    &event.diagnostic,
                    DownloadTransferDiagnostic::RequestFinished { .. }
                )
        })
        .count()
}

fn bytes_for(record: &Record, host: Option<&str>, phase: DownloadTransferPhase) -> u64 {
    record
        .transfer_bytes_by_host_phase
        .iter()
        .filter(|event| event.host.as_deref() == host && event.phase == phase)
        .map(|event| event.bytes)
        .sum()
}

fn assert_server_bytes_match_record(record: &Record, cluster: &FixtureCluster) {
    let mut served_request_count = 0_u64;
    for server in &cluster.servers {
        let served = server.snapshot();
        served_request_count = served_request_count.saturating_add(served.request_count);
        let host = server.host_label();
        for phase in [
            DownloadTransferPhase::AutomaticProbe,
            DownloadTransferPhase::ShardProbe,
            DownloadTransferPhase::RangeChunk,
            DownloadTransferPhase::WholeFile,
        ] {
            assert_eq!(
                bytes_for(record, Some(&host), phase),
                served.bytes_for(phase),
                "download observer and local server disagree for host {host} phase {phase:?}; observer_host_tallies={:?}; local_requests={:?}; diagnostics={:?}",
                record
                    .transfer_bytes_by_host_phase
                    .iter()
                    .filter(|event| event.host.as_deref() == Some(&host))
                    .collect::<Vec<_>>(),
                served.requests,
                record.transfer_diagnostics
            );
        }
    }
    assert!(
        record.observed_request_count_lower_bound >= served_request_count,
        "request diagnostics should cover every complete local request"
    );
}

fn assert_server_phases_match_record(
    record: &Record,
    cluster: &FixtureCluster,
    phases: &[DownloadTransferPhase],
) {
    for server in &cluster.servers {
        let served = server.snapshot();
        let host = server.host_label();
        for phase in phases {
            assert_eq!(
                bytes_for(record, Some(&host), *phase),
                served.bytes_for(*phase),
                "download observer and local server disagree for host {host} phase {phase:?}"
            );
        }
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "The modulo 251 result is in 0..=250 and therefore fits in u8."
)]
fn fixture_body(length: usize) -> Vec<u8> {
    (0..length)
        .map(|index| ((index.wrapping_mul(37) + index / 251) % 251) as u8)
        .collect()
}

#[derive(Clone)]
enum Behavior {
    Success,
    PartialOnce {
        start: u64,
        end: u64,
        prefix_bytes: usize,
        fired: Arc<AtomicBool>,
    },
    PartialEveryRangeChunk {
        prefix_bytes: usize,
    },
}

#[derive(Clone)]
struct ServerSpec {
    body: Arc<Vec<u8>>,
    behavior: Behavior,
    response_delay: Duration,
}

#[derive(Default, Clone)]
struct ServerSnapshot {
    request_count: u64,
    automatic_probe_seen: bool,
    bytes_by_phase: Vec<(DownloadTransferPhase, u64)>,
    requests: Vec<ServedRequestObservation>,
    write_errors: Vec<ServerWriteError>,
}

#[derive(Clone, Debug)]
struct ServedRequestObservation {
    phase: DownloadTransferPhase,
    range: Option<(u64, u64)>,
    response_status: u16,
}

#[derive(Clone, Debug)]
struct ServerWriteError {
    phase: DownloadTransferPhase,
    bytes_written: u64,
    kind: std::io::ErrorKind,
}

impl ServerSnapshot {
    fn bytes_for(&self, phase: DownloadTransferPhase) -> u64 {
        self.bytes_by_phase
            .iter()
            .filter(|(candidate, _)| *candidate == phase)
            .map(|(_, bytes)| *bytes)
            .sum()
    }

    fn add_bytes(&mut self, phase: DownloadTransferPhase, bytes: u64) {
        if let Some((_, total)) = self
            .bytes_by_phase
            .iter_mut()
            .find(|(candidate, _)| *candidate == phase)
        {
            *total = total.saturating_add(bytes);
        } else {
            self.bytes_by_phase.push((phase, bytes));
        }
    }
}

struct LocalServer {
    address: SocketAddr,
    state: Arc<Mutex<ServerSnapshot>>,
    stop: Arc<AtomicBool>,
    listener_thread: Option<JoinHandle<()>>,
}

impl LocalServer {
    fn start(spec: ServerSpec, overlap: Arc<OverlapTracker>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let state = Arc::new(Mutex::new(ServerSnapshot::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_state = Arc::clone(&state);
        let thread_stop = Arc::clone(&stop);
        let listener_thread = thread::spawn(move || {
            let mut handlers = Vec::new();
            while !thread_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let spec = spec.clone();
                        let state = Arc::clone(&thread_state);
                        let overlap = Arc::clone(&overlap);
                        handlers.push(thread::spawn(move || {
                            let _ = handle_request(stream, &spec, &state, &overlap);
                        }));
                        join_finished_handlers(&mut handlers);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
            for handler in handlers {
                let _ = handler.join();
            }
        });
        Ok(Self {
            address,
            state,
            stop,
            listener_thread: Some(listener_thread),
        })
    }

    fn host_label(&self) -> String {
        format!("{}:{}", self.address.ip(), self.address.port())
    }

    fn url(&self) -> String {
        format!("http://{}/media?fixture=cdn-benchmark", self.address)
    }

    fn snapshot(&self) -> ServerSnapshot {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn reset_observations(&self) {
        *self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = ServerSnapshot::default();
    }
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(100));
        if let Some(listener_thread) = self.listener_thread.take() {
            let _ = listener_thread.join();
        }
    }
}

struct FixtureCluster {
    servers: Vec<LocalServer>,
    overlap: Arc<OverlapTracker>,
}

impl FixtureCluster {
    fn start(
        count: usize,
        body: &[u8],
        behavior: Behavior,
        overlap_barrier: bool,
    ) -> std::io::Result<Self> {
        Self::start_with_delays(
            count,
            body,
            behavior,
            overlap_barrier,
            &vec![Duration::ZERO; count],
        )
    }

    fn start_with_delays(
        count: usize,
        body: &[u8],
        behavior: Behavior,
        overlap_barrier: bool,
        delays: &[Duration],
    ) -> std::io::Result<Self> {
        Self::start_with_bodies(
            vec![body.to_vec(); count],
            vec![behavior; count],
            overlap_barrier,
            delays.to_vec(),
        )
    }

    fn start_with_faults(
        count: usize,
        body: &[u8],
        behaviors: Vec<Behavior>,
        overlap_barrier: bool,
        delays: Vec<Duration>,
    ) -> std::io::Result<Self> {
        Self::start_with_bodies(
            vec![body.to_vec(); count],
            behaviors,
            overlap_barrier,
            delays,
        )
    }

    fn start_with_bodies(
        bodies: Vec<Vec<u8>>,
        behaviors: Vec<Behavior>,
        overlap_barrier: bool,
        delays: Vec<Duration>,
    ) -> std::io::Result<Self> {
        if bodies.is_empty() || bodies.len() != behaviors.len() || bodies.len() != delays.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "fixture server configuration lengths do not match",
            ));
        }
        let overlap = Arc::new(OverlapTracker::new(overlap_barrier));
        let mut servers = Vec::with_capacity(bodies.len());
        for ((body, behavior), response_delay) in bodies.into_iter().zip(behaviors).zip(delays) {
            servers.push(LocalServer::start(
                ServerSpec {
                    body: Arc::new(body),
                    behavior,
                    response_delay,
                },
                Arc::clone(&overlap),
            )?);
        }
        Ok(Self { servers, overlap })
    }

    fn reset_observations(&self) {
        for server in &self.servers {
            server.reset_observations();
        }
        self.overlap.reset();
    }

    fn overlap_snapshot(&self) -> OverlapSnapshot {
        self.overlap.snapshot()
    }
}

#[derive(Default)]
struct OverlapState {
    active_range_chunks: u64,
    max_active_range_chunks: u64,
    active_hosts: BTreeMap<String, u64>,
    hosts_at_overlap: BTreeSet<String>,
    released: bool,
}

struct OverlapTracker {
    barrier_enabled: bool,
    state: Mutex<OverlapState>,
    changed: Condvar,
}

impl OverlapTracker {
    fn new(barrier_enabled: bool) -> Self {
        Self {
            barrier_enabled,
            state: Mutex::new(OverlapState::default()),
            changed: Condvar::new(),
        }
    }

    fn enter(self: &Arc<Self>, host: &str) -> RangeActivityGuard {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.active_range_chunks += 1;
        state.max_active_range_chunks =
            state.max_active_range_chunks.max(state.active_range_chunks);
        *state.active_hosts.entry(host.to_owned()).or_default() += 1;

        if state.active_range_chunks >= 2 {
            let active_hosts = state.active_hosts.keys().cloned().collect::<Vec<_>>();
            state.hosts_at_overlap.extend(active_hosts);
            state.released = true;
            self.changed.notify_all();
        } else if self.barrier_enabled && !state.released {
            let deadline = Instant::now() + OVERLAP_GATE_TIMEOUT;
            while !state.released {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    state.released = true;
                    self.changed.notify_all();
                    break;
                }
                let (next, timeout) = self
                    .changed
                    .wait_timeout(state, remaining)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                state = next;
                if timeout.timed_out() && !state.released {
                    state.released = true;
                    self.changed.notify_all();
                }
            }
        }

        RangeActivityGuard {
            tracker: Arc::clone(self),
            host: host.to_owned(),
        }
    }

    fn leave(&self, host: &str) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.active_range_chunks = state.active_range_chunks.saturating_sub(1);
        if let Some(active) = state.active_hosts.get_mut(host) {
            *active = active.saturating_sub(1);
            if *active == 0 {
                state.active_hosts.remove(host);
            }
        }
    }

    fn snapshot(&self) -> OverlapSnapshot {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        OverlapSnapshot {
            max_active_range_chunks: state.max_active_range_chunks,
            hosts_at_overlap: state.hosts_at_overlap.clone(),
        }
    }

    fn reset(&self) {
        *self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = OverlapState::default();
    }
}

struct RangeActivityGuard {
    tracker: Arc<OverlapTracker>,
    host: String,
}

impl Drop for RangeActivityGuard {
    fn drop(&mut self) {
        self.tracker.leave(&self.host);
    }
}

struct OverlapSnapshot {
    max_active_range_chunks: u64,
    hosts_at_overlap: BTreeSet<String>,
}

fn handle_request(
    mut stream: TcpStream,
    spec: &ServerSpec,
    state: &Mutex<ServerSnapshot>,
    overlap: &Arc<OverlapTracker>,
) -> std::io::Result<()> {
    // Accepted sockets may inherit the nonblocking listener mode on some
    // platforms. Restore blocking writes so large fixtures are not truncated
    // when the local client temporarily stops draining its receive buffer.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(SERVER_READ_TIMEOUT))?;
    stream.set_write_timeout(Some(SERVER_WRITE_TIMEOUT))?;
    let request = read_headers(&mut stream)?;
    if request.is_empty() {
        return Ok(());
    }
    let range = parse_range_header(&request);
    let phase = {
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.request_count += 1;
        let phase = request_phase(
            range,
            spec.body.len() as u64,
            &mut state.automatic_probe_seen,
        );
        let response_status = match range {
            Some((start, end)) if start <= end && end < spec.body.len() as u64 => 206,
            Some(_) => 416,
            None => 200,
        };
        state.requests.push(ServedRequestObservation {
            phase,
            range,
            response_status,
        });
        phase
    };
    let host = stream
        .local_addr()
        .map(|address| format!("{}:{}", address.ip(), address.port()))
        .unwrap_or_default();
    let _activity = (phase == DownloadTransferPhase::RangeChunk).then(|| overlap.enter(&host));
    if !spec.response_delay.is_zero() {
        thread::sleep(spec.response_delay);
    }

    match range {
        Some((start, end)) => {
            if start > end || end >= spec.body.len() as u64 {
                return write_empty_response(&mut stream, 416);
            }
            let requested_len = usize::try_from(end - start + 1).unwrap_or(usize::MAX);
            let should_send_partial = match &spec.behavior {
                Behavior::Success => false,
                Behavior::PartialOnce {
                    start: failed_start,
                    end: failed_end,
                    fired,
                    ..
                } => {
                    phase == DownloadTransferPhase::RangeChunk
                        && start == *failed_start
                        && end == *failed_end
                        && !fired.swap(true, Ordering::AcqRel)
                }
                Behavior::PartialEveryRangeChunk { .. } => {
                    phase == DownloadTransferPhase::RangeChunk
                }
            };
            if should_send_partial {
                let prefix_bytes = match &spec.behavior {
                    Behavior::PartialOnce { prefix_bytes, .. }
                    | Behavior::PartialEveryRangeChunk { prefix_bytes } => *prefix_bytes,
                    Behavior::Success => 0,
                }
                .min(requested_len);
                return write_partial_range(
                    &mut stream,
                    spec,
                    start,
                    end,
                    prefix_bytes,
                    phase,
                    state,
                );
            }
            write_range(&mut stream, spec, start, end, phase, state)
        }
        None => write_whole_file(&mut stream, spec, phase, state),
    }
}

fn request_phase(
    range: Option<(u64, u64)>,
    total_size: u64,
    automatic_probe_seen: &mut bool,
) -> DownloadTransferPhase {
    let Some((start, end)) = range else {
        return DownloadTransferPhase::WholeFile;
    };
    let bytes = end.saturating_sub(start).saturating_add(1);
    if start == 0
        && bytes == total_size.min(WHOLE_FILE_PROBE_BYTES as u64)
        && !*automatic_probe_seen
    {
        *automatic_probe_seen = true;
        DownloadTransferPhase::AutomaticProbe
    } else if start == 0 && bytes == total_size.min(SHARD_PROBE_BYTES as u64) {
        DownloadTransferPhase::ShardProbe
    } else {
        DownloadTransferPhase::RangeChunk
    }
}

fn write_range(
    stream: &mut TcpStream,
    spec: &ServerSpec,
    start: u64,
    end: u64,
    phase: DownloadTransferPhase,
    state: &Mutex<ServerSnapshot>,
) -> std::io::Result<()> {
    let start = usize::try_from(start).unwrap_or(usize::MAX);
    let end = usize::try_from(end.saturating_add(1)).unwrap_or(usize::MAX);
    let bytes = spec
        .body
        .get(start..end)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad range"))?;
    write_response_headers(
        stream,
        206,
        bytes.len(),
        Some((
            start as u64,
            end.saturating_sub(1) as u64,
            spec.body.len() as u64,
        )),
    )?;
    write_body_counted(stream, bytes, phase, state)
}

fn write_partial_range(
    stream: &mut TcpStream,
    spec: &ServerSpec,
    start: u64,
    end: u64,
    prefix_bytes: usize,
    phase: DownloadTransferPhase,
    state: &Mutex<ServerSnapshot>,
) -> std::io::Result<()> {
    let start_index = usize::try_from(start).unwrap_or(usize::MAX);
    let end_exclusive = usize::try_from(end.saturating_add(1)).unwrap_or(usize::MAX);
    let body = spec
        .body
        .get(start_index..end_exclusive)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "bad range"))?;
    let prefix = &body[..prefix_bytes.min(body.len())];
    write_response_headers(
        stream,
        206,
        body.len(),
        Some((start, end, spec.body.len() as u64)),
    )?;
    write_body_counted(stream, prefix, phase, state)?;
    let _ = stream.shutdown(Shutdown::Both);
    Ok(())
}

fn write_whole_file(
    stream: &mut TcpStream,
    spec: &ServerSpec,
    phase: DownloadTransferPhase,
    state: &Mutex<ServerSnapshot>,
) -> std::io::Result<()> {
    write_response_headers(stream, 200, spec.body.len(), None)?;
    write_body_counted(stream, spec.body.as_slice(), phase, state)
}

fn write_empty_response(stream: &mut TcpStream, status: u16) -> std::io::Result<()> {
    write_response_headers(stream, status, 0, None)?;
    stream.flush()
}

fn write_response_headers(
    stream: &mut TcpStream,
    status: u16,
    content_length: usize,
    content_range: Option<(u64, u64, u64)>,
) -> std::io::Result<()> {
    let reason = if status == 206 {
        "Partial Content"
    } else if status == 200 {
        "OK"
    } else {
        "Range Not Satisfiable"
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/octet-stream\r\nAccept-Ranges: bytes\r\nContent-Length: {content_length}\r\n"
    )?;
    if let Some((start, end, total)) = content_range {
        write!(stream, "Content-Range: bytes {start}-{end}/{total}\r\n")?;
    }
    stream.write_all(b"Connection: close\r\n\r\n")
}

fn read_headers(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut bytes = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 512];
    loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > 16 * 1024 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "fixture request headers exceeded the bounded size",
            ));
        }
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn parse_range_header(request: &str) -> Option<(u64, u64)> {
    let value = request.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("range") {
            value.trim().strip_prefix("bytes=")
        } else {
            None
        }
    })?;
    let (start, end) = value.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?))
}

fn write_body_counted(
    stream: &mut TcpStream,
    body: &[u8],
    phase: DownloadTransferPhase,
    state: &Mutex<ServerSnapshot>,
) -> std::io::Result<()> {
    let mut bytes_written = 0_usize;
    while bytes_written < body.len() {
        match stream.write(&body[bytes_written..]) {
            Ok(0) => {
                let error = std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "fixture socket accepted a zero-length body write",
                );
                record_write_error(state, phase, bytes_written, error.kind());
                return Err(error);
            }
            Ok(written) => {
                bytes_written = bytes_written.saturating_add(written);
                state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .add_bytes(phase, written as u64);
            }
            Err(error) => {
                record_write_error(state, phase, bytes_written, error.kind());
                return Err(error);
            }
        }
    }
    if let Err(error) = stream.flush() {
        record_write_error(state, phase, bytes_written, error.kind());
        return Err(error);
    }
    Ok(())
}

fn record_write_error(
    state: &Mutex<ServerSnapshot>,
    phase: DownloadTransferPhase,
    bytes_written: usize,
    kind: std::io::ErrorKind,
) {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .write_errors
        .push(ServerWriteError {
            phase,
            bytes_written: bytes_written as u64,
            kind,
        });
}

fn join_finished_handlers(handlers: &mut Vec<JoinHandle<()>>) {
    let mut index = 0;
    while index < handlers.len() {
        if handlers[index].is_finished() {
            let handler = handlers.swap_remove(index);
            let _ = handler.join();
        } else {
            index += 1;
        }
    }
}
