//! IF-01 to IF-04 through the GUI dispatcher with injected ports.
use super::*;
use crate::product_ipc::{ProductServices, parse_embedded, register, unavailable_product_info};
use crate::{shutdown::ShutdownError, startup_checks::StartupAccessSession};
use du_core_contracts::{
    CallEnvelope, Id,
    shutdown_ipc::{AppQuitRequest, AppQuitResponse},
    startup::{
        FullDiskAccessStatus, PrivacySettingsOpenOutcome, StartupDialogOutcome, WebViewAvailability,
    },
};
use du_core_contracts::{
    product_ipc::InstallForm,
    product_ports::{InstallFormPort, SelfExecutablePort},
    product_support::{
        BuildKind, EmbeddedProductInfo, ProductBuildInfo, ProductNotices, SecuritySupportBasis,
        SecuritySupportKind, SupportCapabilityObservation, SupportCapabilityState,
        SupportCombination, SupportCombinationIdentity, SupportCpu, SupportEnvironmentIdentity,
        SupportEnvironmentPort, SupportFeatureRow, SupportMatrix, SupportOsFamily, SupportStatus,
    },
};
use du_platform_os::startup::FakeStartupCapabilities;
use sha2::{Digest, Sha256};
use std::{
    io::{self, Cursor, Read},
    sync::{Condvar, Mutex},
    thread::ThreadId,
    time::Duration,
};

const ID: &str = "0195f856-ab83-7a59-9343-10d29775c9a9";
const COMMANDS: [&str; 4] = [
    "app.product_info",
    "app.notices",
    "app.executable_digest",
    "app.support_info",
];

/// Quit handler that never closes; the product commands never reach it.
struct NeverClosing;

impl crate::shutdown::host::QuitHandler for NeverClosing {
    fn request_quit(&self, _: &AppQuitRequest) -> Result<AppQuitResponse, ShutdownError> {
        Err(ShutdownError::Engine("not used".to_owned()))
    }

    fn is_closing(&self) -> bool {
        false
    }
}

type Gate = Arc<(Mutex<bool>, Condvar)>;

fn release(gate: &Gate) {
    if let Ok(mut open) = gate.0.lock() {
        *open = true;
    }
    gate.1.notify_all();
}

struct FakeEnvironment(SupportEnvironmentIdentity);

impl SupportEnvironmentPort for FakeEnvironment {
    fn identity(&self) -> SupportEnvironmentIdentity {
        self.0.clone()
    }

    fn capabilities(&self) -> Vec<SupportCapabilityObservation> {
        vec![SupportCapabilityObservation {
            area: "webview".into(),
            state: SupportCapabilityState::Limited,
            reason_key: Some("support.capability_limited".into()),
        }]
    }
}

struct FakeForm(InstallForm);

impl InstallFormPort for FakeForm {
    fn install_form(&self) -> InstallForm {
        self.0
    }
}

#[derive(Clone)]
enum Executable {
    Bytes(&'static [u8]),
    /// A deterministic image of the given size in bytes.
    Generated(usize),
    /// The first open fails; later opens return the bytes `abc`.
    FailsOnce,
    /// Opening waits until the gate is released, then returns the bytes `abc`.
    Blocked(Gate),
    OpenFails,
    ReadFails,
}

fn generated(length: usize) -> Vec<u8> {
    (0..length)
        .map(|index| u8::try_from(index % 251).unwrap_or(0))
        .collect()
}

struct FakeExecutable(Executable, Arc<AtomicUsize>, Arc<Mutex<Vec<ThreadId>>>);

struct FailingRead;

impl Read for FailingRead {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("injected read failure"))
    }
}

impl SelfExecutablePort for FakeExecutable {
    fn open(&self) -> io::Result<Box<dyn Read + Send>> {
        let attempt = self.1.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut threads) = self.2.lock() {
            threads.push(std::thread::current().id());
        }
        match &self.0 {
            Executable::Bytes(bytes) => Ok(Box::new(Cursor::new(bytes.to_vec()))),
            Executable::Generated(length) => Ok(Box::new(Cursor::new(generated(*length)))),
            Executable::FailsOnce if attempt == 1 => Err(io::Error::from(io::ErrorKind::NotFound)),
            Executable::FailsOnce => Ok(Box::new(Cursor::new(b"abc".to_vec()))),
            Executable::Blocked(gate) => {
                let mut open = gate.0.lock().map_err(|_| io::Error::other("poisoned"))?;
                while !*open {
                    open = gate
                        .1
                        .wait(open)
                        .map_err(|_| io::Error::other("poisoned"))?;
                }
                Ok(Box::new(Cursor::new(b"abc".to_vec())))
            }
            Executable::OpenFails => Err(io::Error::from(io::ErrorKind::NotFound)),
            Executable::ReadFails => Ok(Box::new(FailingRead)),
        }
    }
}

fn identity(complete: bool) -> SupportEnvironmentIdentity {
    if complete {
        SupportEnvironmentIdentity {
            os_family: SupportOsFamily::Linux,
            release: Some("24.04".into()),
            edition: Some("ubuntu".into()),
            cpu: SupportCpu::X64,
        }
    } else {
        SupportEnvironmentIdentity {
            os_family: SupportOsFamily::Unknown,
            release: None,
            edition: None,
            cpu: SupportCpu::Unknown,
        }
    }
}

fn matrix() -> SupportMatrix {
    SupportMatrix {
        revision: Some("sm-7".into()),
        as_of: Some("2026-11-01".into()),
        combinations: vec![SupportCombination {
            identity: SupportCombinationIdentity {
                os_family: SupportOsFamily::Linux,
                release: Some("24.04".into()),
                edition: Some("ubuntu".into()),
                cpu: SupportCpu::X64,
            },
            status: SupportStatus::OfficiallyVerified,
            security_support: Some(SecuritySupportBasis {
                source: Some("vendor lifecycle page".into()),
                checked_on: Some("2026-10-30".into()),
                kind: Some(SecuritySupportKind::General),
            }),
            verified_details: vec!["container".into()],
            verification_records: vec!["record-1".into()],
            features: vec![SupportFeatureRow {
                area: "webview".into(),
                status: SupportStatus::OfficiallyVerified,
                conditions: Vec::new(),
            }],
        }],
    }
}

fn official() -> EmbeddedProductInfo {
    EmbeddedProductInfo {
        build: ProductBuildInfo {
            product_version: Some("0.0.1".into()),
            build_kind: BuildKind::Official,
            commit: Some("abc123".into()),
            built_at_utc: Some("2026-11-01T00:00:00Z".into()),
            target_os: Some("linux".into()),
            target_cpu: Some("x86_64".into()),
            app_db_format: Some(4),
            snapshot_format: Some(2),
            support_matrix_revision: Some("sm-7".into()),
        },
        notices: ProductNotices {
            license: Some("LICENSE TEXT".into()),
            third_party: Some("THIRD PARTY TEXT".into()),
            source_commit: Some("abc123".into()),
        },
        support_matrix: Some(matrix()),
    }
}

struct Harness {
    engine: du_engine_api::api::Engine,
    opens: Arc<AtomicUsize>,
    threads: Arc<Mutex<Vec<ThreadId>>>,
    access: StartupAccessSession,
}

impl Harness {
    fn new(
        info: EmbeddedProductInfo,
        complete: bool,
        form: InstallForm,
        executable: Executable,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_wait(info, complete, form, executable, None)
    }

    fn with_wait(
        info: EmbeddedProductInfo,
        complete: bool,
        form: InstallForm,
        executable: Executable,
        wait: Option<Duration>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let opens = Arc::new(AtomicUsize::new(0));
        let threads = Arc::new(Mutex::new(Vec::new()));
        let mut services = ProductServices::new(
            info,
            Arc::new(FakeEnvironment(identity(complete))),
            Arc::new(FakeForm(form)),
            Arc::new(FakeExecutable(executable, opens.clone(), threads.clone())),
        );
        if let Some(wait) = wait {
            services = services.with_digest_wait(wait);
        }
        Self::from_services(services, opens, threads)
    }

    fn from_services(
        services: ProductServices,
        opens: Arc<AtomicUsize>,
        threads: Arc<Mutex<Vec<ThreadId>>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut builder = EngineBuilder::default();
        register(&mut builder, services);
        let capabilities = Arc::new(FakeStartupCapabilities::new(
            FullDiskAccessStatus::Granted,
            WebViewAvailability::Unknown,
            PrivacySettingsOpenOutcome::RequestAccepted,
            StartupDialogOutcome::Acknowledged,
        ));
        Ok(Self {
            engine: builder.build()?,
            opens,
            threads,
            access: StartupAccessSession::new(capabilities),
        })
    }

    /// Dispatches through the production `dispatch_with_shutdown` entry used by the GUI.
    fn call(&self, name: &str, body: Value) -> Result<Value, Box<dyn std::error::Error>> {
        let request = json!({"command":"du_call", "args":{"envelope":{
            "name":name, "contractVersion":1, "correlationId":ID, "body":body,
        }}});
        let reply = dispatch_with_shutdown(
            &self.engine,
            &request.to_string(),
            &self.access,
            &NeverClosing,
        )
        .ok_or("missing response")?;
        Ok(serde_json::from_str(&reply)?)
    }
}

#[test]
fn product_info_reports_embedded_values_and_the_observed_install_form()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = Harness::new(official(), true, InstallForm::Deb, Executable::OpenFails)?;
    let reply = harness.call("app.product_info", json!({}))?;
    assert!(reply.get("error").is_none(), "{reply}");
    let body = &reply["body"];
    assert_eq!(body["productVersion"], "0.0.1");
    assert_eq!(body["buildKind"], "official");
    assert_eq!(body["commit"], "abc123");
    assert_eq!(body["targetOs"], "linux");
    assert_eq!(body["installForm"], "deb");
    assert_eq!(body["installFormReasonKey"], Value::Null);
    assert_eq!(body["appDbFormat"], 4);
    assert_eq!(body["snapshotFormat"], 2);
    assert_eq!(body["supportMatrixRevision"], "sm-7");
    assert_eq!(body["supportMatrixAsOf"], "2026-11-01");
    assert_eq!(
        body["notices"],
        json!([
            {"kind":"LICENSE","available":true},
            {"kind":"THIRD-PARTY-NOTICES","available":true},
        ])
    );
    assert_eq!(
        harness.opens.load(Ordering::SeqCst),
        0,
        "product info must not read the executable"
    );
    Ok(())
}

#[test]
fn unavailable_build_values_and_unknown_install_form_are_reported_as_unavailable()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = Harness::new(
        unavailable_product_info(),
        true,
        InstallForm::Unknown,
        Executable::OpenFails,
    )?;
    let body = harness.call("app.product_info", json!({}))?["body"].clone();
    for key in [
        "productVersion",
        "commit",
        "builtAtUtc",
        "targetOs",
        "targetCpu",
        "appDbFormat",
        "snapshotFormat",
        "supportMatrixRevision",
        "supportMatrixAsOf",
    ] {
        assert_eq!(body[key], Value::Null, "{key}");
    }
    assert_eq!(body["buildKind"], "unknown");
    assert_eq!(body["installForm"], "unknown");
    assert_eq!(body["installFormReasonKey"], "product.install_form_unknown");
    assert_eq!(body["notices"][0]["available"], false);
    assert_eq!(body["notices"][1]["available"], false);
    Ok(())
}

#[test]
fn developer_builds_keep_their_marker_and_do_not_claim_a_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let mut info = official();
    info.build.build_kind = BuildKind::Developer;
    info.build.commit = None;
    info.notices.third_party = None;
    info.notices.source_commit = None;
    let harness = Harness::new(info, true, InstallForm::Portable, Executable::OpenFails)?;
    let body = harness.call("app.product_info", json!({}))?["body"].clone();
    assert_eq!(body["buildKind"], "developer");
    assert_eq!(body["commit"], Value::Null);
    assert_eq!(body["notices"][1]["available"], false);
    Ok(())
}

#[test]
fn notices_return_embedded_text_and_report_missing_documents_as_absent()
-> Result<(), Box<dyn std::error::Error>> {
    let mut info = official();
    info.notices.third_party = None;
    let harness = Harness::new(info, true, InstallForm::Deb, Executable::OpenFails)?;
    let license = harness.call("app.notices", json!({"kind":"LICENSE"}))?["body"].clone();
    assert_eq!(license["text"], "LICENSE TEXT");
    assert_eq!(license["sourceCommit"], "abc123");
    let third_party =
        harness.call("app.notices", json!({"kind":"THIRD-PARTY-NOTICES"}))?["body"].clone();
    assert_eq!(third_party["kind"], "THIRD-PARTY-NOTICES");
    assert_eq!(third_party["text"], Value::Null);
    Ok(())
}

#[test]
fn unknown_notice_kinds_and_malformed_bodies_are_invalid_arguments()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = Harness::new(official(), true, InstallForm::Deb, Executable::OpenFails)?;
    for (name, body) in [
        ("app.notices", json!({"kind":"NOTICE"})),
        ("app.notices", json!({"kind":"license"})),
        ("app.notices", json!({})),
        ("app.product_info", json!({"extra":1})),
        ("app.support_info", json!(null)),
        ("app.executable_digest", json!({})),
    ] {
        let reply = harness.call(name, body)?;
        assert_eq!(reply["name"], name);
        assert_eq!(reply["error"]["code"], "INVALID_ARGUMENT", "{name}");
        assert_eq!(
            reply["error"]["reasonKey"], "command.body_invalid",
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn executable_digest_is_computed_once_and_not_reported_as_verified()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = Harness::new(
        official(),
        true,
        InstallForm::Deb,
        Executable::Bytes(b"abc"),
    )?;
    let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    for _ in 0..2 {
        let body =
            harness.call("app.executable_digest", json!({"correlationId":ID}))?["body"].clone();
        assert_eq!(body["sha256"], expected);
        assert_eq!(body["correlationId"], ID);
        assert_eq!(body["reasonKey"], Value::Null);
        assert!(body.get("verified").is_none());
    }
    assert_eq!(harness.opens.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn unreadable_executable_is_an_unavailable_value_not_an_error()
-> Result<(), Box<dyn std::error::Error>> {
    for executable in [Executable::OpenFails, Executable::ReadFails] {
        let harness = Harness::new(official(), true, InstallForm::Deb, executable)?;
        let reply = harness.call("app.executable_digest", json!({"correlationId":ID}))?;
        assert!(reply.get("error").is_none(), "{reply}");
        assert_eq!(reply["body"]["sha256"], Value::Null);
        assert_eq!(reply["body"]["reasonKey"], "product.digest_unavailable");
    }
    Ok(())
}

#[test]
fn support_info_matches_the_embedded_row_and_keeps_security_basis_separate()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = Harness::new(official(), true, InstallForm::Deb, Executable::OpenFails)?;
    let body = harness.call("app.support_info", json!({}))?["body"].clone();
    assert_eq!(body["matrixRevision"], "sm-7");
    assert_eq!(body["asOf"], "2026-11-01");
    assert_eq!(body["environment"]["release"], "24.04");
    assert_eq!(body["combinationStatus"], "officially_verified");
    assert_eq!(body["securitySupport"]["kind"], "general");
    assert_eq!(body["combinations"].as_array().map(Vec::len), Some(1));
    let feature = &body["features"][0];
    assert_eq!(feature["area"], "webview");
    assert_eq!(feature["supportTableStatus"], "officially_verified");
    assert_eq!(feature["currentCapability"], "limited");
    assert_eq!(feature["reasonKey"], "support.capability_limited");
    Ok(())
}

#[test]
fn unidentified_platform_is_unknown_and_never_reported_as_verified()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = Harness::new(
        official(),
        false,
        InstallForm::Unknown,
        Executable::OpenFails,
    )?;
    let reply = harness.call("app.support_info", json!({}))?;
    assert!(reply.get("error").is_none(), "{reply}");
    assert_eq!(reply["body"]["combinationStatus"], "unknown");
    assert_eq!(reply["body"]["reasonKey"], "support.env_unknown");
    assert_eq!(reply["body"]["environment"]["osFamily"], "unknown");
    Ok(())
}

#[test]
fn missing_embedded_matrix_leaves_a_known_environment_unknown()
-> Result<(), Box<dyn std::error::Error>> {
    let harness = Harness::new(
        unavailable_product_info(),
        true,
        InstallForm::Deb,
        Executable::OpenFails,
    )?;
    let body = harness.call("app.support_info", json!({}))?["body"].clone();
    assert_eq!(body["combinationStatus"], "unknown");
    assert_eq!(body["reasonKey"], "support.matrix_unavailable");
    assert_eq!(body["combinations"], json!([]));
    Ok(())
}
mod extra;
