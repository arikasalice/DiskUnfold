use super::*;
use du_core_contracts::scan_observation::{AnchoredDirectory, VolumeDevice};

impl AnchoredDirectory for WindowsDirectory {
    fn device(&self) -> VolumeDevice {
        crate::device::map_probe(crate::device::windows::probe_volume_guid_bytes(
            &self.volume_guid,
        ))
    }

    fn observe_directory_case(
        &self,
        clock: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> Option<du_core_contracts::scan_observation::DirectoryCaseEvidence> {
        let (result, observed_at) = self.case_sensitivity_with_clock(clock);
        Some(case_sensitivity::evidence(result, observed_at))
    }

    fn volume_space_reader(
        &self,
    ) -> Result<Box<dyn du_core_contracts::scan_observation::VolumeSpaceReader>, PortError> {
        let handle = self
            .handle
            .try_clone()
            .map_err(|e| error(e.raw_os_error().unwrap_or(0)))?;
        Ok(Box::new(Self {
            handle: Arc::new(handle),
            volume: self.volume,
            volume_guid: Arc::clone(&self.volume_guid),
            restart: true,
            pending: VecDeque::new(),
            filtered: None,
            correlated: false,
            correlated_records: VecDeque::new(),
            correlated_next_child_id: None,
            correlated_pending_observation: None,
            correlated_error: None,
            correlated_ended: false,
            filter_failed: false,
            finished: false,
            ancestors: None,
        }))
    }

    fn observed_modified_at(&self) -> Option<du_core_contracts::Timestamp> {
        timestamps::modified_at(&self.handle)
    }

    fn creates_no_shared_extents(&self) -> bool {
        extent_sharing::creates_no_shared_extents(&self.handle)
    }

    fn observed_modified_at_timed(
        &self,
        clock: &dyn Fn() -> Option<du_core_contracts::Timestamp>,
    ) -> (
        Option<du_core_contracts::Timestamp>,
        Option<du_core_contracts::Timestamp>,
    ) {
        let modified_at = timestamps::modified_at(&self.handle);
        let observed_at = modified_at.as_ref().and_then(|_| clock());
        (modified_at, observed_at)
    }

    fn open_child(&self, entry: &ObservedEntry) -> Result<Box<dyn AnchoredDirectory>, PortError> {
        self.open_child_directory(entry)
            .map(|directory| Box::new(directory) as Box<dyn AnchoredDirectory>)
    }

    fn ancestor_file_ids(&self) -> Option<&[Vec<u8>]> {
        self.ancestors.as_ref().map(|chain| chain.ids.as_slice())
    }

    fn observed_identity(&self) -> Option<du_core_contracts::scan_observation::HandleIdentity> {
        self.handle_identity()
    }
}
