use crate::sqs::get_default_queues;
use core::net::SocketAddrV4;
use std::env::{args, var};
use std::net::Ipv4Addr;
use std::str::FromStr;
use tracing::{debug, info, warn};

const REQUIRED_ENV_VARS: &str = "export AWS_LAMBDA_FUNCTION_VERSION=$LATEST && export AWS_LAMBDA_FUNCTION_MEMORY_SIZE=128 && export AWS_LAMBDA_FUNCTION_NAME=my-lambda && export AWS_LAMBDA_RUNTIME_API=127.0.0.1:9001";

/// Payloads come from a local file, responses are not sent anywhere
pub(crate) struct LocalConfig {
    /// Decoded payload from the local file. Can be anything as long as it's UTF-8
    pub payload: String,
    /// File name from which the payload was read, as provided in the param
    pub file_name: String,
}

/// Payloads come from SQS and may be sent back to SQS
pub(crate) struct RemoteConfig {
    /// E.g. https://sqs.us-east-1.amazonaws.com/512295225992/proxy_lambda-req
    pub request_queue_url: String,
    /// E.g. https://sqs.us-east-1.amazonaws.com/512295225992/proxy-lambda-resp.
    /// No response is set if this property is None.
    pub response_queue_url: Option<String>,
}

/// A concrete type for either remote or local source of payloads
pub(crate) enum PayloadSources {
    Local(LocalConfig),
    Remote(RemoteConfig),
}

pub(crate) struct Config {
    /// E.g. 127.0.0.1:9001
    pub lambda_api_listener: SocketAddrV4,
    /// Source and destination of request and response payloads
    pub sources: PayloadSources,
}

impl Config {
    /// Creates a new Config instance from environment variables and defaults.
    /// Uses default values where possible.
    /// Panics if the required environment variables are not set.
    pub async fn from_env() -> Self {
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

        // attempt to extract payload from a local file if the file name is provided in the command line arguments
        // alternatively try to find remote queues
        // exit if no sources are set
        let sources = match get_local_payload() {
            Some(local_config) => {
                info!(
                    "Listening on http://{}\n- payload from: {}\n",
                    lambda_api_listener, local_config.file_name
                );

                PayloadSources::Local(local_config)
            }
            None => match get_queues().await {
                Some(remote_config) => {
                    info!(
                        "Listening on http://{}\n- request queue:  {}\n- response queue: {}\n",
                        lambda_api_listener,
                        remote_config.request_queue_url,
                        remote_config.response_queue_url.clone().unwrap_or_else(String::new),
                    );

                    PayloadSources::Remote(remote_config)
                }
                None => {
                    panic!("No payload source is set.\nAdd payload file name as a param for local debugging or create request / response queues for remote debugging.\nSee ReadMe for more info.");
                }
            },
        };
        warn!("Add required env vars and start the lambda:\n{}\n", REQUIRED_ENV_VARS);

        Self {
            lambda_api_listener,
            sources,
        }
    }

    /// A shortcut for unwrapping the remote config.
    /// Panics if the config is not RemoteConfig.
    pub(crate) fn remote_config(&self) -> &RemoteConfig {
        // get the request queue URL from deep inside the config
        match &self.sources {
            PayloadSources::Remote(remote_config) => remote_config,
            _ => panic!("Invalid config: expected RemoteConfig. It's a bug."),
        }
    }
}

/// Returns URLs of the request and response queues, if they exist.
/// Reads values from the environment variables or uses the defaults.
/// Does not panic.
async fn get_queues() -> Option<RemoteConfig> {
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
                return None;
            }
        },
    };

    // the response queue is optional
    let response_queue_url = match response_queue_url {
        Some(v) => Some(v),
        None => default_resp_queue, // this may also be None
    };

    Some(RemoteConfig {
        request_queue_url,
        response_queue_url,
    })
}

/// Extracts the payload from a local file if the file name is provided in the command line arguments.
/// Panics if the payload cannot be read.
fn get_local_payload() -> Option<LocalConfig> {
    // the number of arguments depends on if this is a cargo command or a standalone executable
    // calculate where the params of the command are located inside the argument collection
    let param_idx = args().next().map_or_else(
        || 0, // this an impossible scenario because the very first argument is always the name of the executable
        |v| {
            if v.ends_with(
                &args()
                    .nth(1)
                    .map_or_else(|| "###".to_string(), |v| format!("cargo-{v}")),
            ) {
                2 // invoked as a cargo command: `/home/mx/.cargo/bin/cargo-lambda-debugger lambda-debugger`
            } else {
                1 // invoked as a standalone binary: `/home/mx/projects/gh-forks/lambda-runtime-emulator/target/debug/cargo-lambda-debugger`
            }
        },
    );
    debug!(
        "Param: {param_idx}, args: {}",
        std::env::args().collect::<Vec<String>>().join(" ")
    );

    // attempt to extract payload from a local file if the file name is provided in the command line arguments
    if let Some(payload_file) = args().nth(param_idx) {
        // cargo help lambda-debugger is equivalent to `/home/mx/.cargo/bin/cargo-lambda-debugger lambda-debugger --help`
        if &payload_file == "--help" {
            println!("AWS Lambda environment emulator for local and remote debugging.");
            println!("1. run `cargo lambda-debugger`");
            println!("2. copy the env vars printed by the emulator");
            println!("3. set the env vars in a separate terminal and start your lambda there with `cargo run`");
            println!();
            println!("With local payload: cargo lambda-debugger [payload_file], e.g. lambda_payload.json");
            println!("With payload from AWS: cargo lambda-debugger");
            println!();
            println!("See https://github.com/rimutaka/lambda-debugger-runtime-emulator for more info.");

            std::process::exit(0);
        }

        // read the payload from the file
        match std::fs::read_to_string(payload_file.clone()) {
            Ok(payload) => Some(LocalConfig {
                payload,
                file_name: payload_file,
            }),

            // there is no point proceeding if the payload cannot be read
            Err(e) => {
                panic!("Failed to read payload from {}\n{:?}", payload_file, e)
            }
        }
    } else {
        None
    }
}
