//! Serve verified language resources and receive best-effort missing-key reports.

use std::sync::Arc;

use du_core_contracts::locale_bundle::{
    LocaleBundleRequest, LocaleBundleResponse, LocaleReportMissingRequest,
    LocaleReportMissingResponse,
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
};

use crate::locale::StartupLocale;

pub(super) fn register(builder: &mut EngineBuilder, locale: Arc<StartupLocale>) {
    builder.register(GetBundle(locale.clone()));
    builder.register(ReportMissing(locale));
}

struct GetBundle(Arc<StartupLocale>);

impl UseCase for GetBundle {
    const NAME: &'static str = "i18n.get_bundle";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = LocaleBundleRequest;
    type Response = LocaleBundleResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        self.0
            .registry()
            .bundle_response(&request)
            .ok_or_else(|| EngineError::new("locale.bundle_unavailable", vec![]))
    }
}

struct ReportMissing(Arc<StartupLocale>);

impl UseCase for ReportMissing {
    const NAME: &'static str = "i18n.report_missing";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = LocaleReportMissingRequest;
    type Response = LocaleReportMissingResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        Ok(self.0.registry().report_missing(&request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_engine_api::api::Engine;
    use serde_json::{Value, json};

    fn call(engine: &Engine, name: &str, body: Value) -> Result<Value, Box<dyn std::error::Error>> {
        let request = json!({"command":"du_call","args":{"envelope":{
            "name":name,"contractVersion":1,"correlationId":"018f64aa-7370-7c73-bd8b-34ca07efed18","body":body
        }}});
        let response =
            crate::ipc::dispatch(engine, &request.to_string()).ok_or("missing IPC response")?;
        Ok(serde_json::from_str(&response)?)
    }

    #[test]
    fn gui_dispatch_serves_the_verified_bundle_and_accepts_missing_keys()
    -> Result<(), Box<dyn std::error::Error>> {
        let locale = Arc::new(crate::locale::initialize(
            crate::locale::register()?,
            Some("en"),
        )?);
        let mut builder = EngineBuilder::default();
        register(&mut builder, locale.clone());
        let engine = builder.build()?;
        let body = call(&engine, "i18n.get_bundle", json!({"language":"ko"}))?;
        let revision = body["body"]["bundleRevision"]
            .as_str()
            .ok_or("missing revision")?;
        assert_eq!(body["body"]["bundle"]["fallbackLanguage"], "en");
        assert_eq!(
            call(
                &engine,
                "i18n.get_bundle",
                json!({"language":"ko","knownBundleRevision":revision})
            )?["body"],
            json!({"bundleRevision":revision})
        );
        let unknown = call(
            &engine,
            "i18n.get_bundle",
            json!({"language":"unregistered"}),
        )?;
        assert_eq!(unknown["error"]["reasonKey"], "locale.bundle_unavailable");

        let missing =
            json!({"language":"ko","entries":[{"key":"ui.fixture.missing","marker":"key"}]});
        assert_eq!(
            call(&engine, "i18n.report_missing", missing.clone())?["body"],
            json!({})
        );
        assert_eq!(
            call(&engine, "i18n.report_missing", missing)?["body"],
            json!({})
        );
        assert_eq!(locale.registry().missing_messages().len(), 1);
        let rejected = call(
            &engine,
            "i18n.report_missing",
            json!({"language":"ko","entries":[{"key":"ui.fixture.secret","marker":"key","value":"user data"}]}),
        )?;
        assert_eq!(rejected["error"]["reasonKey"], "command.body_invalid");
        assert_eq!(locale.registry().missing_messages().len(), 1);
        Ok(())
    }
}
