use std::env;
use std::process::{Command, ExitStatus};
use std::collections::HashMap;
use futures::future::join_all;

use aws_config::meta::region::RegionProviderChain;

use aws_sdk_secretsmanager::{Client, config::Region};


fn parse_region_from_arn(arn: &str) -> Option<String> {
    arn.split(':').nth(3).map(String::from)
}

async fn setup_aws_client(region: &str) -> Client {
    let region_provider = RegionProviderChain::first_try(Region::new(region.to_string()));
    let config = aws_config::from_env().region(region_provider).load().await;
    Client::new(&config)
}

fn collect_secret_arns() -> Vec<(String, String)> {
    env::vars()
        .filter(|(key, _)| key.starts_with("SECRET_"))
        .map(|(key, value)| (key[7..].to_string(), value))
        .collect()
}
async fn get_secret_value(
    client: &Client,
    arn: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let resp = client
        .get_secret_value()
        .secret_id(arn)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch secret: {}", e))?;

    resp.secret_string()
        .ok_or_else(|| "Secret value is not a string".into())
        .map(|s| s.to_string())
}

async fn fetch_secret_values(
    clients: &HashMap<String, Client>,
    secrets: Vec<(String, String)>,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let futures = secrets.into_iter().map(|(key, arn)| async move {
        let region = parse_region_from_arn(&arn).ok_or("Invalid ARN")?;
        let client = clients.get(&region).ok_or("No client for region")?;

        let secret_value = get_secret_value(client, &arn).await?;

        Ok((key, secret_value))
    });

    let results: Vec<Result<(String, String), Box<dyn std::error::Error>>> =
        join_all(futures).await;

    results.into_iter().collect()
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

    // Collect secrets from environment variables
    let secrets = collect_secret_arns();

    // Extract unique regions from ARNs
    let unique_regions: Vec<String> = secrets.iter()
        .filter_map(|(_, arn)| parse_region_from_arn(arn))
        .collect::<std::collections::HashSet<String>>()
        .into_iter()
        .collect();

    // Set up AWS SDK clients for each unique region
    let mut clients = HashMap::new();
    for region in unique_regions {
        clients.insert(region.clone(), setup_aws_client(&region).await);
    }

    // Fetch secret values
    let env_vars = fetch_secret_values(&clients, secrets).await?;

    // Run the specified program with the new environment
    let status = run_program(program, program_args, env_vars)?;

    // Exit with the same status as the child program
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_secretsmanager::config::Credentials;

    #[test]
    fn test_parse_region_from_arn() {
        let arn = "arn:aws:secretsmanager:us-west-2:123456789012:secret:mydbpassword";
        assert_eq!(parse_region_from_arn(arn), Some("us-west-2".to_string()));

        let invalid_arn = "invalid:arn:format";
        assert_eq!(parse_region_from_arn(invalid_arn), None);
    }

    #[tokio::test]
    async fn test_setup_aws_client() {
        let client = setup_aws_client("us-west-2").await;
        assert_eq!(client.conf().region().unwrap().to_string(), "us-west-2");
    }

    #[test]
    fn test_collect_secret_arns() {
        env::set_var("SECRET_DB_PASSWORD", "arn:aws:secretsmanager:us-west-2:123456789012:secret:mydbpassword");
        env::set_var("SECRET_API_KEY", "arn:aws:secretsmanager:us-east-1:123456789012:secret:myapikey");
        env::set_var("NOT_A_SECRET", "this should be ignored");

        let secrets = collect_secret_arns();

        assert_eq!(secrets.len(), 2);
        assert!(secrets.contains(&("DB_PASSWORD".to_string(), "arn:aws:secretsmanager:us-west-2:123456789012:secret:mydbpassword".to_string())));
        assert!(secrets.contains(&("API_KEY".to_string(), "arn:aws:secretsmanager:us-east-1:123456789012:secret:myapikey".to_string())));

        // Clean up environment variables
        env::remove_var("SECRET_DB_PASSWORD");
        env::remove_var("SECRET_API_KEY");
        env::remove_var("NOT_A_SECRET");
    }

    #[tokio::test]
    async fn test_fetch_secret_values() {
        // Create mock clients
        let creds = Credentials::new(
            "access_key",
            "secret_key",
            None,
            None,
            "test"
        );
        let mut clients = HashMap::new();
        for region in &["us-west-2", "us-east-1"] {
            let conf = aws_sdk_secretsmanager::Config::builder()
                .credentials_provider(creds.clone())
                .region(Some(Region::new(region.to_string())))
                .build();
            clients.insert(region.to_string(), Client::from_conf(conf));
        }

        // Mock secrets
        let secrets = vec![
            ("DB_PASSWORD".to_string(), "arn:aws:secretsmanager:us-west-2:123456789012:secret:mydbpassword".to_string()),
            ("API_KEY".to_string(), "arn:aws:secretsmanager:us-east-1:123456789012:secret:myapikey".to_string()),
        ];

        // This will fail because we haven't actually mocked the AWS client
        // In a real scenario, you'd use a mocking library to handle this
        let result = fetch_secret_values(&clients, secrets).await;

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