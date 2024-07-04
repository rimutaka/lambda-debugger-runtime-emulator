# These env vars are required by the lambda function you are testing locally

# Run this script in the same terminal window as the lambda to set the vars before its launch
# or set them globally by running this script at startup, e.g. in .bashrc.

# Replace the values, if needed
export AWS_LAMBDA_FUNCTION_VERSION=$LATEST
export AWS_LAMBDA_FUNCTION_MEMORY_SIZE=128
export AWS_LAMBDA_FUNCTION_NAME=my-lambda

# Leave the AWS default (127.0.0.1:9001) unless it conflicts with your local setup.
# It tells the lambda function where the runtime emulator is.
# This value must match the value in env-emulator.sh file.
export AWS_LAMBDA_RUNTIME_API=127.0.0.1:9001