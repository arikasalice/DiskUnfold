//! An explicitly composed job-only subscription path with an atomic runtime cut.
use crate::subscribers::{
    RejoinToken, RuntimeJobCut, SubscriberBus, SubscriberMessage, Subscription,
};
use crate::{FlowLimits, JobClock, JobEvent, JobEvents, JobIdentity, JobRuntime, RuntimeError};
use du_core_contracts::{
    Id,
    job::{HistoryChanged, JobHistory},
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

/// A monotonic time source supplied by the composition point.
pub trait SubscriberClock: Send + Sync {
    /// Returns elapsed time from a stable origin.
    fn now(&self) -> Duration;
}

/// Dependencies of a runtime and its job-only subscriber bus.
pub struct RuntimeSubscribersConfig {
    /// Creates job identifiers.
    pub identity: Arc<dyn JobIdentity>,
    /// Supplies runtime timestamps.
    pub runtime_clock: Arc<dyn JobClock>,
    /// Supplies monotonic batching time.
    pub subscriber_clock: Arc<dyn SubscriberClock>,
    /// Stores job history.
    pub history: Arc<dyn JobHistory>,
    /// Receives every existing event and history notice unchanged.
    pub downstream: Arc<dyn JobEvents>,
    /// Sets the runtime's flow limits.
    pub limits: FlowLimits,
}

struct Fanout {
    bus: Arc<SubscriberBus<JobEvent>>,
    clock: Arc<dyn SubscriberClock>,
    downstream: Arc<dyn JobEvents>,
    failed: Arc<AtomicBool>,
}
impl JobEvents for Fanout {
    fn publish(&self, event: JobEvent) {
        if !matches!(event, JobEvent::ResultVersionChanged(_))
            && !self.bus.publish(event.clone(), self.clock.now())
        {
            self.failed.store(true, Ordering::Release);
        }
        self.downstream.publish(event);
    }
    fn publish_history_changed(&self, event: HistoryChanged) {
        self.downstream.publish_history_changed(event);
    }
    fn publish_progress(&self, event: crate::JobProgressEvent) {
        self.downstream.publish_progress(event);
    }
    fn retire_job(&self, id: &Id) {
        if !self.bus.retire_job(id) {
            self.failed.store(true, Ordering::Release);
        }
        self.downstream.retire_job(id);
    }
}

/// A runtime and the only job-event bus to which its snapshots are bound.
/// This composition is opt-in and is not installed by the desktop.
pub struct RuntimeSubscribers {
    runtime: Arc<JobRuntime>,
    bus: Arc<SubscriberBus<JobEvent>>,
    failed: Arc<AtomicBool>,
}
impl RuntimeSubscribers {
    /// Construct a runtime whose sequenced job events feed this instance's bus.
    pub fn new(config: RuntimeSubscribersConfig) -> Result<Self, RuntimeError> {
        let bus = Arc::new(SubscriberBus::default());
        let failed = Arc::new(AtomicBool::new(false));
        let sink = Arc::new(Fanout {
            bus: bus.clone(),
            clock: config.subscriber_clock,
            downstream: config.downstream,
            failed: failed.clone(),
        });
        let runtime = Arc::new(JobRuntime::new_with_limits(
            config.identity,
            config.runtime_clock,
            config.history,
            sink,
            config.limits,
        )?);
        Ok(Self {
            runtime,
            bus,
            failed,
        })
    }
    /// Return the runtime used by this composition.
    pub fn runtime(&self) -> Arc<JobRuntime> {
        self.runtime.clone()
    }
    /// Create a subscriber bound to this instance.
    pub fn subscribe(&self) -> Option<RuntimeSubscription> {
        Some(RuntimeSubscription {
            subscription: self.bus.subscribe()?,
            failed: self.failed.clone(),
        })
    }
    /// Capture a cut and rejoin under the fixed job-table then bus lock order.
    /// Returns none for a foreign, stale or already rejoined token.
    /// Only job state and terminal summaries are covered by this cut.
    pub fn rejoin(
        &self,
        subscriber: &RuntimeSubscription,
        token: RejoinToken,
    ) -> Result<Option<HashMap<Id, RuntimeJobCut>>, RuntimeError> {
        if self.failed.load(Ordering::Acquire) || subscriber.subscription.is_poisoned() {
            return Err(RuntimeError::DeliveryUnavailable);
        }
        if !self.bus.owns(&subscriber.subscription) {
            return Ok(None);
        }
        subscriber.subscription.rejoin_runtime(&self.runtime, token)
    }
}

/// A job-event subscription tied to one runtime composition.
pub struct RuntimeSubscription {
    subscription: Subscription<JobEvent>,
    failed: Arc<AtomicBool>,
}
impl RuntimeSubscription {
    /// Return the next eligible job event or the resynchronization notice.
    pub fn try_next(
        &self,
        now: Duration,
    ) -> Result<Option<SubscriberMessage<JobEvent>>, RuntimeError> {
        if self.failed.load(Ordering::Acquire) || self.subscription.is_poisoned() {
            return Err(RuntimeError::DeliveryUnavailable);
        }
        Ok(self.subscription.try_next(now))
    }
    /// Obtain a ticket for the current detached episode.
    pub fn begin_rejoin(&self) -> Result<Option<RejoinToken>, RuntimeError> {
        if self.failed.load(Ordering::Acquire) || self.subscription.is_poisoned() {
            return Err(RuntimeError::DeliveryUnavailable);
        }
        Ok(self.subscription.begin_rejoin())
    }
    /// Forget a terminal job only after the runtime retires it and delivery is drained.
    pub fn forget_retired_job(&self, id: &Id) -> bool {
        self.subscription.forget_retired_job(id)
    }
}

#[cfg(test)]
mod tests;
