use super::*;
use du_core_contracts::{CallEnvelope, ENVELOPE_VERSION, ResultEnvelope, ports::PortError};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};

struct Ids;
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes([1; 16])
    }
}
struct RandomByte(u8);
impl Random for RandomByte {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(self.0);
        Ok(())
    }
}
fn call(engine: &crate::api::Engine, name: &str, body: Value) -> ResultEnvelope {
    let id = Id::from_v7_bytes([3; 16]);
    let caller = CallerCtx::new(Channel::Gui, id.clone(), None);
    engine.call(
        CallEnvelope {
            name: name.into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: id,
            body,
        },
        &caller,
    )
}

#[test]
fn engine_channels_are_session_owned_and_retired() -> Result<(), Box<dyn std::error::Error>> {
    let retired = Arc::new(AtomicBool::new(false));
    let flag = retired.clone();
    let mut a = EngineBuilder::default();
    a.on_session_retire(move || flag.store(true, Ordering::Release));
    let channels = ViewChannels::register(&mut a, Arc::new(Ids), &RandomByte(11), 4)?;
    let a = a.build()?;
    let mut b = EngineBuilder::default();
    ViewChannels::register(&mut b, Arc::new(Ids), &RandomByte(12), 4)?;
    let b = b.build()?;
    let opened = call(&a, "viz.view.open", json!({}));
    assert!(opened.error.is_none());
    let channel: VizViewChannel = serde_json::from_value(opened.body)?;
    let wrong = call(&b, "viz.view.close", serde_json::to_value(&channel)?);
    assert_eq!(
        wrong.error.as_ref().map(|e| e.reason_key.as_str()),
        Some("ref.unavailable")
    );
    assert!(channels.validate(&channel.view_channel).is_ok());
    let closed = call(&a, "viz.view.close", serde_json::to_value(&channel)?);
    assert!(closed.error.is_none());
    assert_eq!(closed.body, json!({}));
    let repeated = call(&a, "viz.view.close", serde_json::to_value(&channel)?);
    assert_eq!(
        repeated.error.as_ref().map(|e| e.reason_key.as_str()),
        Some("ref.unavailable")
    );
    let next: VizViewChannel = serde_json::from_value(call(&a, "viz.view.open", json!({})).body)?;
    assert_ne!(channel.view_channel, next.view_channel);
    a.retire_window_session();
    assert!(retired.load(Ordering::Acquire));
    assert!(channels.validate(&next.view_channel).is_err());
    Ok(())
}

#[test]
fn invalid_newer_request_does_not_stop_or_advance_current() -> Result<(), Box<dyn std::error::Error>>
{
    let mut builder = EngineBuilder::default();
    let channels = ViewChannels::register(&mut builder, Arc::new(Ids), &RandomByte(13), 2)?;
    let engine = builder.build()?;
    let opened: VizViewChannel =
        serde_json::from_value(call(&engine, "viz.view.open", json!({})).body)?;
    let (first, ()) = channels.promote_validated(&opened.view_channel, 1, Ok::<_, &str>(()))??;
    assert_eq!(
        channels
            .promote_validated::<(), _>(&opened.view_channel, 3, Err("invalid"))?
            .err(),
        Some("invalid")
    );
    assert!(!first.is_stopped());
    let (second, ()) = channels.promote_validated(&opened.view_channel, 2, Ok::<_, &str>(()))??;
    assert!(first.is_stopped());
    assert!(!second.is_stopped());
    assert!(
        channels
            .promote_validated(&opened.view_channel, 1, Ok::<_, &str>(()))
            .is_err()
    );
    assert!(
        call(&engine, "viz.view.close", serde_json::to_value(&opened)?)
            .error
            .is_none()
    );
    assert!(second.is_stopped());
    Ok(())
}
