# Resolve AWS Secrets

This tool retrieves secrets from AWS Secrets Manager resources given as environment variables and defines them as environment variables to the program executed as argument.

The secrets are expected to be prefixed with `SECRET_` and to contain a valid ARN of a secretmanager resource, such as `SECRET_FOO="arn:aws:secretsmanager:us-west-2:123456789012:secret:myapikey`.

The tool then creates environment variables `FOO=secret_value`, where `secret_value`is the value stored in the `SECRET_FOO` secretmanager secret.

The tool then runs the program given as command line argument with the resolved secrets defined as such environment variables.

It is meant to be used from Lambda functions that use Docker images, which lack the ability to resolve secrets from ARNs.

## Usage

1. Set up your Lambda function with environment variables in the format `SECRET_FOO=arn:aws:secretsmanager:region:account-id:secret:secret-name`.

2. Add the binary to your Lambda function using our prebuilt Docker image: `cristim/resolve-aws-secrets:latest` or use your own image you can build using the Makefile.

   ```bash
   COPY --from=cristim/resolve-aws-secrets:latest /resolve-aws-secrets /resolve-aws-secrets
   ```

3. Edit the entrypoint configuration of your Lambda function's Docker image:

   ```bash
   CMD ["initial-entrypoint", "--arg1", "--arg2"]
   ```

   to

   ```bash
   CMD ["/resolve-aws-secrets", "initial-entrypoint", "--arg1", "--arg2"]
   ```

4. The tool will resolve all the secrets named `SECRET_FOO=<arn>` into `FOO=secret-value`.

5. In your Lambda function code, just use the environment variables as `FOO`, without the `SECRET_` prefix.

## Secret rotation

In case secrets get rotated, one way to refresh the secrets is by crashing the function with an error status code after the secrets were rotated and no longer work. This should trigger a rerun of the Lambda function, so the secret values will be resolved again.

## IAM Configuration

Ensure that your Lambda function IAM role has the usual IAM permissions needed to access the secrets in AWS Secrets Manager.

No additional configuration is required. The extension uses the AWS SDK's default credential provider chain and connects to the region of each secretmanager ARN.

## Building the code (optional, for local development or running your own fork)

Prerequisites

- Docker
- make
- Rust 1.69 or later

1. Clone this repository:

   ```shell
   git clone https://github.com/your-username/resolve-aws-secrets.git
   cd resolve-aws-secrets
   ```

2. Build the Docker image (optional):

   ```shell
   export DOCKER_USERNAME=your-dockerhub-username
   export DOCKER_PASSWORD=your-dockerhub-password
   make
   ```

## Contributing

Contributions are welcome, feel free to submit issues or Pull Requests as usual.

## License

This project is @2024 Cristian Magherusan-Stanciu of [leanercloud.com](https://leanercloud.com), and licensed under the MIT License.

Check out more of our projects at [github.com/LeanerCloud](https://github.com/LeanerCloud).
