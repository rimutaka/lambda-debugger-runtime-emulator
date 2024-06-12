use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::Bytes;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::Error;
use hyper::{Method, Request, Response};
use hyper_util::rt::TokioIo;
use regex::Regex;
use std::env::var;
use std::str::FromStr;
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

mod config;
mod sqs;

async fn api_next_invocation() -> Response<BoxBody<Bytes, Error>> {
    // get the next SQS message or wait for it to arrive
    // this call will block until a message is available
    let sqs_message = sqs::get_input().await;

    info!("{}", sqs_message.payload);

    Response::builder()
        .status(hyper::StatusCode::OK)
        .header("lambda-runtime-aws-request-id", sqs_message.receipt_handle)
        .header("lambda-runtime-deadline-ms", sqs_message.ctx.deadline)
        .header(
            "lambda-runtime-invoked-function-arn",
            sqs_message.ctx.invoked_function_arn,
        )
        .header("lambda-runtime-trace-id", sqs_message.ctx.xray_trace_id)
        .body(full(sqs_message.payload))
        .expect("Failed to create a response")
}

async fn api_response(response: Bytes, receipt_handle: String) -> Response<BoxBody<Bytes, Error>> {
    let sqs_payload = match String::from_utf8(response.as_ref().to_vec()) {
        Ok(v) => v,
        Err(e) => {
            panic!(
                "Non-UTF-8 response from Lambda. {:?}\n{}",
                e,
                hex::encode(response.as_ref())
            );
        }
    };

    info!("Lambda response: {sqs_payload}");

    sqs::send_output(sqs_payload, receipt_handle).await;

    Response::builder()
        .status(hyper::StatusCode::OK)
        .body(empty())
        .expect("Failed to create a response")
}

async fn api_error(resp: Bytes) -> Response<BoxBody<Bytes, Error>> {
    match String::from_utf8(resp.as_ref().to_vec()) {
        Ok(v) => {
            info!("Lambda error: {v}");
        }
        Err(e) => {
            warn!(
                "Non-UTF-8 error response from Lambda. {:?}\n{}",
                e,
                hex::encode(resp.as_ref())
            );
        }
    }

    info!("Ctlr-C your lambda or this runtime within 10s to avoid a rerun");
    sleep(Duration::from_secs(10)).await;

    // lambda allows for more informative error responses, but this may be enough for now
    Response::builder()
        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
        .body(empty())
        .expect("Failed to create a response")
}

async fn lambda_api_handler(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    info!("Request URL: {:?}", req.uri());

    // Next invocation (https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-next)
    if req.method() == Method::GET && req.uri().path().ends_with("/invocation/next") {
        return Ok(api_next_invocation().await);
    }

    // There should be no other GET request types
    if req.method() != Method::POST {
        panic!("Invalid request: {:?}", req);
    }

    // Invocation response (https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-response)
    if req.uri().path().ends_with("/response") {
        // the request ID in the URL is the receipt handle for SQS
        // path template: /runtime/invocation/AwsRequestId/response
        let regex = Regex::new(r"/runtime/invocation/(.+)/response").expect("Invalid response URL regex. It's a bug.");

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

        let bytes = match req.into_body().collect().await {
            Ok(v) => v.to_bytes(),
            Err(e) => panic!("Failed to read lambda response: {:?}", e),
        };
        return Ok(api_response(bytes, receipt_handle).await);
    }

    // Initialization error (https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-initerror) and
    // Invocation error (https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-invokeerror)
    // are rolled together into a single handler because it is not clear how to handle errors
    // and if the error should be propagated upstream
    if req.uri().path().ends_with("/error") {
        let bytes = match req.into_body().collect().await {
            Ok(v) => v.to_bytes(),
            Err(e) => panic!("Failed to read lambda response: {:?}", e),
        };
        return Ok(api_error(bytes).await);
    }

    panic!("Unknown request type: {:?}", req);
}

// We create some utility functions to make Empty and Full bodies
// fit our broadened Response body type.
fn empty() -> BoxBody<Bytes, hyper::Error> {
    Empty::<Bytes>::new().map_err(|never| match never {}).boxed()
}

fn full<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into()).map_err(|never| match never {}).boxed()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt().with_max_level(tracing::Level::INFO).init();

    let config = config::Config::from_env();

    info!("Listening on http://{}", config.lambda_api_listener);

    // We create a TcpListener and bind it to 127.0.0.1:3000
    let listener = TcpListener::bind(config.lambda_api_listener).await?;

    // We start a loop to continuously accept incoming connections
    loop {
        let (stream, _) = listener.accept().await?;

        // Use an adapter to access something implementing `tokio::io` traits as if they implement
        // `hyper::rt` IO traits.
        let io = TokioIo::new(stream);

        // Spawn a tokio task to serve multiple connections concurrently
        tokio::task::spawn(async move {
            // Finally, we bind the incoming connection to our `hello` service
            if let Err(err) = http1::Builder::new()
                // `service_fn` converts our function in a `Service`
                .serve_connection(io, service_fn(lambda_api_handler))
                .await
            {
                eprintln!("Error serving TCP connection: {:?}", err);
            }
        });
    }
}

/// A standard routine for initializing a tracing provider for use in `main` and inside test functions.
/// * tracing_level: pass None if not known in advance and should be taken from an env var
pub fn init_tracing(tracing_level: Option<tracing::Level>) {
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
        .with_ansi(false)
        .without_time()
        .init();
}
