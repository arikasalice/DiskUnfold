//! F15 shutdown command and lifecycle event bodies (IF-06 and IF-07).
//!
//! The published F15 HLD and DLD define these operations and the event's
//! semantic values, but defer or omit the literal JSON field names and enum
//! tokens. The names and tokens below follow the repository's camelCase body
//! and snake_case enum conventions. They remain a type-only projection until
//! the design records those wire spellings and the host binds the command.

use crate::Timestamp;
use serde::{Deserialize, Serialize};

fn deserialize_confirmation<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    bool::deserialize(deserializer).map(Some)
}

/// Selects the shutdown behavior requested through IF-06 `app.quit`.
///
/// The tokens `normal` and `without_waiting` are inferred from F15 HLD rev 116
/// IF-06's two documented modes. This is a closed request enum: an unknown
/// mode is rejected so the host cannot guess how to shut down.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AppQuitMode {
    /// Stop accepting work and wait for the defined shutdown steps.
    Normal,
    /// Do not wait for remaining work after the user confirms its disposition.
    WithoutWaiting,
}

/// GUI request body for F15 HLD rev 116 IF-06 `app.quit`.
///
/// The correlation ID belongs to the shared [`crate::CallEnvelope`], not this
/// body. `confirm_unfinished_changes` is optional on the wire and defaults to
/// false. Literal field spelling is inferred; see the module documentation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppQuitRequest {
    /// Shutdown mode selected by the GUI; its two wire tokens are inferred.
    pub mode: AppQuitMode,
    /// Whether the user confirmed not waiting; omission means false and the
    /// wire property spelling is inferred.
    #[serde(
        default,
        deserialize_with = "deserialize_confirmation",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "typescript", ts(optional))]
    pub confirm_unfinished_changes: Option<bool>,
}

impl AppQuitRequest {
    /// Treats an omitted confirmation the same as an explicit `false`.
    pub const fn confirms_unfinished_changes(&self) -> bool {
        matches!(self.confirm_unfinished_changes, Some(true))
    }
}

/// Outcome values documented for the F15 IF-06 shutdown request.
///
/// The enum tokens follow the published outcome names. A newer host outcome
/// maps to `Unknown` so GUI code can avoid making a false assumption.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
pub enum AppQuitOutcome {
    /// Shutdown was accepted and the lifecycle event stream will report progress.
    #[serde(rename = "accepted")]
    Accepted,
    /// Shutdown was already in progress.
    #[serde(rename = "already_closing")]
    AlreadyClosing,
    /// Confirmation is required because change work is still in progress.
    #[serde(rename = "confirm_required")]
    ConfirmRequired,
    /// A newer host returned an outcome this GUI does not recognize.
    #[serde(rename = "unknown", other)]
    Unknown,
}

/// IF-06 response body returned for each `app.quit` request.
///
/// F15 HLD rev 116 specifies the in-progress change-work count for
/// `confirm_required`; the response property spelling and its absent-on-other
/// outcomes representation are inferred. This type is not yet bound to a host
/// command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AppQuitResponse {
    /// Accepted, already-closing, or confirmation-required outcome; the
    /// property spelling is inferred.
    pub outcome: AppQuitOutcome,
    /// In-progress change-work count for confirmation; the property spelling
    /// is inferred.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "typescript", ts(optional))]
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub in_progress_change_count: Option<u64>,
}

/// Lifecycle states documented by F15 HLD rev 116 and DLD rev 70.
///
/// The state tokens use the repository's snake_case convention. Unknown future
/// states become `Unknown`, which is safe for presentation and does not assert
/// that a newly added state is terminal or active.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[cfg_attr(feature = "typescript", ts(type = "string"))]
pub enum AppLifecycleState {
    /// The host is preparing the initial application session.
    #[serde(rename = "starting")]
    Starting,
    /// Startup is blocked by a required check or access condition.
    #[serde(rename = "blocked")]
    Blocked,
    /// The application is ready for normal use.
    #[serde(rename = "running")]
    Running,
    /// MCP access has been closed as the first shutdown stage.
    #[serde(rename = "access_closed")]
    AccessClosed,
    /// The host is draining or recording remaining work.
    #[serde(rename = "draining")]
    Draining,
    /// Shutdown has completed and the process is leaving.
    #[serde(rename = "closed")]
    Closed,
    /// The event producer sent a lifecycle state not known to this reader.
    #[serde(rename = "unknown", other)]
    Unknown,
}

/// IF-07 `app.lifecycle_changed` event body.
///
/// F15 HLD rev 116 / DLD rev 70 define the semantic fields as current state,
/// previous state, remaining work count, monotonically increasing state
/// sequence, and observation time. These camelCase property names are inferred;
/// the shared event metadata stays in [`crate::EventEnvelope`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AppLifecycleChanged {
    /// Current application lifecycle state; the property spelling is inferred.
    pub state: AppLifecycleState,
    /// State immediately before the transition; the property spelling is
    /// inferred.
    pub previous_state: AppLifecycleState,
    /// Number of jobs still unfinished during shutdown; the property spelling
    /// is inferred.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub remaining_job_count: u64,
    /// Monotonically increasing state sequence for this app session; the
    /// property spelling is inferred.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub state_sequence: u64,
    /// UTC observation time in the shared timestamp format; the property
    /// spelling is inferred.
    pub observed_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CallEnvelope, EventEnvelope, Id};
    use serde_json::json;

    #[test]
    fn quit_request_defaults_confirmation_and_rejects_unknown_input() {
        let normal: AppQuitRequest = serde_json::from_value(json!({"mode":"normal"}))
            .expect("normal shutdown request should deserialize");
        assert_eq!(normal.mode, AppQuitMode::Normal);
        assert_eq!(normal.confirm_unfinished_changes, None);
        assert!(!normal.confirms_unfinished_changes());
        assert_eq!(
            serde_json::to_value(normal).expect("default request serializes"),
            json!({"mode":"normal"})
        );

        let confirmed: AppQuitRequest = serde_json::from_value(json!({
            "mode":"without_waiting",
            "confirmUnfinishedChanges":true
        }))
        .expect("confirmed waitless request should deserialize");
        assert_eq!(confirmed.mode, AppQuitMode::WithoutWaiting);
        assert_eq!(confirmed.confirm_unfinished_changes, Some(true));
        assert!(confirmed.confirms_unfinished_changes());

        assert!(
            serde_json::from_value::<AppQuitRequest>(json!({
                "mode":"without_waiting",
                "futureField":true
            }))
            .is_err()
        );
        assert!(serde_json::from_value::<AppQuitRequest>(json!({"mode":"future_mode"})).is_err());
        assert!(
            serde_json::from_value::<AppQuitRequest>(
                json!({"mode":"normal","confirmUnfinishedChanges":null})
            )
            .is_err()
        );
    }

    #[test]
    fn quit_request_and_response_use_the_inferred_camel_case_shape() {
        let request = AppQuitRequest {
            mode: AppQuitMode::WithoutWaiting,
            confirm_unfinished_changes: Some(true),
        };
        assert_eq!(
            serde_json::to_value(request).expect("request serializes"),
            json!({"mode":"without_waiting","confirmUnfinishedChanges":true})
        );

        let response = AppQuitResponse {
            outcome: AppQuitOutcome::ConfirmRequired,
            in_progress_change_count: Some(2),
        };
        assert_eq!(
            serde_json::to_value(response).expect("response serializes"),
            json!({"outcome":"confirm_required","inProgressChangeCount":2})
        );

        let envelope = CallEnvelope {
            name: "app.quit".to_owned(),
            contract_version: 1,
            correlation_id: Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")
                .expect("valid UUIDv7 correlation id"),
            body: serde_json::to_value(AppQuitRequest {
                mode: AppQuitMode::Normal,
                confirm_unfinished_changes: None,
            })
            .expect("request body serializes"),
        };
        let wire = serde_json::to_value(envelope).expect("call envelope serializes");
        assert_eq!(wire["name"], "app.quit");
        assert!(wire["correlationId"].is_string());
        assert!(wire["body"].get("correlationId").is_none());
    }

    #[test]
    fn unknown_response_enums_and_extra_fields_are_tolerated() {
        assert_eq!(
            serde_json::from_value::<AppQuitResponse>(json!({
                "outcome":"future_outcome",
                "futureField":true
            }))
            .expect("unknown response values remain readable")
            .outcome,
            AppQuitOutcome::Unknown
        );

        let lifecycle: AppLifecycleChanged = serde_json::from_value(json!({
            "state":"future_state",
            "previousState":"running",
            "remainingJobCount":1,
            "stateSequence":3,
            "observedAt":"2026-11-13T00:00:00.000Z",
            "futureField":true
        }))
        .expect("event readers accept unknown fields and states");
        assert_eq!(lifecycle.state, AppLifecycleState::Unknown);
        assert_eq!(lifecycle.previous_state, AppLifecycleState::Running);
        assert_eq!(lifecycle.state_sequence, 3);
        assert_eq!(
            serde_json::to_value(AppLifecycleState::Unknown).expect("unknown state serializes"),
            json!("unknown")
        );
    }

    #[test]
    fn lifecycle_body_round_trips_inside_the_shared_event_envelope()
    -> Result<(), Box<dyn std::error::Error>> {
        let id = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
        let lifecycle = AppLifecycleChanged {
            state: AppLifecycleState::Draining,
            previous_state: AppLifecycleState::AccessClosed,
            remaining_job_count: 4,
            state_sequence: 2,
            observed_at: serde_json::from_value(json!("2026-11-13T00:00:00.000Z"))?,
        };
        let body = serde_json::to_value(&lifecycle)?;
        let envelope = EventEnvelope {
            topic: "app.lifecycle_changed".to_owned(),
            contract_version: 1,
            seq: 8,
            correlation_id: Some(id.clone()),
            job_ref: None,
            result_version: None,
            occurred_at: serde_json::from_value(json!("2026-11-13T00:00:00.000Z"))?,
            body,
        };
        assert!(envelope.validate());

        let wire = serde_json::to_value(&envelope)?;
        assert_eq!(wire["topic"], "app.lifecycle_changed");
        assert_eq!(wire["contractVersion"], 1);
        assert_eq!(wire["body"]["state"], "draining");
        assert_eq!(wire["body"]["previousState"], "access_closed");
        assert_eq!(wire["body"]["stateSequence"], 2);

        let unknown_envelope: EventEnvelope = serde_json::from_value(json!({
            "topic":"app.lifecycle_changed",
            "contractVersion":1,
            "seq":9,
            "occurredAt":"2026-11-13T00:00:00.000Z",
            "body":wire["body"],
            "futureEnvelopeField":true
        }))?;
        let unknown_body: AppLifecycleChanged = serde_json::from_value(unknown_envelope.body)?;
        assert_eq!(unknown_body.state, AppLifecycleState::Draining);
        Ok(())
    }
}
