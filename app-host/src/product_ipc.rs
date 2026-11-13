//! F15 product and support IPC handlers.
//!
//! Implements HLD rev 116 IF-01 `app.product_info`, IF-02 `app.notices`,
//! IF-03 `app.executable_digest` and IF-04 `app.support_info` for DLD rev 70
//! FR5.HD2.app-host.DD1 and FR6.HD2.app-host.DD1. Handlers read only embedded
//! build values and operating-system ports. They open no database and no scan state.
use std::{
    io::Read,
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use du_core_contracts::{
    product_ipc::{
        ExecutableDigestRequest, ExecutableDigestResponse, NoticeAvailability, NoticeKind,
        NoticesRequest, NoticesResponse, ProductInfo, ProductInfoRequest, SupportInfo,
        SupportInfoRequest,
    },
    product_ports::{InstallFormPort, SelfExecutablePort},
    product_support::{
        BuildKind, EmbeddedProductInfo, ProductBuildInfo, ProductNotices, SupportEnvironmentPort,
    },
};
use du_engine_api::{
    api::{CallerCtx, Channel, EngineBuilder, UseCase},
    error::EngineError,
    support,
};
use sha2::{Digest, Sha256};

const DIGEST_UNAVAILABLE: &str = "product.digest_unavailable";
const INSTALL_FORM_UNKNOWN: &str = "product.install_form_unknown";
/// Reason key used when launcher-supplied product metadata does not parse.
///
/// The key is not yet in the published reason-key list and needs design registration.
pub(crate) const METADATA_INVALID: &str = "product.metadata_invalid";
/// Longest time one digest request waits for a running hash before reporting it unavailable.
const DIGEST_WAIT: Duration = Duration::from_secs(30);

/// Parses the product metadata JSON embedded by the launcher build.
///
/// Any parse failure maps to `product.metadata_invalid` so a broken build cannot
/// present invented product values.
pub(crate) fn parse_embedded(json: &str) -> Result<EmbeddedProductInfo, std::io::Error> {
    serde_json::from_str(json).map_err(|_| std::io::Error::other(METADATA_INVALID))
}

/// State of the executable digest computation.
enum Slot {
    /// No attempt has been made yet.
    Idle,
    /// A worker thread is hashing the executable.
    Running,
    /// The successful digest, cached for the rest of the run.
    Done(String),
    /// The latest attempt failed; the next request starts a new attempt.
    Failed,
}

/// Computes the executable digest on a worker thread so no IPC lane hashes inline.
struct DigestJob {
    executable: Arc<dyn SelfExecutablePort>,
    slot: Mutex<Slot>,
    finished: Condvar,
    wait: Duration,
}

impl DigestJob {
    fn lock(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts a hashing attempt unless one is running or a digest is already cached.
    fn start(self: &Arc<Self>) {
        let mut slot = self.lock();
        if matches!(*slot, Slot::Running | Slot::Done(_)) {
            return;
        }
        *slot = Slot::Running;
        let job = self.clone();
        let spawned = std::thread::Builder::new()
            .name("diskunfold-exe-digest".into())
            .spawn(move || {
                let result = hash_executable(job.executable.as_ref());
                *job.lock() = result.map_or(Slot::Failed, Slot::Done);
                job.finished.notify_all();
            });
        if spawned.is_err() {
            *slot = Slot::Failed;
        }
    }

    /// Returns the digest, waiting at most `wait` for a running attempt.
    ///
    /// Only a successful digest is cached; after a failure the next call retries.
    fn get(self: &Arc<Self>) -> Result<String, ()> {
        self.start();
        let deadline = Instant::now() + self.wait;
        let mut state = self.lock();
        while matches!(*state, Slot::Running) {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            state = self
                .finished
                .wait_timeout(state, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        match &*state {
            Slot::Done(digest) => Ok(digest.clone()),
            _ => Err(()),
        }
    }
}

fn hash_executable(executable: &dyn SelfExecutablePort) -> Result<String, ()> {
    let mut reader = executable.open().map_err(|_| ())?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0_u8; 64 * 1024];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => hasher.update(&chunk[..read]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err(()),
        }
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Embedded values and ports shared by the four product commands.
pub(crate) struct ProductServices {
    info: EmbeddedProductInfo,
    environment: Arc<dyn SupportEnvironmentPort>,
    install_form: Arc<dyn InstallFormPort>,
    digest: Arc<DigestJob>,
}

impl ProductServices {
    /// Composes the services from explicit ports. Tests inject fakes here.
    pub(crate) fn new(
        info: EmbeddedProductInfo,
        environment: Arc<dyn SupportEnvironmentPort>,
        install_form: Arc<dyn InstallFormPort>,
        executable: Arc<dyn SelfExecutablePort>,
    ) -> Self {
        Self {
            info,
            environment,
            install_form,
            digest: Arc::new(DigestJob {
                executable,
                slot: Mutex::new(Slot::Idle),
                finished: Condvar::new(),
                wait: DIGEST_WAIT,
            }),
        }
    }

    /// Shortens the digest wait; tests use it to observe a still-running hash.
    #[cfg(test)]
    pub(crate) fn with_digest_wait(mut self, wait: Duration) -> Self {
        if let Some(job) = Arc::get_mut(&mut self.digest) {
            job.wait = wait;
        }
        self
    }

    /// Composes the services with the production operating-system adapters.
    ///
    /// The executable digest starts hashing on a worker thread right away, so a
    /// later `app.executable_digest` request normally finds it finished (HLD IF-03).
    pub(crate) fn system(info: EmbeddedProductInfo) -> Self {
        let services = Self::new(
            info,
            Arc::new(du_platform_os::support_environment::SystemSupportEnvironment),
            Arc::new(du_platform_os::install_form::SystemInstallForm),
            Arc::new(du_platform_os::self_executable::SystemSelfExecutable),
        );
        services.digest.start();
        services
    }

    fn product_info(&self) -> ProductInfo {
        let build = &self.info.build;
        let install_form = self.install_form.install_form();
        let known = install_form != du_core_contracts::product_ipc::InstallForm::Unknown;
        let matrix = support::embedded_matrix(&self.info);
        ProductInfo {
            product_version: build.product_version.clone(),
            build_kind: build.build_kind,
            commit: build.commit.clone(),
            built_at_utc: build.built_at_utc.clone(),
            target_os: build.target_os.clone(),
            target_cpu: build.target_cpu.clone(),
            install_form,
            install_form_reason_key: (!known).then(|| INSTALL_FORM_UNKNOWN.to_owned()),
            app_db_format: build.app_db_format,
            snapshot_format: build.snapshot_format,
            support_matrix_revision: matrix.and_then(|matrix| {
                build
                    .support_matrix_revision
                    .clone()
                    .or_else(|| matrix.revision.clone())
            }),
            support_matrix_as_of: matrix.and_then(|matrix| matrix.as_of.clone()),
            notices: [NoticeKind::License, NoticeKind::ThirdPartyNotices]
                .into_iter()
                .map(|kind| NoticeAvailability {
                    kind,
                    available: self.notice_text(kind).is_some(),
                })
                .collect(),
        }
    }

    fn notice_text(&self, kind: NoticeKind) -> Option<&str> {
        let notices = &self.info.notices;
        match kind {
            NoticeKind::License => notices.license.as_deref(),
            NoticeKind::ThirdPartyNotices => notices.third_party.as_deref(),
        }
        .filter(|text| !text.trim().is_empty())
    }

    fn support_info(&self) -> SupportInfo {
        let matrix = support::embedded_matrix(&self.info)
            .cloned()
            .unwrap_or_default();
        support::evaluate(
            &matrix,
            self.environment.identity(),
            &self.environment.capabilities(),
        )
    }
}

/// Product info used when the launcher supplies none; every value is reported unavailable.
///
/// Entry points that carry no build information call this. It logs loudly so a
/// launcher that forgot to pass its build values is noticed.
pub(crate) fn unavailable_product_info() -> EmbeddedProductInfo {
    eprintln!(
        "DiskUnfold: product build information was not supplied by this entry point; \
         product and support information will be reported as unavailable"
    );
    EmbeddedProductInfo {
        build: ProductBuildInfo {
            product_version: None,
            build_kind: BuildKind::Unknown,
            commit: None,
            built_at_utc: None,
            target_os: None,
            target_cpu: None,
            app_db_format: None,
            snapshot_format: None,
            support_matrix_revision: None,
        },
        notices: ProductNotices {
            license: None,
            third_party: None,
            source_commit: None,
        },
        support_matrix: None,
    }
}

/// Registers IF-01 to IF-04 with the composed GUI engine.
pub(crate) fn register(builder: &mut EngineBuilder, services: ProductServices) {
    let services = Arc::new(services);
    builder.register(ProductInfoQuery(services.clone()));
    builder.register(NoticesQuery(services.clone()));
    builder.register(ExecutableDigestQuery(services.clone()));
    builder.register(SupportInfoQuery(services));
}

struct ProductInfoQuery(Arc<ProductServices>);

impl UseCase for ProductInfoQuery {
    const NAME: &'static str = "app.product_info";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ProductInfoRequest;
    type Response = ProductInfo;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        Ok(self.0.product_info())
    }
}

struct NoticesQuery(Arc<ProductServices>);

impl UseCase for NoticesQuery {
    const NAME: &'static str = "app.notices";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = NoticesRequest;
    type Response = NoticesResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        Ok(NoticesResponse {
            kind: request.kind,
            text: self.0.notice_text(request.kind).map(str::to_owned),
            source_commit: self.0.info.notices.source_commit.clone(),
        })
    }
}

struct ExecutableDigestQuery(Arc<ProductServices>);

impl UseCase for ExecutableDigestQuery {
    const NAME: &'static str = "app.executable_digest";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ExecutableDigestRequest;
    type Response = ExecutableDigestResponse;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(
        &self,
        request: Self::Request,
        _: &CallerCtx,
    ) -> Result<Self::Response, EngineError> {
        let (sha256, reason_key) = match self.0.digest.get() {
            Ok(digest) => (Some(digest), None),
            Err(()) => (None, Some(DIGEST_UNAVAILABLE.to_owned())),
        };
        Ok(ExecutableDigestResponse {
            correlation_id: request.correlation_id,
            sha256,
            reason_key,
        })
    }
}

struct SupportInfoQuery(Arc<ProductServices>);

impl UseCase for SupportInfoQuery {
    const NAME: &'static str = "app.support_info";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = SupportInfoRequest;
    type Response = SupportInfo;

    fn check_scope(&self, _: &Self::Request, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: Self::Request, _: &CallerCtx) -> Result<Self::Response, EngineError> {
        Ok(self.0.support_info())
    }
}
