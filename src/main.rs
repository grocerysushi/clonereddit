mod http;
mod media;
mod model;
mod schedule_time;
mod view;

use std::{
    env, io,
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use http::{Request, Response};
use model::State;

const DEFAULT_PORT: u16 = 4173;
const PUBLISH_INTERVAL: Duration = Duration::from_secs(1);
const PUBLISH_RETRY: Duration = Duration::from_secs(10);

fn main() {
    if let Err(error) = run() {
        eprintln!("Commonline: {error}");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut args = env::args().skip(1);
    let port = match args.next().as_deref() {
        Some("--help" | "-h") => {
            println!("Usage: commonline [--port PORT]");
            println!("COMMONLINE_PORT sets the default port ({DEFAULT_PORT}). Use 0 for an available port.");
            println!(
                "COMMONLINE_STATE sets the path to the state file (default: ./commonline.state)."
            );
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
    let state_path = env::var_os("COMMONLINE_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("commonline.state"));
    let state = State::load(&state_path).map_err(|error| {
        io::Error::new(error.kind(), format!("{}: {error}", state_path.display()))
    })?;
    let mut app = App {
        state,
        state_path,
        retry_after: None,
    };
    app.publish_due();
    let app = Arc::new(Mutex::new(app));
    let scheduler = Arc::clone(&app);
    let (stop, shutdown) = mpsc::channel::<()>();
    let timer = thread::Builder::new().name("commonline-publisher".into()).spawn(move || {
        while matches!(shutdown.recv_timeout(PUBLISH_INTERVAL), Err(mpsc::RecvTimeoutError::Timeout)) {
            match scheduler.lock() {
                Ok(mut app) => app.publish_due(),
                Err(_) => {
                    eprintln!("Commonline stopped scheduled publishing because the local state is unavailable. Restart the server to retry.");
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
    state_path: PathBuf,
    retry_after: Option<Instant>,
}

impl App {
    fn route(&mut self, request: Request) -> Response {
        self.publish_due();
        view::route(request, &mut self.state, &self.state_path)
    }

    fn publish_due(&mut self) {
        let started = Instant::now();
        if self.retry_after.is_some_and(|retry| started < retry) {
            return;
        }
        match self
            .state
            .publish_due(&self.state_path, schedule_time::now())
        {
            Ok(_) => {
                if self.retry_after.take().is_some() {
                    eprintln!("Commonline resumed scheduled publishing.");
                }
            }
            Err(error) => {
                if self.retry_after.is_none() {
                    eprintln!(
                        "Commonline paused scheduled publishing. Could not save {} ({error}). Check that the state file and its directory are writable. Retrying every {} seconds.",
                        self.state_path.display(),
                        PUBLISH_RETRY.as_secs(),
                    );
                }
                self.retry_after = Some(started + PUBLISH_RETRY);
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
        "usage: commonline [--port PORT]",
    )
}
