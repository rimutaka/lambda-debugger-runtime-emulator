use http_body_util::combinators::BoxBody;
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use std::env::var;
use std::str::FromStr;
use tokio::net::TcpListener;
use tracing::{error, info};

mod config;
mod handlers;
mod sqs;

/// The handler function converted into a Tower service to run in the background
/// and serve the incoming HTTP requests from the local lambda.
async fn lambda_api_handler(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    info!("Request URL: {:?}", req.uri());

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

    panic!("Unknown request type: {:?}", req);
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    init_tracing(None); // use the hardcoded default for now

    let config = config::Config::from_env();

    // bind to a TCP port and start a loop to continuously accept incoming connections
    info!("Listening on http://{}", config.lambda_api_listener);
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

/// A standard routine for initializing a tracing provider for use in `main` and inside test functions.
/// * tracing_level: pass None if not known in advance and should be taken from an env var
fn init_tracing(tracing_level: Option<tracing::Level>) {
    // get the log level from an env var
    let tracing_level = match tracing_level {
        Some(v) => v,
        None => match var("LAMBDA_PROXY_TRACING_LEVEL") {
            Err(_) => tracing::Level::INFO,
            Ok(v) => tracing::Level::from_str(&v).expect("Invalid tracing level. Use trace, debug, error or info"),
        },
    };

    // init the logger with the specified level
    tracing_subscriber::fmt()
        .with_max_level(tracing_level)
        .with_ansi(true)
        // .without_time()
        .compact()
        .init();
}
