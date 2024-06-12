use handlers::{api_error, api_next_invocation, api_response};
use http_body_util::{combinators::BoxBody, BodyExt};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use regex::Regex;
use std::cell::OnceCell;
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
        // Next invocation - the local lambda sends this requests and waits for an invocation with no time limit.
        // We simulate this by waiting for a message to arrive in the SQS queue.
        // See https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-next
        return Ok(api_next_invocation().await);
    }

    if req.method() != Method::POST {
        // There should be no other GET request types other than the above.
        // It is a hard error is there is another type of GET request.
        panic!("Invalid GET request: {:?}", req);
    }

    if req.uri().path().ends_with("/response") {
        // Invocation response is sent by the local lambda when it successfully completed processing.
        // We forward the response to the SQS queue where it is picked up by the remote proxy lambda
        // and forwarded to the original caller, e.g. API Gateway.
        // See https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-response

        // The regex extracts the receipt handle from the path, e.g. /runtime/invocation/[aws-req-id]/response
        // where the request ID in the URL is the receipt handle for SQS - it is not the actual lambda request ID.
        // We need to store the receipt handle somewhere and placing it into the request-id param seems like an easy way to do it
        // because the local lambda will return it with the response.
        // The receipt handle can be a long string with /, - and other non-alphanumeric characters.
        let cell = OnceCell::new();
        let regex = cell.get_or_init(|| {
            Regex::new(r"/runtime/invocation/(.+)/response").expect("Invalid response URL regex. It's a bug.")
        });
        let receipt_handle = regex
            .captures(req.uri().path())
            .unwrap_or_else(|| panic!("URL parsing regex failed on: {:?}. It' a bug", req.uri()))
            .get(1)
            .unwrap_or_else(|| {
                panic!(
                    "Request URL does not conform to /runtime/invocation/AwsRequestId/response: {:?}",
                    req.uri()
                )
            })
            .as_str()
            .to_owned();

        // convert the lambda response to bytes
        let bytes = match req.into_body().collect().await {
            Ok(v) => v.to_bytes(),
            Err(e) => panic!("Failed to read lambda response: {:?}", e),
        };
        return Ok(api_response(bytes, receipt_handle).await);
    }

    if req.uri().path().ends_with("/error") {
        // Initialization error (https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-initerror) and
        // Invocation error (https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-invokeerror)
        // are rolled together into a single handler because it is not clear how to handle errors
        // and if the error should be propagated upstream
        let bytes = match req.into_body().collect().await {
            Ok(v) => v.to_bytes(),
            Err(e) => panic!("Failed to read lambda response: {:?}", e),
        };
        return Ok(api_error(bytes).await);
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
