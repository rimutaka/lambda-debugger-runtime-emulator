//! An example of a lambda function that can run on AWS in `--release` and locally in `debug` mode.
//!
//! Parts marked with `#[cfg(debug_assertions)]` attribute run only locally.
//! Parts marked with `#[cfg(not(debug_assertions))]` attribute run only on AWS.
//! The rest of the code is common to both, e.g. `fn handler`.
//!
//! ## Running locally with `cargo run`
//! 1. `fn main` calls `fn run` from inside `mod proxy` that is compiled in debug mode.
//! 2. `proxy::run` interacts with SQS and the handler function.
//!
//! ## Running on AWS with `cargo build --release`
//! 1. `fn main` calls `fn run` from inside `lambda_runtime`.
//! 2. `lambda_runtime::run` interacts with AWS Lambda backend and the handler function, no SQS is involved.
//! If you deploy a debug version of the binary to AWS it will try interact with SQS instead of Lambda runtime.
//!
//! This code requires some environmental vars set up inside `mod proxy`.
//!
//! ## Deployment and invocation
//!
//! 1. Deploy the main proxy function in place of some AWS Lambda function you already have set up.
//! 2. Configure the AWS proxy to talk to the right queues (see [README.md](/README.md))
//! 3. Configure this local example on your dev machine to talk to the same queues (env vars)
//! 4. Replace `fn handler` in this example with your own handler function
//! 5. Run locally with `cargo run`, wait for it to build and invoke the AWS lambda
//! You should see log messages in your terminal when a new message is picked up locally, processed and dispatched.
//! The rest of your AWS workflow should not notice anything different other than it took a bit longer to get a response.
//!
//! ## Production examples
//!
//! * https://github.com/stackmuncher/stm_server/blob/master/stm_html_ui/src/main.rs
//! * https://github.com/stackmuncher/stm_server/blob/master/stm_inbox_router/src/main.rs

#[cfg(not(debug_assertions))]
use lambda_runtime::handler_fn;
use lambda_runtime::{Context, Error};
use serde_json::Value;

#[tokio::main]
async fn main() -> Result<(), Error> {
    // init the logger with the specified level
    let tsub = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false);
    // time is not needed in CloudWatch, but is useful in console
    #[cfg(not(debug_assertions))]
    let tsub = tsub.without_time();
    tsub.init();

    // use an SQS proxy when running locally in debug mode
    #[cfg(debug_assertions)]
    return proxy::run().await;

    // use Lambda runtime when running on AWS in release mode
    #[cfg(not(debug_assertions))]
    return lambda_runtime::run(handler_fn(my_handler)).await;
}

/// This module is only used for local debugging via SQS and should not be deployed to Lambda.
/// Keep it as a permanent part of your production code to quickly run and debug the handler locally any time.
/// It will only be compiled in debug mode.
#[cfg(debug_assertions)]
mod proxy {
    use lambda_debug_proxy_client::{get_input, send_output};
    use lambda_runtime::Error;
    use rusoto_core::region::Region;
    use tracing::info;

    const AWS_REGION: Region = Region::UsEast1; // replace with the region where SQS queues are located
    const REQUEST_QUEUE_URL_ENV: &str = "STM_HTML_LAMBDA_PROXY_REQ"; // create an env var with the queue URL (AWS -> local)
    const RESPONSE_QUEUE_URL_ENV: &str = "STM_HTML_LAMBDA_PROXY_RESP"; // create an env var with the queue URL (local -> AWS)

    pub(crate) async fn run() -> Result<(), Error> {
        let request_queue_url = std::env::var(REQUEST_QUEUE_URL_ENV)
            .expect(&format!(
                "Missing {} env var with the SQS request queue URL",
                REQUEST_QUEUE_URL_ENV
            ))
            .trim()
            .to_string();

        let response_queue_url = std::env::var(RESPONSE_QUEUE_URL_ENV)
            .expect(&format!(
                "Missing {} env var with the SQS request queue URL",
                RESPONSE_QUEUE_URL_ENV
            ))
            .trim()
            .to_string();

        // an infinite loop that imitates Lambda runtime waiting and dispatching messages
        loop {
            // get event and context details from REQUEST queue
            let (payload, receipt_handle) = get_input(&AWS_REGION, &request_queue_url).await?;
            info!("New msg arrived");
            // invoke the handler - replace it with an invocation of your own handler
            let response = super::my_handler(payload.event, payload.ctx).await?;

            // send back the response and delete the message from the queue
            send_output(
                response,
                receipt_handle,
                &AWS_REGION,
                &request_queue_url,
                &response_queue_url,
            )
            .await?;
            info!("Msg sent");
        }
    }
}

/// A basic example of a handler that passes input to output without any modifications.
/// Replace it with your own handler.
async fn my_handler(event: Value, _ctx: Context) -> Result<Value, Error> {
    // do something useful here

    // return back the result
    Ok(event)
}
