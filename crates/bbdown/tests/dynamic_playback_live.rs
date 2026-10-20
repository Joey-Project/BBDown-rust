use bbdown_core::{
    BiliClient, ClientConfig, CredentialStore, Credentials, EndpointConfig, Error, Input,
    PlaybackPlan, RestrictedAreaConfig, Selection,
};
use std::{env, path::PathBuf, time::Duration};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const TOTAL_DEADLINE: Duration = Duration::from_secs(45);
const LIVE_INPUT_MID: u64 = 441_167_301;

#[derive(Clone, Copy)]
struct SafeFailure {
    stage: &'static str,
    kind: &'static str,
    status: Option<u16>,
    api_code: Option<i64>,
}

impl SafeFailure {
    const fn new(stage: &'static str, kind: &'static str) -> Self {
        Self {
            stage,
            kind,
            status: None,
            api_code: None,
        }
    }

    const fn with_status(stage: &'static str, kind: &'static str, status: Option<u16>) -> Self {
        Self {
            stage,
            kind,
            status,
            api_code: None,
        }
    }

    const fn with_api_code(stage: &'static str, code: i64) -> Self {
        Self {
            stage,
            kind: "api",
            status: None,
            api_code: Some(code),
        }
    }
}

#[tokio::test]
#[ignore = "requires an explicitly selected credential store and an opt-in live run"]
async fn legacy_space_dynamic_playback_plan_live_acceptance() -> Result<(), &'static str> {
    match run_live_acceptance().await {
        Ok(()) => Ok(()),
        Err(failure) => Err(report_safe_failure(failure)),
    }
}

fn report_safe_failure(failure: SafeFailure) -> &'static str {
    if let Some(code) = failure.api_code {
        eprintln!(
            "dynamic playback acceptance failed: stage={} kind={} api_code={code}",
            failure.stage, failure.kind
        );
    } else if let Some(status) = failure.status {
        eprintln!(
            "dynamic playback acceptance failed: stage={} kind={} http_status={status}",
            failure.stage, failure.kind
        );
    } else {
        eprintln!(
            "dynamic playback acceptance failed: stage={} kind={}",
            failure.stage, failure.kind
        );
    }
    "dynamic playback acceptance failed; see the safe stage summary above"
}

async fn run_live_acceptance() -> Result<(), SafeFailure> {
    let path = env::var_os("BBDOWN_DYNAMIC_LIVE_CREDENTIAL_FILE")
        .map(PathBuf::from)
        .ok_or(SafeFailure::new(
            "credential_store",
            "missing_explicit_path",
        ))?;
    if !path.is_file() {
        return Err(SafeFailure::new("credential_store", "file_not_found"));
    }

    let store = CredentialStore::new(path);
    let selected = match env::var("BBDOWN_DYNAMIC_LIVE_PROFILE") {
        Ok(profile) => store
            .load_profile(&profile)
            .map_err(|_| SafeFailure::new("credential_store", "load_failed"))?,
        Err(env::VarError::NotPresent) => store
            .load()
            .map_err(|_| SafeFailure::new("credential_store", "load_failed"))?,
        Err(env::VarError::NotUnicode(_)) => {
            return Err(SafeFailure::new("credential_store", "invalid_profile"));
        }
    };
    let cookie = selected
        .cookie
        .filter(|cookie| !cookie.trim().is_empty())
        .ok_or(SafeFailure::new(
            "credential_store",
            "selected_profile_has_no_cookie",
        ))?;

    // Deliberately copy only the selected Web cookie. Access keys and TV keys never enter the client.
    // The legacy client has no public request-count cap or retry switch; this single invocation is
    // bounded by the per-request timeout and the aggregate test deadline only.
    // EndpointConfig defaults to the official API and restricted-area routes are disabled. The
    // public client config has no redirect policy; production HTTP defaults remain in effect, so
    // this harness cannot block a server redirect.
    let credentials = Credentials::default().with_cookie(cookie);
    let client = BiliClient::new(
        ClientConfig::default()
            .with_endpoints(EndpointConfig::default())
            .with_credentials(credentials)
            .with_restricted_area(RestrictedAreaConfig::default())
            .with_request_timeout(REQUEST_TIMEOUT),
    );

    let plan = match tokio::time::timeout(
        TOTAL_DEADLINE,
        client.plan_playback_input(
            Input::SpaceDynamic(LIVE_INPUT_MID),
            Some(Selection::Page(1)),
        ),
    )
    .await
    {
        Err(_) => return Err(SafeFailure::new("legacy_plan", "total_deadline")),
        Ok(Err(error)) => return Err(classify_error(&error)),
        Ok(Ok(plan)) => plan,
    };

    print_safe_plan_summary(&plan)?;
    Ok(())
}

fn classify_error(error: &Error) -> SafeFailure {
    match error {
        Error::Http(http_error) => SafeFailure::with_status(
            http_error
                .url()
                .map_or("legacy_plan", |url| stage_for_path(url.path())),
            "http",
            http_error.status().map(|status| status.as_u16()),
        ),
        Error::Api { code, .. } => SafeFailure::with_api_code("legacy_plan", *code),
        Error::AccessRestricted(_) => SafeFailure::new("legacy_plan", "access_restricted"),
        Error::InvalidInput(_) => SafeFailure::new("legacy_plan", "invalid_input"),
        Error::SelectionRequired { .. } => SafeFailure::new("legacy_plan", "selection_required"),
        Error::Unsupported(_) => SafeFailure::new("legacy_plan", "unsupported"),
        Error::MissingField(_) => SafeFailure::new("legacy_plan", "missing_field"),
        Error::Url(_) => SafeFailure::new("legacy_plan", "url"),
        Error::Json(_) => SafeFailure::new("legacy_plan", "json"),
        Error::Io(_) => SafeFailure::new("legacy_plan", "io"),
        Error::MuxFailed { .. } => SafeFailure::new("legacy_plan", "mux_failed"),
        Error::Cancelled { .. } => SafeFailure::new("legacy_plan", "cancelled"),
    }
}

fn stage_for_path(path: &str) -> &'static str {
    match path {
        "/x/frontend/finger/spi" => "spi",
        "/x/web-interface/nav" => "nav",
        "/x/polymer/web-dynamic/v1/feed/space" => "feed",
        "/x/web-interface/view" => "view",
        "/x/player/playurl" => "playurl",
        _ => "legacy_plan",
    }
}

fn print_safe_plan_summary(plan: &PlaybackPlan) -> Result<(), SafeFailure> {
    if plan.entries.len() != 1 {
        return Err(SafeFailure::new("candidate", "unexpected_entry_count"));
    }
    let Some(entry) = plan.entries.first() else {
        return Err(SafeFailure::new("candidate", "empty_plan"));
    };
    if entry.source != bbdown_core::StreamSource::NormalWeb || entry.aid == 0 || entry.cid == 0 {
        return Err(SafeFailure::new("candidate", "unexpected_entry_identity"));
    }
    let Some(variant) = entry
        .variants
        .iter()
        .find(|variant| variant.video.is_some())
    else {
        return Err(SafeFailure::new("candidate", "no_video_variant"));
    };
    let video = variant
        .video
        .as_ref()
        .ok_or(SafeFailure::new("candidate", "no_video_request"))?;
    if !(video.url.starts_with("https://") || video.url.starts_with("http://")) {
        return Err(SafeFailure::new("candidate", "invalid_video_request_url"));
    }
    let duration = entry
        .duration_seconds
        .or(video.duration_seconds)
        .filter(|duration| *duration > 0)
        .ok_or(SafeFailure::new("candidate", "missing_duration"))?;
    let codec = video
        .codec_family
        .map(|family| format!("{family:?}"))
        .filter(|family| family != "Unknown" && family != "Other")
        .ok_or(SafeFailure::new("candidate", "missing_codec"))?;
    let qualities = entry
        .qualities
        .iter()
        .map(|quality| quality.id.to_string())
        .collect::<Vec<_>>()
        .join(",");

    println!(
        "dynamic playback acceptance passed: entries={} variants={} aid={} cid={} duration_seconds={} quality_ids={} video_codec_family={}",
        plan.entries.len(),
        entry.variants.len(),
        entry.aid,
        entry.cid,
        duration,
        if qualities.is_empty() {
            "none"
        } else {
            &qualities
        },
        codec
    );
    Ok(())
}
