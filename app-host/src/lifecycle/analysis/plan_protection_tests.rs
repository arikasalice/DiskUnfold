use super::plan_tests::{
    FakeAnalysis, PlanTestApp, assemble_with_protection_facts, ok, wait_for_fixed,
};
use du_core_contracts::protection_facts::{
    KnownLocationFact, KnownLocationId, LinkBoundaryFact, ObservedFileIdentity,
    ObservedProtectionItem, ProtectionFact, ProtectionFactsPort, ProtectionGuardFact,
    ProtectionOperation, VolumeRoleFact,
};
use serde_json::json;
use std::sync::Arc;

struct FixedProtectionFacts {
    unknown: bool,
}

impl FixedProtectionFacts {
    fn value<T>(&self, value: T) -> ProtectionFact<T> {
        if self.unknown {
            ProtectionFact::Unknown(du_core_contracts::protection_facts::fact_reason::PARTIAL)
        } else {
            ProtectionFact::Known(value)
        }
    }
}

impl ProtectionFactsPort for FixedProtectionFacts {
    fn identity(&self, item: &ObservedProtectionItem) -> ProtectionFact<ObservedFileIdentity> {
        if self.unknown {
            ProtectionFact::Unknown(du_core_contracts::protection_facts::fact_reason::PARTIAL)
        } else {
            item.identity.clone().map_or_else(
                || {
                    ProtectionFact::Unknown(
                        du_core_contracts::protection_facts::fact_reason::PARTIAL,
                    )
                },
                ProtectionFact::Known,
            )
        }
    }

    fn link_boundary(&self, _: &ObservedProtectionItem) -> ProtectionFact<LinkBoundaryFact> {
        self.value(LinkBoundaryFact::WithinVolume)
    }

    fn volume_role(&self, _: &ObservedProtectionItem) -> ProtectionFact<VolumeRoleFact> {
        self.value(VolumeRoleFact::Ordinary)
    }

    fn known_location(
        &self,
        _: &ObservedProtectionItem,
        _: du_core_contracts::ruleset::KnownLocationTableVersion,
    ) -> KnownLocationFact {
        if self.unknown {
            KnownLocationFact::Unknown(du_core_contracts::protection_facts::fact_reason::PARTIAL)
        } else {
            KnownLocationFact::Listed(KnownLocationId::UserHome)
        }
    }

    fn os_component(&self, _: &ObservedProtectionItem) -> ProtectionFact<bool> {
        self.value(false)
    }

    fn guard_age(&self, _: &ObservedProtectionItem) -> ProtectionFact<ProtectionGuardFact> {
        self.value(ProtectionGuardFact::Clear)
    }

    fn guard_permission(
        &self,
        _: &ObservedProtectionItem,
        _: ProtectionOperation,
    ) -> ProtectionFact<ProtectionGuardFact> {
        self.value(ProtectionGuardFact::Clear)
    }

    fn guard_disk_backed(&self, _: &ObservedProtectionItem) -> ProtectionFact<ProtectionGuardFact> {
        self.value(ProtectionGuardFact::Clear)
    }
}

#[test]
fn gui_ipc_builds_a_real_file_with_known_facts_and_rejects_unknown_facts() -> TestResult {
    let directory = tempfile::tempdir()?;
    let file = directory.path().join("candidate.txt");
    std::fs::write(&file, b"reviewable plan target")?;

    for unknown in [false, true] {
        let analysis = FakeAnalysis::real().with_path(file.clone());
        let item = analysis.item_ref(1);
        let app: PlanTestApp = assemble_with_protection_facts(
            analysis,
            Some(Arc::new(FixedProtectionFacts { unknown })),
        )?;
        let opened = ok(
            &app.engine,
            "plan.draft_open",
            json!({"source":"manual","itemRefs":[item]}),
        )?;
        let edited = ok(
            &app.engine,
            "plan.draft_edit",
            json!({
                "draftRef":opened["draftRef"],
                "editSeq":opened["editSeq"],
                "edit":{"kind":"set_action","itemRef":item,"action":"delete"}
            }),
        )?;
        let build = ok(
            &app.engine,
            "plan.build",
            json!({"draftRef":opened["draftRef"],"editSeq":edited["editSeq"]}),
        )?;
        let fixed = wait_for_fixed(&app.events, &app.engine, &build["jobRef"])?;
        if unknown {
            let result = ok(
                &app.engine,
                "plan.item_reason",
                json!({"planVersionId":fixed["planVersionId"],"itemNo":1}),
            )?;
            assert_eq!(result["disposition"], "rejected");
            assert_eq!(result["reasonKey"], "plan.item.undetermined");
        } else {
            let result = ok(
                &app.engine,
                "plan.get",
                json!({"planVersionId":fixed["planVersionId"],"section":"executable"}),
            )?;
            assert_eq!(result["items"].as_array().map(Vec::len), Some(1));
            assert_eq!(result["items"][0]["disposition"], "included");
            assert_eq!(result["items"][0]["action"], "delete");
        }
    }
    Ok(())
}

pub(super) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
