//! Explicit, opt-in live CDN benchmark. Run with `--ignored`; normal test and CI runs skip it.

use std::{
    collections::{BTreeMap, HashSet},
    env,
    fs::File,
    io::{Read, Write as _},
    path::PathBuf,
    sync::Mutex,
    time::Instant,
};

use bbdown_core::{
    BiliClient, ClientConfig, DownloadMode, DownloadOptions, DownloadPlan, DownloadProgressEvent,
    DownloadReport, Error, MediaHostOptions, MediaStream, MuxOptions, RetryPolicy, StreamSelection,
    probe_media_cdns,
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
const DOWNLOAD_TIMEOUT_ERROR: &str = "download request timeout elapsed";
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
    size_probe_sample_bytes: u64,
    size_probe_elapsed_ms: u128,
    success: bool,
    elapsed_ms: u128,
    size: Option<u64>,
    sha256: Option<String>,
    shard_bytes_by_host: BTreeMap<String, u64>,
    whole_file_fallback: Option<bool>,
    wire_bytes_observed: bool,
    valid_three_way_sample: bool,
    validity_reasons: Vec<&'static str>,
    error_class: Option<&'static str>,
}

#[derive(Debug)]
struct BenchmarkSettings {
    raw_url: String,
    sample: String,
    entry_index: u32,
    repetitions: usize,
    order_offset: usize,
}

#[derive(Clone, Copy, Debug)]
struct SizeDiscovery {
    size: u64,
    source: &'static str,
    sample_bytes: u64,
    elapsed_ms: u128,
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
    error_class: Option<&'static str>,
}

#[tokio::test]
#[ignore = "opt-in live network benchmark; run explicitly with --ignored"]
async fn cdn_benchmark_harness() -> anyhow::Result<()> {
    let settings = BenchmarkSettings::from_env()?;
    let client =
        BiliClient::new(ClientConfig::default().with_request_timeout(DOWNLOAD_REQUEST_TIMEOUT));
    let case = resolve_case(&client, &settings).await?;
    let (temp_root, root) = output_root()?;
    let mut records = Vec::new();
    for repetition in 0..settings.repetitions {
        let mut repetition_records =
            run_repetition(&client, &case, &settings, &root, repetition).await?;
        records.append(&mut repetition_records);
        for record in &records[records.len() - GROUPS.len()..] {
            println!("{}", serde_json::to_string(record)?);
        }
    }
    std::io::stdout().flush()?;
    if env::var_os("BBDOWN_CDN_BENCHMARK_KEEP_OUTPUT").is_some() {
        let retained_path = temp_root.keep();
        eprintln!(
            "cdn_benchmark_output_retained path={}",
            retained_path.display()
        );
    }
    anyhow::ensure!(
        records.iter().all(|record| record.valid_three_way_sample),
        "CDN benchmark sample is invalid; inspect JSON validity_reasons"
    );
    Ok(())
}

impl BenchmarkSettings {
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

async fn resolve_case(
    client: &BiliClient,
    settings: &BenchmarkSettings,
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
    let size = discover_selected_size(client, &selected).await?;
    anyhow::ensure!(
        size.size <= MAX_MEDIA_BYTES,
        "selected representation exceeds the sample limit"
    );
    selected.size = Some(size.size);
    let donor = Url::parse(&selected.base_url)?;
    let fixed_host = env::var("BBDOWN_CDN_BENCHMARK_FIXED_HOST")
        .unwrap_or_else(|_| donor.host_str().unwrap_or_default().to_owned());
    anyhow::ensure!(!fixed_host.is_empty(), "fixed host is required");
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
    root: &std::path::Path,
    repetition: usize,
) -> anyhow::Result<Vec<Record>> {
    let mut records = Vec::with_capacity(GROUPS.len());
    for order in 0..GROUPS.len() {
        let group = GROUPS[(order + settings.order_offset + repetition) % GROUPS.len()];
        records.push(run_group(client, case, settings, root, repetition, order, group).await?);
    }
    mark_valid_samples(&mut records, &case.fixed_host);
    Ok(records)
}

async fn run_group(
    client: &BiliClient,
    case: &BenchmarkCase,
    settings: &BenchmarkSettings,
    root: &std::path::Path,
    repetition: usize,
    order: usize,
    group: Group,
) -> anyhow::Result<Record> {
    let output_dir = root.join(format!("run-{repetition:02}-{order:02}"));
    std::fs::create_dir(&output_dir)?;
    let plan = plan_for_group(&case.plan, group, &case.fixed_host, &case.candidate_hosts)?;
    let shard_bytes = Mutex::new(BTreeMap::new());
    let progress = |event: &DownloadProgressEvent| {
        if let DownloadProgressEvent::CdnShardCompleted { host, bytes, .. } = event {
            let mut totals = shard_bytes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *totals.entry(host.clone()).or_default() += bytes;
        }
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
    let result = client
        .download_plan_with_progress(&plan, options, &progress)
        .await;
    let elapsed_ms = began.elapsed().as_millis();
    let error_class = result.as_ref().err().map(classify_error);
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
            error_class,
        },
    ))
}

fn download_options(
    output_dir: &std::path::Path,
    case: &BenchmarkCase,
    group: Group,
) -> DownloadOptions {
    DownloadOptions::new(output_dir)
        .with_retry_policy(RetryPolicy::single_attempt())
        .with_download_mode(DownloadMode::VideoOnly)
        .with_stream_selection(StreamSelection::video(case.selected.id))
        .with_resume(false)
        .with_subtitles(false)
        .with_danmaku(false)
        .with_cover(false)
        .with_mux(MuxOptions::Disabled)
        .with_cdn_parallelism(if matches!(group, Group::FixedHostBaseline) {
            1
        } else {
            4
        })
}

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
        error_class,
    } = observation;
    let (result, error_class) = match result {
        Ok(report) => (Some(report), None),
        Err(_) => (None, error_class),
    };
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
    let success = size == Some(case.size.size) && sha256.is_some();
    let shard_total = shards.values().copied().sum::<u64>();
    let whole_file_fallback = if !success {
        None
    } else if matches!(group, Group::FixedHostBaseline) {
        Some(false)
    } else {
        Some(shard_total == 0)
    };
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
        size_probe_sample_bytes: case.size.sample_bytes,
        size_probe_elapsed_ms: case.size.elapsed_ms,
        success,
        elapsed_ms,
        size,
        sha256,
        shard_bytes_by_host: shards,
        whole_file_fallback,
        wire_bytes_observed: false,
        valid_three_way_sample: false,
        validity_reasons: Vec::new(),
        error_class,
    }
}

async fn discover_selected_size(
    client: &BiliClient,
    selected: &MediaStream,
) -> anyhow::Result<SizeDiscovery> {
    if let Some(size) = selected.size.filter(|size| *size > 0) {
        return Ok(SizeDiscovery {
            size,
            source: "plan",
            sample_bytes: 0,
            elapsed_ms: 0,
        });
    }

    let mut donor_only = selected.clone();
    donor_only.backup_urls.clear();
    let began = Instant::now();
    let results = tokio::time::timeout(
        SIZE_PROBE_TIMEOUT,
        probe_media_cdns(client, &donor_only, &MediaHostOptions::default()),
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
    Ok(SizeDiscovery {
        size,
        source: "probe",
        sample_bytes: result.bytes,
        elapsed_ms: began.elapsed().as_millis(),
    })
}

fn mark_valid_samples(records: &mut [Record], fixed_host: &str) {
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
        if hosts.len() != 1 || !host_matches(hosts[0], fixed_host) {
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
        size_probe_sample_bytes: 0,
        size_probe_elapsed_ms: 0,
        success: true,
        elapsed_ms: 100,
        size: Some(100),
        sha256: Some("same-digest".to_owned()),
        shard_bytes_by_host: BTreeMap::new(),
        whole_file_fallback: None,
        wire_bytes_observed: false,
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
    let parse_host = |host: &str| Url::parse(&format!("http://{host}"));
    match (parse_host(candidate), parse_host(fixed_host)) {
        (Ok(candidate), Ok(fixed)) => candidate
            .host_str()
            .zip(fixed.host_str())
            .is_some_and(|(left, right)| left.eq_ignore_ascii_case(right)),
        _ => false,
    }
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
    let mut url = source.clone();
    url.set_host(Some(host))
        .map_err(|error| anyhow::anyhow!("invalid candidate host: {error}"))?;
    Ok(url.into())
}

fn read_hosts(selected: &MediaStream, fixed_host: &str) -> anyhow::Result<Vec<String>> {
    let requested = env::var("BBDOWN_CDN_BENCHMARK_HOSTS").ok();
    let mut hosts = requested.map_or_else(
        || {
            std::iter::once(fixed_host.to_owned())
                .chain(
                    selected
                        .backup_urls
                        .iter()
                        .filter_map(|url| Url::parse(url).ok()?.host_str().map(str::to_owned)),
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
    let mut seen_hosts = HashSet::new();
    hosts.retain(|host| seen_hosts.insert(host.clone()));
    anyhow::ensure!(
        (2..=MAX_MULTI_HOSTS).contains(&hosts.len()),
        "multi-host group requires between 2 and {MAX_MULTI_HOSTS} distinct hosts; set BBDOWN_CDN_BENCHMARK_HOSTS"
    );
    anyhow::ensure!(
        hosts
            .iter()
            .any(|host| host.eq_ignore_ascii_case(fixed_host)),
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

    let discovered =
        discover_selected_size(&BiliClient::new(ClientConfig::default()), &stream).await?;

    assert_eq!(discovered.size, 100_000);
    assert_eq!(discovered.source, "probe");
    assert_eq!(discovered.sample_bytes, MAX_SIZE_PROBE_SAMPLE_BYTES);
    assert_eq!(discovery.calls(), 1);
    assert_eq!(sample.calls(), 1);
    Ok(())
}
