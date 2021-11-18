#[cfg(not(debug_assertions))]
use lambda_runtime::{handler_fn, Error};

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

    #[cfg(debug_assertions)]
    return proxy::run().await;

    // call the actual handler of the request
    #[cfg(not(debug_assertions))]
    return lambda_runtime::run(handler_fn(handler::my_handler)).await;
}

/// This module is only used for local debugging via SQS and should
/// not be deployed to Lambda.
#[cfg(debug_assertions)]
mod proxy {
    use lambda_debug_proxy_client::{get_input, send_output};
    use lambda_runtime::Error;
    use rusoto_core::region::Region;
    use tracing::info;

    const AWS_REGION: Region = Region::UsEast1; // replace with your preferred region
    const REQUEST_QUEUE_URL_ENV: &str = "STM_HTML_LAMBDA_PROXY_REQ"; // add your queue URL there
    const RESPONSE_QUEUE_URL_ENV: &str = "STM_HTML_LAMBDA_PROXY_RESP"; // add your queue URL there

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

        loop {
            // get event and context details from the queue
            let (payload, receipt_handle) = get_input(&AWS_REGION, &request_queue_url).await?;
            info!("New msg arrived");
            // invoke the handler
            let response = crate::handler::my_handler(payload.event, payload.ctx).await?;

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
