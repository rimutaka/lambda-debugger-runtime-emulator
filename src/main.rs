use lambda::{handler_fn, Context};
use rusoto_core::region::Region;
use rusoto_sqs::{DeleteMessageRequest, ReceiveMessageRequest, SendMessageRequest, Sqs, SqsClient};
use serde::Serialize;
use serde_json::Value;
use std::env::var;
use std::str::FromStr;
use tracing::{debug, error};

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Serialize, Debug)]
struct RequestPayload {
    event: Value,
    ctx: Context,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // get the log level from an env var
    let tracing_level = match var("LAMBDA_PROXY_TRACING_LEVEL") {
        Err(_) => tracing::Level::INFO,
        Ok(v) => tracing::Level::from_str(&v)
            .expect("Invalid tracing level. Use trace, debug, error or info"),
    };

    // init the logger with the specified level
    tracing_subscriber::fmt()
        .with_max_level(tracing_level)
        .with_ansi(false)
        .without_time()
        .init();

    let func = handler_fn(my_handler);
    lambda::run(func).await?;

    Ok(())
}

//pub(crate) async fn my_handler(event: Value, _ctx: Context) -> Result<Value, Error> {
async fn my_handler(event: Value, ctx: Context) -> Result<Value, Error> {
    debug!("Event: {:?}", event);
    debug!("Context: {:?}", ctx);

    // get some env vars
    if var("AWS_DEFAULT_REGION").is_err() && var("AWS_REGION").is_err() {
        error!("Either AWS_DEFAULT_REGION or AWS_REGION env vars must be set.");
        panic!();
    }

    let request_queue_url =
        var("LAMBDA_PROXY_REQ_QUEUE_URL").expect("Missing LAMBDA_PROXY_REQ_QUEUE_URL");
    let response_queue_url =
        var("LAMBDA_PROXY_RESP_QUEUE_URL").expect("Missing LAMBDA_PROXY_RESP_QUEUE_URL");

    debug!(
        "ReqQ URL: {}, RespQ URL {}",
        request_queue_url, response_queue_url
    );

    let region = Region::default();
    debug!("Region: {:?}", region);
    let client = SqsClient::new(region);

    // clear the response queue to avoid getting a stale message from a previously timed out request
    purge_response_queue(&client, &response_queue_url).await?;

    // Sending part
    let request_payload = RequestPayload { event, ctx };

    let message_body =
        serde_json::to_string(&request_payload).expect("Failed to serialize event + context");
    debug!("Message body: {}", message_body);

    let send_result = client
        .send_message(SendMessageRequest {
            message_body,
            queue_url: request_queue_url,
            ..Default::default()
        })
        .await?;

    let msg_id = send_result.message_id.unwrap_or_default();
    debug!("Sent with ID: {}", msg_id);

    // start listening to the response
    loop {
        debug!("20s loop");
        let resp = client
            .receive_message(ReceiveMessageRequest {
                max_number_of_messages: Some(1),
                queue_url: response_queue_url.clone(),
                wait_time_seconds: Some(20),
                ..Default::default()
            })
            .await?;

        // wait until a message arrives or the function is killed by AWS
        if resp.messages.is_none() {
            debug!("No messages yet");
            continue;
        }

        // an empty list returns when the queue wait time expires
        let msgs = resp.messages.expect("Failed to get list of messages");
        if msgs.len() == 0 {
            debug!("No messages yet");
            continue;
        }

        // message arrived
        let body = msgs[0].body.as_ref().expect("Failed to get message body");
        debug!("Response:{}", body);

        // delete it from the queue so it's not picked up again
        client
            .delete_message(DeleteMessageRequest {
                queue_url: response_queue_url.clone(),
                receipt_handle: msgs[0]
                    .receipt_handle
                    .as_ref()
                    .expect("Failed to get msg receipt")
                    .into(),
            })
            .await?;
        debug!("Message deleted");

        // return the contents of the message as JSON Value

        return Ok(Value::from_str(body)?);
    }
}

async fn purge_response_queue(client: &SqsClient, response_queue_url: &String) -> Result<(), Error> {
    debug!("Purging the queue, one msg at a time.");
    loop {
        let resp = client
            .receive_message(ReceiveMessageRequest {
                max_number_of_messages: Some(10),
                queue_url: response_queue_url.clone(),
                wait_time_seconds: Some(0),
                ..Default::default()
            })
            .await?;

        // wait until a message arrives or the function is killed by AWS
        if resp.messages.is_none() {
            debug!("No stale messages (resp.messages.is_none)");
            return Ok(());
        }

        // an empty list returns when the queue wait time expires
        let msgs = resp.messages.expect("Failed to get list of messages");
        if msgs.is_empty() {
            debug!("No stale messages (resp.messages.is_empty)");
            return Ok(());
        }

        debug!("Deleting {} stale messages", msgs.len());

        for msg in msgs {
            // delete it from the queue
            client
                .delete_message(DeleteMessageRequest {
                    queue_url: response_queue_url.clone(),
                    receipt_handle: msg
                        .receipt_handle
                        .as_ref()
                        .expect("Failed to get msg receipt")
                        .into(),
                })
                .await?;
            debug!("Message deleted");
        }
    }
}
