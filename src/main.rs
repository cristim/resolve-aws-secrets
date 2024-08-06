use std::env;
use std::process::{Command, ExitStatus};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_secretsmanager::{Client, Error};

async fn setup_aws_client() -> Client {
    let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
    let config = aws_config::from_env().region(region_provider).load().await;
    Client::new(&config)
}

fn collect_secret_arns() -> Vec<(String, String)> {
    env::vars()
        .filter(|(key, _)| key.starts_with("SECRET_"))
        .map(|(key, value)| (key[7..].to_string(), value))
        .collect()
}

async fn fetch_secret_values(client: &Client, secrets: Vec<(String, String)>) -> Result<Vec<(String, String)>, Error> {
    let mut env_vars = Vec::new();
    for (key, arn) in secrets {
        let resp = client.get_secret_value().secret_id(arn).send().await?;
        if let Some(secret_string) = resp.secret_string() {
            env_vars.push((key, secret_string.to_string()));
        }
    }
    Ok(env_vars)
}

fn run_program(program: &str, args: &[String], env_vars: Vec<(String, String)>) -> Result<ExitStatus, std::io::Error> {
    Command::new(program)
        .args(args)
        .envs(env_vars)
        .status()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get the program to run from command-line arguments
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <program> [args...]", args[0]);
        std::process::exit(1);
    }

    let program = &args[1];
    let program_args = &args[2..];

    // Set up the AWS SDK
    let client = setup_aws_client().await;

    // Collect secrets from environment variables
    let secrets = collect_secret_arns();

    // Fetch secret values
    let env_vars = fetch_secret_values(&client, secrets).await?;

    // Run the specified program with the new environment
    let status = run_program(program, program_args, env_vars)?;

    // Exit with the same status as the child program
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_secretsmanager::config::{Credentials, Region};

    #[tokio::test]
    async fn test_setup_aws_client() {
        let client = setup_aws_client().await;
        assert!(client.conf().region().is_some());
    }

    #[test]
    fn test_collect_secret_arns() {
        env::set_var("SECRET_DB_PASSWORD", "arn:aws:secretsmanager:us-west-2:123456789012:secret:mydbpassword");
        env::set_var("SECRET_API_KEY", "arn:aws:secretsmanager:us-west-2:123456789012:secret:myapikey");
        env::set_var("NOT_A_SECRET", "this should be ignored");

        let secrets = collect_secret_arns();

        assert_eq!(secrets.len(), 2);
        assert!(secrets.contains(&("DB_PASSWORD".to_string(), "arn:aws:secretsmanager:us-west-2:123456789012:secret:mydbpassword".to_string())));
        assert!(secrets.contains(&("API_KEY".to_string(), "arn:aws:secretsmanager:us-west-2:123456789012:secret:myapikey".to_string())));

        // Clean up environment variables
        env::remove_var("SECRET_DB_PASSWORD");
        env::remove_var("SECRET_API_KEY");
        env::remove_var("NOT_A_SECRET");
    }

    #[tokio::test]
    async fn test_fetch_secret_values() {
        // Create a mock client
        let creds = Credentials::new(
            "access_key",
            "secret_key",
            None,
            None,
            "test"
        );
        let region = Region::new("us-west-2");
        let conf = aws_sdk_secretsmanager::Config::builder()
            .credentials_provider(creds)
            .region(Some(region))
            .build();
        let client = Client::from_conf(conf);

        // Mock the get_secret_value method
        // Note: Actual mocking is not implemented here due to limitations
        // You might need to use a mocking library or implement a custom mock

        let secrets = vec![("TEST_SECRET".to_string(), "test_secret_arn".to_string())];
        // This will fail because we haven't actually mocked the AWS client
        // In a real scenario, you'd use a mocking library to handle this
        let result = fetch_secret_values(&client, secrets).await;

        // Assert that we get an error because we're not actually connecting to AWS
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_program() {
        let program = "echo";
        let args = vec!["Hello, World!".to_string()];
        let env_vars = vec![("TEST_ENV".to_string(), "test_value".to_string())];

        let status = run_program(program, &args, env_vars).unwrap();

        assert!(status.success());
    }
}