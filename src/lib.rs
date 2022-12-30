use log::Log;
use rocket::{
    fairing::{Fairing, Info, Kind},
    Request, Response,
};
use sentry::ClientInitGuard;

pub use sentry::types::Dsn;

pub struct SentryLogger {
    inner: Box<dyn Log>,
}

impl SentryLogger {
    pub fn new(inner: Box<dyn Log>) -> SentryLogger {
        SentryLogger { inner }
    }

    pub fn init() {
        log::set_boxed_logger(Box::new(SentryLogger::new(Box::new(
            env_logger::builder().parse_default_env().build(),
        ))))
        .expect("failed to setup logging");
        log::set_max_level(log::LevelFilter::max());
    }
}

impl Log for SentryLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn || self.inner.enabled(metadata)
    }

    fn log(&self, record: &log::Record) {
        self.inner.log(record);

        if record.level() <= log::Level::Warn {
            // Choices here might need review in future.
            // The current mapping puts the location of the log function
            // as culprit, but sentry's documentation is extremely cagey
            // about where that sort of information needs to go, and in
            // general on when to use culprit vs transaction vs tags
            // vs extra.
            let uuid = sentry::types::Uuid::new_v4();
            let event = sentry::protocol::Event {
                event_id: uuid,
                message: Some(format!("{}", record.args())),
                logger: Some(record.target().into()),
                culprit: Some(format!(
                    "{}: {}:{}",
                    record.module_path().unwrap_or("(unknown_module)"),
                    record.file().unwrap_or("(unknown_file)"),
                    record.line().unwrap_or(0),
                )),
                level: match record.level() {
                    log::Level::Error => sentry::Level::Error,
                    log::Level::Warn => sentry::Level::Warning,
                    log::Level::Info => sentry::Level::Info,
                    log::Level::Debug => sentry::Level::Debug,
                    log::Level::Trace => sentry::Level::Debug,
                },
                ..Default::default()
            };
            sentry::capture_event(event);
        }
    }

    fn flush(&self) {
        todo!()
    }
}

pub struct SentryFairing {
    _guard: ClientInitGuard,
}

impl SentryFairing {
    pub fn new(dsn: Dsn, name: &'static str) -> SentryFairing {
        SentryFairing {
            _guard: sentry::init((
                dsn,
                sentry::ClientOptions {
                    release: option_env!("TAG").map(|v| v.into()),
                    environment: match std::env::var("ENVIRONMENT") {
                        Ok(v) => Some(v.into()),
                        Err(_) => None,
                    },
                    server_name: Some(name.into()),
                    ..Default::default()
                },
            )),
        }
    }
}

#[rocket::async_trait]
impl Fairing for SentryFairing {
    // This is a request and response fairing named "GET/POST Counter".
    fn info(&self) -> Info {
        Info {
            name: "Sentry",
            kind: Kind::Response,
        }
    }

    async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
        if response.status().code < 200 || response.status().code >= 400 {
            sentry::capture_message(
                &format!(
                    "Abnormal response {} ({}), on request for {} ({})",
                    response.status().code,
                    response.status().reason().unwrap_or("Unknown reason"),
                    request.uri(),
                    match request.route() {
                        Some(r) => match &r.name {
                            Some(name) => name,
                            None => "Unnamed route",
                        },
                        None => "No route associated",
                    },
                ),
                sentry::Level::Error,
            );
        }
    }
}
