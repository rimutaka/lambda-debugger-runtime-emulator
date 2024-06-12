use core::net::SocketAddrV4;
use std::env::var;
use std::net::Ipv4Addr;
use std::str::FromStr;

pub(crate) struct Config {
    /// E.g. 127.0.0.1:9001
    pub lambda_api_listener: SocketAddrV4,
    /// E.g. https://sqs.us-east-1.amazonaws.com/512295225992/LAMBDA_PROXY_REQ
    pub request_queue_url: String,
    /// E.g. https://sqs.us-east-1.amazonaws.com/512295225992/LAMBDA_PROXY_RESP
    pub response_queue_url: String,
}

impl Config {
    /// Create a new Config instance from the environment variables.
    /// Uses default values where possible.
    /// Panics if the required environment variables are not set.
    pub fn from_env() -> Self {
        let request_queue_url = var("LAMBDA_PROXY_REQ_QUEUE_URL").expect("Missing LAMBDA_PROXY_REQ_QUEUE_URL env var");
        let response_queue_url =
            var("LAMBDA_PROXY_RESP_QUEUE_URL").expect("Missing LAMBDA_PROXY_RESP_QUEUE_URL env var");

        // this one can do with a default value
        let listener_ip_str = var("AWS_LAMBDA_RUNTIME_API")
            .expect("AWS_LAMBDA_RUNTIME_API env var is not specified. Using default 127.0.0.1:9001");

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

        Self {
            lambda_api_listener,
            request_queue_url,
            response_queue_url,
        }
    }
}
