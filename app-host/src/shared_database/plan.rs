use du_core_contracts::{
    approval_check::{
        ApprovalCheckFinishRecord, ApprovalCheckRecord, ApprovalCheckRecorder,
        ApprovalCheckStartRecord,
    },
    plan_version_store::{PlanVersionRecord, PlanVersionStore},
    ports::{PortError, PortErrorKind},
    ruleset::{RulesetRead, RulesetReadError, RulesetRow},
};

use super::SharedDatabase;
impl PlanVersionStore for SharedDatabase {
    fn append_fixed(&mut self, version: &PlanVersionRecord) -> Result<(), PortError> {
        self.0
            .connection
            .lock()
            .map_err(|_| database_port_error())?
            .append_fixed(version)
    }

    fn read(&self, version_id: &du_core_contracts::Id) -> Result<PlanVersionRecord, PortError> {
        self.0
            .connection
            .lock()
            .map_err(|_| database_port_error())?
            .read(version_id)
    }

    fn read_lineage(
        &self,
        lineage_id: &du_core_contracts::Id,
    ) -> Result<Vec<PlanVersionRecord>, PortError> {
        self.0
            .connection
            .lock()
            .map_err(|_| database_port_error())?
            .read_lineage(lineage_id)
    }
}

impl ApprovalCheckRecorder for SharedDatabase {
    fn append_start(&mut self, record: &ApprovalCheckStartRecord) -> Result<(), PortError> {
        self.0
            .connection
            .lock()
            .map_err(|_| database_port_error())?
            .append_start(record)
    }

    fn append_finish(&mut self, record: &ApprovalCheckFinishRecord) -> Result<(), PortError> {
        self.0
            .connection
            .lock()
            .map_err(|_| database_port_error())?
            .append_finish(record)
    }

    fn read_for_request(
        &self,
        request_version_id: &du_core_contracts::Id,
    ) -> Result<Vec<ApprovalCheckRecord>, PortError> {
        self.0
            .connection
            .lock()
            .map_err(|_| database_port_error())?
            .read_for_request(request_version_id)
    }

    fn read_for_job(
        &self,
        job_id: &du_core_contracts::Id,
    ) -> Result<Vec<ApprovalCheckRecord>, PortError> {
        self.0
            .connection
            .lock()
            .map_err(|_| database_port_error())?
            .read_for_job(job_id)
    }
}

impl RulesetRead for SharedDatabase {
    fn read_current(&self) -> Result<Option<RulesetRow>, RulesetReadError> {
        self.0
            .connection
            .lock()
            .map_err(|_| RulesetReadError::StorageUnavailable)?
            .read_current()
    }

    fn read_version(&self, version: i64) -> Result<Option<RulesetRow>, RulesetReadError> {
        self.0
            .connection
            .lock()
            .map_err(|_| RulesetReadError::StorageUnavailable)?
            .read_version(version)
    }
}

fn database_port_error() -> PortError {
    PortError {
        kind: PortErrorKind::Io,
        reason_key: "db.storage_unavailable".into(),
        os_code: None,
    }
}
