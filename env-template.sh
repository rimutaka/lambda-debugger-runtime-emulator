# This file is a sample list of all environmental variables exported from AWS Lambda.
#
# To generate your own list:
# 1. deploy proxy-lambda function to AWS
# 2. run a test event in the console
# 3. copy your list of environmental variables from the CloudWatch log
#
# This list contains more variables than what is returned by the AWS CLI when you deploy the function.
# See env-lambda.sh for the subset of vars required to run the runtime emulator.

export AWS_DEFAULT_REGION=us-east-1
export AWS_LAMBDA_FUNCTION_MEMORY_SIZE=128
export AWS_LAMBDA_FUNCTION_NAME=lambda-debug-proxy
export AWS_LAMBDA_FUNCTION_VERSION=$LATEST
export AWS_LAMBDA_INITIALIZATION_TYPE=on-demand
export AWS_LAMBDA_LOG_FORMAT=Text
export AWS_LAMBDA_LOG_GROUP_NAME=/aws/lambda/lambda-debug-proxy
export AWS_LAMBDA_LOG_STREAM_NAME=2024/06/04/lambda-debug-proxy[$LATEST]c026cde3732340049aff322b9cc3c19b
export AWS_LAMBDA_RUNTIME_API=127.0.0.1:9001 # This is the default value. Change it only if it conflicts with your local setup
export AWS_REGION=us-east-1
export AWS_XRAY_CONTEXT_MISSING=LOG_ERROR
export AWS_XRAY_DAEMON_ADDRESS=169.254.79.129:2000
export PROXY_LAMBDA_REQ_QUEUE_URL=https://sqs.us-east-1.amazonaws.com/512295225992/proxy_lambda_req # Replace with your own queue URL (the a/c number will be different!)
export PROXY_LAMBDA_RESP_QUEUE_URL=https://sqs.us-east-1.amazonaws.com/512295225992/proxy_lambda_resp # Replace with your own queue URL (the a/c number will be different!)
export LAMBDA_RUNTIME_DIR=/var/runtime
export LAMBDA_TASK_ROOT=/var/task
export LANG=en_US.UTF-8
export LD_LIBRARY_PATH=/lib64:/usr/lib64:/var/runtime:/var/runtime/lib:/var/task:/var/task/lib:/opt/lib
export PATH=/usr/local/bin:/usr/bin/:/bin:/opt/bin
export TZ=:UTC
export _AWS_XRAY_DAEMON_ADDRESS=169.254.79.129
export _AWS_XRAY_DAEMON_PORT=2000
export _HANDLER=hello.handler