//! Result version event publisher.
use crate::{JobRuntime, delivery::enqueue, model::*};
use du_core_contracts::result_events::{ResultVersionNotice, ResultVersionPublisher};
use std::sync::{Arc, Mutex};

struct ResultPublisher(std::sync::Weak<Mutex<Table>>);

impl ResultVersionPublisher for ResultPublisher {
    fn publish_result_version(&self, notice: ResultVersionNotice) {
        if notice.validate()
            && let Some(table) = self.0.upgrade()
            && let Ok(table) = table.lock()
            && !table.shutting_down
        {
            let event = JobEvent::ResultVersionChanged(Box::new(notice));
            let _ = enqueue(&table, Delivery::Event(event));
        }
    }
}

impl JobRuntime {
    /// Shares the event lane without giving queued job bodies ownership of their runtime.
    pub fn result_version_publisher(&self) -> Arc<dyn ResultVersionPublisher> {
        Arc::new(ResultPublisher(Arc::downgrade(&self.table)))
    }
}
