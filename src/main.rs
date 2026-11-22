mod account_deletion;
mod auth;
mod http;
mod mail;
mod media;
mod model;
mod schedule_time;
mod totp;
mod view;

use std::{
    env, io,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use auth::AuthStore;
use http::{Request, Response};
use mail::AccountMail;
use model::State;

const DEFAULT_PORT: u16 = 4173;
const PUBLISH_INTERVAL: Duration = Duration::from_secs(1);
const MAINTENANCE_RETRY: Duration = Duration::from_secs(10);

fn main() {
    if let Err(error) = run() {
        eprintln!("Commonline: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut args = env::args().skip(1);
    let state_path = env::var_os("COMMONLINE_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("commonline.state"));
    let port = match args.next().as_deref() {
        Some("--help" | "-h") => {
            println!("Usage: commonline [--port PORT | --legacy-recovery]");
            println!("COMMONLINE_PORT sets the default port ({DEFAULT_PORT}). Use 0 for an available port.");
            println!(
                "COMMONLINE_STATE sets the path to the state file (default: ./commonline.state)."
            );
            println!(
                "Stop the server before using --legacy-recovery to claim a pre-account profile."
            );
            return Ok(());
        }
        Some("--legacy-recovery") if args.next().is_none() => {
            if !state_path.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "No existing state file to recover.",
                ));
            }
            let state = State::load(&state_path)?;
            let mut auth = AuthStore::load(&state_path)?;
            let username = state.legacy_username().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "No pre-account profile to recover.",
                )
            })?;
            let code = auth
                .issue_legacy_recovery(username)
                .map_err(io::Error::other)?;
            println!("Recovery code for {username}: {code}");
            return Ok(());
        }
        Some("--port") => parse_port(&args.next().ok_or_else(usage_error)?)?,
        Some(_) => return Err(usage_error()),
        None => match env::var("COMMONLINE_PORT") {
            Ok(value) => parse_port(&value)?,
            Err(env::VarError::NotPresent) => DEFAULT_PORT,
            Err(error) => return Err(io::Error::new(io::ErrorKind::InvalidInput, error)),
        },
    };
    if args.next().is_some() {
        return Err(usage_error());
    }
    let state = State::load(&state_path).map_err(|error| {
        io::Error::new(error.kind(), format!("{}: {error}", state_path.display()))
    })?;
    let mut app = App {
        state,
        auth: AuthStore::load(&state_path)?,
        mail: AccountMail::from_env()?,
        state_path,
        retry_after: None,
    };
    account_deletion::finish_pending(&mut app.state, &app.state_path, &mut app.auth)
        .map_err(io::Error::other)?;
    app.run_maintenance();
    let app = Arc::new(Mutex::new(app));
    let scheduler = Arc::clone(&app);
    let (stop, shutdown) = mpsc::channel::<()>();
    let timer = thread::Builder::new().name("commonline-publisher".into()).spawn(move || {
        while matches!(shutdown.recv_timeout(PUBLISH_INTERVAL), Err(mpsc::RecvTimeoutError::Timeout)) {
            match scheduler.lock() {
                Ok(mut app) => app.run_maintenance(),
                Err(_) => {
                    eprintln!("Commonline stopped background maintenance because the local state is unavailable. Restart the server to retry.");
                    break;
                }
            }
        }
    })?;

    let server = http::serve(port, move |request: Request| {
        if request.method == "GET" {
            match request.path.as_str() {
                "/style.css" => {
                    return Response::text(
                        200,
                        "text/css; charset=utf-8",
                        include_bytes!("../assets/style.css").as_slice(),
                    )
                }
                "/plaza.jpg" => {
                    return Response::text(
                        200,
                        "image/jpeg",
                        include_bytes!("../assets/plaza.jpg").as_slice(),
                    )
                }
                "/chat-live.js" => {
                    return Response::text(
                        200,
                        "text/javascript; charset=utf-8",
                        include_bytes!("../assets/chat-live.js").as_slice(),
                    )
                }
                "/chat-live.wasm" => {
                    return Response::text(
                        200,
                        "application/wasm",
                        include_bytes!("../assets/chat-live.wasm").as_slice(),
                    )
                }
                "/post-format.js" => {
                    return Response::text(
                        200,
                        "text/javascript; charset=utf-8",
                        include_bytes!("../assets/post-format.js").as_slice(),
                    )
                }
                "/post-format.wasm" => {
                    return Response::text(
                        200,
                        "application/wasm",
                        include_bytes!("../assets/post-format.wasm").as_slice(),
                    )
                }
                "/comment-share.js" => {
                    return Response::text(
                        200,
                        "text/javascript; charset=utf-8",
                        include_bytes!("../assets/comment-share.js").as_slice(),
                    )
                }
                _ => {}
            }
        }
        match app.lock() {
            Ok(mut app) => app.route(request),
            Err(_) => Response::text(
                500,
                "text/plain; charset=utf-8",
                "The local state is unavailable.",
            ),
        }
    });
    drop(stop);
    let _ = timer.join();
    server
}

struct App {
    state: State,
    auth: AuthStore,
    mail: AccountMail,
    state_path: PathBuf,
    retry_after: Option<Instant>,
}

impl App {
    fn route(&mut self, request: Request) -> Response {
        let token = request.cookie.as_deref().and_then(auth::session_token);
        if request.method == "GET" && request.path == "/messages/live" {
            let viewer = token.and_then(|token| self.auth.username(token));
            return view::live_messages(&self.state, viewer, &request.query);
        }
        self.run_maintenance();
        let username = token.and_then(|token| self.auth.username(token));
        self.state.activate_profile(username);
        view::route(
            request,
            &mut self.state,
            &self.state_path,
            &mut self.auth,
            &self.mail,
        )
    }

    fn run_maintenance(&mut self) {
        let started = Instant::now();
        if self.retry_after.is_some_and(|retry| started < retry) {
            return;
        }
        let now = schedule_time::now();
        let result =
            account_deletion::finish_pending(&mut self.state, &self.state_path, &mut self.auth)
                .and_then(|()| {
                    self.state
                        .expire_restrictions(&self.state_path, now)
                        .map(|_| ())
                        .map_err(|error| format!("Could not save restriction expiry. {error}"))
                })
                .and_then(|()| {
                    self.state
                        .publish_due(&self.state_path, now)
                        .map(|_| ())
                        .map_err(|error| format!("Could not publish scheduled posts. {error}"))
                })
                .and_then(|()| self.state.expire_inline_uploads(&self.state_path, now));
        match result {
            Ok(()) => {
                if self.retry_after.take().is_some() {
                    eprintln!("Commonline resumed scheduled publishing.");
                }
            }
            Err(error) => {
                if self.retry_after.is_none() {
                    eprintln!(
                        "Commonline paused scheduled publishing. Could not update {} ({error}). Check that the state file and its directory are writable. Retrying every {} seconds.",
                        self.state_path.display(),
                        MAINTENANCE_RETRY.as_secs(),
                    );
                }
                self.retry_after = Some(started + MAINTENANCE_RETRY);
            }
        }
    }
}

fn parse_port(value: &str) -> io::Result<u16> {
    value.parse().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "the port must be between 0 and 65535",
        )
    })
}

fn usage_error() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: commonline [--port PORT | --legacy-recovery]",
    )
}
