use super::*;

/// Re-judges one stored plan through the same current-facts pipeline used by F10.
///
/// F28 uses this handle from the app-host approval boundary. It is not registered
/// as an IPC command, so approval preparation remains behind the host's source and
/// window checks (FR1.HD1.app-host.DD1).
#[derive(Clone)]
pub struct ApprovalPrepareService {
    versions: Arc<dyn PlanVersionPort>,
    analysis: Arc<dyn PlanAnalysisSource>,
    target_check: Arc<dyn TargetCheckPort>,
    platform: Arc<dyn DomainPlatformFactsPort>,
    rulesets: Arc<dyn ProtectionRulesetPort>,
    clock: Arc<dyn ClockPort>,
    ids: Arc<dyn PlanIdPort>,
}

/// A stored plan and its current protection re-judgement.
pub struct ApprovalPreparedPlan {
    /// Verified immutable plan contents.
    pub version: du_change_planner::ports::PlanVersion,
    /// Current protection evaluation bound to one ruleset read.
    pub rejudgement: du_change_planner::ports::PlanRejudgement,
}

impl ApprovalPrepareService {
    /// Reads and re-judges a real plan version without writing storage or files.
    pub fn prepare(&self, version_id: &Id) -> Result<ApprovalPreparedPlan, &'static str> {
        let version = self.versions.read(version_id).map_err(|error| {
            if error.reason_key == "db.corrupt" {
                "db.corrupt"
            } else {
                "approval.request_not_found"
            }
        })?;
        let analysis = self
            .analysis
            .pin(&version.source_version_id)
            .map_err(|_| "approval.review_unavailable")?;
        let builder =
            du_change_planner::api::PlanBuilder::standard(du_change_planner::api::PlannerPorts {
                analysis: analysis.as_ref(),
                target_check: self.target_check.as_ref(),
                platform: self.platform.as_ref(),
                rulesets: self.rulesets.as_ref(),
                versions: self.versions.as_ref(),
                clock: self.clock.as_ref(),
                ids: self.ids.as_ref(),
            });
        let rejudgement = builder
            .rejudge(version_id)
            .map_err(|_| "approval.review_unavailable")?;
        Ok(ApprovalPreparedPlan {
            version,
            rejudgement,
        })
    }
}

impl super::PlanService {
    pub(super) fn approval_prepare_service(&self) -> ApprovalPrepareService {
        ApprovalPrepareService {
            versions: self.version_reader.clone(),
            analysis: self.analysis_source.clone(),
            target_check: self.target_check.clone(),
            platform: self.platform.clone(),
            rulesets: self.rulesets.clone(),
            clock: self.clock.clone(),
            ids: self.ids.clone(),
        }
    }
}
