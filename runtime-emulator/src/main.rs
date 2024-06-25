use async_once::AsyncOnce;
use config::Config;
use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use lazy_static::lazy_static;
use std::str::FromStr;
use tokio::net::TcpListener;
use tracing::{debug, error, warn};
use tracing_subscriber::filter::Directive;
use tracing_subscriber::EnvFilter;

mod config;
mod handlers;
mod sqs;

// Cannot use OnceCell because it does not support async initialization
lazy_static! {
    pub(crate) static ref CONFIG: AsyncOnce<Config> = AsyncOnce::new(async { Config::from_env().await });
}

/// The handler function converted into a Tower service to run in the background
/// and serve the incoming HTTP requests from the local lambda.
async fn lambda_api_handler(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    debug!("Request URL: {:?}", req.uri());

    if req.method() == Method::GET && req.uri().path().ends_with("/invocation/next") {
        return Ok(handlers::next_invocation::handler().await);
    }

    if req.method() != Method::POST {
        // There should be no other GET request types other than the above.
        panic!("Invalid GET request: {:?}", req);
    }

    if req.uri().path().ends_with("/response") {
        return Ok(handlers::lambda_response::handler(req).await);
    }

    if req.uri().path().ends_with("/error") {
        return Ok(handlers::lambda_error::handler(req).await);
    }

    // this should not be happening unless there is a bug or someone is sending requests manually
    warn!("Unknown request type: {:?}", req);
    Ok(handlers::lambda_error::handler(req).await)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_tracing();
    let config = CONFIG.get().await;

    // bind to a TCP port and start a loop to continuously accept incoming connections
    let listener = TcpListener::bind(config.lambda_api_listener).await?;

    loop {
        let (stream, _) = listener.accept().await?;
        let io = TokioIo::new(stream);

        // Spawn a tokio task to serve multiple connections concurrently
        tokio::task::spawn(async move {
            // bind the incoming connection to lambda_api_handler service
            if let Err(err) = http1::Builder::new()
                // `service_fn` comes from Tower, convert the handler function into a service
                .serve_connection(io, service_fn(lambda_api_handler))
                .await
            {
                error!("Error serving TCP connection: {:?}", err);
            }
        });
    }
}

/// Initializes the tracing from RUST_LOG env var if present or sets minimal logging:
/// - INFO for the emulator
/// - ERROR for everything else
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(
                    Directive::from_str("runtime_emulator=info").expect("Invalid logging filter. It's a bug."),
                )
                .from_env_lossy(),
        )
        // .with_env_filter(filter)
        .with_ansi(true)
        .with_target(false)
        .compact()
        .init();
}
