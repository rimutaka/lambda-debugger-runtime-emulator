# Lambda Runtime Emulator for local debugging

This runtime emulator allows debugging AWS Lambda functions written in Rust locally while receiving the payload from AWS and sending the responses back as if you were doing remote debugging inside the AWS environment.


## How it works

__Production configuration__

Consider this typical Lambda use case:

![function needed debugging](./img/lambda-debugger-usecase.png)

__Debugging configuration__

This project contains two Rust crates for local Lambda debugging:

- _proxy-lambda_ forwards Lambda requests and responses between AWS and your development machine in real time
- _runtime-emulator_ provides Lambda APIs to run a Lambda function locally and exchange payloads with AWS

![function debugged locally](./img/lambda-debugger-components.png)

### Limitations

* no environment constraints, e.g. memory or execution time
* panics are not reported back to AWS
* no concurrent request handling
* no support for X-Trace or Extensions APIs

## Getting started

### Overview

__Initial setup:__

- clone and build this repository locally
- create _request_ and _response_ queues in SQS with IAM permissions

__Per Lambda function:__

- deploy _proxy-lambda_ in place of the function you need to debug
- run the emulator locally as a binary or with `cargo run`
- run your lambda locally with `cargo run`


## Deployment in detail

### SQS configuration

Create two SQS queues with an identical configuration:

- `proxy_lambda_req` for requests to be sent from AWS to your local lambda under debugging. ___Required___.
- `proxy_lambda_resp` if you want responses from your local lambda to be returned to the caller. _Optional_.

See _Advanced setup_ section for more info on how to customize queue names and other settings.

Recommended queue settings:

- **Queue type**: Standard
- **Maximum message size**: 256 KB
- **Default visibility timeout**: 10 Seconds
- **Message retention period**: 1 Hour
- **Receive message wait time**: 20 Seconds

This IAM policy grants _proxy-lambda_ access to the queues.
It assumes that you already have sufficient privileges to access Lambda and SQS from your local machine.

Replace _Principal_ and _Resource_ IDs with your values before adding this policy to the queue config.

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Effect": "Allow",
      "Principal": {
        "AWS": "arn:aws:iam::512295225992:role/lambda_basic"
      },
      "Action": [
        "sqs:DeleteMessage",
        "sqs:GetQueueAttributes",
        "sqs:ReceiveMessage",
        "sqs:SendMessage"
      ],
      "Resource": "arn:aws:sqs:us-east-1:512295225992:proxy_lambda_req"
    }
  ]
}
```
- _Principal_ - is the IAM role your lambda assumes (check Lambda's Permissions Config tab in the AWS console to find the value)
- _Resource_ - the ARN of the queue the policy is attached to (see the queue details page in the AWS console to find the value)

Use different _Resource_ values for _request_ and _response_ queues:

- `arn:aws:sqs:[your_region]:[your_aws_account]:proxy_lambda_req` for `proxy_lambda_req` queue
- `arn:aws:sqs:[your_region]:[your_aws_account]:proxy_lambda_resp` for `proxy_lambda_resp` queue


### Building and deploying _proxy-lambda_

The _proxy lambda_ function should be deployed to AWS Lambda in place of the function you want to debug.

Replace the following parts of the snippet with your values before running it from the project root:
- _target_ - the architecture of the lambda function on AWS, e.g. `x86_64-unknown-linux-gnu`
- _region_ - the region of the lambda function, e.g. `us-east-1`
- _name_ - the name of the lambda function you want to replace with the proxy, e.g. `my-lambda`

```
target=x86_64-unknown-linux-gnu 
region=us-east-1
name=my-lambda

cargo build --release --target $target
cp ./target/$target/release/proxy-lambda ./bootstrap && zip proxy.zip bootstrap && rm bootstrap
aws lambda update-function-code --region $region --function-name $name --zip-file fileb://proxy.zip
```

A deployed _proxy-lambda_ should return _OK_ or time out waiting for a response if you run it with a test event from the AWS console. Check CloudWatch logs for a detailed execution report.

## Debugging

__Pre-requisites:__
- _proxy-lambda_ was deployed to AWS
- SQS queues were created with the recommended access policies

![list of sqs queues](/img/sqs-queues.png)

__Launching the local lambda:__
- start the _runtime-emulator_ in the terminal as a binary or with `cargo run`
- run [env-lambda.sh](env-lambda.sh) in a terminal window on your local machine
- start your lambda in the same terminal window with `cargo run`

![launch example](/img/emulator-launch.png)

__Debugging:__
- trigger the event on AWS as part of your normal data flow, e.g. by a user action on a webpage
- the emulator should display the lambda payload and forward it to your local lambda for processing
- debug as needed

__Success, failure and replay:__

- successful responses are sent back to the caller if the response queue is configured
- panics or handler errors are not sent back to AWS
- the same incoming SQS message is reused until the lambda completes successfully
- _runtime-emulator_ deletes the incoming message (request) when the local lambda completes successfully
- _proxy-lambda_ deletes the outgoing message (response) after forwarding it to the caller
- _proxy-lambda_ clears the response queue before forwarding a request
- purge the request queue manually to delete stale requests

If the local lambda fails, terminates or panics, you can make changes to its code and run it again to reuse the same incoming payload from the request queue.


## Advanced setup

### Custom SQS queue names

By default, _proxy-lambda_ and the local _runtime-emulator_ attempt to connect to `proxy_lambda_req` and `proxy_lambda_resp` queues in the same region.

Provide these env vars to _proxy-lambda_ and _runtime-emulator_ if your queue names differ from the defaults:
- `PROXY_LAMBDA_REQ_QUEUE_URL` - _request_ queue, e.g. https://sqs.us-east-1.amazonaws.com/512295225992/debug_request
- `PROXY_LAMBDA_RESP_QUEUE_URL` - _response_ queue, e.g. https://sqs.us-east-1.amazonaws.com/512295225992/debug_response

### Late responses

Debugging the local lambda may take longer than the AWS service is willing to wait. For example, _proxy-lambda_ function can be configured to wait for up to 15 minutes, but the AWS API Gateway wait time is limited to 30 seconds. If a response from the local lambda arrives after 5 minutes, the _proxy-lambda_ can still forward it to the API Gateway, but that service would already time out. You may need to re-run the request for it to complete successfully end-to-end.

### Not waiting for responses from local lambda

It may be inefficient to have _proxy-lambda_ waiting for a response from the local lambda.

Option 1: delete _proxy_lambda_resp_ queue

Option 2: add `PROXY_LAMBDA_RESP_QUEUE_URL` env var with no value to instruct _proxy-lambda_ and _runtime-emulator_ to not wait for the local lambda response

Option 3: make _proxy_lambda_resp_ queue inaccessible by changing its IAM policy. E.g. change the resource name from the correct queue name `"Resource": "arn:aws:sqs:us-east-1:512295225992:proxy_lambda_resp"` to `"Resource": "arn:aws:sqs:us-east-1:512295225992:proxy_lambda_resp_BLOCKED"`.
Both, _proxy-lambda_ and _runtime-emulator_  treat the access error as a hint to not expect a response.

### Canceling long _proxy-lambda_ wait

Send a random message to the response queue via the AWS console to make _proxy-lambda_ exit and become available for a new request.

### Large payloads and data compression

The size of the SQS payload is [limited to 262,144 bytes by SQS](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/quotas-messages.html). To get around this limitation _proxy-lambda_ and _runtime-emulator_ compress oversized payloads using [flate2 crate](https://crates.io/crates/flate2) and send it as an encoded Base58 string. The encoding/decoding happens automatically at both ends.

The data compression can take up to a minute in debug mode. It is significantly faster with release builds.

### Logging

Both _proxy-lambda_ and _runtime-emulator_ use `RUST_LOG` env var to set the logging level and filters.
If `RUST_LOG` is not present or is empty, both crates log at the _INFO_ level and suppress logging from their dependencies.
See [https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#example-syntax] for more info.

Examples of `RUST_LOG` values:
- `error` - log errors only from all crates and dependencies
- `warn,runtime_emulator=info` - _INFO_ level for the _runtime-emulator_, _WARN_ level for everything else
- `proxy=debug` - detailed logging in _proxy-lambda_