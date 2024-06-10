use aws_sdk_sqs::Client as SqsClient;
use flate2::read::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env::var;
use std::io::prelude::*;
use std::str::FromStr;
use tracing::{debug, info, warn};
use tracing_subscriber::field::debug;

use std::convert::Infallible;
use std::net::SocketAddr;

use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::{Body, Bytes, Frame};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::Error;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

mod config;
mod sqs;

async fn log_request(req: &Request<hyper::body::Incoming>) {
    debug!("Request URL: {:?}", req.uri());

    // match req.into_body().collect().await {
    //     Ok(body) => {
    //         let body = String::from_utf8_lossy(&body.to_bytes());
    //         debug!("Request body: {:?}", body);
    //     }
    //     Err(e) => {
    //         debug!("Error reading request body: {:?}", e);
    //     }
    // };
}

async fn api_next_invocation() -> Response<BoxBody<Bytes, Error>> {
    Response::builder()
        .status(hyper::StatusCode::OK)
        .header("lambda-runtime-aws-request-id", "8476a536-e9f4-11e8-9739-2dfe598c3fcd")
        .header("Lambda-Runtime-Deadline-Ms", "1542409706888")
        .header(
            "Lambda-Runtime-Invoked-Function-Arn",
            "arn:aws:lambda:us-east-2:123456789012:function:custom-runtime",
        )
        .header(
            "Lambda-Runtime-Trace-Id",
            "Root=1-5bef4de7-ad49b0e87f6ef6c87fc2e700;Parent=9a9197af755a6419;Sampled=1",
        )
        .body(full("{}"))
        .expect("Failed to create a response")
}

async fn api_response(resp: Bytes) -> Response<BoxBody<Bytes, Error>> {
    match String::from_utf8(resp.as_ref().to_vec()) {
        Ok(v) => debug!("Lambda response: {v}"),
        Err(e) => {
            warn!(
                "Non-UTF-8 response from Lambda. {:?}\n{}",
                e,
                hex::encode(resp.as_ref())
            );
        }
    }

    Response::builder()
        .status(hyper::StatusCode::OK)
        .body(empty())
        .expect("Failed to create a response")
}

async fn api_error(resp: Bytes) -> Response<BoxBody<Bytes, Error>> {
    match String::from_utf8(resp.as_ref().to_vec()) {
        Ok(v) => debug!("Lambda error: {v}"),
        Err(e) => {
            warn!(
                "Non-UTF-8 error response from Lambda. {:?}\n{}",
                e,
                hex::encode(resp.as_ref())
            );
        }
    }

    // lambda allows for moroe informative error responses, but this may be enough for now
    Response::builder()
        .status(hyper::StatusCode::INTERNAL_SERVER_ERROR)
        .body(empty())
        .expect("Failed to create a response")
}

async fn lambda_api_handler(
    req: Request<hyper::body::Incoming>,
) -> Result<Response<BoxBody<Bytes, hyper::Error>>, hyper::Error> {
    log_request(&req).await;

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
        // in theory we should extract the request ID, but since there is only one active request / response at a time
        // and the proxy does not maintain any state, there is no point knowing the request ID
        // We should simply forward the lambda response to the response queue.
        let bytes = match req.into_body().collect().await {
            Ok(v) => v.to_bytes(),
            Err(e) => panic!("Failed to read lambda response: {:?}", e),
        };
        return Ok(api_response(bytes).await);
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

    // match (req.method(), req.uri().path().ends_with(pat)) {
    //     (&Method::GET, "/2018-06-01/runtime/invocation/next") => Ok(api_next_invocation().await),
    //     (&Method::POST, "/echo") => Ok(Response::new(req.into_body().boxed())),
    //     (&Method::POST, "/echo/uppercase") => {
    //         // Map this body's frame to a different type
    //         let frame_stream = req.into_body().map_frame(|frame| {
    //             let frame = if let Ok(data) = frame.into_data() {
    //                 // Convert every byte in every Data frame to uppercase
    //                 data.iter().map(|byte| byte.to_ascii_uppercase()).collect::<Bytes>()
    //             } else {
    //                 Bytes::new()
    //             };

    //             Frame::data(frame)
    //         });

    //         Ok(Response::new(frame_stream.boxed()))
    //     }
    //     (&Method::POST, "/echo/reversed") => {
    //         // Protect our server from massive bodies.
    //         let upper = req.body().size_hint().upper().unwrap_or(u64::MAX);
    //         if upper > 1024 * 64 {
    //             let mut resp = Response::new(full("Body too big"));
    //             *resp.status_mut() = hyper::StatusCode::PAYLOAD_TOO_LARGE;
    //             return Ok(resp);
    //         }

    //         // Await the whole body to be collected into a single `Bytes`...
    //         let whole_body = req.collect().await?.to_bytes();

    //         // Iterate the whole body in reverse order and collect into a new Vec.
    //         let reversed_body = whole_body.iter().rev().cloned().collect::<Vec<u8>>();

    //         Ok(Response::new(full(reversed_body)))
    //     }
    //     // Return 404 Not Found for other routes.
    //     _ => {
    //         // print a warning into the console and return a blank 404 to the caller
    //         warn!("Unknown request type: {:?}", req);

    //         let mut not_found = Response::new(req.into_body().boxed());
    //         *not_found.status_mut() = StatusCode::NOT_FOUND;
    //         Ok(not_found)
    //     }
    // }
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
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_ansi(false)
        .without_time()
        .init();

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

mod test {
    // const AWS_REGION: Region = Region::UsEast1; // replace with your preferred region
    // const REQUEST_QUEUE_URL_ENV: &str = "STM_HTML_LAMBDA_PROXY_REQ"; // add your queue URL there
    // const RESPONSE_QUEUE_URL_ENV: &str = "STM_HTML_LAMBDA_PROXY_RESP"; // add your queue URL there
}
