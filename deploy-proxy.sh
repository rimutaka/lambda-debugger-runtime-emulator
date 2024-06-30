target=x86_64-unknown-linux-gnu
region=us-east-1
lambda=my-lambda
crate=proxy-lambda

cargo build --release --target $target
cp ./target/$target/release/$crate ./bootstrap && zip proxy.zip bootstrap && rm bootstrap
aws lambda update-function-code --region $region --function-name $lambda --zip-file fileb://proxy.zip

# Available targets: 
# x86_64-unknown-linux-gnu
# x86_64-unknown-linux-musl
# aarch64-unknown-linux-gnu
# aarch64-unknown-linux-musl