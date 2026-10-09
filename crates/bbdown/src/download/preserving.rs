use super::{
    DanmakuUpdateOptions, DownloadArchive, DownloadArchiveEntryRecord, DownloadFileKind,
    DownloadedFile, archive_entry_allows_danmaku_update, archive_entry_matches_plan_entry,
    archive_storage_path, current_unix_seconds, danmaku_update_archive_formats,
    ensure_directory_exists, refresh_archive_entry_danmaku_content_key,
    refresh_archive_record_danmaku_content_key, remember_archive_entry_file,
};
use crate::{
    BiliClient, DanmakuFormat, DanmakuUpdateReport, DownloadEntry, DownloadPlan, Error, Result,
    danmaku, progress::NoopDownloadProgress,
};
use std::collections::HashSet;
use std::fs::{self as std_fs, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs as async_fs;

static UNIQUE_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// One ASS preservation result, tied to the selected archive entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DanmakuAssStatistics {
    pub path: PathBuf,
    pub index: u32,
    pub cid: u64,
    pub generated_events: usize,
}

/// A file replacement prepared without changing its destination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedDanmakuFile {
    logical_path: PathBuf,
    path: PathBuf,
    output_bytes: Vec<u8>,
    expected_original_bytes: Option<Vec<u8>>,
    kind: DownloadFileKind,
}

impl StagedDanmakuFile {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn output_bytes(&self) -> &[u8] {
        &self.output_bytes
    }

    #[must_use]
    pub fn expected_original_bytes(&self) -> Option<&[u8]> {
        self.expected_original_bytes.as_deref()
    }
}

/// Prepared danmaku outputs. Calling `publish` replaces the complete file group.
///
/// The publisher checks old file contents, resolved destinations, and captured basic access
/// permissions again before replacement. Inode and mtime changes are not mutation signals;
/// callers coordinating File Provider or other writers must still serialize those writers.
/// The filesystem does not provide a multi-file crash-atomic transaction.
#[derive(Clone, Debug)]
pub struct StagedDanmakuUpdate {
    report: DanmakuUpdateReport,
    updated_archive: DownloadArchive,
    files: Vec<StagedDanmakuFile>,
    ass_statistics: Vec<DanmakuAssStatistics>,
}

impl StagedDanmakuUpdate {
    #[must_use]
    pub const fn report(&self) -> &DanmakuUpdateReport {
        &self.report
    }

    #[must_use]
    pub const fn updated_archive(&self) -> &DownloadArchive {
        &self.updated_archive
    }

    #[must_use]
    pub fn files(&self) -> &[StagedDanmakuFile] {
        &self.files
    }

    #[must_use]
    pub fn ass_statistics(&self) -> &[DanmakuAssStatistics] {
        &self.ass_statistics
    }

    /// Publish all prepared files, rolling back detectable partial replacements on failure.
    pub fn publish(self) -> Result<DanmakuUpdateReport> {
        publish_file_group(&self.files)?;
        Ok(self.report)
    }
}

impl BiliClient {
    /// Prepare preserving updates for every selected archive entry without changing destinations.
    pub async fn stage_preserving_danmaku_update_for_archive(
        &self,
        plan: &DownloadPlan,
        archive: &DownloadArchive,
        options: DanmakuUpdateOptions,
    ) -> Result<StagedDanmakuUpdate> {
        stage_preserving(self, plan, archive, options, Vec::new()).await
    }

    /// Load and capture the archive before network access, then stage it with all sidecars.
    pub async fn stage_preserving_danmaku_update_for_archive_file(
        &self,
        plan: &DownloadPlan,
        archive_path: impl AsRef<Path>,
        options: DanmakuUpdateOptions,
    ) -> Result<StagedDanmakuUpdate> {
        let archive_path = archive_path.as_ref();
        let resolved_path = resolve_destination_path(archive_path)?;
        let original = read_optional_bytes(&resolved_path)?;
        let archive = match &original {
            Some(bytes) => serde_json::from_slice::<DownloadArchive>(bytes)?,
            None => DownloadArchive::default(),
        };
        let mut staged = stage_preserving(self, plan, &archive, options, Vec::new()).await?;
        let archive_bytes = serde_json::to_vec_pretty(&staged.updated_archive)?;
        staged.files.push(StagedDanmakuFile {
            logical_path: archive_path.to_path_buf(),
            path: resolved_path,
            output_bytes: archive_bytes,
            expected_original_bytes: original,
            kind: DownloadFileKind::Danmaku,
        });
        ensure_unique_destinations(&staged.files)?;
        Ok(staged)
    }
}

async fn stage_preserving(
    client: &BiliClient,
    plan: &DownloadPlan,
    archive: &DownloadArchive,
    options: DanmakuUpdateOptions,
    mut files: Vec<StagedDanmakuFile>,
) -> Result<StagedDanmakuUpdate> {
    let archive_formats = danmaku_update_archive_formats(&options.danmaku_formats);
    let mut updated_archive = archive.clone();
    let mut reports = Vec::new();
    let mut ass_statistics = Vec::new();

    for record in &mut updated_archive.records {
        let record_key = record.content_key.clone();
        let mut updated = false;
        for entry in &mut record.entries {
            if !archive_entry_allows_danmaku_update(&record_key, entry) {
                continue;
            }
            let Some(plan_entry) = plan
                .entries
                .iter()
                .find(|candidate| archive_entry_matches_plan_entry(entry, candidate))
            else {
                continue;
            };
            ensure_directory_exists(&entry.directory).await?;
            let entry_result = stage_entry(client, plan_entry, entry, &options).await?;
            for staged_file in &entry_result.files {
                remember_archive_entry_file(entry, &staged_file.logical_path);
                files.push(staged_file.clone());
            }
            reports.push(entry_result.report);
            if let Some(stats) = entry_result.ass_statistics {
                ass_statistics.push(stats);
            }
            refresh_archive_entry_danmaku_content_key(entry, &archive_formats);
            updated = true;
        }
        if updated {
            refresh_archive_record_danmaku_content_key(record, &archive_formats);
            record.completed_at_unix = current_unix_seconds();
        }
    }
    if reports.is_empty() {
        return Err(Error::InvalidInput(
            "download archive does not contain entries selected by the current plan".to_owned(),
        ));
    }
    ensure_unique_destinations(&files)?;
    Ok(StagedDanmakuUpdate {
        report: DanmakuUpdateReport { entries: reports },
        updated_archive,
        files,
        ass_statistics,
    })
}

struct StagedEntry {
    report: super::EntryDanmakuUpdateReport,
    files: Vec<StagedDanmakuFile>,
    ass_statistics: Option<DanmakuAssStatistics>,
}

async fn stage_entry(
    client: &BiliClient,
    plan_entry: &DownloadEntry,
    archive_entry: &DownloadArchiveEntryRecord,
    options: &DanmakuUpdateOptions,
) -> Result<StagedEntry> {
    let fetched_xml =
        download_danmaku_source(client, plan_entry, &archive_entry.directory, options).await?;
    let xml_logical_path = archive_entry.directory.join("danmaku.xml");
    let (xml_path, original_xml_bytes) = read_destination(&xml_logical_path)?;
    let existing_xml = bytes_to_text(original_xml_bytes.as_deref(), "existing danmaku XML")?;
    let merged = danmaku::merge_xml_preserving(existing_xml, &fetched_xml)?;
    let mut files = vec![StagedDanmakuFile {
        logical_path: xml_logical_path,
        path: xml_path,
        output_bytes: merged.xml.as_bytes().to_vec(),
        expected_original_bytes: original_xml_bytes.clone(),
        kind: DownloadFileKind::Danmaku,
    }];
    let mut ass_statistics = None;
    if options.danmaku_formats.contains(DanmakuFormat::Ass) {
        let ass_logical_path = archive_entry.directory.join("danmaku.ass");
        let (ass_path, original_ass_bytes) = read_destination(&ass_logical_path)?;
        let ass = danmaku::xml_to_ass_validated(&merged.xml)?;
        let generated_events = ass
            .lines()
            .filter(|line| line.starts_with("Dialogue:"))
            .count();
        files.push(StagedDanmakuFile {
            logical_path: ass_logical_path.clone(),
            path: ass_path,
            output_bytes: ass.as_bytes().to_vec(),
            expected_original_bytes: original_ass_bytes,
            kind: DownloadFileKind::DanmakuAss,
        });
        ass_statistics = Some(DanmakuAssStatistics {
            path: ass_logical_path,
            index: archive_entry.index,
            cid: archive_entry.cid,
            generated_events,
        });
    }
    let report_files = files
        .iter()
        .map(|file| DownloadedFile {
            kind: file.kind.clone(),
            path: file.logical_path.clone(),
            bytes_written: u64::try_from(file.output_bytes.len()).unwrap_or(u64::MAX),
            resumed_from: 0,
        })
        .collect();
    Ok(StagedEntry {
        report: super::EntryDanmakuUpdateReport {
            index: archive_entry.index,
            aid: archive_entry.aid,
            bvid: archive_entry.bvid.clone(),
            cid: archive_entry.cid,
            epid: archive_entry.epid,
            title: archive_entry.title.clone(),
            directory: archive_entry.directory.clone(),
            existing_comments: merged.existing_comments,
            fetched_comments: merged.fetched_comments,
            appended_comments: merged.appended_comments,
            files: report_files,
        },
        files,
        ass_statistics,
    })
}

async fn download_danmaku_source(
    client: &BiliClient,
    plan_entry: &DownloadEntry,
    directory: &Path,
    options: &DanmakuUpdateOptions,
) -> Result<String> {
    let temp_dir = create_unique_directory(directory)?;
    let source_path = temp_dir.path().join("source.xml");
    let request =
        super::DownloadFileRequest::new(plan_entry, &source_path, DownloadFileKind::Danmaku, None);
    let download_options = super::DownloadOptions::default()
        .with_retry_policy(options.retry)
        .with_resume(false)
        .with_download_idle_timeout(options.download_idle_timeout);
    let cancellation = crate::DownloadCancellationToken::new();
    let result = client
        .download_url_to_file(
            &plan_entry.danmaku.xml_url,
            &request,
            &download_options,
            &NoopDownloadProgress,
            &cancellation,
        )
        .await;
    let bytes = match result {
        Ok(_) => async_fs::read(&source_path).await.map_err(Error::Io),
        Err(error) => Err(error),
    };
    let cleanup = temp_dir.close();
    if let Err(error) = cleanup
        && error.kind() != std::io::ErrorKind::NotFound
        && bytes.is_ok()
    {
        return Err(Error::Io(error));
    }
    let bytes = bytes?;
    String::from_utf8(bytes)
        .map_err(|_| Error::InvalidInput("downloaded danmaku XML is not UTF-8".to_owned()))
}

fn read_destination(logical_path: &Path) -> Result<(PathBuf, Option<Vec<u8>>)> {
    let path = resolve_destination_path(logical_path)?;
    Ok((path.clone(), read_optional_bytes(&path)?))
}

// The canonical parent resolves every existing ancestor symlink; joining the leaf also
// represents a not-yet-created file. This path is the chosen-destination signal. File bytes
// are checked separately, so benign inode or timestamp changes do not reject publication.
fn resolve_destination_path(logical_path: &Path) -> Result<PathBuf> {
    let storage_path = archive_storage_path(logical_path)?;
    let mut candidate = super::absolute_path(&storage_path);
    let mut visited = HashSet::new();
    loop {
        candidate = super::canonicalize_existing_prefix(&candidate);
        if !visited.insert(candidate.clone()) {
            return Err(Error::InvalidInput(format!(
                "danmaku destination contains a symlink cycle: {}",
                logical_path.display()
            )));
        }
        match std_fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let target = std_fs::read_link(&candidate).map_err(Error::Io)?;
                candidate = if target.is_absolute() {
                    target
                } else {
                    candidate
                        .parent()
                        .unwrap_or_else(|| Path::new(""))
                        .join(target)
                };
            }
            Ok(_) => return std_fs::canonicalize(candidate).map_err(Error::Io),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let parent = candidate.parent().ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "danmaku destination has no parent directory: {}",
                        logical_path.display()
                    ))
                })?;
                let file_name = candidate.file_name().ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "danmaku destination has no file name: {}",
                        logical_path.display()
                    ))
                })?;
                let resolved_parent = std_fs::canonicalize(parent).map_err(Error::Io)?;
                return Ok(resolved_parent.join(file_name));
            }
            Err(error) => return Err(Error::Io(error)),
        }
    }
}

fn read_optional_bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    match std_fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Io(error)),
    }
}

fn bytes_to_text<'a>(bytes: Option<&'a [u8]>, label: &str) -> Result<&'a str> {
    match bytes {
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(|_| Error::InvalidInput(format!("{label} is not UTF-8"))),
        None => Ok(""),
    }
}

fn ensure_unique_destinations(files: &[StagedDanmakuFile]) -> Result<()> {
    let mut destinations = std::collections::HashSet::new();
    for file in files {
        let key = super::comparable_output_path_key(&file.path);
        if !destinations.insert(key) {
            return Err(Error::InvalidInput(format!(
                "danmaku update destinations overlap: {}",
                file.logical_path.display()
            )));
        }
    }
    Ok(())
}

fn publish_file_group(files: &[StagedDanmakuFile]) -> Result<()> {
    publish_file_group_with(files, replace_target)
}

fn publish_file_group_with<F>(files: &[StagedDanmakuFile], replace: F) -> Result<()>
where
    F: FnMut(&PreparedReplacement) -> std::result::Result<(), ReplaceFailure>,
{
    publish_file_group_with_ops(files, replace, rollback_target)
}

fn publish_file_group_with_ops<F, R>(
    files: &[StagedDanmakuFile],
    replace: F,
    rollback: R,
) -> Result<()>
where
    F: FnMut(&PreparedReplacement) -> std::result::Result<(), ReplaceFailure>,
    R: FnMut(&PreparedReplacement) -> std::io::Result<()>,
{
    publish_file_group_with_prepare_hook(files, replace, rollback, || Ok(()))
}

fn publish_file_group_with_prepare_hook<F, R, H>(
    files: &[StagedDanmakuFile],
    mut replace: F,
    mut rollback: R,
    mut after_prepare: H,
) -> Result<()>
where
    F: FnMut(&PreparedReplacement) -> std::result::Result<(), ReplaceFailure>,
    R: FnMut(&PreparedReplacement) -> std::io::Result<()>,
    H: FnMut() -> Result<()>,
{
    ensure_unique_destinations(files)?;
    for file in files {
        verify_destination_and_original(file)?;
    }

    // Permission policy is selected at publication time, after staged bytes and the chosen
    // destination have been checked. Metadata such as inode and mtime is deliberately ignored.
    let policies = files
        .iter()
        .map(capture_destination_policy)
        .collect::<Result<Vec<_>>>()?;
    let prepared = prepare_replacements(files, &policies)?;
    if let Err(error) = after_prepare() {
        cleanup_prepared(&prepared, true);
        return Err(error);
    }
    for (file, policy) in files.iter().zip(&policies) {
        if let Err(error) = verify_destination_and_original(file)
            .and_then(|()| verify_destination_policy(file, policy.current.as_ref()))
        {
            cleanup_prepared(&prepared, true);
            return Err(error);
        }
    }

    for (replaced, item) in prepared.iter().enumerate() {
        if let Err(failure) = replace(item) {
            let failed_index = replaced;
            let mut failed_rollbacks = std::collections::HashSet::new();
            let mut recovery_paths = Vec::new();
            let mut rollback_errors = Vec::new();
            if failure.original_removed
                && let Err(rollback_error) = rollback(item)
            {
                failed_rollbacks.insert(failed_index);
                if let Some(path) = &item.recovery {
                    recovery_paths.push(path.clone());
                }
                recovery_paths.push(item.target.clone());
                rollback_errors.push(rollback_error.to_string());
            }
            for prior in prepared[..replaced].iter().rev() {
                if let Err(rollback_error) = rollback(prior) {
                    let index = prepared
                        .iter()
                        .position(|candidate| std::ptr::eq(candidate, prior))
                        .unwrap_or(0);
                    failed_rollbacks.insert(index);
                    if let Some(path) = &prior.recovery {
                        recovery_paths.push(path.clone());
                    }
                    recovery_paths.push(prior.target.clone());
                    rollback_errors.push(rollback_error.to_string());
                }
            }
            for (index, item) in prepared.iter().enumerate() {
                remove_owned_temp(&item.output_temp);
                if !failed_rollbacks.contains(&index)
                    && let Some(recovery) = &item.recovery
                {
                    remove_owned_temp(recovery);
                }
            }
            if recovery_paths.is_empty() {
                return Err(Error::Io(failure.error));
            }
            return Err(Error::InvalidInput(format!(
                "danmaku replacement failed and rollback was incomplete; recovery copies or affected targets: {}; rollback errors: {}",
                recovery_paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                rollback_errors.join("; ")
            )));
        }
    }
    cleanup_prepared(&prepared, true);
    Ok(())
}

fn prepare_replacements(
    files: &[StagedDanmakuFile],
    policies: &[CapturedDestinationPolicy],
) -> Result<Vec<PreparedReplacement>> {
    let mut prepared = Vec::with_capacity(files.len());
    for (file, policy) in files.iter().zip(policies) {
        let parent = file.path.parent().unwrap_or_else(|| Path::new("."));
        let output_temp = match create_and_write_unique(
            parent,
            ".bbdown-preserving-stage",
            &file.output_bytes,
            policy.output,
        ) {
            Ok(path) => path,
            Err(error) => {
                cleanup_prepared(&prepared, true);
                return Err(error);
            }
        };
        let recovery = match &file.expected_original_bytes {
            Some(bytes) => {
                match create_and_write_unique(
                    parent,
                    ".bbdown-preserving-recovery",
                    bytes,
                    policy.output,
                ) {
                    Ok(path) => Some(path),
                    Err(error) => {
                        remove_owned_temp(&output_temp);
                        cleanup_prepared(&prepared, true);
                        return Err(error);
                    }
                }
            }
            None => None,
        };
        prepared.push(PreparedReplacement {
            target: file.path.clone(),
            output_temp,
            recovery,
            had_original: file.expected_original_bytes.is_some(),
            access_policy: policy.output,
        });
    }
    Ok(prepared)
}

struct PreparedReplacement {
    target: PathBuf,
    output_temp: PathBuf,
    recovery: Option<PathBuf>,
    had_original: bool,
    access_policy: AccessPolicy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AccessPolicy {
    #[cfg(unix)]
    mode: u32,
    #[cfg(not(unix))]
    readonly: bool,
}

struct CapturedDestinationPolicy {
    current: Option<AccessPolicy>,
    output: AccessPolicy,
}

impl AccessPolicy {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            Self {
                // Compare rwx and special mode bits; ignore file type and identity metadata.
                mode: metadata.permissions().mode() & 0o7777,
            }
        }
        #[cfg(not(unix))]
        {
            Self {
                readonly: metadata.permissions().readonly(),
            }
        }
    }

    fn apply(self, path: &Path) -> std::io::Result<()> {
        let mut permissions = std_fs::metadata(path)?.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(self.mode);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(self.readonly);
        std_fs::set_permissions(path, permissions)
    }

    fn apply_to_file(self, file: &std::fs::File) -> std::io::Result<()> {
        let mut permissions = file.metadata()?.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            permissions.set_mode(self.mode);
        }
        #[cfg(not(unix))]
        permissions.set_readonly(self.readonly);
        file.set_permissions(permissions)
    }
}

fn capture_destination_policy(file: &StagedDanmakuFile) -> Result<CapturedDestinationPolicy> {
    let current = match std_fs::metadata(&file.path) {
        Ok(metadata) => Some(AccessPolicy::from_metadata(&metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(Error::Io(error)),
    };
    let output = match current {
        Some(policy) => policy,
        None => default_access_policy(file.path.parent().unwrap_or_else(|| Path::new(".")))?,
    };
    Ok(CapturedDestinationPolicy { current, output })
}

fn verify_destination_policy(
    file: &StagedDanmakuFile,
    expected: Option<&AccessPolicy>,
) -> Result<()> {
    let current = match std_fs::metadata(&file.path) {
        Ok(metadata) => Some(AccessPolicy::from_metadata(&metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(Error::Io(error)),
    };
    if current.as_ref() != expected {
        return Err(Error::InvalidInput(format!(
            "danmaku destination access permissions changed during publication preparation: {}",
            file.logical_path.display()
        )));
    }
    Ok(())
}

fn verify_original(file: &StagedDanmakuFile) -> Result<()> {
    let current = match read_optional_bytes(&file.path) {
        Ok(current) => current,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    match (&file.expected_original_bytes, current) {
        (Some(_), None) => Err(Error::InvalidInput(format!(
            "danmaku destination disappeared after staging: {}",
            file.path.display()
        ))),
        (None, Some(_)) => Err(Error::InvalidInput(format!(
            "danmaku destination appeared after staging: {}",
            file.path.display()
        ))),
        (Some(expected), Some(actual)) if expected != &actual => Err(Error::InvalidInput(format!(
            "danmaku destination content changed after staging: {}",
            file.path.display()
        ))),
        _ => Ok(()),
    }
}

fn verify_destination_and_original(file: &StagedDanmakuFile) -> Result<()> {
    let current_path = resolve_destination_path(&file.logical_path)?;
    if current_path != file.path {
        return Err(Error::InvalidInput(format!(
            "danmaku destination changed after staging: {}",
            file.logical_path.display()
        )));
    }
    verify_original(file)
}

struct ReplaceFailure {
    error: std::io::Error,
    original_removed: bool,
}

fn replace_target(item: &PreparedReplacement) -> std::result::Result<(), ReplaceFailure> {
    if item.had_original {
        std_fs::remove_file(&item.target).map_err(|error| ReplaceFailure {
            error,
            original_removed: false,
        })?;
    }
    match std_fs::rename(&item.output_temp, &item.target) {
        Ok(()) => Ok(()),
        Err(error) => Err(ReplaceFailure {
            error,
            original_removed: item.had_original,
        }),
    }
}

fn rollback_target(item: &PreparedReplacement) -> std::io::Result<()> {
    if !item.had_original {
        return match std_fs::remove_file(&item.target) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        };
    }
    let recovery = item
        .recovery
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing recovery copy"))?;
    match std_fs::remove_file(&item.target) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    std_fs::copy(recovery, &item.target)?;
    item.access_policy.apply(&item.target)
}

fn cleanup_prepared(prepared: &[PreparedReplacement], include_recovery: bool) {
    for item in prepared {
        remove_owned_temp(&item.output_temp);
        if include_recovery && let Some(recovery) = &item.recovery {
            remove_owned_temp(recovery);
        }
    }
}

fn remove_owned_temp(path: &Path) {
    #[cfg(windows)]
    if let Ok(metadata) = std_fs::metadata(path) {
        let mut permissions = metadata.permissions();
        if permissions.readonly() {
            permissions.set_readonly(false);
            let _ = std_fs::set_permissions(path, permissions);
        }
    }
    let _ = std_fs::remove_file(path);
}

fn create_unique_directory(parent: &Path) -> Result<tempfile::TempDir> {
    tempfile::Builder::new()
        .prefix(".bbdown-preserving-source-")
        .tempdir_in(parent)
        .map_err(Error::Io)
}

fn create_and_write_unique(
    parent: &Path,
    label: &str,
    bytes: &[u8],
    policy: AccessPolicy,
) -> Result<PathBuf> {
    loop {
        let path = unique_candidate(parent, label);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Keep staged output and recovery bytes private until the selected policy is applied.
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                if let Err(error) = file
                    .write_all(bytes)
                    .and_then(|()| file.sync_all())
                    .and_then(|()| policy.apply_to_file(&file))
                {
                    drop(file);
                    remove_owned_temp(&path);
                    return Err(Error::Io(error));
                }
                drop(file);
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(Error::Io(error)),
        }
    }
}

fn default_access_policy(parent: &Path) -> Result<AccessPolicy> {
    loop {
        let path = unique_candidate(parent, ".bbdown-preserving-mode-probe");
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => {
                let result = std_fs::metadata(&path)
                    .map(|metadata| AccessPolicy::from_metadata(&metadata))
                    .map_err(Error::Io);
                drop(file);
                remove_owned_temp(&path);
                return result;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(Error::Io(error)),
        }
    }
}

fn unique_candidate(parent: &Path, label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let sequence = UNIQUE_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        "{label}-{}-{stamp:x}-{sequence:x}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        ReplaceFailure, StagedDanmakuFile, create_unique_directory, publish_file_group,
        publish_file_group_with, publish_file_group_with_ops, publish_file_group_with_prepare_hook,
        replace_target, resolve_destination_path, rollback_target,
    };
    use crate::DownloadFileKind;
    use std::fs;
    use std::path::Path;

    fn require_err<T, E: std::fmt::Debug>(
        result: std::result::Result<T, E>,
        message: &str,
    ) -> anyhow::Result<E> {
        match result {
            Ok(_) => anyhow::bail!("{message}"),
            Err(error) => Ok(error),
        }
    }

    fn staged(
        path: &Path,
        original: &[u8],
        output: &[u8],
    ) -> std::result::Result<StagedDanmakuFile, crate::Error> {
        Ok(StagedDanmakuFile {
            logical_path: path.to_path_buf(),
            path: resolve_destination_path(path)?,
            output_bytes: output.to_vec(),
            expected_original_bytes: Some(original.to_vec()),
            kind: DownloadFileKind::Danmaku,
        })
    }

    #[cfg(unix)]
    #[test]
    fn publisher_accepts_prepublication_mode_change_and_preserves_current_policy()
    -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("danmaku.xml");
        fs::write(&path, b"old")?;
        let file = staged(&path, b"old", b"new")?;
        let mut permissions = fs::metadata(&path)?.permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions)?;

        publish_file_group(&[file])?;

        assert_eq!(fs::read(&path)?, b"new");
        assert!(fs::metadata(&path)?.permissions().readonly());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn publisher_rejects_access_policy_change_during_preparation() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir()?;
        let path = temp.path().join("danmaku.xml");
        fs::write(&path, b"old")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640))?;
        let file = staged(&path, b"old", b"new")?;

        let error = require_err(
            publish_file_group_with_prepare_hook(&[file], replace_target, rollback_target, || {
                fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                    .map_err(crate::Error::Io)
            }),
            "permission changes during preparation must be rejected",
        )?;

        assert!(error.to_string().contains("access permissions changed"));
        assert_eq!(fs::read(&path)?, b"old");
        assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read_dir(temp.path())?.count(), 1);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn publisher_preserves_distinct_existing_target_modes() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir()?;
        let paths = ["archive.json", "danmaku.xml", "danmaku.ass", "custom.xml"]
            .map(|name| temp.path().join(name));
        let modes = [0o600, 0o640, 0o440, 0o1640];
        let contents = [
            b"old archive".as_slice(),
            b"old xml",
            b"old ass",
            b"old custom",
        ];
        let outputs = [
            b"new archive".as_slice(),
            b"new xml",
            b"new ass",
            b"new custom",
        ];
        let files = paths
            .iter()
            .zip(contents)
            .zip(outputs)
            .zip(modes)
            .map(|(((path, original), output), mode)| {
                fs::write(path, original)?;
                fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
                staged(path, original, output)
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;

        publish_file_group(&files)?;

        for ((path, output), mode) in paths.iter().zip(outputs).zip(modes) {
            assert_eq!(fs::read(path)?, output);
            assert_eq!(fs::metadata(path)?.permissions().mode() & 0o7777, mode);
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn publisher_keeps_umask_mode_for_new_targets() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir()?;
        let mode_probe = temp.path().join("mode-probe");
        fs::write(&mode_probe, b"")?;
        let default_mode = fs::metadata(&mode_probe)?.permissions().mode() & 0o777;
        let target = temp.path().join("new.xml");
        let file = StagedDanmakuFile {
            logical_path: target.clone(),
            path: resolve_destination_path(&target)?,
            output_bytes: b"new".to_vec(),
            expected_original_bytes: None,
            kind: DownloadFileKind::Danmaku,
        };

        publish_file_group(&[file])?;

        assert_eq!(
            fs::metadata(target)?.permissions().mode() & 0o777,
            default_mode
        );
        Ok(())
    }

    #[test]
    fn publisher_rejects_changed_content_without_replacing_it() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("danmaku.xml");
        fs::write(&path, b"old")?;
        let file = staged(&path, b"old", b"new")?;
        fs::write(&path, b"changed")?;

        let error = require_err(
            publish_file_group(&[file]),
            "content mismatch must be rejected",
        )?;

        assert!(error.to_string().contains("content changed"));
        assert_eq!(fs::read(path)?, b"changed");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn publisher_accepts_inode_replacement_with_equal_content() -> anyhow::Result<()> {
        use std::os::unix::fs::MetadataExt;

        let temp = tempfile::tempdir()?;
        let path = temp.path().join("danmaku.xml");
        let replacement = temp.path().join("replacement.xml");
        fs::write(&path, b"old")?;
        let original_inode = fs::metadata(&path)?.ino();
        let file = staged(&path, b"old", b"new")?;
        fs::write(&replacement, b"old")?;
        fs::rename(replacement, &path)?;
        assert_ne!(fs::metadata(&path)?.ino(), original_inode);

        publish_file_group(&[file])?;

        assert_eq!(fs::read(path)?, b"new");
        Ok(())
    }

    #[test]
    fn publisher_distinguishes_missing_appeared_and_unreadable_targets() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let disappeared = temp.path().join("disappeared.xml");
        fs::write(&disappeared, b"old")?;
        let disappeared_stage = staged(&disappeared, b"old", b"new")?;
        fs::remove_file(&disappeared)?;
        let disappeared_error = require_err(
            publish_file_group(&[disappeared_stage]),
            "a file removed after staging must be detected",
        )?;
        assert!(
            disappeared_error
                .to_string()
                .contains("disappeared after staging")
        );

        let missing = temp.path().join("missing.xml");
        let missing_stage = StagedDanmakuFile {
            logical_path: missing.clone(),
            path: resolve_destination_path(&missing)?,
            output_bytes: b"new".to_vec(),
            expected_original_bytes: None,
            kind: DownloadFileKind::Danmaku,
        };
        fs::write(&missing, b"appeared")?;
        let appeared_error = require_err(
            publish_file_group(&[missing_stage]),
            "a file created after staging must not be overwritten",
        )?;
        assert!(
            appeared_error
                .to_string()
                .contains("appeared after staging")
        );
        assert_eq!(fs::read(&missing)?, b"appeared");

        let unreadable = temp.path().join("directory.xml");
        fs::create_dir(&unreadable)?;
        let unreadable_error = require_err(
            publish_file_group(&[StagedDanmakuFile {
                logical_path: unreadable.clone(),
                path: resolve_destination_path(&unreadable)?,
                output_bytes: b"new".to_vec(),
                expected_original_bytes: None,
                kind: DownloadFileKind::Danmaku,
            }]),
            "directory read errors must not be treated as absence",
        )?;
        assert!(matches!(
            unreadable_error,
            crate::Error::Io(error) if error.kind() == std::io::ErrorKind::IsADirectory
        ));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn publisher_rejects_symlink_retargeting() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let first_target = temp.path().join("first.xml");
        let second_target = temp.path().join("second.xml");
        let link = temp.path().join("danmaku.xml");
        fs::write(&first_target, b"old")?;
        fs::write(&second_target, b"old")?;
        symlink(&first_target, &link)?;
        let file = StagedDanmakuFile {
            logical_path: link.clone(),
            path: resolve_destination_path(&link)?,
            output_bytes: b"new".to_vec(),
            expected_original_bytes: Some(b"old".to_vec()),
            kind: DownloadFileKind::Danmaku,
        };
        fs::remove_file(&link)?;
        symlink(&second_target, &link)?;

        let error = require_err(publish_file_group(&[file]), "retargeted symlinks must fail")?;

        assert!(error.to_string().contains("destination changed"));
        assert_eq!(fs::read(first_target)?, b"old");
        assert_eq!(fs::read(second_target)?, b"old");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn destination_resolves_multihop_symlinks_to_missing_final_target() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let final_parent = temp.path().join("final");
        fs::create_dir(&final_parent)?;
        let final_target = final_parent.join("danmaku.xml");
        let second_link = temp.path().join("second.xml");
        let first_link = temp.path().join("first.xml");
        symlink(&final_target, &second_link)?;
        symlink(&second_link, &first_link)?;

        assert_eq!(
            resolve_destination_path(&first_link)?,
            fs::canonicalize(final_parent)?.join("danmaku.xml")
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn destination_rejects_symlink_cycles() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let first = temp.path().join("first.xml");
        let second = temp.path().join("second.xml");
        symlink(&second, &first)?;
        symlink(&first, &second)?;

        let error = require_err(
            resolve_destination_path(&first),
            "cyclic symlink destinations must fail safely",
        )?;
        assert!(error.to_string().contains("symlink cycle"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn publisher_rejects_parent_directory_symlink_retargeting() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let first_directory = temp.path().join("first-directory");
        let second_directory = temp.path().join("second-directory");
        let alias = temp.path().join("selected-directory");
        fs::create_dir(&first_directory)?;
        fs::create_dir(&second_directory)?;
        let first_target = first_directory.join("danmaku.xml");
        let second_target = second_directory.join("danmaku.xml");
        fs::write(&first_target, b"same old bytes")?;
        fs::write(&second_target, b"same old bytes")?;
        symlink(&first_directory, &alias)?;
        let logical_path = alias.join("danmaku.xml");
        let file = StagedDanmakuFile {
            logical_path: logical_path.clone(),
            path: resolve_destination_path(&logical_path)?,
            output_bytes: b"new output".to_vec(),
            expected_original_bytes: Some(b"same old bytes".to_vec()),
            kind: DownloadFileKind::Danmaku,
        };
        fs::remove_file(&alias)?;
        symlink(&second_directory, &alias)?;

        let error = require_err(
            publish_file_group(&[file]),
            "retargeted parent directory symlink must be rejected",
        )?;

        assert!(error.to_string().contains("destination changed"));
        assert_eq!(fs::read(first_target)?, b"same old bytes");
        assert_eq!(fs::read(second_target)?, b"same old bytes");
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn publisher_rejects_parent_retarget_when_both_destinations_are_absent() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let first_directory = temp.path().join("first-directory");
        let second_directory = temp.path().join("second-directory");
        let alias = temp.path().join("selected-directory");
        fs::create_dir(&first_directory)?;
        fs::create_dir(&second_directory)?;
        symlink(&first_directory, &alias)?;
        let logical_path = alias.join("danmaku.xml");
        let file = StagedDanmakuFile {
            logical_path: logical_path.clone(),
            path: resolve_destination_path(&logical_path)?,
            output_bytes: b"new output".to_vec(),
            expected_original_bytes: None,
            kind: DownloadFileKind::Danmaku,
        };
        fs::remove_file(&alias)?;
        symlink(&second_directory, &alias)?;

        let error = require_err(
            publish_file_group(&[file]),
            "retargeted absent destination must be rejected",
        )?;

        assert!(error.to_string().contains("destination changed"));
        assert!(!first_directory.join("danmaku.xml").exists());
        assert!(!second_directory.join("danmaku.xml").exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn publisher_accepts_stable_parent_directory_symlink() -> anyhow::Result<()> {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir()?;
        let target_directory = temp.path().join("target-directory");
        let alias = temp.path().join("selected-directory");
        fs::create_dir(&target_directory)?;
        let target = target_directory.join("danmaku.xml");
        fs::write(&target, b"old")?;
        symlink(&target_directory, &alias)?;
        let logical_path = alias.join("danmaku.xml");
        let file = StagedDanmakuFile {
            logical_path: logical_path.clone(),
            path: resolve_destination_path(&logical_path)?,
            output_bytes: b"new".to_vec(),
            expected_original_bytes: Some(b"old".to_vec()),
            kind: DownloadFileKind::Danmaku,
        };

        publish_file_group(&[file])?;

        assert_eq!(fs::read(target)?, b"new");
        assert!(fs::symlink_metadata(alias)?.file_type().is_symlink());
        Ok(())
    }

    #[test]
    fn publisher_rolls_back_prior_files_after_nth_replacement_failure() -> anyhow::Result<()> {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir()?;
        let first_path = temp.path().join("first.xml");
        let second_path = temp.path().join("second.xml");
        fs::write(&first_path, b"first-old")?;
        fs::write(&second_path, b"second-old")?;
        #[cfg(unix)]
        {
            fs::set_permissions(&first_path, fs::Permissions::from_mode(0o600))?;
            fs::set_permissions(&second_path, fs::Permissions::from_mode(0o440))?;
        }
        let files = [
            staged(&first_path, b"first-old", b"first-new")?,
            staged(&second_path, b"second-old", b"second-new")?,
        ];
        let mut calls = 0;

        let error = publish_file_group_with(&files, |replacement| {
            calls += 1;
            if calls == 2 {
                fs::remove_file(&replacement.target).map_err(|error| ReplaceFailure {
                    error,
                    original_removed: false,
                })?;
                Err(ReplaceFailure {
                    error: std::io::Error::other("injected replacement failure"),
                    original_removed: true,
                })
            } else {
                replace_target(replacement)
            }
        });
        let error = require_err(error, "injected replacement failure must fail the group")?;

        assert!(error.to_string().contains("injected replacement failure"));
        assert_eq!(fs::read(&first_path)?, b"first-old");
        assert_eq!(fs::read(&second_path)?, b"second-old");
        #[cfg(unix)]
        {
            assert_eq!(
                fs::metadata(&first_path)?.permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(&second_path)?.permissions().mode() & 0o777,
                0o440
            );
        }
        let remaining = fs::read_dir(temp.path())?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<std::io::Result<Vec<_>>>()?;
        assert_eq!(
            remaining.len(),
            2,
            "only the original targets should remain"
        );
        Ok(())
    }

    #[test]
    fn publisher_retains_recovery_copy_when_rollback_fails() -> anyhow::Result<()> {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir()?;
        let first_path = temp.path().join("first.xml");
        let second_path = temp.path().join("second.xml");
        fs::write(&first_path, b"first-old")?;
        fs::write(&second_path, b"second-old")?;
        #[cfg(unix)]
        fs::set_permissions(&second_path, fs::Permissions::from_mode(0o400))?;
        let resolved_second_path = resolve_destination_path(&second_path)?;
        let files = [
            staged(&first_path, b"first-old", b"first-new")?,
            staged(&second_path, b"second-old", b"second-new")?,
        ];
        let mut calls = 0;

        let error = publish_file_group_with_ops(
            &files,
            |replacement| {
                calls += 1;
                if calls == 2 {
                    fs::remove_file(&replacement.target).map_err(|error| ReplaceFailure {
                        error,
                        original_removed: false,
                    })?;
                    Err(ReplaceFailure {
                        error: std::io::Error::other("injected replacement failure"),
                        original_removed: true,
                    })
                } else {
                    replace_target(replacement)
                }
            },
            |replacement| {
                if replacement.target == resolved_second_path {
                    Err(std::io::Error::other("injected rollback failure"))
                } else {
                    rollback_target(replacement)
                }
            },
        );
        let error = require_err(error, "rollback failure must be surfaced")?;

        assert!(error.to_string().contains("injected rollback failure"));
        let files = fs::read_dir(temp.path())?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        let recovery = files
            .iter()
            .find(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().contains("preserving-recovery"))
            })
            .ok_or_else(|| anyhow::anyhow!("rollback recovery copy was not retained"))?;
        assert!(error.to_string().contains(&recovery.display().to_string()));
        assert_eq!(fs::read(recovery)?, b"second-old");
        #[cfg(unix)]
        assert_eq!(fs::metadata(recovery)?.permissions().mode() & 0o777, 0o400);
        assert_eq!(fs::read(first_path)?, b"first-old");
        assert!(!second_path.exists());
        Ok(())
    }

    #[test]
    fn source_staging_directory_is_exclusive_and_private() -> anyhow::Result<()> {
        let temp = tempfile::tempdir()?;
        let directory = create_unique_directory(temp.path())?;
        fs::write(directory.path().join("source.xml"), b"fetched")?;
        directory.close()?;
        assert_eq!(fs::read_dir(temp.path())?.count(), 0);
        Ok(())
    }
}
