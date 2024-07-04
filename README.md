# Lambda Runtime Emulator for local and remote debugging

This emulator allows running Lambda functions locally with either a local payload from a file or a remote payload from AWS as if the local lambda was running there.

## Debugging with local payload

Use this method for simple use cases where a single static payload is sufficient.

1. Save your payload into a file, e.g. save `{"command": "echo"}` into `test-payload.json` file
2. Start the emulator with the payload file name as its only param, e.g. `runtime-emulator test-payload.json`
3. Start your lambda with `cargo run`

The lambda will connect to the emulator and receive the payload.
You can re-run your lambda with the same payload as many times as needed.

## Debugging with remote payload

Use this method to get dynamic payload from other AWS services or when you need to send back a dynamic response, e.g. to process a request triggered by a user action on a website involving API Gateway as in the following diagram:

![function needed debugging](./img/lambda-debugger-usecase.png)

__Remote debugging configuration__

This project provides the tools necessary to bring the AWS payload to your local machine in real-time, run the lambda and send back the response as if the local lambda was running on AWS.

- _proxy-lambda_ forwards Lambda requests and responses between AWS and your development machine in real time
- _runtime-emulator_ provides Lambda APIs to run a lambda function locally and exchange payloads with AWS

![function debugged locally](./img/lambda-debugger-components.png)

### Limitations

This Lambda emulator does not provide the full runtime capabilities of AWS:

* no environment constraints, e.g. memory or execution time
* panics are not reported back to AWS
* no concurrent request handling
* no support for X-Trace or Extensions APIs
* no stream responses
* smaller maximum payload size

## Getting started with remote debugging

- create _request_ and _response_ queues in SQS with IAM permissions
- deploy _proxy-lambda_ in place of the function you need to debug
- run the emulator locally as a binary or with `cargo run`
- run your lambda locally with `cargo run`

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
#### Modifying the policy with your IDs

You need to replace _Principal_ and _Resource_ IDs with your values before adding the above policy to your queues:

- _Principal_ - is the IAM role your lambda assumes (check Lambda's Permissions Config tab in the AWS console to find the value)
- _Resource_ - the ARN of the queue the policy is attached to (see the queue details page in the AWS console to find the value)

Use different _Resource_ values for _request_ and _response_ queues:

- `arn:aws:sqs:[your_region]:[your_aws_account]:proxy_lambda_req` for `proxy_lambda_req` queue
- `arn:aws:sqs:[your_region]:[your_aws_account]:proxy_lambda_resp` for `proxy_lambda_resp` queue


### Building and deploying _proxy-lambda_

The _proxy-lambda_ function should be deployed to AWS in place of the function you want to debug.

Replace the following parts of the bash script below with your values before running it from the project root:
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

A deployed _proxy-lambda_ should return _OK_ or time out waiting for a response if you run it with a test event from the AWS console.
Check CloudWatch logs for a detailed execution report.

### Debugging

__Pre-requisites:__
- _proxy-lambda_ was deployed to AWS
- SQS queues were created with the recommended access policies

![list of sqs queues](/img/sqs-queues.png)

__Launching the local lambda:__
- start the _runtime-emulator_ in the terminal as a binary or with `cargo run`
- add environmental variables from the prompt printed by the _runtime-emulator_ to the lambda terminal
- start your lambda with `cargo run`

![launch example](/img/emulator-launch.png)

__Debugging:__
- trigger the event on AWS as part of your normal data flow, e.g. by a user action on a webpage
- the emulator should display the lambda payload and forward it to your local lambda for processing
- debug as needed

__Success, failure and replay:__

- successful responses are sent back to the caller if the response queue is configured (`proxy_lambda_resp`)
- panics or handler errors are not sent back to AWS
- the same incoming SQS message is reused until the lambda completes successfully
- _runtime-emulator_ deletes the request message from `proxy_lambda_req` queue when the local lambda completes successfully
- _proxy-lambda_ deletes the response message from `proxy_lambda_resp` queue after forwarding it to the caller, e.g. to API Gateway
- _proxy-lambda_ purges `proxy_lambda_resp` queue before sending a new request to `proxy_lambda_resp`
- you have to purge `proxy_lambda_req` queue manually to delete stale requests

If the local lambda fails, terminates or panics, you can make changes to its code and run it again to reuse the same incoming payload from the request queue.


## Advanced remote debugging setup

### Custom SQS queue names

By default, _proxy-lambda_ and the local _runtime-emulator_ attempt to connect to `proxy_lambda_req` and `proxy_lambda_resp` queues in the same region.

Provide these env vars to _proxy-lambda_ and _runtime-emulator_ if your queue names differ from the defaults:
- `PROXY_LAMBDA_REQ_QUEUE_URL` - _request_ queue, e.g. https://sqs.us-east-1.amazonaws.com/512295225992/debug_request
- `PROXY_LAMBDA_RESP_QUEUE_URL` - _response_ queue, e.g. https://sqs.us-east-1.amazonaws.com/512295225992/debug_response

### Late responses

Debugging the local lambda may take longer than the AWS service is willing to wait.
For example, _proxy-lambda_ function can be configured to wait for up to 15 minutes, but the AWS API Gateway wait time is limited to 30 seconds.

Assume that it took you 5 minutes to fix the lambda code and return the correct response.
If _proxy-lambda_ was configured to wait for that long it would still forward the response to the API Gateway which timed out 4.5 min earlier.
In that case, you may need to trigger another request for it to complete successfully end-to-end.

### Not waiting for responses from local lambda

It may be inefficient to have _proxy-lambda_ waiting for a response from the local lambda because it takes too long or no response is necessary.
Both _proxy-lambda_ and _runtime-emulator_ would not expect a response if the response queue is inaccessible.

Option 1: delete _proxy_lambda_resp_ queue

Option 2: add `PROXY_LAMBDA_RESP_QUEUE_URL` env var with no value to _proxy-lambda_ and _runtime-emulator_

Option 3: make _proxy_lambda_resp_ queue inaccessible by changing its IAM policy.
E.g. change the resource name from the correct queue name `"Resource": "arn:aws:sqs:us-east-1:512295225992:proxy_lambda_resp"` to a non-existent name like this `"Resource": "arn:aws:sqs:us-east-1:512295225992:proxy_lambda_resp_BLOCKED"`.
Both _proxy-lambda_ and _runtime-emulator_ treat the access error as a hint to not expect a response.

### Canceling long _proxy-lambda_ wait

If your _proxy-lambda_ is configured to expect a long debugging time, e.g. 30 minutes, you may want to cancel the wait for a rerun.
Since it is impossible to kill a running lambda instance on AWS, the easiest way to cancel the wait is to send a random message to `proxy_lambda_resp` queue via the AWS console.
The waiting _proxy-lambda_ will forward it to the caller and become available for a new request.

### Large payloads and data compression

The size of the SQS payload is [limited to 262,144 bytes by SQS](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/quotas-messages.html) while [Lambda allows up to 6MB](https://docs.aws.amazon.com/lambda/latest/dg/gettingstarted-limits.html).
_proxy-lambda_ and _runtime-emulator_ compress oversized payloads using [flate2 crate](https://crates.io/crates/flate2) and send them as an encoded Base58 string to get around that limitation.

The data compression can take up to a minute in debug mode. It is significantly faster with release builds.

### Logging

Both _proxy-lambda_ and _runtime-emulator_ use `RUST_LOG` env var to set the logging level and filters.
If `RUST_LOG` is not present or is empty, both crates log at the _INFO_ level and suppress logging from their dependencies.
See [https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html#example-syntax] for more info.

Examples of `RUST_LOG` values:
- `error` - log errors only from all crates and dependencies
- `warn,runtime_emulator=info` - _INFO_ level for the _runtime-emulator_, _WARN_ level for everything else
- `proxy=debug` - detailed logging in _proxy-lambda_