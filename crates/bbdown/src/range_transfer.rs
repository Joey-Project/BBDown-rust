use std::{
    io::{Seek, SeekFrom, Write},
    path::Path,
    time::{Duration, Instant},
};

use futures_util::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, HeaderMap, RANGE};

#[cfg(test)]
use crate::progress::NoopDownloadProgress;
use crate::{
    Error, Result,
    progress::{
        CdnCandidateExclusionReason, DownloadProgressSink, DownloadTransferDiagnostic,
        DownloadTransferFailureReason, DownloadTransferPhase, DownloadTransferRequestOutcome,
        TransferReporter, TransferRequestReporter,
    },
};

const MAX_RANGE_BYTES: u64 = 8 * 1024 * 1024;
const CANDIDATE_PROBE_BYTES: u64 = 16 * 1024;
const MAX_CDN_CANDIDATES: usize = 8;
#[cfg(not(test))]
const CANDIDATE_PROBE_TIMEOUT_LIMIT: Duration = Duration::from_secs(2);
#[cfg(test)]
const CANDIDATE_PROBE_TIMEOUT_LIMIT: Duration = Duration::from_secs(10);

type ChunkFuture<'a> = BoxFuture<'a, (u64, usize, usize, Result<RangeFetch>)>;

#[derive(Debug)]
pub(crate) struct RangeFetch {
    pub(crate) bytes: Vec<u8>,
    pub(crate) total_size: u64,
    pub(crate) elapsed: Duration,
}

/// Fetches one bounded byte range and validates the server's complete range claim.
/// Errors deliberately omit the request URL because media URLs may contain signatures.
#[cfg(test)]
#[allow(clippy::too_many_arguments)] // Keep the range contract explicit at its call sites.
pub(crate) async fn fetch_range(
    client: &reqwest::Client,
    url: &str,
    headers: HeaderMap,
    start: u64,
    end_inclusive: u64,
    expected_total: Option<u64>,
    request_timeout: Duration,
    idle_timeout: Option<Duration>,
) -> Result<RangeFetch> {
    fetch_range_inner::<NoopDownloadProgress>(
        client,
        url,
        headers,
        start,
        end_inclusive,
        expected_total,
        request_timeout,
        idle_timeout,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)] // Keep the complete range request context explicit.
pub(crate) async fn fetch_range_with_observer<P>(
    client: &reqwest::Client,
    url: &str,
    headers: HeaderMap,
    start: u64,
    end_inclusive: u64,
    expected_total: Option<u64>,
    request_timeout: Duration,
    idle_timeout: Option<Duration>,
    observer: Option<&TransferRequestReporter<'_, P>>,
) -> Result<RangeFetch>
where
    P: DownloadProgressSink + ?Sized,
{
    let result = fetch_range_inner(
        client,
        url,
        headers,
        start,
        end_inclusive,
        expected_total,
        request_timeout,
        idle_timeout,
        observer,
    )
    .await;
    if let Some(observer) = observer {
        observer.finish(match &result {
            Ok(_) => DownloadTransferRequestOutcome::Succeeded,
            Err(error) if error.is_cancelled() => DownloadTransferRequestOutcome::Cancelled,
            Err(error) => DownloadTransferRequestOutcome::Failed {
                reason: range_failure_reason(error),
            },
        });
    }
    result
}

#[allow(clippy::too_many_arguments)] // Shared validator receives the same explicit request context.
async fn fetch_range_inner<P>(
    client: &reqwest::Client,
    url: &str,
    headers: HeaderMap,
    start: u64,
    end_inclusive: u64,
    expected_total: Option<u64>,
    request_timeout: Duration,
    idle_timeout: Option<Duration>,
    observer: Option<&TransferRequestReporter<'_, P>>,
) -> Result<RangeFetch>
where
    P: DownloadProgressSink + ?Sized,
{
    let requested_len = end_inclusive
        .checked_sub(start)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| invalid("invalid byte range"))?;
    if requested_len > MAX_RANGE_BYTES {
        return Err(invalid("byte range exceeds the 8 MiB limit"));
    }

    let began = Instant::now();
    tokio::time::timeout(request_timeout, async {
        let response = client
            .get(url)
            .headers(headers)
            .header(RANGE, format!("bytes={start}-{end_inclusive}"))
            .send()
            .await
            .map_err(|_| invalid("range request failed"))?;

        if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(Error::InvalidInput(format!(
                "range server did not return HTTP 206 (received {})",
                response.status().as_u16()
            )));
        }
        let content_range = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| invalid("missing or invalid Content-Range"))?;
        let total_size = parse_content_range(content_range, start, end_inclusive)?;
        if expected_total.is_some_and(|expected| expected != total_size) {
            return Err(invalid("range total size does not match expected size"));
        }
        if let Some(content_length) = response.headers().get(CONTENT_LENGTH) {
            let declared = content_length
                .to_str()
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .ok_or_else(|| invalid("invalid Content-Length"))?;
            if declared != requested_len {
                return Err(invalid("Content-Length does not match requested range"));
            }
        }

        let mut stream = response.bytes_stream();
        let capacity = usize::try_from(requested_len)
            .map_err(|_| invalid("requested byte range exceeds platform capacity"))?;
        let mut bytes = Vec::with_capacity(capacity);
        loop {
            let next = match idle_timeout {
                Some(timeout) => tokio::time::timeout(timeout, stream.next())
                    .await
                    .map_err(|_| invalid("range response stalled"))?,
                None => stream.next().await,
            };
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(|_| invalid("range response body failed"))?;
            if let Some(observer) = observer {
                observer.body_chunk(chunk.len());
            }
            let new_len = bytes
                .len()
                .checked_add(chunk.len())
                .ok_or_else(|| invalid("range response is too large"))?;
            if new_len as u64 > requested_len {
                return Err(invalid("range response exceeds requested length"));
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.len() as u64 != requested_len {
            return Err(invalid("range response length is incomplete"));
        }

        Ok(RangeFetch {
            bytes,
            total_size,
            elapsed: began.elapsed(),
        })
    })
    .await
    .map_err(|_| invalid("range request timed out"))?
}

fn range_failure_reason(error: &Error) -> DownloadTransferFailureReason {
    match error {
        Error::InvalidInput(message) if message == "range request failed" => {
            DownloadTransferFailureReason::RequestFailed
        }
        Error::InvalidInput(message) if message == "range response stalled" => {
            DownloadTransferFailureReason::BodyStalled
        }
        Error::InvalidInput(message) if message == "range request timed out" => {
            DownloadTransferFailureReason::TimedOut
        }
        Error::InvalidInput(message) if message == "range response body failed" => {
            DownloadTransferFailureReason::BodyReadFailed
        }
        Error::InvalidInput(message)
            if message == "range response exceeds requested length"
                || message == "range response is too large" =>
        {
            DownloadTransferFailureReason::OversizedBody
        }
        Error::InvalidInput(message) if message == "range response length is incomplete" => {
            DownloadTransferFailureReason::IncompleteBody
        }
        Error::Io(_) => DownloadTransferFailureReason::WriteFailed,
        _ => DownloadTransferFailureReason::InvalidResponse,
    }
}

fn candidate_exclusion_reason(error: &Error) -> CdnCandidateExclusionReason {
    match error {
        Error::InvalidInput(message)
            if message == "range total size does not match expected size"
                || message == "Content-Range does not match requested range" =>
        {
            CdnCandidateExclusionReason::SizeMismatch
        }
        _ => CdnCandidateExclusionReason::ProbeFailed,
    }
}

fn parse_content_range(value: &str, expected_start: u64, expected_end: u64) -> Result<u64> {
    let value = value
        .strip_prefix("bytes ")
        .ok_or_else(|| invalid("invalid Content-Range unit"))?;
    let (range, total) = value
        .split_once('/')
        .ok_or_else(|| invalid("invalid Content-Range"))?;
    let (start, end) = range
        .split_once('-')
        .ok_or_else(|| invalid("invalid Content-Range"))?;
    let start = start
        .parse::<u64>()
        .map_err(|_| invalid("invalid Content-Range start"))?;
    let end = end
        .parse::<u64>()
        .map_err(|_| invalid("invalid Content-Range end"))?;
    let total = total
        .parse::<u64>()
        .map_err(|_| invalid("invalid Content-Range total"))?;
    if start != expected_start || end != expected_end || total <= end {
        return Err(invalid("Content-Range does not match requested range"));
    }
    Ok(total)
}

fn invalid(message: &str) -> Error {
    Error::InvalidInput(message.to_owned())
}

/// Downloads a file through bounded, dynamically scheduled Range requests into a temporary file.
/// Candidate URLs are grouped by matching sample bytes and URL path/query. Sample and size checks
/// reduce accidental mixing but do not prove whole-file equality; each range is fetched once.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
#[cfg(test)]
pub(crate) async fn download_sharded_to_temp<F>(
    client: &reqwest::Client,
    urls: &[String],
    headers: HeaderMap,
    expected_total: u64,
    concurrency: usize,
    chunk_size: u64,
    request_timeout: Duration,
    idle_timeout: Option<Duration>,
    dest_dir: &Path,
    on_chunk: F,
) -> Result<tempfile::TempPath>
where
    F: FnMut(u64, &str),
{
    let progress = NoopDownloadProgress;
    let observer = TransferReporter::new(&progress, None, None, None, None);
    download_sharded_to_temp_with_progress(
        client,
        urls,
        headers,
        expected_total,
        concurrency,
        chunk_size,
        request_timeout,
        idle_timeout,
        dest_dir,
        &observer,
        on_chunk,
    )
    .await
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) async fn download_sharded_to_temp_with_progress<F, P>(
    client: &reqwest::Client,
    urls: &[String],
    headers: HeaderMap,
    expected_total: u64,
    concurrency: usize,
    chunk_size: u64,
    request_timeout: Duration,
    idle_timeout: Option<Duration>,
    dest_dir: &Path,
    transfer_reporter: &TransferReporter<'_, P>,
    mut on_chunk: F,
) -> Result<tempfile::TempPath>
where
    F: FnMut(u64, &str),
    P: DownloadProgressSink + ?Sized,
{
    if expected_total == 0 {
        return Err(invalid("expected media size must be nonzero"));
    }
    if !(2..=8).contains(&concurrency) {
        return Err(invalid("range concurrency must be between 2 and 8"));
    }
    if chunk_size == 0 || chunk_size > MAX_RANGE_BYTES {
        return Err(invalid("range chunk size must be between 1 byte and 8 MiB"));
    }
    if urls.is_empty() {
        return Err(invalid("no media CDN candidates were provided"));
    }

    let probe_end = CANDIDATE_PROBE_BYTES.min(expected_total) - 1;
    let probe_timeout = request_timeout.min(CANDIDATE_PROBE_TIMEOUT_LIMIT);
    let mut probes = FuturesUnordered::new();
    for (index, url) in urls.iter().take(MAX_CDN_CANDIDATES).enumerate() {
        let probe_headers = headers.clone();
        probes.push(async move {
            let observer =
                transfer_reporter.begin_request(url, DownloadTransferPhase::ShardProbe, None);
            let result = fetch_range_with_observer(
                client,
                url,
                probe_headers,
                0,
                probe_end,
                Some(expected_total),
                probe_timeout,
                idle_timeout,
                observer.as_ref(),
            )
            .await;
            (index, url.as_str(), result, observer)
        });
    }
    let mut completed_probes = Vec::new();
    while let Some(probe) = probes.next().await {
        completed_probes.push(probe);
    }
    completed_probes.sort_by_key(|(index, _, _, _)| *index);

    let mut candidate_groups: Vec<Vec<(usize, &str, Vec<u8>)>> = Vec::new();
    for (index, url, probe_result, _) in &completed_probes {
        if let Ok(probe) = probe_result {
            if let Some(group) = candidate_groups
                .iter_mut()
                .find(|group| group[0].2 == probe.bytes && same_path_query(group[0].1, url))
            {
                group.push((*index, url, probe.bytes.clone()));
            } else {
                candidate_groups.push(vec![(*index, url, probe.bytes.clone())]);
            }
        }
    }
    if candidate_groups.is_empty() {
        for (_, _, probe_result, observer) in &completed_probes {
            if let (Err(error), Some(observer)) = (probe_result, observer) {
                observer.diagnostic(DownloadTransferDiagnostic::CandidateExcluded {
                    reason: candidate_exclusion_reason(error),
                });
            }
        }
        return Err(invalid("no CDN candidate passed the range probe"));
    }
    let mut largest_group_index = 0;
    for index in 1..candidate_groups.len() {
        if candidate_groups[index].len() > candidate_groups[largest_group_index].len() {
            largest_group_index = index;
        }
    }
    let winner_group = candidate_groups.swap_remove(largest_group_index);
    let winner_url = winner_group[0].1;
    let winner_sample = winner_group[0].2.clone();
    let winner_indices: Vec<usize> = winner_group.iter().map(|(index, _, _)| *index).collect();
    let candidates: Vec<&str> = winner_group.into_iter().map(|(_, url, _)| url).collect();
    for (index, url, probe_result, observer) in &completed_probes {
        let Some(observer) = observer else { continue };
        let diagnostic = if winner_indices.contains(index) {
            DownloadTransferDiagnostic::CandidateSelected
        } else if let Ok(probe) = probe_result {
            let reason = if !same_path_query(winner_url, url) {
                CdnCandidateExclusionReason::PathQueryMismatch
            } else if probe.bytes != winner_sample {
                CdnCandidateExclusionReason::SampleMismatch
            } else {
                CdnCandidateExclusionReason::OutsideWinningGroup
            };
            DownloadTransferDiagnostic::CandidateExcluded { reason }
        } else if let Err(error) = probe_result {
            DownloadTransferDiagnostic::CandidateExcluded {
                reason: candidate_exclusion_reason(error),
            }
        } else {
            continue;
        };
        observer.diagnostic(diagnostic);
    }

    let chunk_count = expected_total.div_ceil(chunk_size);
    let concurrency = u64::try_from(concurrency)
        .map_err(|_| invalid("range concurrency exceeds platform limits"))?;
    let active_count = usize::try_from(chunk_count.min(concurrency))
        .map_err(|_| invalid("range concurrency exceeds platform limits"))?;
    let mut staging = tempfile::NamedTempFile::new_in(dest_dir)?;
    staging.as_file().set_len(expected_total)?;

    let mut pending: FuturesUnordered<ChunkFuture<'_>> = FuturesUnordered::new();
    let mut next_chunk = 0_u64;
    let mut lane_sources: Vec<usize> = (0..active_count)
        .map(|lane| lane % candidates.len())
        .collect();
    for (lane, &preferred_source) in lane_sources.iter().enumerate() {
        pending.push(schedule_chunk(
            client,
            &candidates,
            headers.clone(),
            expected_total,
            chunk_size,
            request_timeout,
            idle_timeout,
            transfer_reporter,
            next_chunk,
            lane,
            preferred_source,
        ));
        next_chunk += 1;
    }

    while let Some((chunk_index, lane, successful_source, result)) = pending.next().await {
        let fetched = result?;
        lane_sources[lane] = successful_source;
        let offset = chunk_index * chunk_size;
        staging.as_file_mut().seek(SeekFrom::Start(offset))?;
        staging.as_file_mut().write_all(&fetched.bytes)?;
        let source = candidates[successful_source];
        on_chunk(fetched.bytes.len() as u64, source);

        if next_chunk < chunk_count {
            pending.push(schedule_chunk(
                client,
                &candidates,
                headers.clone(),
                expected_total,
                chunk_size,
                request_timeout,
                idle_timeout,
                transfer_reporter,
                next_chunk,
                lane,
                lane_sources[lane],
            ));
            next_chunk += 1;
        }
    }

    staging.as_file_mut().flush()?;
    Ok(staging.into_temp_path())
}

#[allow(clippy::too_many_arguments)] // This mirrors the explicit Range request context.
fn schedule_chunk<'a, 'p, P>(
    client: &'a reqwest::Client,
    candidates: &'a [&'a str],
    headers: HeaderMap,
    expected_total: u64,
    chunk_size: u64,
    request_timeout: Duration,
    idle_timeout: Option<Duration>,
    transfer_reporter: &'a TransferReporter<'p, P>,
    chunk_index: u64,
    lane: usize,
    preferred_source: usize,
) -> BoxFuture<'a, (u64, usize, usize, Result<RangeFetch>)>
where
    P: DownloadProgressSink + ?Sized,
    'p: 'a,
{
    async move {
        let start = chunk_index * chunk_size;
        let end = expected_total.min(start.saturating_add(chunk_size)) - 1;
        let candidate_index = preferred_source % candidates.len();
        let mut last_error = None;
        let mut retry_reason = None;
        for offset in 0..candidates.len() {
            let index = (candidate_index + offset) % candidates.len();
            let observer = transfer_reporter.begin_request(
                candidates[index],
                DownloadTransferPhase::RangeChunk,
                retry_reason,
            );
            match fetch_range_with_observer(
                client,
                candidates[index],
                headers.clone(),
                start,
                end,
                Some(expected_total),
                request_timeout,
                idle_timeout,
                observer.as_ref(),
            )
            .await
            {
                Ok(fetched) => return (chunk_index, lane, index, Ok(fetched)),
                Err(error) => {
                    retry_reason = Some(range_failure_reason(&error));
                    last_error = Some(error);
                }
            }
        }
        (
            chunk_index,
            lane,
            candidate_index,
            Err(last_error.unwrap_or_else(|| invalid("no CDN range source available"))),
        )
    }
    .boxed()
}

fn same_path_query(left: &str, right: &str) -> bool {
    let (Ok(left), Ok(right)) = (url::Url::parse(left), url::Url::parse(right)) else {
        return false;
    };
    left.scheme() == right.scheme() && left.path() == right.path() && left.query() == right.query()
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicU64, Ordering},
            mpsc,
        },
        time::Duration,
    };

    use httpmock::prelude::*;
    use httpmock::{HttpMockRequest, HttpMockResponse};
    use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, HeaderMap};

    use super::{fetch_range, fetch_range_with_observer};
    use crate::progress::{
        CdnCandidateExclusionReason, DownloadProgressEvent, DownloadTransferDiagnostic,
        DownloadTransferFailureReason, DownloadTransferPhase, DownloadTransferRequestOutcome,
        TransferReporter,
    };

    fn raw_response_server(
        response: &'static [u8],
        body_observed: mpsc::Receiver<()>,
    ) -> std::io::Result<(String, std::thread::JoinHandle<std::io::Result<()>>)> {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept()?;
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request)?;
            stream.write_all(response)?;
            stream.flush()?;
            body_observed
                .recv_timeout(Duration::from_secs(10))
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::TimedOut, error))?;
            Ok(())
        });
        Ok((format!("http://{address}/asset"), server))
    }

    fn read_request_headers(stream: &mut std::net::TcpStream) -> std::io::Result<String> {
        use std::io::Read;

        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let read = stream.read(&mut buffer)?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "client closed before sending request headers",
                ));
            }
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                return String::from_utf8(request)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error));
            }
            if request.len() > 16 * 1024 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "request headers exceeded test limit",
                ));
            }
        }
    }

    fn observe_partial_body(
        event: &DownloadProgressEvent,
        expected_bytes: u64,
        received: &AtomicU64,
        acknowledgement_sent: &AtomicBool,
        acknowledgement: &mpsc::Sender<()>,
    ) {
        let DownloadProgressEvent::TransferBytesReceived { bytes_delta, .. } = event else {
            return;
        };
        let total = received
            .fetch_add(*bytes_delta, Ordering::Relaxed)
            .saturating_add(*bytes_delta);
        if total >= expected_bytes && !acknowledgement_sent.swap(true, Ordering::Relaxed) {
            let _ = acknowledgement.send(());
        }
    }

    fn received_bytes(
        events: &[DownloadProgressEvent],
    ) -> (u64, Vec<DownloadTransferFailureReason>) {
        let mut total = 0_u64;
        let mut reasons = Vec::new();
        for event in events {
            match event {
                DownloadProgressEvent::TransferBytesReceived { bytes_delta, .. } => {
                    total = total.saturating_add(*bytes_delta);
                }
                DownloadProgressEvent::TransferDiagnostic {
                    diagnostic:
                        DownloadTransferDiagnostic::RequestFinished {
                            outcome: DownloadTransferRequestOutcome::Failed { reason },
                            ..
                        },
                    ..
                } => reasons.push(*reason),
                _ => {}
            }
        }
        (total, reasons)
    }

    #[derive(Default)]
    struct ChunkOverlapState {
        first_chunk_waiting: bool,
        overlapped: bool,
    }

    async fn fetch(server: &MockServer) -> crate::Result<super::RangeFetch> {
        fetch_range(
            &reqwest::Client::new(),
            &server.url("/asset?token=secret"),
            HeaderMap::new(),
            2,
            4,
            Some(10),
            Duration::from_secs(10),
            Some(Duration::from_secs(10)),
        )
        .await
    }

    fn range_mock<'a>(
        server: &'a MockServer,
        status: u16,
        content_range: &'a str,
        body: &'a str,
    ) -> httpmock::Mock<'a> {
        server.mock(|when, then| {
            when.method(GET).path("/asset").header("range", "bytes=2-4");
            then.status(status)
                .header(CONTENT_RANGE.as_str(), content_range)
                .body(body);
        })
    }

    #[tokio::test]
    async fn accepts_exact_partial_range() -> anyhow::Result<()> {
        let server = MockServer::start();
        let mock = range_mock(&server, 206, "bytes 2-4/10", "abc");
        let result = fetch(&server).await?;
        assert_eq!(result.bytes, b"abc");
        assert_eq!(result.total_size, 10);
        mock.assert();
        Ok(())
    }

    #[tokio::test]
    async fn rejects_server_ignoring_range() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/asset");
            then.status(200).body("abc");
        });
        let result = fetch(&server).await;
        assert!(matches!(
            result,
            Err(crate::Error::InvalidInput(message))
                if message.contains("HTTP 206") && !message.contains("secret")
        ));
    }

    #[tokio::test]
    async fn rejects_wrong_range_and_total() {
        let server = MockServer::start();
        let _wrong = range_mock(&server, 206, "bytes 1-3/10", "abc");
        assert!(matches!(
            fetch(&server).await,
            Err(crate::Error::InvalidInput(_))
        ));

        let server = MockServer::start();
        let _wrong_total = range_mock(&server, 206, "bytes 2-4/11", "abc");
        assert!(matches!(
            fetch(&server).await,
            Err(crate::Error::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn rejects_short_and_long_bodies() {
        let server = MockServer::start();
        let _short = range_mock(&server, 206, "bytes 2-4/10", "ab");
        assert!(matches!(
            fetch(&server).await,
            Err(crate::Error::InvalidInput(_))
        ));

        let server = MockServer::start();
        let _long = range_mock(&server, 206, "bytes 2-4/10", "abcd");
        assert!(matches!(
            fetch(&server).await,
            Err(crate::Error::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn rejects_inconsistent_content_length() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/asset");
            then.status(206)
                .header(CONTENT_RANGE.as_str(), "bytes 2-4/10")
                .header(CONTENT_LENGTH.as_str(), "2")
                .body("abc");
        });
        assert!(matches!(
            fetch(&server).await,
            Err(crate::Error::InvalidInput(_))
        ));
    }

    #[tokio::test]
    async fn applies_total_timeout_to_response_wait() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method(GET).path("/asset");
            then.status(206)
                .header(CONTENT_RANGE.as_str(), "bytes 2-4/10")
                .body("abc")
                .delay(Duration::from_millis(100));
        });
        let result = fetch_range(
            &reqwest::Client::new(),
            &server.url("/asset?token=secret"),
            HeaderMap::new(),
            2,
            4,
            Some(10),
            Duration::from_millis(10),
            Some(Duration::from_secs(1)),
        )
        .await;
        assert!(matches!(
            result,
            Err(crate::Error::InvalidInput(message))
                if message.contains("timed out") && !message.contains("secret")
        ));
    }

    fn delayed_body_server(
        delay: Duration,
    ) -> std::io::Result<(String, std::thread::JoinHandle<std::io::Result<()>>)> {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept()?;
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request)?;
            stream
                .write_all(
                    b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 2-4/10\r\nContent-Length: 3\r\nConnection: close\r\n\r\na",
                )?;
            stream.flush()?;
            std::thread::sleep(delay);
            stream.write_all(b"bc")?;
            Ok(())
        });
        Ok((format!("http://{address}/asset"), server))
    }

    #[tokio::test]
    async fn optional_idle_timeout_controls_stalled_body_reads() -> anyhow::Result<()> {
        let (url, server) = delayed_body_server(Duration::from_millis(1_100))?;
        let timed_out = fetch_range(
            &reqwest::Client::new(),
            &url,
            HeaderMap::new(),
            2,
            4,
            Some(10),
            Duration::from_secs(3),
            Some(Duration::from_millis(100)),
        )
        .await;
        server
            .join()
            .map_err(|_| anyhow::anyhow!("test server thread panicked"))??;
        assert!(matches!(
            timed_out,
            Err(crate::Error::InvalidInput(message)) if message.contains("stalled")
        ));

        let (url, server) = delayed_body_server(Duration::from_millis(1_100))?;
        let completed = fetch_range(
            &reqwest::Client::new(),
            &url,
            HeaderMap::new(),
            2,
            4,
            Some(10),
            Duration::from_secs(3),
            None,
        )
        .await;
        server
            .join()
            .map_err(|_| anyhow::anyhow!("test server thread panicked"))??;
        assert_eq!(completed?.bytes, b"abc");
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // Keep synchronized response fixtures beside byte assertions.
    #[tokio::test]
    async fn transfer_events_count_consumed_truncated_oversized_and_stalled_range_bytes()
    -> anyhow::Result<()> {
        let cases: [(&'static [u8], u64, DownloadTransferFailureReason); 2] = [
            (
                b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 2-4/10\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n2\r\nab\r\n0\r\n\r\n",
                2,
                DownloadTransferFailureReason::IncompleteBody,
            ),
            (
                b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 2-4/10\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\nabcd\r\n0\r\n\r\n",
                4,
                DownloadTransferFailureReason::OversizedBody,
            ),
        ];
        for (response, expected_bytes, expected_reason) in cases {
            let (body_observed_sender, body_observed_receiver) = mpsc::channel();
            let (url, server) = raw_response_server(response, body_observed_receiver)?;
            let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let sink_events = std::sync::Arc::clone(&events);
            let received = std::sync::Arc::new(AtomicU64::new(0));
            let observed_total = std::sync::Arc::clone(&received);
            let acknowledgement_sent = std::sync::Arc::new(AtomicBool::new(false));
            let acknowledgement_state = std::sync::Arc::clone(&acknowledgement_sent);
            let sink = move |event: &DownloadProgressEvent| match sink_events.lock() {
                Ok(mut events) => {
                    observe_partial_body(
                        event,
                        expected_bytes,
                        &observed_total,
                        &acknowledgement_state,
                        &body_observed_sender,
                    );
                    events.push(event.clone());
                }
                Err(poisoned) => {
                    observe_partial_body(
                        event,
                        expected_bytes,
                        &observed_total,
                        &acknowledgement_state,
                        &body_observed_sender,
                    );
                    poisoned.into_inner().push(event.clone());
                }
            };
            let reporter = TransferReporter::new(&sink, None, None, None, None);
            let observer = reporter.begin_request(&url, DownloadTransferPhase::RangeChunk, None);
            let result = fetch_range_with_observer(
                &reqwest::Client::new(),
                &url,
                HeaderMap::new(),
                2,
                4,
                Some(10),
                Duration::from_secs(2),
                Some(Duration::from_secs(2)),
                observer.as_ref(),
            )
            .await;
            server
                .join()
                .map_err(|_| anyhow::anyhow!("raw response server panicked"))??;
            assert!(result.is_err());
            let snapshot = match events.lock() {
                Ok(events) => events.clone(),
                Err(poisoned) => poisoned.into_inner().clone(),
            };
            let (total, reasons) = received_bytes(&snapshot);
            assert_eq!(total, expected_bytes);
            assert_eq!(reasons, vec![expected_reason]);
        }

        let (url, server) = delayed_body_server(Duration::from_millis(1_100))?;
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink_events = std::sync::Arc::clone(&events);
        let sink = move |event: &DownloadProgressEvent| match sink_events.lock() {
            Ok(mut events) => events.push(event.clone()),
            Err(poisoned) => poisoned.into_inner().push(event.clone()),
        };
        let reporter = TransferReporter::new(&sink, None, None, None, None);
        let observer = reporter.begin_request(&url, DownloadTransferPhase::RangeChunk, None);
        let result = fetch_range_with_observer(
            &reqwest::Client::new(),
            &url,
            HeaderMap::new(),
            2,
            4,
            Some(10),
            Duration::from_secs(3),
            Some(Duration::from_millis(100)),
            observer.as_ref(),
        )
        .await;
        server
            .join()
            .map_err(|_| anyhow::anyhow!("delayed response server panicked"))??;
        assert!(result.is_err());
        let snapshot = match events.lock() {
            Ok(events) => events.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let (total, reasons) = received_bytes(&snapshot);
        assert_eq!(total, 1);
        assert_eq!(reasons, vec![DownloadTransferFailureReason::BodyStalled]);
        Ok(())
    }

    fn add_media_server(
        server: &MockServer,
        body: String,
        failed_ranges: Vec<String>,
        reported_total: Option<u64>,
    ) -> httpmock::Mock<'_> {
        let body = std::sync::Arc::new(body);
        server.mock(|when, then| {
            when.method(GET).path("/media");
            then.respond_with(move |request: &HttpMockRequest| {
                let request_headers = request.headers();
                let range = request_headers
                    .get("range")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default();
                if failed_ranges.iter().any(|failed| failed == range) {
                    return HttpMockResponse::builder().status(503).body("").build();
                }
                let parsed_range = range
                    .strip_prefix("bytes=")
                    .and_then(|value| value.split_once('-'))
                    .and_then(|(start, end)| {
                        Some((start.parse::<usize>().ok()?, end.parse::<usize>().ok()?))
                    });
                let Some((start, end)) = parsed_range else {
                    return HttpMockResponse::builder().status(400).body("").build();
                };
                let Some(part) = body.get(start..=end) else {
                    return HttpMockResponse::builder().status(416).body("").build();
                };
                let total = body.len() as u64;
                let part = part.to_owned();
                HttpMockResponse::builder()
                    .status(206)
                    .header(
                        "Content-Range",
                        format!("bytes {start}-{end}/{}", reported_total.unwrap_or(total)),
                    )
                    .body(part)
                    .build()
            });
        })
    }

    async fn sharded(
        urls: &[String],
        expected_total: u64,
        dest_dir: &std::path::Path,
        on_chunk: impl FnMut(u64, &str),
    ) -> crate::Result<tempfile::TempPath> {
        super::download_sharded_to_temp(
            &reqwest::Client::new(),
            urls,
            HeaderMap::new(),
            expected_total,
            2,
            10_000,
            Duration::from_secs(10),
            Some(Duration::from_secs(10)),
            dest_dir,
            on_chunk,
        )
        .await
    }

    fn assert_downloaded_bytes(actual: &[u8], expected: &[u8]) {
        if actual != expected {
            let first_difference = actual
                .iter()
                .zip(expected)
                .position(|(actual, expected)| actual != expected)
                .unwrap_or_else(|| actual.len().min(expected.len()));
            assert!(
                actual == expected,
                "downloaded bytes differ: actual_len={}, expected_len={}, first_difference={first_difference}, actual_byte={:?}, expected_byte={:?}",
                actual.len(),
                expected.len(),
                actual.get(first_difference),
                expected.get(first_difference),
            );
        }
    }

    #[tokio::test]
    async fn assembles_chunks_from_two_matching_cdns() -> anyhow::Result<()> {
        let server_a = MockServer::start();
        let server_b = MockServer::start();
        let body = format!("{}{}", "a".repeat(10_000), "b".repeat(10_000));
        let mock_a = add_media_server(&server_a, body.clone(), Vec::new(), None);
        let mock_b = add_media_server(&server_b, body.clone(), Vec::new(), None);
        let urls = vec![server_a.url("/media"), server_b.url("/media")];
        let dir = tempfile::tempdir()?;
        let mut completed = Vec::new();
        let path = super::download_sharded_to_temp(
            &reqwest::Client::new(),
            &urls,
            HeaderMap::new(),
            20_000,
            2,
            10_000,
            Duration::from_secs(2),
            Some(Duration::from_secs(2)),
            dir.path(),
            |bytes, _| completed.push(bytes),
        )
        .await?;
        assert_eq!(std::fs::read(path)?, body.as_bytes());
        assert_eq!(completed.iter().sum::<u64>(), 20_000);
        assert_eq!(completed.len(), 2);
        mock_a.assert_calls(2);
        mock_b.assert_calls(2);
        Ok(())
    }

    #[tokio::test]
    async fn assembles_parallel_ranges_from_one_host() -> anyhow::Result<()> {
        let body = [vec![b'a'; 10_000], vec![b'b'; 10_000]].concat();
        let (url, server) = overlapping_range_server(body.clone())?;
        let urls = vec![url];
        let dir = tempfile::tempdir()?;
        let mut completed = Vec::new();
        let downloaded = sharded(&urls, 20_000, dir.path(), |bytes, _| completed.push(bytes)).await;
        let overlapped = server
            .join()
            .map_err(|_| anyhow::anyhow!("barrier server thread panicked"))??;
        let path = downloaded?;

        assert_eq!(std::fs::read(path)?, body.as_slice());
        assert_eq!(completed.iter().sum::<u64>(), 20_000);
        assert_eq!(completed.len(), 2);
        assert!(
            overlapped,
            "two shard requests must be active at the same time"
        );
        Ok(())
    }

    fn overlapping_range_server(
        body: Vec<u8>,
    ) -> std::io::Result<(String, std::thread::JoinHandle<std::io::Result<bool>>)> {
        use std::{
            net::TcpListener,
            sync::{Arc, Condvar, Mutex},
            thread,
            time::Instant,
        };
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let body = Arc::new(body);
        let barrier = Arc::new((Mutex::new(ChunkOverlapState::default()), Condvar::new()));
        let server = thread::spawn(move || {
            listener.set_nonblocking(true)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut handlers = Vec::new();
            while handlers.len() < 3 && Instant::now() < deadline {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let body = Arc::clone(&body);
                        let barrier = Arc::clone(&barrier);
                        handlers.push(thread::spawn(move || {
                            serve_range_connection(stream, body.as_slice(), &barrier)
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => return Err(error),
                }
            }
            if handlers.len() != 3 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "expected one probe and two shard requests",
                ));
            }
            for handler in handlers {
                handler
                    .join()
                    .map_err(|_| std::io::Error::other("range connection handler panicked"))??;
            }
            let overlapped = barrier
                .0
                .lock()
                .map_err(|_| std::io::Error::other("barrier mutex poisoned"))?
                .overlapped;
            Ok(overlapped)
        });
        Ok((format!("http://{address}/media"), server))
    }

    fn serve_range_connection(
        mut stream: std::net::TcpStream,
        body: &[u8],
        barrier: &(std::sync::Mutex<ChunkOverlapState>, std::sync::Condvar),
    ) -> std::io::Result<()> {
        use std::io::Write;

        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let range = read_range_header(&mut stream)?;
        let (start, end) = parse_range(&range)?;
        if (start, end) != (0, 16_383) {
            wait_for_overlapping_chunks(barrier)?;
        }
        let part = body.get(start..=end).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "range is out of bounds")
        })?;
        write!(
            stream,
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len(),
            part.len()
        )?;
        stream.write_all(part)?;
        stream.flush()
    }

    fn read_range_header(stream: &mut std::net::TcpStream) -> std::io::Result<String> {
        use std::io::Read;

        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let count = stream.read(&mut buffer)?;
            if count == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "request headers ended early",
                ));
            }
            request.extend_from_slice(&buffer[..count]);
        }
        String::from_utf8_lossy(&request)
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("range")
                    .then_some(value.trim().strip_prefix("bytes=")?.to_owned())
            })
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "missing Range header")
            })
    }

    fn parse_range(range: &str) -> std::io::Result<(usize, usize)> {
        let (start, end) = range.split_once('-').ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid Range header")
        })?;
        let start = start.parse::<usize>().map_err(std::io::Error::other)?;
        let end = end.parse::<usize>().map_err(std::io::Error::other)?;
        Ok((start, end))
    }

    fn wait_for_overlapping_chunks(
        barrier: &(std::sync::Mutex<ChunkOverlapState>, std::sync::Condvar),
    ) -> std::io::Result<()> {
        let (lock, condition) = barrier;
        let mut state = lock
            .lock()
            .map_err(|_| std::io::Error::other("barrier mutex poisoned"))?;
        if state.first_chunk_waiting {
            state.overlapped = true;
            condition.notify_all();
        } else {
            state.first_chunk_waiting = true;
            let (mut next, timeout) = condition
                .wait_timeout_while(state, Duration::from_millis(1_500), |state| {
                    !state.overlapped
                })
                .map_err(|_| std::io::Error::other("barrier wait poisoned"))?;
            if timeout.timed_out() && !next.overlapped {
                next.first_chunk_waiting = false;
            }
            state = next;
        }
        drop(state);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sharded_download_future_is_send_for_multithreaded_embedding() -> anyhow::Result<()> {
        let server_a = MockServer::start();
        let server_b = MockServer::start();
        add_media_server(&server_a, "x".to_owned(), Vec::new(), None);
        add_media_server(&server_b, "x".to_owned(), Vec::new(), None);
        let urls = vec![server_a.url("/media"), server_b.url("/media")];
        let dir = tempfile::tempdir()?;
        let dest_dir = dir.path().to_path_buf();

        let downloaded = tokio::spawn(async move {
            super::download_sharded_to_temp(
                &reqwest::Client::new(),
                &urls,
                HeaderMap::new(),
                1,
                2,
                1,
                Duration::from_secs(2),
                Some(Duration::from_secs(2)),
                &dest_dir,
                |_, _| {},
            )
            .await
        })
        .await??;

        assert_eq!(std::fs::read(downloaded)?, b"x");
        Ok(())
    }

    #[tokio::test]
    async fn selects_largest_prefix_group_when_first_candidate_is_an_outlier() -> anyhow::Result<()>
    {
        let outlier = MockServer::start();
        let compatible_a = MockServer::start();
        let compatible_b = MockServer::start();
        let outlier_mock = add_media_server(&outlier, "x".repeat(20_000), Vec::new(), None);
        let expected = "z".repeat(20_000);
        let second_cdn_mock = add_media_server(&compatible_a, expected.clone(), Vec::new(), None);
        let third_cdn_mock = add_media_server(&compatible_b, expected.clone(), Vec::new(), None);
        let urls = vec![
            outlier.url("/media"),
            compatible_a.url("/media"),
            compatible_b.url("/media"),
        ];
        let dir = tempfile::tempdir()?;
        let path = sharded(&urls, 20_000, dir.path(), |_, _| {}).await?;

        assert_eq!(std::fs::read(path)?, expected.as_bytes());
        assert_eq!(outlier_mock.calls(), 1);
        assert_eq!(second_cdn_mock.calls(), 2);
        assert_eq!(third_cdn_mock.calls(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn probes_bad_candidates_concurrently_before_using_compatible_group() -> anyhow::Result<()>
    {
        let slow_bad_a = MockServer::start();
        let slow_bad_b = MockServer::start();
        let compatible_a = MockServer::start();
        let compatible_b = MockServer::start();
        for server in [&slow_bad_a, &slow_bad_b] {
            server.mock(|when, then| {
                when.method(GET).path("/media");
                then.status(503).delay(Duration::from_millis(300));
            });
        }
        let body = "c".repeat(20_000);
        add_media_server(&compatible_a, body.clone(), Vec::new(), None);
        add_media_server(&compatible_b, body.clone(), Vec::new(), None);
        let urls = vec![
            slow_bad_a.url("/media"),
            slow_bad_b.url("/media"),
            compatible_a.url("/media"),
            compatible_b.url("/media"),
        ];
        let dir = tempfile::tempdir()?;
        let began = std::time::Instant::now();
        let path = super::download_sharded_to_temp(
            &reqwest::Client::new(),
            &urls,
            HeaderMap::new(),
            20_000,
            2,
            10_000,
            Duration::from_secs(30),
            Some(Duration::from_secs(2)),
            dir.path(),
            |_, _| {},
        )
        .await?;

        assert_eq!(std::fs::read(path)?, body.as_bytes());
        assert!(began.elapsed() < Duration::from_millis(500));
        Ok(())
    }

    #[tokio::test]
    async fn selects_one_compatible_candidate_without_mixing_other_probe_groups()
    -> anyhow::Result<()> {
        let good = MockServer::start();
        let different = MockServer::start();
        let wrong_total = MockServer::start();
        let body = "x".repeat(20_000);
        let good_mock = add_media_server(&good, body.clone(), Vec::new(), None);
        let different_mock = add_media_server(&different, "y".repeat(20_000), Vec::new(), None);
        let wrong_total_mock =
            add_media_server(&wrong_total, body.clone(), Vec::new(), Some(20_001));
        let dir = tempfile::tempdir()?;
        let urls = vec![
            good.url("/media"),
            different.url("/media"),
            wrong_total.url("/media"),
        ];
        let mut completed = Vec::new();
        let path = sharded(&urls, 20_000, dir.path(), |bytes, source| {
            completed.push((bytes, source.to_owned()));
        })
        .await?;
        assert_eq!(std::fs::read(path)?, body.as_bytes());
        assert_eq!(good_mock.calls(), 3, "one probe and two shard ranges");
        assert_eq!(different_mock.calls(), 1);
        assert_eq!(wrong_total_mock.calls(), 1);
        assert_eq!(completed.len(), 2);
        assert_eq!(
            completed.iter().map(|(bytes, _)| bytes).sum::<u64>(),
            20_000
        );
        assert!(completed.iter().all(|(_, source)| source == &urls[0]));
        Ok(())
    }

    #[tokio::test]
    async fn retries_failed_range_on_another_candidate() -> anyhow::Result<()> {
        let canonical = MockServer::start();
        let alternate = MockServer::start();
        let body = "z".repeat(20_000);
        let canonical_mock = add_media_server(&canonical, body.clone(), Vec::new(), None);
        let alternate_mock = add_media_server(
            &alternate,
            body.clone(),
            vec!["bytes=10000-19999".into()],
            None,
        );
        let dir = tempfile::tempdir()?;
        let urls = vec![canonical.url("/media"), alternate.url("/media")];
        let mut completed = Vec::new();
        let path = sharded(&urls, 20_000, dir.path(), |bytes, source| {
            completed.push((bytes, source.to_owned()));
        })
        .await?;
        assert_downloaded_bytes(&std::fs::read(path)?, body.as_bytes());
        assert_eq!(canonical_mock.calls(), 3);
        assert_eq!(alternate_mock.calls(), 2);
        assert_eq!(
            completed.iter().map(|(bytes, _)| bytes).sum::<u64>(),
            20_000
        );
        assert_eq!(completed.len(), 2);
        assert!(completed.iter().all(|(_, source)| source == &urls[0]));
        Ok(())
    }

    #[tokio::test]
    async fn reports_candidate_compatibility_reasons_with_host_only_context() -> anyhow::Result<()>
    {
        let canonical = MockServer::start();
        let alternate = MockServer::start();
        let outlier = MockServer::start();
        let body = "p".repeat(20_000);
        let different_body = "q".repeat(20_000);
        add_media_server(&canonical, body.clone(), Vec::new(), None);
        add_media_server(&alternate, body.clone(), Vec::new(), None);
        add_media_server(&outlier, different_body, Vec::new(), None);
        // Keep each lease alive for the whole request sequence. Dropping a MockServer returns
        // its server to httpmock's shared pool, where another parallel test may reset it.
        let servers = [canonical, alternate, outlier];
        let urls = servers
            .iter()
            .map(|server| format!("{}?token=codex_synth_v1_bearer_a", server.url("/media")))
            .collect::<Vec<_>>();
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink_events = std::sync::Arc::clone(&events);
        let sink = move |event: &DownloadProgressEvent| match sink_events.lock() {
            Ok(mut events) => events.push(event.clone()),
            Err(poisoned) => poisoned.into_inner().push(event.clone()),
        };
        let reporter = TransferReporter::new(&sink, None, None, None, None);
        let dir = tempfile::tempdir()?;
        let path = super::download_sharded_to_temp_with_progress(
            &reqwest::Client::new(),
            &urls,
            HeaderMap::new(),
            20_000,
            2,
            10_000,
            Duration::from_secs(10),
            Some(Duration::from_secs(10)),
            dir.path(),
            &reporter,
            |_, _| {},
        )
        .await?;
        assert_downloaded_bytes(&std::fs::read(path)?, body.as_bytes());
        let snapshot = match events.lock() {
            Ok(events) => events.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let selected = snapshot
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    DownloadProgressEvent::TransferDiagnostic {
                        diagnostic: DownloadTransferDiagnostic::CandidateSelected,
                        ..
                    }
                )
            })
            .count();
        let excluded = snapshot
            .iter()
            .filter_map(|event| match event {
                DownloadProgressEvent::TransferDiagnostic {
                    diagnostic: DownloadTransferDiagnostic::CandidateExcluded { reason },
                    host: Some(host),
                    ..
                } => Some((*reason, host)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(selected, 2);
        assert_eq!(excluded.len(), 1);
        assert_eq!(excluded[0].0, CdnCandidateExclusionReason::SampleMismatch);
        assert!(excluded[0].1.starts_with("127.0.0.1:"));
        let serialized = serde_json::to_string(&snapshot)?;
        assert!(!serialized.contains("codex_synth_v1_bearer_a"));
        assert!(!serialized.contains("/media"));
        assert!(serialized.contains("sample_mismatch"));
        Ok(())
    }

    #[allow(clippy::too_many_lines)] // One fixture verifies request IDs, partial bytes, and retry ordering.
    #[tokio::test]
    async fn partial_range_attempt_counts_before_alternate_candidate_retries() -> anyhow::Result<()>
    {
        use std::{io::Write, net::TcpListener};

        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let first_host = format!("127.0.0.1:{}", address.port());
        let body = format!("{}{}", "a".repeat(10_000), "b".repeat(10_000));
        let body_for_server = body.clone();
        let (partial_observed_sender, partial_observed_receiver) = mpsc::channel();
        let first_server = std::thread::spawn(move || -> std::io::Result<()> {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept()?;
                let request = read_request_headers(&mut stream)?;
                let range = request.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("range")
                        .then(|| value.trim().to_owned())
                });
                match range.as_deref() {
                    Some("bytes=0-16383") => {
                        stream.write_all(
                            b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-16383/20000\r\nContent-Length: 16384\r\nConnection: close\r\n\r\n",
                        )?;
                        stream.write_all(&body_for_server.as_bytes()[..16 * 1024])?;
                    }
                    Some("bytes=0-9999") => {
                        stream.write_all(
                            b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-9999/20000\r\nContent-Length: 10000\r\nConnection: close\r\n\r\n",
                        )?;
                        stream.write_all(b"aaaaaaa")?;
                        stream.flush()?;
                        partial_observed_receiver
                            .recv_timeout(Duration::from_secs(10))
                            .map_err(|error| {
                                std::io::Error::new(std::io::ErrorKind::TimedOut, error)
                            })?;
                    }
                    other => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("unexpected range request: {other:?}"),
                        ));
                    }
                }
                stream.flush()?;
            }
            Ok(())
        });
        let alternate = MockServer::start();
        let alternate_mock = add_media_server(&alternate, body.clone(), Vec::new(), None);
        let token = "codex_synth_v1_bearer_a";
        let urls = vec![
            format!("http://{address}/media?token={token}"),
            format!("{}?token={token}", alternate.url("/media")),
        ];
        let alternate_parsed = url::Url::parse(&urls[1])?;
        let alternate_host = format!(
            "{}:{}",
            alternate_parsed
                .host_str()
                .ok_or_else(|| anyhow::anyhow!("alternate URL has no host"))?,
            alternate_parsed
                .port()
                .ok_or_else(|| anyhow::anyhow!("alternate URL has no port"))?
        );
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink_events = std::sync::Arc::clone(&events);
        let partial_bytes = std::sync::Arc::new(AtomicU64::new(0));
        let partial_bytes_seen = std::sync::Arc::clone(&partial_bytes);
        let acknowledgement_sent = std::sync::Arc::new(AtomicBool::new(false));
        let acknowledgement_state = std::sync::Arc::clone(&acknowledgement_sent);
        let sink = move |event: &DownloadProgressEvent| {
            if let DownloadProgressEvent::TransferBytesReceived {
                phase: DownloadTransferPhase::RangeChunk,
                host: Some(host),
                bytes_delta,
                ..
            } = event
                && host == &first_host
            {
                let received = partial_bytes_seen
                    .fetch_add(*bytes_delta, Ordering::Relaxed)
                    .saturating_add(*bytes_delta);
                if received >= 7 && !acknowledgement_state.swap(true, Ordering::Relaxed) {
                    let _ = partial_observed_sender.send(());
                }
            }
            match sink_events.lock() {
                Ok(mut events) => events.push(event.clone()),
                Err(poisoned) => poisoned.into_inner().push(event.clone()),
            }
        };
        let kind = crate::DownloadFileKind::Video;
        let temp = tempfile::tempdir()?;
        let output_path = temp.path().join("partial-retry.m4s");
        let reporter = TransferReporter::new(
            &sink,
            Some(3),
            Some("Partial retry fixture"),
            Some(&kind),
            Some(&output_path),
        );
        let assembled = super::download_sharded_to_temp_with_progress(
            &reqwest::Client::new(),
            &urls,
            HeaderMap::new(),
            20_000,
            2,
            10_000,
            Duration::from_secs(10),
            Some(Duration::from_secs(10)),
            temp.path(),
            &reporter,
            |_, _| {},
        )
        .await?;
        first_server
            .join()
            .map_err(|_| anyhow::anyhow!("partial range server panicked"))??;
        assert_eq!(std::fs::read(assembled)?, body.as_bytes());
        assert_eq!(alternate_mock.calls(), 3);

        let events = match events.lock() {
            Ok(events) => events.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        let received_by_phase = events
            .iter()
            .filter_map(|event| match event {
                DownloadProgressEvent::TransferBytesReceived {
                    phase, bytes_delta, ..
                } => Some((*phase, *bytes_delta)),
                _ => None,
            })
            .fold(
                std::collections::BTreeMap::new(),
                |mut totals, (phase, bytes)| {
                    *totals.entry(format!("{phase:?}")).or_insert(0_u64) += bytes;
                    totals
                },
            );
        assert_eq!(received_by_phase.get("ShardProbe"), Some(&32_768));
        assert_eq!(received_by_phase.get("RangeChunk"), Some(&20_007));
        assert_eq!(received_by_phase.values().sum::<u64>(), 52_775);
        let failed_request_id = events.iter().find_map(|event| match event {
            DownloadProgressEvent::TransferDiagnostic {
                request_id: Some(request_id),
                phase: Some(DownloadTransferPhase::RangeChunk),
                diagnostic:
                    DownloadTransferDiagnostic::RequestFinished {
                        outcome:
                            DownloadTransferRequestOutcome::Failed {
                                reason: DownloadTransferFailureReason::BodyReadFailed,
                            },
                        bytes_received: 7,
                    },
                host: Some(host),
                ..
            } if host == &format!("127.0.0.1:{}", address.port()) => Some(*request_id),
            _ => None,
        });
        assert!(failed_request_id.is_some());
        let retried_request_id = events.iter().find_map(|event| match event {
            DownloadProgressEvent::TransferDiagnostic {
                request_id: Some(request_id),
                phase: Some(DownloadTransferPhase::RangeChunk),
                diagnostic:
                    DownloadTransferDiagnostic::RetryScheduled {
                        reason: DownloadTransferFailureReason::BodyReadFailed,
                    },
                host: Some(host),
                ..
            } if host == &alternate_host => Some(*request_id),
            _ => None,
        });
        assert!(retried_request_id.is_some());
        let request_ids = events
            .iter()
            .filter_map(|event| match event {
                DownloadProgressEvent::TransferDiagnostic {
                    request_id: Some(request_id),
                    diagnostic: DownloadTransferDiagnostic::RequestFinished { .. },
                    ..
                } => Some(*request_id),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(request_ids, (1..=5).collect());
        let serialized = serde_json::to_string(&events)?;
        assert!(!serialized.contains(token));
        assert!(!serialized.contains("/media"));
        Ok(())
    }

    #[tokio::test]
    async fn secondary_ranges_are_not_duplicated_on_primary_and_size_prefix_are_not_full_proof()
    -> anyhow::Result<()> {
        let canonical = MockServer::start();
        let alternate = MockServer::start();
        let canonical_body = format!("{}{}", "p".repeat(16_384), "a".repeat(3_616));
        let alternate_body = format!("{}{}", "p".repeat(16_384), "b".repeat(3_616));
        let canonical_mock = add_media_server(&canonical, canonical_body.clone(), Vec::new(), None);
        let alternate_mock = add_media_server(&alternate, alternate_body.clone(), Vec::new(), None);
        let dir = tempfile::tempdir()?;
        let urls = vec![canonical.url("/media"), alternate.url("/media")];

        let path = sharded(&urls, 20_000, dir.path(), |_, _| {}).await?;
        let expected = [
            &canonical_body.as_bytes()[..10_000],
            &alternate_body.as_bytes()[10_000..],
        ]
        .concat();
        assert_downloaded_bytes(&std::fs::read(path)?, &expected);
        assert_eq!(canonical_mock.calls(), 2);
        assert_eq!(alternate_mock.calls(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn removes_staging_file_when_all_range_sources_fail() -> anyhow::Result<()> {
        let server_a = MockServer::start();
        let server_b = MockServer::start();
        let body = "q".repeat(20_000);
        let failures = vec!["bytes=0-9999".into(), "bytes=10000-19999".into()];
        add_media_server(&server_a, body.clone(), failures.clone(), None);
        add_media_server(&server_b, body.clone(), failures, None);
        let dir = tempfile::tempdir()?;
        let urls = vec![server_a.url("/media"), server_b.url("/media")];
        assert!(matches!(
            sharded(&urls, 20_000, dir.path(), |_, _| {}).await,
            Err(crate::Error::InvalidInput(_))
        ));
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
        Ok(())
    }
}
