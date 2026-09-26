use std::{
    future::Future,
    io::{Seek, SeekFrom, Write},
    path::Path,
    pin::Pin,
    time::{Duration, Instant},
};

use futures_util::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, HeaderMap, RANGE};

use crate::{Error, Result};

const MAX_RANGE_BYTES: u64 = 8 * 1024 * 1024;
const CANDIDATE_PROBE_BYTES: u64 = 16 * 1024;
const MAX_CDN_CANDIDATES: usize = 8;

type ChunkFuture<'a> = Pin<Box<dyn Future<Output = (u64, usize, usize, Result<RangeFetch>)> + 'a>>;

#[derive(Debug)]
pub(crate) struct RangeFetch {
    pub(crate) bytes: Vec<u8>,
    pub(crate) total_size: u64,
    pub(crate) elapsed: Duration,
}

/// Fetches one bounded byte range and validates the server's complete range claim.
/// Errors deliberately omit the request URL because media URLs may contain signatures.
#[allow(clippy::too_many_arguments)] // Keep the range contract explicit at its call sites.
pub(crate) async fn fetch_range(
    client: &reqwest::Client,
    url: &str,
    headers: HeaderMap,
    start: u64,
    end_inclusive: u64,
    expected_total: Option<u64>,
    request_timeout: Duration,
    idle_timeout: Duration,
) -> Result<RangeFetch> {
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
            let next = tokio::time::timeout(idle_timeout, stream.next())
                .await
                .map_err(|_| invalid("range response stalled"))?;
            let Some(chunk) = next else { break };
            let chunk = chunk.map_err(|_| invalid("range response body failed"))?;
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
/// Candidate URLs must match the probe, and every non-canonical chunk is checked against the
/// canonical candidate before it is written.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(crate) async fn download_sharded_to_temp<F>(
    client: &reqwest::Client,
    urls: &[String],
    headers: HeaderMap,
    expected_total: u64,
    concurrency: usize,
    chunk_size: u64,
    request_timeout: Duration,
    idle_timeout: Duration,
    dest_dir: &Path,
    mut on_chunk: F,
) -> Result<tempfile::TempPath>
where
    F: FnMut(u64),
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
    let probe_timeout = request_timeout.min(Duration::from_secs(2));
    let mut probes = FuturesUnordered::new();
    for (index, url) in urls.iter().take(MAX_CDN_CANDIDATES).enumerate() {
        let probe_headers = headers.clone();
        probes.push(async move {
            let result = fetch_range(
                client,
                url,
                probe_headers,
                0,
                probe_end,
                Some(expected_total),
                probe_timeout,
                idle_timeout,
            )
            .await;
            (index, url.as_str(), result)
        });
    }
    let mut completed_probes = Vec::new();
    while let Some(probe) = probes.next().await {
        completed_probes.push(probe);
    }
    completed_probes.sort_by_key(|(index, _, _)| *index);

    let mut candidate_groups: Vec<Vec<(&str, Vec<u8>)>> = Vec::new();
    for (_, url, probe_result) in completed_probes {
        if let Ok(probe) = probe_result {
            if let Some(group) = candidate_groups
                .iter_mut()
                .find(|group| group[0].1 == probe.bytes)
            {
                group.push((url, probe.bytes));
            } else {
                candidate_groups.push(vec![(url, probe.bytes)]);
            }
        }
    }
    if candidate_groups.is_empty() {
        return Err(invalid("no CDN candidate passed the range probe"));
    }
    let mut largest_group_index = 0;
    for index in 1..candidate_groups.len() {
        if candidate_groups[index].len() > candidate_groups[largest_group_index].len() {
            largest_group_index = index;
        }
    }
    if candidate_groups[largest_group_index].len() < 2 {
        return Err(invalid("fewer than two compatible CDN candidates remain"));
    }
    let candidates: Vec<&str> = candidate_groups
        .swap_remove(largest_group_index)
        .into_iter()
        .map(|(url, _)| url)
        .collect();
    let canonical_source = 0;

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
            next_chunk,
            lane,
            preferred_source,
            canonical_source,
        ));
        next_chunk += 1;
    }

    while let Some((chunk_index, lane, successful_source, result)) = pending.next().await {
        let fetched = result?;
        lane_sources[lane] = successful_source;
        let offset = chunk_index * chunk_size;
        staging.as_file_mut().seek(SeekFrom::Start(offset))?;
        staging.as_file_mut().write_all(&fetched.bytes)?;
        on_chunk(fetched.bytes.len() as u64);

        if next_chunk < chunk_count {
            pending.push(schedule_chunk(
                client,
                &candidates,
                headers.clone(),
                expected_total,
                chunk_size,
                request_timeout,
                idle_timeout,
                next_chunk,
                lane,
                lane_sources[lane],
                canonical_source,
            ));
            next_chunk += 1;
        }
    }

    staging.as_file_mut().flush()?;
    Ok(staging.into_temp_path())
}

#[allow(clippy::too_many_arguments)] // This mirrors the explicit Range request context.
fn schedule_chunk<'a>(
    client: &'a reqwest::Client,
    candidates: &'a [&'a str],
    headers: HeaderMap,
    expected_total: u64,
    chunk_size: u64,
    request_timeout: Duration,
    idle_timeout: Duration,
    chunk_index: u64,
    lane: usize,
    preferred_source: usize,
    canonical_source: usize,
) -> BoxFuture<'a, (u64, usize, usize, Result<RangeFetch>)> {
    async move {
        let start = chunk_index * chunk_size;
        let end = expected_total.min(start.saturating_add(chunk_size)) - 1;
        let candidate_index = preferred_source % candidates.len();
        let candidate = candidates[candidate_index];
        if candidate_index == canonical_source {
            let fetched = fetch_range(
                client,
                candidate,
                headers.clone(),
                start,
                end,
                Some(expected_total),
                request_timeout,
                idle_timeout,
            )
            .await;
            return (chunk_index, lane, candidate_index, fetched);
        }

        let (fetched, canonical) = tokio::join!(
            fetch_range(
                client,
                candidate,
                headers.clone(),
                start,
                end,
                Some(expected_total),
                request_timeout,
                idle_timeout,
            ),
            fetch_range(
                client,
                candidates[canonical_source],
                headers.clone(),
                start,
                end,
                Some(expected_total),
                request_timeout,
                idle_timeout,
            ),
        );
        let canonical = match canonical {
            Ok(canonical) => canonical,
            Err(error) => return (chunk_index, lane, candidate_index, Err(error)),
        };
        let Ok(fetched) = fetched else {
            return (chunk_index, lane, canonical_source, Ok(canonical));
        };
        if canonical.bytes != fetched.bytes {
            return (
                chunk_index,
                lane,
                candidate_index,
                Err(invalid("CDN byte range does not match canonical candidate")),
            );
        }
        (chunk_index, lane, candidate_index, Ok(fetched))
    }
    .boxed()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use httpmock::prelude::*;
    use httpmock::{HttpMockRequest, HttpMockResponse};
    use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, HeaderMap};

    use super::fetch_range;

    async fn fetch(server: &MockServer) -> crate::Result<super::RangeFetch> {
        fetch_range(
            &reqwest::Client::new(),
            &server.url("/asset?token=secret"),
            HeaderMap::new(),
            2,
            4,
            Some(10),
            Duration::from_secs(2),
            Duration::from_secs(2),
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
            Duration::from_secs(1),
        )
        .await;
        assert!(matches!(
            result,
            Err(crate::Error::InvalidInput(message))
                if message.contains("timed out") && !message.contains("secret")
        ));
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
        on_chunk: impl FnMut(u64),
    ) -> crate::Result<tempfile::TempPath> {
        super::download_sharded_to_temp(
            &reqwest::Client::new(),
            urls,
            HeaderMap::new(),
            expected_total,
            2,
            10_000,
            Duration::from_secs(2),
            Duration::from_secs(2),
            dest_dir,
            on_chunk,
        )
        .await
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
            Duration::from_secs(2),
            dir.path(),
            |bytes| completed.push(bytes),
        )
        .await?;
        assert_eq!(std::fs::read(path)?, body.as_bytes());
        assert_eq!(completed.iter().sum::<u64>(), 20_000);
        assert_eq!(completed.len(), 2);
        mock_a.assert_calls(3);
        mock_b.assert_calls(2);
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
        let path = sharded(&urls, 20_000, dir.path(), |_| {}).await?;

        assert_eq!(std::fs::read(path)?, expected.as_bytes());
        assert_eq!(outlier_mock.calls(), 1);
        assert_eq!(second_cdn_mock.calls(), 3);
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
            Duration::from_secs(2),
            dir.path(),
            |_| {},
        )
        .await?;

        assert_eq!(std::fs::read(path)?, body.as_bytes());
        assert!(began.elapsed() < Duration::from_millis(500));
        Ok(())
    }

    #[tokio::test]
    async fn excludes_candidates_with_different_probe_bytes_or_total() -> anyhow::Result<()> {
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
        let result = sharded(&urls, 20_000, dir.path(), |_| {}).await;
        assert!(matches!(
            result,
            Err(crate::Error::InvalidInput(message))
                if message.contains("fewer than two compatible CDN candidates")
        ));
        assert_eq!(good_mock.calls(), 1);
        assert_eq!(different_mock.calls(), 1);
        assert_eq!(wrong_total_mock.calls(), 1);
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
        let path = sharded(&urls, 20_000, dir.path(), |_| {}).await?;
        assert_eq!(std::fs::read(path)?, body.as_bytes());
        assert_eq!(canonical_mock.calls(), 3);
        assert_eq!(alternate_mock.calls(), 2);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_later_chunk_mismatch_and_cleans_staging_file() -> anyhow::Result<()> {
        let canonical = MockServer::start();
        let alternate = MockServer::start();
        let canonical_body = format!("{}{}", "p".repeat(16_384), "a".repeat(3_616));
        let alternate_body = format!("{}{}", "p".repeat(16_384), "b".repeat(3_616));
        let canonical_mock = add_media_server(&canonical, canonical_body, Vec::new(), None);
        let alternate_mock = add_media_server(&alternate, alternate_body, Vec::new(), None);
        let dir = tempfile::tempdir()?;
        let urls = vec![canonical.url("/media"), alternate.url("/media")];

        let result = sharded(&urls, 20_000, dir.path(), |_| {}).await;

        assert!(
            matches!(
                &result,
                Err(crate::Error::InvalidInput(message))
                    if message.contains("does not match canonical candidate")
            ),
            "unexpected sharded result: {result:?}"
        );
        assert_eq!(canonical_mock.calls(), 3);
        assert_eq!(alternate_mock.calls(), 2);
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
        Ok(())
    }

    #[tokio::test]
    async fn rejects_alternate_chunk_when_canonical_verification_fails() -> anyhow::Result<()> {
        let canonical = MockServer::start();
        let alternate = MockServer::start();
        let body = "v".repeat(20_000);
        let canonical_mock = add_media_server(
            &canonical,
            body.clone(),
            vec!["bytes=10000-19999".into()],
            None,
        );
        let alternate_mock = add_media_server(&alternate, body, Vec::new(), None);
        let dir = tempfile::tempdir()?;
        let urls = vec![canonical.url("/media"), alternate.url("/media")];

        let result = sharded(&urls, 20_000, dir.path(), |_| {}).await;

        assert!(matches!(result, Err(crate::Error::InvalidInput(_))));
        assert_eq!(canonical_mock.calls(), 3);
        assert_eq!(alternate_mock.calls(), 2);
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
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
            sharded(&urls, 20_000, dir.path(), |_| {}).await,
            Err(crate::Error::InvalidInput(_))
        ));
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
        Ok(())
    }
}
