use crate::sqs::get_default_queues;
use core::net::SocketAddrV4;
use std::env::var;
use std::net::Ipv4Addr;
use std::str::FromStr;
use tracing::info;

pub(crate) struct Config {
    /// E.g. 127.0.0.1:9001
    pub lambda_api_listener: SocketAddrV4,
    /// E.g. https://sqs.us-east-1.amazonaws.com/512295225992/proxy_lambda-req
    pub request_queue_url: String,
    /// E.g. https://sqs.us-east-1.amazonaws.com/512295225992/proxy-lambda-resp.
    /// No response is set if this property is None.
    pub response_queue_url: Option<String>,
}

impl Config {
    /// Creates a new Config instance from environment variables and defaults.
    /// Uses default values where possible.
    /// Panics if the required environment variables are not set.
    pub async fn from_env() -> Self {
        // queue names from env vars have higher priority than the defaults
        let request_queue_url = var("PROXY_LAMBDA_REQ_QUEUE_URL").ok();
        let response_queue_url = var("LAMBDA_PROXY_RESP_QUEUE_URL").ok();

        // only get the default queue names if the env vars are not set because the call is expensive (SQS List Queues)
        let (default_req_queue, default_resp_queue) = if request_queue_url.is_none() || response_queue_url.is_none() {
            get_default_queues().await
        } else {
            (None, None)
        };

        // choose between default and env var queues for request - at least one is required
        let request_queue_url = match request_queue_url {
            Some(v) => v,
            None => match default_req_queue {
                Some(v) => v,
                None => {
                    panic!("Request queue URL is not set. Set PROXY_LAMBDA_REQ_QUEUE_URL or create a queue with the name proxy_lambda_req")
                }
            },
        };

        // the response queue is optional
        let response_queue_url = match response_queue_url {
            Some(v) => Some(v),
            None => default_resp_queue, // this may also be None
        };

        // 127.0.0.1:9001 is the default endpoint used on AWS
        let listener_ip_str = var("AWS_LAMBDA_RUNTIME_API").unwrap_or_else(|_e| "127.0.0.1:9001".to_string());

        let lambda_api_listener = match listener_ip_str.split_once(':') {
            Some((ip, port)) => {
                let listener_ip = std::net::Ipv4Addr::from_str(ip).expect(
                    "Invalid IP address in AWS_LAMBDA_RUNTIME_API env var. Must be a valid IP4, e.g. 127.0.0.1",
                );
                let listener_port = port.parse::<u16>().expect(
                    "Invalid port number in AWS_LAMBDA_RUNTIME_API env var. Must be a valid port number, e.g. 9001",
                );
                SocketAddrV4::new(listener_ip, listener_port)
            }
            None => SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 9001),
        };

        info!(
            "Listening on http://{}\n- request queue: {}\n- response queue:{}\n",
            lambda_api_listener,
            request_queue_url,
            response_queue_url.clone().unwrap_or_else(String::new),
        );

        Self {
            lambda_api_listener,
            request_queue_url,
            response_queue_url,
        }
    }
}
