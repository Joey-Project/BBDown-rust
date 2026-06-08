use anyhow::{Context, bail, ensure};
use assert_cmd::Command;
use bbdown_core::{CredentialStore, Credentials};
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

const LIVE_SAMPLES_FILE: &str = "live-e2e.samples.json";
const RESTRICTED_AREAS: &[&str] = &["cn", "th", "hk", "tw"];
const CLI_OVERRIDE_ENV_VARS: &[&str] = &[
    "BBDOWN_API_BASE",
    "BBDOWN_PGC_BASE",
    "BBDOWN_INTL_BASE",
    "BBDOWN_COMMENT_BASE",
    "BBDOWN_PASSPORT_BASE",
    "BBDOWN_TV_PASSPORT_BASE",
    "BBDOWN_TV_PASSPORT_POLL_BASE",
    "BBDOWN_RESTRICTED_AREA",
    "BBDOWN_RESTRICTED_AREA_PROXY",
    "BBDOWN_RESTRICTED_API_PROXY",
    "BBDOWN_CREDENTIAL_FILE",
    "BBDOWN_REQUEST_TIMEOUT_SECONDS",
    "BBDOWN_COOKIE",
    "BBDOWN_ACCESS_KEY",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveManifest {
    #[serde(default, alias = "credentials_file")]
    credential_file: Option<PathBuf>,
    #[serde(default)]
    access_key_file: Option<PathBuf>,
    #[serde(default)]
    request_timeout_seconds: Option<u64>,
    #[serde(default)]
    restricted_area_proxy: Vec<String>,
    #[serde(default)]
    restricted_api_proxy: Vec<String>,
    #[serde(default)]
    restricted_area_proxy_all_areas: Vec<String>,
    #[serde(default)]
    restricted_api_proxy_all_areas: Vec<String>,
    cases: Vec<LiveCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveCase {
    name: String,
    kind: LiveCaseKind,
    url: String,
    #[serde(default)]
    selection: Option<String>,
    #[serde(default)]
    actions: Vec<LiveAction>,
    #[serde(default, alias = "credentials_file")]
    credential_file: Option<PathBuf>,
    #[serde(default)]
    access_key_file: Option<PathBuf>,
    #[serde(default)]
    request_timeout_seconds: Option<u64>,
    #[serde(default)]
    restricted_area: Option<String>,
    #[serde(default)]
    restricted_area_proxy: Vec<String>,
    #[serde(default)]
    restricted_api_proxy: Vec<String>,
    #[serde(default)]
    restricted_area_proxy_all_areas: Vec<String>,
    #[serde(default)]
    restricted_api_proxy_all_areas: Vec<String>,
    #[serde(default)]
    expect: LiveExpect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LiveCaseKind {
    Normal,
    Pgc,
    Intl,
    RestrictedPgc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LiveAction {
    Info,
    Plan,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveExpect {
    info: Option<String>,
    #[serde(default, alias = "plan_sources")]
    allowed_plan_sources: Vec<String>,
    #[serde(default)]
    required_plan_sources: Vec<String>,
    #[serde(default)]
    allow_plan_error: bool,
    #[serde(default)]
    plan_error_contains: Vec<String>,
    min_entries: Option<usize>,
    require_streams: Option<bool>,
}

#[test]
#[ignore = "requires local live-e2e.samples.json"]
fn live_manifest_cases() -> anyhow::Result<()> {
    let manifest_path = find_live_manifest_path()?;
    let manifest = LiveManifest::load(&manifest_path)?;
    ensure!(
        !manifest.cases.is_empty(),
        "{LIVE_SAMPLES_FILE} must contain at least one live case"
    );

    for case in &manifest.cases {
        let temp = tempfile::tempdir()?;
        let credential_file = temp.path().join("credentials.json");
        write_case_credentials(&manifest, case, &manifest_path, &credential_file)
            .with_context(|| format!("failed to prepare credentials for `{}`", case.name))?;

        if case.runs(LiveAction::Info) {
            let result = run_live_command(&manifest, case, &credential_file, "info")
                .with_context(|| format!("live info failed for `{}`", case.name))?;
            ensure!(result.success, "live info failed: {}", result.stderr);
            let json = parse_live_json(&result.stdout, case, &manifest_path)?;
            assert_info(&json, case)
                .with_context(|| format!("live info assertion failed for `{}`", case.name))?;
        }

        if case.runs(LiveAction::Plan) {
            let result = run_live_command(&manifest, case, &credential_file, "plan")
                .with_context(|| format!("live plan failed for `{}`", case.name))?;
            if result.success {
                let json = parse_live_json(&result.stdout, case, &manifest_path)?;
                assert_plan(&json, case)
                    .with_context(|| format!("live plan assertion failed for `{}`", case.name))?;
            } else {
                assert_plan_error(&result.stderr, case).with_context(|| {
                    format!("live plan error assertion failed for `{}`", case.name)
                })?;
            }
        }
    }

    Ok(())
}

#[test]
fn manifest_expands_all_area_proxy_specs() -> anyhow::Result<()> {
    let manifest: LiveManifest = serde_json::from_str(
        r#"{
          "credential_file": "auth.json",
          "restricted_api_proxy_all_areas": ["https://proxy.example"],
          "cases": [{
            "name": "restricted",
            "kind": "restricted_pgc",
            "url": "ep1",
            "restricted_area_proxy_all_areas": ["https://play.example"],
            "expect": {"info": "season", "plan_error_contains": ["restricted"]}
          }]
        }"#,
    )?;
    let specs = manifest.proxy_args(&manifest.cases[0]);

    let expected = [
        ("--restricted-api-proxy", "cn=https://proxy.example"),
        ("--restricted-api-proxy", "th=https://proxy.example"),
        ("--restricted-api-proxy", "hk=https://proxy.example"),
        ("--restricted-api-proxy", "tw=https://proxy.example"),
        ("--restricted-area-proxy", "cn=https://play.example"),
        ("--restricted-area-proxy", "th=https://play.example"),
        ("--restricted-area-proxy", "hk=https://play.example"),
        ("--restricted-area-proxy", "tw=https://play.example"),
    ]
    .map(|(flag, spec)| (flag, spec.to_owned()));
    assert_eq!(specs, expected);
    Ok(())
}

#[test]
fn manifest_rejects_unknown_fields() -> anyhow::Result<()> {
    let Err(error) = serde_json::from_str::<LiveManifest>(
        r#"{
          "credential_file": "auth.json",
          "cases": [{
            "name": "restricted",
            "kind": "restricted_pgc",
            "url": "ep1",
            "expect": {"required_plan_source": ["pgc_proxy"]}
          }]
        }"#,
    ) else {
        bail!("manifest assertion typos must fail parsing");
    };

    assert!(error.to_string().contains("unknown field"));
    Ok(())
}

#[test]
fn credentials_support_telegram_auth_state_and_access_key_file() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let auth_state = temp.path().join("bilibili-auth.json");
    let access_key_file = temp.path().join("access_key.txt");
    let output = temp.path().join("credentials.json");
    fs::write(
        &auth_state,
        serde_json::json!({
            "cookie": "SESSDATA=COOKIE_SECRET",
            "mid": 1,
            "uname": "tester",
            "stored_at_unix": 1
        })
        .to_string(),
    )?;
    fs::write(&access_key_file, "ACCESS_SECRET\n")?;
    let manifest: LiveManifest = serde_json::from_str(
        r#"{
          "credential_file": "bilibili-auth.json",
          "access_key_file": "access_key.txt",
          "cases": [{"name": "normal", "kind": "normal", "url": "av1"}]
        }"#,
    )?;

    write_case_credentials(
        &manifest,
        &manifest.cases[0],
        &temp.path().join("samples.json"),
        &output,
    )?;
    let credentials = CredentialStore::new(output).load()?;

    assert_eq!(
        credentials.cookie.as_deref(),
        Some("SESSDATA=COOKIE_SECRET")
    );
    assert_eq!(credentials.access_key.as_deref(), Some("ACCESS_SECRET"));
    assert_eq!(credentials.tv_access_key, None);
    Ok(())
}

#[test]
fn plan_error_allowance_requires_explicit_restricted_access() -> anyhow::Result<()> {
    let mut case = live_case_for_assertions();
    let stderr = "Error: access restricted: restricted-area resolver failed";

    let Err(error) = assert_plan_error(stderr, &case) else {
        bail!("error allowance must be explicit");
    };
    assert!(error.to_string().contains("plan failed unexpectedly"));

    case.kind = LiveCaseKind::Normal;
    case.expect.allow_plan_error = true;
    let Err(error) = assert_plan_error(stderr, &case) else {
        bail!("only restricted PGC may allow errors");
    };
    assert!(
        error
            .to_string()
            .contains("only valid for restricted_pgc cases")
    );

    case.kind = LiveCaseKind::RestrictedPgc;
    let Err(error) = assert_plan_error("Error: network timeout", &case) else {
        bail!("allowed failures must still be access restrictions");
    };
    assert!(error.to_string().contains("only accepts access-restricted"));

    case.expect.plan_error_contains = vec!["PgcProxy area=hk Failed (API code".to_owned()];
    assert_plan_error(
        "Error: access restricted: PgcProxy area=hk Failed (API code 400)",
        &case,
    )?;
    Ok(())
}

#[test]
fn required_plan_sources_must_appear() -> anyhow::Result<()> {
    let mut case = live_case_for_assertions();
    case.expect.allowed_plan_sources = vec!["pgc_web".to_owned(), "pgc_proxy".to_owned()];
    case.expect.required_plan_sources = vec!["pgc_proxy".to_owned()];
    let json = serde_json::json!({
        "entries": [{
            "source": "pgc_web",
            "streams": {"videos": [{"id": 80}], "flv_segments": []}
        }]
    });

    let Err(error) = assert_plan(&json, &case) else {
        bail!("required source must be enforced");
    };
    assert!(
        error
            .to_string()
            .contains("expected at least one plan entry from source `pgc_proxy`")
    );

    case.expect.required_plan_sources = vec!["pgc_web".to_owned()];
    assert_plan(&json, &case)?;
    Ok(())
}

fn live_case_for_assertions() -> LiveCase {
    LiveCase {
        name: "assertion".to_owned(),
        kind: LiveCaseKind::RestrictedPgc,
        url: "ep1".to_owned(),
        selection: None,
        actions: Vec::new(),
        credential_file: None,
        access_key_file: None,
        request_timeout_seconds: None,
        restricted_area: None,
        restricted_area_proxy: Vec::new(),
        restricted_api_proxy: Vec::new(),
        restricted_area_proxy_all_areas: Vec::new(),
        restricted_api_proxy_all_areas: Vec::new(),
        expect: LiveExpect {
            info: None,
            allowed_plan_sources: Vec::new(),
            required_plan_sources: Vec::new(),
            allow_plan_error: false,
            plan_error_contains: Vec::new(),
            min_entries: None,
            require_streams: None,
        },
    }
}

impl LiveManifest {
    fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
    }

    fn proxy_args(&self, case: &LiveCase) -> Vec<(&'static str, String)> {
        let mut args = Vec::new();
        push_proxy_args(
            &mut args,
            "--restricted-area-proxy",
            &self.restricted_area_proxy,
            &self.restricted_area_proxy_all_areas,
        );
        push_proxy_args(
            &mut args,
            "--restricted-api-proxy",
            &self.restricted_api_proxy,
            &self.restricted_api_proxy_all_areas,
        );
        push_proxy_args(
            &mut args,
            "--restricted-area-proxy",
            &case.restricted_area_proxy,
            &case.restricted_area_proxy_all_areas,
        );
        push_proxy_args(
            &mut args,
            "--restricted-api-proxy",
            &case.restricted_api_proxy,
            &case.restricted_api_proxy_all_areas,
        );
        args
    }
}

impl LiveCase {
    fn runs(&self, action: LiveAction) -> bool {
        self.actions.is_empty() || self.actions.contains(&action)
    }

    fn default_info_kind(&self) -> &'static str {
        match self.kind {
            LiveCaseKind::Normal => "video",
            LiveCaseKind::Pgc | LiveCaseKind::Intl | LiveCaseKind::RestrictedPgc => "season",
        }
    }

    fn request_timeout_seconds(&self, manifest: &LiveManifest) -> Option<u64> {
        self.request_timeout_seconds
            .or(manifest.request_timeout_seconds)
    }
}

fn run_live_command(
    manifest: &LiveManifest,
    case: &LiveCase,
    credential_file: &Path,
    command_name: &str,
) -> anyhow::Result<LiveCommandResult> {
    let mut command = live_command(manifest, case, credential_file)?;
    command.arg(command_name).arg(&case.url);
    if let Some(selection) = &case.selection {
        command.args(["--select", selection]);
    }
    command.arg("--json");

    let output = command.output()?;
    Ok(LiveCommandResult {
        success: output.status.success(),
        stdout: output.stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

struct LiveCommandResult {
    success: bool,
    stdout: Vec<u8>,
    stderr: String,
}

fn parse_live_json(output: &[u8], case: &LiveCase, manifest_path: &Path) -> anyhow::Result<Value> {
    serde_json::from_slice(output).with_context(|| {
        format!(
            "failed to parse JSON for `{}` from {}",
            case.name,
            manifest_path.display()
        )
    })
}

fn live_command(
    manifest: &LiveManifest,
    case: &LiveCase,
    credential_file: &Path,
) -> anyhow::Result<Command> {
    let mut command = Command::cargo_bin("bbdown")?;
    for name in CLI_OVERRIDE_ENV_VARS {
        command.env_remove(name);
    }
    command.arg("--credential-file").arg(credential_file);
    if let Some(seconds) = case.request_timeout_seconds(manifest) {
        command
            .arg("--request-timeout-seconds")
            .arg(seconds.to_string());
    }
    if let Some(area) = &case.restricted_area {
        command.arg("--restricted-area").arg(area);
    }
    for (flag, spec) in manifest.proxy_args(case) {
        command.arg(flag).arg(spec);
    }
    Ok(command)
}

fn assert_info(json: &Value, case: &LiveCase) -> anyhow::Result<()> {
    let expected = case
        .expect
        .info
        .as_deref()
        .unwrap_or_else(|| case.default_info_kind());
    match expected {
        "video" => ensure!(json.get("video").is_some(), "expected video info object"),
        "season" => ensure!(json.get("season").is_some(), "expected season info object"),
        other => bail!("unsupported expected info kind `{other}`"),
    }
    Ok(())
}

fn assert_plan(json: &Value, case: &LiveCase) -> anyhow::Result<()> {
    let entries = json["entries"]
        .as_array()
        .with_context(|| "plan JSON must contain entries array")?;
    let min_entries = case.expect.min_entries.unwrap_or(1);
    ensure!(
        entries.len() >= min_entries,
        "expected at least {min_entries} entries, got {}",
        entries.len()
    );

    if !case.expect.allowed_plan_sources.is_empty() {
        for entry in entries {
            let source = entry["source"]
                .as_str()
                .with_context(|| "plan entry must contain source")?;
            ensure!(
                case.expect
                    .allowed_plan_sources
                    .iter()
                    .any(|expected| expected == source),
                "unexpected plan source `{source}`; expected one of {:?}",
                case.expect.allowed_plan_sources
            );
        }
    }
    for expected in &case.expect.required_plan_sources {
        ensure!(
            entries
                .iter()
                .filter_map(|entry| entry["source"].as_str())
                .any(|source| source == expected),
            "expected at least one plan entry from source `{expected}`"
        );
    }

    if case.expect.require_streams.unwrap_or(true) {
        for entry in entries {
            let streams = &entry["streams"];
            let has_dash_video = streams["videos"]
                .as_array()
                .is_some_and(|videos| !videos.is_empty());
            let has_flv = streams["flv_segments"]
                .as_array()
                .is_some_and(|segments| !segments.is_empty());
            ensure!(
                has_dash_video || has_flv,
                "plan entry must contain at least one video or FLV stream"
            );
        }
    }

    Ok(())
}

fn assert_plan_error(stderr: &str, case: &LiveCase) -> anyhow::Result<()> {
    ensure!(
        case.expect.allow_plan_error,
        "plan failed unexpectedly: {stderr}"
    );
    ensure!(
        case.kind == LiveCaseKind::RestrictedPgc,
        "plan error allowance is only valid for restricted_pgc cases"
    );
    ensure!(
        stderr.contains("access restricted"),
        "plan error allowance only accepts access-restricted failures: {stderr}"
    );
    ensure!(
        !case.expect.plan_error_contains.is_empty(),
        "plan failed unexpectedly: {stderr}"
    );
    for expected in &case.expect.plan_error_contains {
        ensure!(
            stderr.contains(expected),
            "plan error did not contain `{expected}`: {stderr}"
        );
    }
    Ok(())
}

fn write_case_credentials(
    manifest: &LiveManifest,
    case: &LiveCase,
    manifest_path: &Path,
    output_path: &Path,
) -> anyhow::Result<()> {
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let mut credentials = Credentials::default();

    if let Some(path) = case
        .credential_file
        .as_ref()
        .or(manifest.credential_file.as_ref())
    {
        let path = resolve_manifest_path(manifest_dir, path);
        merge_credentials_file(&mut credentials, &path)
            .with_context(|| format!("failed to load credential file {}", path.display()))?;
    }
    if let Some(path) = case
        .access_key_file
        .as_ref()
        .or(manifest.access_key_file.as_ref())
    {
        let path = resolve_manifest_path(manifest_dir, path);
        let access_key = fs::read_to_string(&path)
            .with_context(|| format!("failed to read access key file {}", path.display()))?;
        let access_key = access_key.trim();
        if !access_key.is_empty() {
            credentials.access_key = Some(access_key.to_owned());
        }
    }

    CredentialStore::new(output_path.to_owned()).save(&credentials)?;
    Ok(())
}

fn merge_credentials_file(credentials: &mut Credentials, path: &Path) -> anyhow::Result<()> {
    let raw = fs::read_to_string(path)?;
    let json: Value = serde_json::from_str(&raw)?;
    let mut matched = false;

    if let Some(cookie) = json["cookie"].as_str().filter(|value| !value.is_empty()) {
        credentials.cookie = Some(cookie.to_owned());
        matched = true;
    }
    if let Some(access_key) = json["access_key"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        credentials.access_key = Some(access_key.to_owned());
        matched = true;
    }
    if let Some(tv_access_key) = json["tv_access_key"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        credentials.tv_access_key = Some(tv_access_key.to_owned());
        matched = true;
    }

    ensure!(
        matched,
        "credential file must contain at least one of cookie, access_key, or tv_access_key"
    );
    Ok(())
}

fn push_proxy_args(
    args: &mut Vec<(&'static str, String)>,
    flag: &'static str,
    specs: &[String],
    all_area_specs: &[String],
) {
    args.extend(specs.iter().cloned().map(|spec| (flag, spec)));
    for spec in all_area_specs {
        args.extend(
            RESTRICTED_AREAS
                .iter()
                .map(|area| (flag, format!("{area}={spec}"))),
        );
    }
}

fn resolve_manifest_path(manifest_dir: &Path, path: &Path) -> PathBuf {
    let expanded = expand_home_path(path);
    if expanded.is_absolute() {
        expanded
    } else {
        manifest_dir.join(expanded)
    }
}

fn find_live_manifest_path() -> anyhow::Result<PathBuf> {
    let mut starts = vec![std::env::current_dir()?];
    starts.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));

    for mut dir in starts {
        loop {
            let candidate = dir.join(LIVE_SAMPLES_FILE);
            if candidate.exists() {
                return Ok(candidate);
            }
            if !dir.pop() {
                break;
            }
        }
    }

    bail!("{LIVE_SAMPLES_FILE} was not found in the current directory or parent directories")
}

fn expand_home_path(path: &Path) -> PathBuf {
    let path_text = path.to_string_lossy();
    if path_text == "~" {
        return std::env::var_os("HOME").map_or_else(|| path.to_path_buf(), PathBuf::from);
    }
    if let Some(rest) = path_text.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    path.to_path_buf()
}
