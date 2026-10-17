//! Durable job receipts (`jobs/<uuid>.json`). A job is recorded as running before
//! any download starts and receives exactly one terminal outcome. A receipt still
//! marked running at startup belonged to a launcher that stopped unexpectedly and
//! becomes `interrupted`; its staging directory is removed, while verified,
//! committed content stays reusable.

use std::fs;
use std::io;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::DownloadReport;
use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::install::lock::FinalInstallPlan;
use crate::storage::document::{self, Loaded, SCHEMA_VERSION};
use crate::storage::models::OperationKind;
use crate::storage::paths::{DataRoot, InstanceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobReceipt {
    pub schema_version: u64,
    pub job_id: Uuid,
    pub kind: OperationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<InstanceId>,
    pub created_at: DateTime<Utc>,
    pub state: JobState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<ErrorCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<DownloadReport>,
    /// The frozen plan of a new installation; a retry reuses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<Box<FinalInstallPlan>>,
}

fn save(root: &DataRoot, receipt: &JobReceipt) -> LauncherResult<()> {
    document::save(&root.job_receipt(receipt.job_id), receipt, None)
        .map_err(|e| LauncherError::from_io(&e, "Saving the job record failed"))
}

impl JobReceipt {
    /// Persists a new running job.
    pub fn start(
        root: &DataRoot,
        job_id: Uuid,
        kind: OperationKind,
        instance_id: Option<InstanceId>,
        plan: Option<FinalInstallPlan>,
    ) -> LauncherResult<Self> {
        let receipt = Self {
            schema_version: SCHEMA_VERSION,
            job_id,
            kind,
            instance_id,
            created_at: Utc::now(),
            state: JobState::Running,
            finished_at: None,
            error_code: None,
            downloads: None,
            plan: plan.map(Box::new),
        };
        save(root, &receipt)?;
        Ok(receipt)
    }

    pub fn load(root: &DataRoot, job_id: Uuid) -> LauncherResult<Option<Self>> {
        match document::load_limited::<Self>(
            &root.job_receipt(job_id),
            None,
            document::MAX_LARGE_DOCUMENT_BYTES,
        ) {
            Ok(Loaded::Valid(receipt)) => Ok(Some(receipt)),
            Ok(_) => Ok(None),
            Err(error) => Err(LauncherError::from_io(
                &error,
                "Reading the job record failed",
            )),
        }
    }

    /// Records the terminal outcome. A job that is already terminal keeps its
    /// first outcome; the call then returns `false`.
    pub fn finish(
        &mut self,
        root: &DataRoot,
        result: &LauncherResult<DownloadReport>,
    ) -> LauncherResult<bool> {
        if self.state != JobState::Running {
            return Ok(false);
        }
        let mut next = self.clone();
        next.finished_at = Some(Utc::now());
        match result {
            Ok(report) => {
                next.state = JobState::Succeeded;
                next.downloads = Some(report.clone());
            }
            Err(error) => {
                next.state = if error.code == ErrorCode::OperationCancelled {
                    JobState::Cancelled
                } else {
                    JobState::Failed
                };
                next.error_code = Some(error.code);
            }
        }
        save(root, &next)?;
        *self = next;
        Ok(true)
    }
}

/// All readable receipts (unreadable or newer ones are skipped).
pub fn list(root: &DataRoot) -> LauncherResult<Vec<JobReceipt>> {
    let entries = match fs::read_dir(root.jobs_dir()) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(LauncherError::from_io(&error, "Reading job records failed")),
    };
    let mut receipts = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name
            .strip_suffix(".json")
            .and_then(|s| Uuid::parse_str(s).ok())
        else {
            continue;
        };
        if let Ok(Some(receipt)) = JobReceipt::load(root, id) {
            receipts.push(receipt);
        }
    }
    receipts.sort_by_key(|r| r.created_at);
    Ok(receipts)
}

/// The frozen plan of the most recent unfinished installation of `instance`.
pub fn resumable_plan(
    root: &DataRoot,
    instance: InstanceId,
) -> LauncherResult<Option<FinalInstallPlan>> {
    Ok(list(root)?
        .into_iter()
        .rev()
        .filter(|r| r.instance_id == Some(instance) && r.kind == OperationKind::Install)
        .find_map(|r| match (r.state, r.plan) {
            (JobState::Succeeded, _) | (_, None) => None,
            (_, Some(plan)) => Some(*plan),
        }))
}

/// Startup reconciliation, run while holding the DATA_ROOT lock and before any
/// job can start: running receipts become interrupted and every staging
/// directory is removed. Returns the interrupted receipts.
pub fn reconcile_jobs(root: &DataRoot) -> LauncherResult<Vec<JobReceipt>> {
    let mut interrupted = Vec::new();
    for mut receipt in list(root)? {
        if receipt.state == JobState::Running {
            receipt.state = JobState::Interrupted;
            receipt.finished_at = Some(Utc::now());
            receipt.error_code = Some(ErrorCode::OperationInterrupted);
            save(root, &receipt)?;
            interrupted.push(receipt);
        }
    }
    match fs::remove_dir_all(root.downloads_dir()) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(LauncherError::from_io(
                &error,
                "Removing incomplete downloads failed",
            ));
        }
    }
    Ok(interrupted)
}
