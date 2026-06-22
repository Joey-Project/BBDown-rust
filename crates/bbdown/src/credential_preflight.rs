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
    IntlWeb,
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
            [CredentialKind::AccessKey, CredentialKind::TvAccessKey],
            true,
        )
    }

    #[must_use]
    pub fn restricted_area_access_key() -> Self {
        Self::new(
            CredentialPreflightRequestPath::RestrictedAreaProxy,
            [CredentialKind::AccessKey],
            false,
        )
    }

    #[must_use]
    pub fn intl_access_key() -> Self {
        Self::new(
            CredentialPreflightRequestPath::IntlWeb,
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
    pub fn from_media_request_context(
        mode: CredentialPreflightMode,
        status: &CredentialProfileLifecycleStatus,
        playurl_mode: PlayurlMode,
        restricted_area: &RestrictedAreaConfig,
        restricted_area_proxy_may_run: bool,
        intl_access_key_may_run: bool,
    ) -> Self {
        Self::evaluate(
            mode,
            status,
            credential_preflight_requirements_for_media_request(
                playurl_mode,
                restricted_area,
                restricted_area_proxy_may_run,
                intl_access_key_may_run,
            ),
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
    credential_preflight_requirements_for_media_request(playurl_mode, restricted_area, true, false)
}

#[must_use]
pub fn credential_preflight_requirements_for_media_request(
    playurl_mode: PlayurlMode,
    restricted_area: &RestrictedAreaConfig,
    restricted_area_proxy_may_run: bool,
    intl_access_key_may_run: bool,
) -> Vec<CredentialPreflightRequirement> {
    credential_preflight_requirements_for_media_paths(
        Some(playurl_mode),
        restricted_area,
        restricted_area_proxy_may_run,
        intl_access_key_may_run,
    )
}

#[must_use]
pub fn credential_preflight_requirements_for_media_paths(
    playurl_mode: Option<PlayurlMode>,
    restricted_area: &RestrictedAreaConfig,
    restricted_area_proxy_may_run: bool,
    intl_access_key_may_run: bool,
) -> Vec<CredentialPreflightRequirement> {
    let mut requirements = match playurl_mode {
        Some(PlayurlMode::Web) => {
            vec![CredentialPreflightRequirement::web_playurl_cookie_optional()]
        }
        Some(PlayurlMode::Tv) => vec![CredentialPreflightRequirement::tv_playurl_access_key()],
        Some(PlayurlMode::App) => vec![CredentialPreflightRequirement::app_playurl_access_key()],
        None => Vec::new(),
    };
    if intl_access_key_may_run {
        requirements.push(CredentialPreflightRequirement::intl_access_key());
    }
    if playurl_mode != Some(PlayurlMode::Tv)
        && restricted_area_proxy_may_run
        && !restricted_area.proxies.is_empty()
    {
        requirements.push(CredentialPreflightRequirement::restricted_area_access_key());
    }
    requirements
}

fn evaluate_requirement(
    status: &CredentialProfileLifecycleStatus,
    requirement: &CredentialPreflightRequirement,
) -> CredentialPreflightRequirementStatus {
    let selected = selected_credential(status, requirement);
    let (selected_kind, selected_status) = selected.map_or(
        (None, CredentialLifecycleStatus::Missing),
        |(kind, status)| (Some(kind), status),
    );
    let satisfied = selected_status == CredentialLifecycleStatus::Fresh
        || (!requirement.required && selected_status == CredentialLifecycleStatus::Missing);
    CredentialPreflightRequirementStatus {
        request_path: requirement.request_path,
        credential_kinds: requirement.credential_kinds.clone(),
        required: requirement.required,
        selected_kind,
        selected_status,
        satisfied,
    }
}

fn selected_credential(
    status: &CredentialProfileLifecycleStatus,
    requirement: &CredentialPreflightRequirement,
) -> Option<(CredentialKind, CredentialLifecycleStatus)> {
    if requirement.request_path == CredentialPreflightRequestPath::AppPlayurl {
        return requirement
            .credential_kinds
            .iter()
            .copied()
            .find(|kind| credential_present(status, *kind))
            .map(|kind| (kind, credential_status(status, kind)))
            .or_else(|| fallback_credential(status, requirement));
    }
    fallback_credential(status, requirement)
}

fn fallback_credential(
    status: &CredentialProfileLifecycleStatus,
    requirement: &CredentialPreflightRequirement,
) -> Option<(CredentialKind, CredentialLifecycleStatus)> {
    requirement
        .credential_kinds
        .iter()
        .copied()
        .map(|kind| (kind, credential_status(status, kind)))
        .filter(|(_, status)| *status != CredentialLifecycleStatus::Missing)
        .min_by_key(|(_, status)| credential_status_rank(*status))
}

fn credential_present(status: &CredentialProfileLifecycleStatus, kind: CredentialKind) -> bool {
    status
        .credential_statuses
        .iter()
        .find(|credential| credential.kind == kind)
        .is_some_and(|credential| credential.present)
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
    let blocking = mode == CredentialPreflightMode::Fail
        && (status.required || status.selected_status != CredentialLifecycleStatus::Missing);
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
        CredentialPreflightRequestPath::IntlWeb => "intl playurl",
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
        CredentialProfiles, Credentials, RestrictedArea, RestrictedAreaProxy,
    };

    #[test]
    fn fail_mode_treats_missing_restricted_area_access_key_as_optional() -> crate::Result<()> {
        let status = CredentialProfiles::default().profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;

        let report = CredentialPreflightReport::evaluate(
            CredentialPreflightMode::Fail,
            &status,
            [CredentialPreflightRequirement::restricted_area_access_key()],
        );

        assert!(!report.has_blocking_issues());
        assert!(report.issues.is_empty());
        assert!(report.requirements[0].satisfied);
        assert!(!report.requirements[0].required);
        assert_eq!(
            report.requirements[0].credential_kinds,
            [CredentialKind::AccessKey]
        );
        Ok(())
    }

    #[test]
    fn fail_mode_blocks_required_intl_access_key() -> crate::Result<()> {
        let status = CredentialProfiles::default().profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;

        let report = CredentialPreflightReport::evaluate(
            CredentialPreflightMode::Fail,
            &status,
            [CredentialPreflightRequirement::intl_access_key()],
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
    fn media_paths_context_can_check_intl_without_web_cookie() {
        let requirements = credential_preflight_requirements_for_media_paths(
            None,
            &RestrictedAreaConfig::default(),
            false,
            true,
        );

        assert_eq!(requirements.len(), 1);
        assert_eq!(
            requirements[0].request_path,
            CredentialPreflightRequestPath::IntlWeb
        );
        assert_eq!(
            requirements[0].credential_kinds,
            [CredentialKind::AccessKey]
        );
    }

    #[test]
    fn fail_mode_treats_missing_optional_web_cookie_as_satisfied() -> crate::Result<()> {
        let status = CredentialProfiles::default().profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;

        let report = CredentialPreflightReport::evaluate(
            CredentialPreflightMode::Fail,
            &status,
            [CredentialPreflightRequirement::web_playurl_cookie_optional()],
        );

        assert!(!report.has_blocking_issues());
        assert!(report.issues.is_empty());
        assert_eq!(
            report.requirements[0].selected_status,
            CredentialLifecycleStatus::Missing
        );
        assert!(report.requirements[0].satisfied);
        Ok(())
    }

    #[test]
    fn fail_mode_blocks_non_fresh_optional_web_cookie() -> crate::Result<()> {
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "default",
            Credentials::default().with_cookie("SESSDATA=COOKIE"),
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::Cookie,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::WebQrLogin)
                .with_checked_at_unix_millis(1),
        );
        profiles.set_profile_metadata("default", metadata)?;
        let status = profiles.profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(10_000).with_stale_after_millis(Some(1_000)),
        )?;

        let report = CredentialPreflightReport::evaluate(
            CredentialPreflightMode::Fail,
            &status,
            [CredentialPreflightRequirement::web_playurl_cookie_optional()],
        );

        assert!(report.has_blocking_issues());
        assert_eq!(report.issues[0].status, CredentialLifecycleStatus::Stale);
        assert!(report.issues[0].blocking);
        Ok(())
    }

    #[test]
    fn fail_mode_blocks_non_fresh_optional_restricted_area_access_key() -> crate::Result<()> {
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile("default", Credentials::default().with_access_key("ACCESS"))?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_checked_at_unix_millis(1),
        );
        profiles.set_profile_metadata("default", metadata)?;
        let status = profiles.profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(10_000).with_stale_after_millis(Some(1_000)),
        )?;

        let report = CredentialPreflightReport::evaluate(
            CredentialPreflightMode::Fail,
            &status,
            [CredentialPreflightRequirement::restricted_area_access_key()],
        );

        assert!(report.has_blocking_issues());
        assert_eq!(report.issues[0].status, CredentialLifecycleStatus::Stale);
        assert!(report.issues[0].blocking);
        Ok(())
    }

    #[test]
    fn app_requirement_reports_all_alternatives_when_missing() -> crate::Result<()> {
        let status = CredentialProfiles::default().profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;

        let report = CredentialPreflightReport::evaluate(
            CredentialPreflightMode::Fail,
            &status,
            [CredentialPreflightRequirement::app_playurl_access_key()],
        );

        assert!(report.has_blocking_issues());
        assert_eq!(report.requirements[0].selected_kind, None);
        assert_eq!(
            report.requirements[0].selected_status,
            CredentialLifecycleStatus::Missing
        );
        assert!(
            report.issues[0]
                .message
                .contains("access_key or tv_access_key")
        );
        Ok(())
    }

    #[test]
    fn app_requirement_accepts_fresh_tv_access_key_when_generic_key_is_absent() -> crate::Result<()>
    {
        let mut profiles = CredentialProfiles::default();
        profiles.set_profile(
            "default",
            Credentials::default().with_tv_access_key("TV_ACCESS"),
        )?;
        let mut metadata = CredentialProfileMetadata::default();
        metadata.set_credential(
            CredentialKind::TvAccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::TvQrLogin)
                .with_acquired_at_unix_millis(1_000),
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
    fn app_requirement_checks_present_generic_access_key_before_fresh_tv_access_key()
    -> crate::Result<()> {
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
                .with_acquired_at_unix_millis(10_000),
        );
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_checked_at_unix_millis(1),
        );
        profiles.set_profile_metadata("default", metadata)?;
        let status = profiles.profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(10_000).with_stale_after_millis(Some(1_000)),
        )?;

        let report = CredentialPreflightReport::evaluate(
            CredentialPreflightMode::Fail,
            &status,
            [CredentialPreflightRequirement::app_playurl_access_key()],
        );

        assert!(report.has_blocking_issues());
        assert_eq!(
            report.requirements[0].selected_kind,
            Some(CredentialKind::AccessKey)
        );
        assert_eq!(
            report.requirements[0].selected_status,
            CredentialLifecycleStatus::Stale
        );
        Ok(())
    }

    #[test]
    fn renew_mode_refreshes_stale_generic_app_access_key_when_tv_key_is_also_stale()
    -> crate::Result<()> {
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
                .with_checked_at_unix_millis(1),
        );
        metadata.set_credential(
            CredentialKind::AccessKey,
            CredentialLifecycleMetadata::default()
                .with_source(CredentialLifecycleSource::AccessKeyLogin)
                .with_access_key_provider(AccessKeyProvider::BalhBiliplus)
                .with_checked_at_unix_millis(1)
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
            [CredentialPreflightRequirement::app_playurl_access_key()],
        );

        assert!(report.should_attempt_access_key_renewal());
        assert_eq!(
            report.requirements[0].selected_kind,
            Some(CredentialKind::AccessKey)
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
    fn client_context_marks_web_cookie_and_restricted_area_proxy_access_key_optional()
    -> crate::Result<()> {
        let status = CredentialProfiles::default().profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;
        let restricted_area = RestrictedAreaConfig::default()
            .with_area_hint(RestrictedArea::Hk)
            .with_proxy(RestrictedAreaProxy::playurl(
                "https://proxy.example/playurl",
                Some(RestrictedArea::Hk),
            ));

        let report = CredentialPreflightReport::from_client_context(
            CredentialPreflightMode::Warn,
            &status,
            PlayurlMode::Web,
            &restricted_area,
        );

        assert_eq!(report.requirements.len(), 2);
        assert!(report.requirements.iter().any(|requirement| {
            requirement.request_path == CredentialPreflightRequestPath::RestrictedAreaProxy
                && !requirement.required
                && requirement.satisfied
        }));
        assert!(!report.issues.iter().any(|issue| {
            issue.request_path == CredentialPreflightRequestPath::WebPlayurl
                && issue.status == CredentialLifecycleStatus::Missing
        }));
        Ok(())
    }

    #[test]
    fn client_context_does_not_require_access_key_for_restricted_area_hint_without_proxy()
    -> crate::Result<()> {
        let status = CredentialProfiles::default().profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;
        let restricted_area = RestrictedAreaConfig::default().with_area_hint(RestrictedArea::Hk);

        let report = CredentialPreflightReport::from_client_context(
            CredentialPreflightMode::Fail,
            &status,
            PlayurlMode::Web,
            &restricted_area,
        );

        assert_eq!(report.requirements.len(), 1);
        assert!(!report.issues.iter().any(|issue| {
            issue.request_path == CredentialPreflightRequestPath::RestrictedAreaProxy
        }));
        Ok(())
    }

    #[test]
    fn media_request_context_can_skip_restricted_area_proxy_requirement_for_non_pgc_input()
    -> crate::Result<()> {
        let status = CredentialProfiles::default().profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;
        let restricted_area = RestrictedAreaConfig::default().with_proxy(
            RestrictedAreaProxy::playurl("https://proxy.example/playurl", Some(RestrictedArea::Hk)),
        );

        let report = CredentialPreflightReport::from_media_request_context(
            CredentialPreflightMode::Fail,
            &status,
            PlayurlMode::Web,
            &restricted_area,
            false,
            false,
        );

        assert_eq!(report.requirements.len(), 1);
        assert!(!report.has_blocking_issues());
        Ok(())
    }

    #[test]
    fn media_request_context_skips_restricted_area_proxy_requirement_for_tv_playurl()
    -> crate::Result<()> {
        let status = CredentialProfiles::default().profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;
        let restricted_area = RestrictedAreaConfig::default().with_proxy(
            RestrictedAreaProxy::playurl("https://proxy.example/playurl", Some(RestrictedArea::Hk)),
        );

        let report = CredentialPreflightReport::from_media_request_context(
            CredentialPreflightMode::Fail,
            &status,
            PlayurlMode::Tv,
            &restricted_area,
            true,
            false,
        );

        assert_eq!(report.requirements.len(), 1);
        assert!(report.issues.iter().all(|issue| {
            issue.request_path != CredentialPreflightRequestPath::RestrictedAreaProxy
        }));
        Ok(())
    }

    #[test]
    fn media_request_context_requires_access_key_for_intl_input() -> crate::Result<()> {
        let status = CredentialProfiles::default().profile_lifecycle_status(
            "default",
            &CredentialLifecyclePolicy::at_unix_millis(1_000),
        )?;

        let report = CredentialPreflightReport::from_media_request_context(
            CredentialPreflightMode::Fail,
            &status,
            PlayurlMode::Web,
            &RestrictedAreaConfig::default(),
            false,
            true,
        );

        assert!(report.has_blocking_issues());
        assert!(report.issues.iter().any(|issue| {
            issue.request_path == CredentialPreflightRequestPath::IntlWeb
                && issue.credential_kinds == [CredentialKind::AccessKey]
        }));
        Ok(())
    }
}
