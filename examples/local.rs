use lambda::Context;
use rusoto_core::region::Region;
use rusoto_sqs::{DeleteMessageRequest, ReceiveMessageRequest, SendMessageRequest, Sqs, SqsClient};
use serde::Deserialize;
use serde_json::Value;
use tracing::{info};

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

const AWS_REGION: Region = Region::ApSoutheast2; // replace with your preferred region
const REQUEST_QUEUE_URL: &str = ""; // insert your queue URL here
const RESPONSE_QUEUE_URL: &str = ""; // insert your queue URL here

#[derive(Deserialize, Debug)]
struct RequestPayload {
    pub event: Value,
    pub ctx: Context,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // init the logger with the specified level
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_ansi(false)
        .without_time()
        .init();

    // get event and context details from the queue
    let (payload, receipt_handle) = get_input().await?;

    // invoke the handler
    let response = my_handler(payload.event, payload.ctx).await?;

    // send back the response and delete the message from the queue
    send_output(response, receipt_handle).await?;

    Ok(())
}

//pub(crate) async fn my_handler(event: Value, _ctx: Context) -> Result<Value, Error> {
pub(crate) async fn my_handler(event: Value, ctx: Context) -> Result<Value, Error> {
    info!("Event: {:?}", event);
    info!("Context: {:?}", ctx);

    // do something useful here

    // return back the result
    Ok(event)
}

/// Read a message from the queue and return the payload as Lambda structures
async fn get_input() -> Result<(RequestPayload, String), Error> {
    let client = SqsClient::new(AWS_REGION);

    // start listening to the response
    loop {
        let resp = client
            .receive_message(ReceiveMessageRequest {
                max_number_of_messages: Some(1),
                queue_url: REQUEST_QUEUE_URL.to_owned(),
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
        let payload: RequestPayload =
            serde_json::from_str(body).expect("Failed to deserialize msg body");

        return Ok((payload, receipt_handle));
    }
}

/// Send back the response and delete the message from the queue.
async fn send_output(response: Value, receipt_handle: String) -> Result<(), Error> {
    let client = SqsClient::new(AWS_REGION);

    client
        .send_message(SendMessageRequest {
            message_body: response.to_string(),
            queue_url: RESPONSE_QUEUE_URL.to_owned(),
            ..Default::default()
        })
        .await?;

    // delete the request msg from the queue so it cannot be replayed again
    client
        .delete_message(DeleteMessageRequest {
            queue_url: REQUEST_QUEUE_URL.to_owned(),
            receipt_handle,
        })
        .await?;

    Ok(())
}
