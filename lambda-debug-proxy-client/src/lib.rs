use bs58;
use flate2::read::GzEncoder;
use flate2::Compression;
use lambda_runtime::{Context, Error};
use rusoto_core::region::Region;
use rusoto_sqs::{DeleteMessageRequest, ReceiveMessageRequest, SendMessageRequest, Sqs, SqsClient};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::env::var;
use std::io::prelude::*;
use std::str::FromStr;
use tracing::info;

#[derive(Deserialize, Debug, Serialize)]
pub struct RequestPayload {
    pub event: Value,
    pub ctx: Context,
}

/// Reads a message from the specified SQS queue and returns the payload as Lambda structures
pub async fn get_input(aws_region: &Region, request_queue_url: &str) -> Result<(RequestPayload, String), Error> {
    let client = SqsClient::new(aws_region.clone());

    // start listening to the response
    loop {
        let resp = client
            .receive_message(ReceiveMessageRequest {
                max_number_of_messages: Some(1),
                queue_url: request_queue_url.to_string(),
                wait_time_seconds: Some(20),
                ..Default::default()
            })
            .await?;

        // wait until a message arrives or the function is killed by AWS
        if resp.messages.is_none() {
            continue;
        }

        // an empty list returns when the queue wait time expires
        let msgs = resp.messages.expect("Failed to get list of messages");
        if msgs.len() == 0 {
            continue;
        }

        // the message receipt is needed to delete the message from the queue later
        let receipt_handle = msgs[0]
            .receipt_handle
            .as_ref()
            .expect("Failed to get msg receipt")
            .to_owned();

        // convert JSON encoded body into event + ctx structures as defined by Lambda Runtime
        let body = msgs[0].body.as_ref().expect("Failed to get message body");
        let payload: RequestPayload = serde_json::from_str(body).expect("Failed to deserialize msg body");

        return Ok((payload, receipt_handle));
    }
}

/// Send back the response and delete the message from the queue.
pub async fn send_output(
    response: Value,
    receipt_handle: String,
    aws_region: &Region,
    request_queue_url: &str,
    response_queue_url: &str,
) -> Result<(), Error> {
    let client = SqsClient::new(aws_region.clone());

    let response = compress_output(response.to_string());

    // SQS messages must be shorter than 262144 bytes
    if response.len() < 262144 {
        client
            .send_message(SendMessageRequest {
                message_body: response,
                queue_url: response_queue_url.to_string(),
                ..Default::default()
            })
            .await?;
    } else {
        info!("Message size: {}B, max allowed: 262144B", response.len());
    }

    // delete the request msg from the queue so it cannot be replayed again
    client
        .delete_message(DeleteMessageRequest {
            queue_url: request_queue_url.to_string(),
            receipt_handle,
        })
        .await?;

    Ok(())
}

/// Compresses and encodes the output as Base58 if the message is larger than what is
/// allowed in SQS (262,144 bytes)
fn compress_output(response: String) -> String {
    // is it small enough to fit in?
    if response.len() < 262144 {
        return response;
    }

    println!("Message size: {}B, max allowed: 262144B. Compressing...", response.len());

    // try to decompress the body
    let mut gzipper = GzEncoder::new(response.as_bytes(), Compression::fast());
    let mut gzipped: Vec<u8> = Vec::new();
    let compressed_len = match gzipper.read_to_end(&mut gzipped) {
        Ok(v) => v,
        Err(e) => {
            // this may not be the best option - returning an error may be more appropriate
            panic!("Failed to gzip the payload: {}", e);
        }
    };

    // encode to base58
    let response = bs58::encode(&gzipped).into_string();

    info!("Compressed: {}, encoded: {}", compressed_len, response.len());

    response
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
