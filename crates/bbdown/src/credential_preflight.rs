use crate::{
    AccessKeyAutomaticRefreshReadiness, AccessKeyRenewalDecision, CredentialKind,
    CredentialLifecycleCredentialStatus, CredentialLifecycleStatus,
    CredentialProfileLifecycleStatus, PlayurlMode, RestrictedAreaConfig,
};
use serde::{Deserialize, Serialize};

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialPreflightMode {
    #[default]
    Off,
    Warn,
    Fail,
    Renew,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialPreflightRequestPath {
    WebPlayurl,
    TvPlayurl,
    AppPlayurl,
    RestrictedAreaProxy,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialPreflightRequirement {
    pub request_path: CredentialPreflightRequestPath,
    pub credential_kinds: Vec<CredentialKind>,
    pub required: bool,
}

impl CredentialPreflightRequirement {
    #[must_use]
    pub fn new(
        request_path: CredentialPreflightRequestPath,
        credential_kinds: impl IntoIterator<Item = CredentialKind>,
        required: bool,
    ) -> Self {
        Self {
            request_path,
            credential_kinds: credential_kinds.into_iter().collect(),
            required,
        }
    }

    #[must_use]
    pub fn web_playurl_cookie_optional() -> Self {
        Self::new(
            CredentialPreflightRequestPath::WebPlayurl,
            [CredentialKind::Cookie],
            false,
        )
    }

    #[must_use]
    pub fn tv_playurl_access_key() -> Self {
        Self::new(
            CredentialPreflightRequestPath::TvPlayurl,
            [CredentialKind::TvAccessKey],
            true,
        )
    }

    #[must_use]
    pub fn app_playurl_access_key() -> Self {
        Self::new(
            CredentialPreflightRequestPath::AppPlayurl,
            [CredentialKind::TvAccessKey, CredentialKind::AccessKey],
            true,
        )
    }

    #[must_use]
    pub fn restricted_area_access_key() -> Self {
        Self::new(
            CredentialPreflightRequestPath::RestrictedAreaProxy,
            [CredentialKind::AccessKey],
            true,
        )
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialPreflightRequirementStatus {
    pub request_path: CredentialPreflightRequestPath,
    pub credential_kinds: Vec<CredentialKind>,
    pub required: bool,
    pub selected_kind: Option<CredentialKind>,
    pub selected_status: CredentialLifecycleStatus,
    pub satisfied: bool,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialPreflightIssue {
    pub request_path: CredentialPreflightRequestPath,
    pub credential_kinds: Vec<CredentialKind>,
    pub selected_kind: Option<CredentialKind>,
    pub status: CredentialLifecycleStatus,
    pub required: bool,
    pub blocking: bool,
    pub message: String,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CredentialPreflightReport {
    pub mode: CredentialPreflightMode,
    pub profile: String,
    pub requirements: Vec<CredentialPreflightRequirementStatus>,
    pub issues: Vec<CredentialPreflightIssue>,
    pub access_key_renewal: AccessKeyRenewalDecision,
}

impl CredentialPreflightReport {
    #[must_use]
    pub fn evaluate(
        mode: CredentialPreflightMode,
        status: &CredentialProfileLifecycleStatus,
        requirements: impl IntoIterator<Item = CredentialPreflightRequirement>,
    ) -> Self {
        let requirements = requirements.into_iter().collect::<Vec<_>>();
        let mut requirement_statuses = Vec::with_capacity(requirements.len());
        let mut issues = Vec::new();
        for requirement in requirements {
            let evaluated = evaluate_requirement(status, &requirement);
            if let Some(issue) = preflight_issue(mode, &evaluated) {
                issues.push(issue);
            }
            requirement_statuses.push(evaluated);
        }
        Self {
            mode,
            profile: status.profile.clone(),
            requirements: requirement_statuses,
            issues,
            access_key_renewal: AccessKeyRenewalDecision::from_profile_status(status, false),
        }
    }

    #[must_use]
    pub fn from_client_context(
        mode: CredentialPreflightMode,
        status: &CredentialProfileLifecycleStatus,
        playurl_mode: PlayurlMode,
        restricted_area: &RestrictedAreaConfig,
    ) -> Self {
        Self::evaluate(
            mode,
            status,
            credential_preflight_requirements(playurl_mode, restricted_area),
        )
    }

    #[must_use]
    pub fn has_blocking_issues(&self) -> bool {
        self.issues.iter().any(|issue| issue.blocking)
    }

    #[must_use]
    pub fn should_attempt_access_key_renewal(&self) -> bool {
        self.mode == CredentialPreflightMode::Renew
            && self.access_key_renewal.requires_reauthorization()
            && self.access_key_renewal.automatic_refresh_readiness
                == AccessKeyAutomaticRefreshReadiness::Ready
            && self.issues.iter().any(|issue| {
                issue.selected_kind == Some(CredentialKind::AccessKey)
                    || issue.credential_kinds == [CredentialKind::AccessKey]
            })
    }
}

#[must_use]
pub fn credential_preflight_requirements(
    playurl_mode: PlayurlMode,
    restricted_area: &RestrictedAreaConfig,
) -> Vec<CredentialPreflightRequirement> {
    let mut requirements = match playurl_mode {
        PlayurlMode::Web => vec![CredentialPreflightRequirement::web_playurl_cookie_optional()],
        PlayurlMode::Tv => vec![CredentialPreflightRequirement::tv_playurl_access_key()],
        PlayurlMode::App => vec![CredentialPreflightRequirement::app_playurl_access_key()],
    };
    if restricted_area.area_hint.is_some() || !restricted_area.proxies.is_empty() {
        requirements.push(CredentialPreflightRequirement::restricted_area_access_key());
    }
    requirements
}

fn evaluate_requirement(
    status: &CredentialProfileLifecycleStatus,
    requirement: &CredentialPreflightRequirement,
) -> CredentialPreflightRequirementStatus {
    let selected = requirement
        .credential_kinds
        .iter()
        .copied()
        .map(|kind| (kind, credential_status(status, kind)))
        .min_by_key(|(_, status)| credential_status_rank(*status));
    let (selected_kind, selected_status) = selected.map_or(
        (None, CredentialLifecycleStatus::Missing),
        |(kind, status)| (Some(kind), status),
    );
    let satisfied = selected_status == CredentialLifecycleStatus::Fresh;
    CredentialPreflightRequirementStatus {
        request_path: requirement.request_path,
        credential_kinds: requirement.credential_kinds.clone(),
        required: requirement.required,
        selected_kind,
        selected_status,
        satisfied,
    }
}

fn credential_status(
    status: &CredentialProfileLifecycleStatus,
    kind: CredentialKind,
) -> CredentialLifecycleStatus {
    status
        .credential_statuses
        .iter()
        .find(|credential| credential.kind == kind)
        .map_or(CredentialLifecycleStatus::Missing, credential_status_value)
}

fn credential_status_value(
    credential: &CredentialLifecycleCredentialStatus,
) -> CredentialLifecycleStatus {
    credential.status
}

fn preflight_issue(
    mode: CredentialPreflightMode,
    status: &CredentialPreflightRequirementStatus,
) -> Option<CredentialPreflightIssue> {
    if status.selected_status == CredentialLifecycleStatus::Fresh
        || (!status.required && status.selected_status == CredentialLifecycleStatus::Missing)
        || mode == CredentialPreflightMode::Off
    {
        return None;
    }
    let blocking = mode == CredentialPreflightMode::Fail;
    Some(CredentialPreflightIssue {
        request_path: status.request_path,
        credential_kinds: status.credential_kinds.clone(),
        selected_kind: status.selected_kind,
        status: status.selected_status,
        required: status.required,
        blocking,
        message: credential_preflight_issue_message(status),
    })
}

fn credential_preflight_issue_message(status: &CredentialPreflightRequirementStatus) -> String {
    let requirement = credential_requirement_label(status);
    let path = credential_request_path_label(status.request_path);
    match status.selected_status {
        CredentialLifecycleStatus::Missing => format!("{path} requires {requirement}"),
        CredentialLifecycleStatus::Unknown => {
            format!("{requirement} for {path} has no lifecycle metadata")
        }
        CredentialLifecycleStatus::Stale => {
            format!("{requirement} for {path} has stale lifecycle metadata")
        }
        CredentialLifecycleStatus::Expiring => {
            format!("{requirement} for {path} expires soon")
        }
        CredentialLifecycleStatus::Expired => {
            format!("{requirement} for {path} has expired lifecycle metadata")
        }
        CredentialLifecycleStatus::Fresh => {
            format!("{requirement} for {path} is fresh")
        }
    }
}

fn credential_requirement_label(status: &CredentialPreflightRequirementStatus) -> String {
    if let Some(kind) = status.selected_kind {
        return credential_kind_label(kind).to_owned();
    }
    status
        .credential_kinds
        .iter()
        .copied()
        .map(credential_kind_label)
        .collect::<Vec<_>>()
        .join(" or ")
}

fn credential_request_path_label(path: CredentialPreflightRequestPath) -> &'static str {
    match path {
        CredentialPreflightRequestPath::WebPlayurl => "WEB playurl",
        CredentialPreflightRequestPath::TvPlayurl => "TV playurl",
        CredentialPreflightRequestPath::AppPlayurl => "APP playurl",
        CredentialPreflightRequestPath::RestrictedAreaProxy => "restricted-area proxy",
    }
}

fn credential_kind_label(kind: CredentialKind) -> &'static str {
    match kind {
        CredentialKind::Cookie => "cookie",
        CredentialKind::AccessKey => "access_key",
        CredentialKind::TvAccessKey => "tv_access_key",
    }
}

fn credential_status_rank(status: CredentialLifecycleStatus) -> u8 {
    match status {
        CredentialLifecycleStatus::Fresh => 0,
        CredentialLifecycleStatus::Unknown => 1,
        CredentialLifecycleStatus::Stale => 2,
        CredentialLifecycleStatus::Expiring => 3,
        CredentialLifecycleStatus::Expired => 4,
        CredentialLifecycleStatus::Missing => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AccessKeyProvider, AccessKeyProviderSecret, AccessKeyRefreshKeypair,
        AccessKeyRefreshProvider, CredentialLifecycleMetadata, CredentialLifecyclePolicy,
        CredentialLifecycleSource, CredentialProfileMetadata, CredentialProfileSecrets,
        CredentialProfiles, Credentials, RestrictedArea,
    };

    #[test]
    fn fail_mode_blocks_required_restricted_area_access_key() -> crate::Result<()> {
        let status = CredentialProfiles::default().profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;

        let report = CredentialPreflightReport::evaluate(
            CredentialPreflightMode::Fail,
            &status,
            [CredentialPreflightRequirement::restricted_area_access_key()],
        );

        assert!(report.has_blocking_issues());
        assert_eq!(report.issues[0].status, CredentialLifecycleStatus::Missing);
        assert_eq!(
            report.issues[0].credential_kinds,
            [CredentialKind::AccessKey]
        );
        Ok(())
    }

    #[test]
    fn app_requirement_accepts_fresh_tv_access_key_alternative() -> crate::Result<()> {
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "default",
            Credentials::default()
                .with_tv_access_key("TV_ACCESS")
                .with_access_key("ACCESS"),
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::TvAccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::TvQrLogin)
                .with_acquired_at_unix_millis(1_000),
        );
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_expires_at_unix_millis(500),
        );
        profiles.set_profile_metadata("default", metadata)?;
        let status = profiles.profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;

        let report = CredentialPreflightReport::evaluate(
            CredentialPreflightMode::Fail,
            &status,
            [CredentialPreflightRequirement::app_playurl_access_key()],
        );

        assert!(!report.has_blocking_issues());
        assert_eq!(
            report.requirements[0].selected_kind,
            Some(CredentialKind::TvAccessKey)
        );
        Ok(())
    }

    #[test]
    fn renew_mode_requests_ready_stale_access_key_refresh() -> crate::Result<()> {
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile("default", Credentials::default().with_access_key("ACCESS"))?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_access_key_provider(AccessKeyProvider::BalhBiliplus)
                .with_acquired_at_unix_millis(1_000)
                .with_refresh_token_present(true),
        );
        profiles.set_profile_metadata("default", metadata)?;
        let mut secrets = CredentialProfileSecrets::default();
        secrets.set_access_key_provider(
            AccessKeyProvider::BalhBiliplus,
            AccessKeyProviderSecret::default()
                .with_refresh_token("REFRESH")
                .with_refresh_provider(AccessKeyRefreshProvider::BilibiliMainOauth2)
                .with_refresh_keypair(AccessKeyRefreshKeypair::BiliTv),
        );
        profiles.set_profile_secrets("default", secrets)?;
        let status = profiles.profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(10_000).with_stale_after_millis(Some(1_000)),
        )?;

        let report = CredentialPreflightReport::evaluate(
            CredentialPreflightMode::Renew,
            &status,
            [CredentialPreflightRequirement::restricted_area_access_key()],
        );

        assert!(report.should_attempt_access_key_renewal());
        assert_eq!(
            report.access_key_renewal.automatic_refresh_readiness,
            AccessKeyAutomaticRefreshReadiness::Ready
        );
        Ok(())
    }

    #[test]
    fn client_context_marks_web_cookie_optional_and_restricted_area_access_key_required()
    -> crate::Result<()> {
        let status = CredentialProfiles::default().profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;
        let restricted_area = RestrictedAreaConfig::default().with_area_hint(RestrictedArea::Hk);

        let report = CredentialPreflightReport::from_client_context(
            CredentialPreflightMode::Warn,
            &status,
            PlayurlMode::Web,
            &restricted_area,
        );

        assert_eq!(report.requirements.len(), 2);
        assert!(report.issues.iter().any(|issue| {
            issue.request_path == CredentialPreflightRequestPath::RestrictedAreaProxy
                && issue.required
        }));
        assert!(!report.issues.iter().any(|issue| {
            issue.request_path == CredentialPreflightRequestPath::WebPlayurl
                && issue.status == CredentialLifecycleStatus::Missing
        }));
        Ok(())
    }
}
