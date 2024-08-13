use aws_config::meta::region::RegionProviderChain;
use aws_sdk_secretsmanager::Client as SecretsManagerClient;
use aws_sdk_ssm::Client as SsmClient;
use std::env;
use std::error::Error;
use tracing::{error, info, instrument};

mod environment_processor;
mod secret_manager;
mod ssm_manager;

#[cfg(test)]
pub mod tests;

use crate::environment_processor::process_environment;
use crate::secret_manager::SecretsManagerClientTrait;
use crate::ssm_manager::SsmClientTrait;

#[tokio::main]
#[instrument]
async fn main() -> Result<(), Box<dyn Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("Starting application");

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        error!("Insufficient arguments provided");
        eprintln!("Usage: {} <program> [args...]", args[0]);
        std::process::exit(1);
    }

    info!("Initializing AWS configuration");
    let region_provider = RegionProviderChain::default_provider().or_else("us-east-1");
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(region_provider)
        .load()
        .await;

    info!("Creating AWS clients");
    let secretsmanager_client = SecretsManagerClient::new(&config);
    let ssm_client = SsmClient::new(&config);

    info!("Processing environment");
    let secrets = process_environment(&secretsmanager_client, &ssm_client).await?;
    info!("Processed {} environment variables", secrets.len());

    // Create a new environment with both existing and new variables
    let mut new_env: std::collections::HashMap<String, String> = env::vars().collect();
    for (key, value) in &secrets {
        info!("Setting environment variable: {}", key);
        new_env.insert(key.clone(), value.clone());
    }

    info!("Executing command: {}", args[1]);
    let status = std::process::Command::new(&args[1])
        .args(&args[2..])
        .envs(&new_env)
        .status()?;

    let exit_code = status.code().unwrap_or(1);
    info!("Command exited with status code: {}", exit_code);
    std::process::exit(exit_code)
}
