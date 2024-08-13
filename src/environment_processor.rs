use aws_sdk_secretsmanager::Client as SecretsManagerClient;
use aws_sdk_ssm::Client as SsmClient;
use serde_json::Value;
use std::env;
use std::error::Error;
use tracing::{info, instrument, warn};

use crate::secret_manager::get_secret;
use crate::ssm_manager::get_ssm_parameter;

#[instrument(skip(secretsmanager_client, ssm_client))]
pub async fn process_environment(
    secretsmanager_client: &SecretsManagerClient,
    ssm_client: &SsmClient,
) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    info!("Processing environment variables");
    let mut results = Vec::new();

    process_secret_envs(secretsmanager_client, &mut results).await?;
    process_ssm_parameter_arn(ssm_client, secretsmanager_client, &mut results).await?;
    process_ssm_parameter_name(ssm_client, secretsmanager_client, &mut results).await?;

    Ok(results)
}

async fn process_secret_envs(
    secretsmanager_client: &SecretsManagerClient,
    results: &mut Vec<(String, String)>,
) -> Result<(), Box<dyn Error>> {
    for (key, value) in env::vars() {
        if key.starts_with("SECRET_") && value.starts_with("arn:") {
            info!("Processing secret: {}", key);
            let secret_value = get_secret(secretsmanager_client, &value).await?;
            results.push((key.trim_start_matches("SECRET_").to_string(), secret_value));
        }
    }
    Ok(())
}

async fn process_ssm_parameter_arn(
    ssm_client: &SsmClient,
    secretsmanager_client: &SecretsManagerClient,
    results: &mut Vec<(String, String)>,
) -> Result<(), Box<dyn Error>> {
    if let Ok(ssm_arn) = env::var("SECRETS_PARAMETER_ARN") {
        info!("Processing SSM parameter ARN");
        let ssm_secrets =
            process_ssm_parameter(ssm_client, secretsmanager_client, &ssm_arn).await?;
        results.extend(ssm_secrets);
    }
    Ok(())
}

async fn process_ssm_parameter_name(
    ssm_client: &SsmClient,
    secretsmanager_client: &SecretsManagerClient,
    results: &mut Vec<(String, String)>,
) -> Result<(), Box<dyn Error>> {
    if let Ok(ssm_name) = env::var("SECRETS_PARAMETER_NAME") {
        info!("Processing SSM parameter name");
        let ssm_secrets =
            process_ssm_parameter(ssm_client, secretsmanager_client, &ssm_name).await?;
        results.extend(ssm_secrets);
    }
    Ok(())
}

#[instrument(skip(ssm_client, secretsmanager_client))]
async fn process_ssm_parameter(
    ssm_client: &SsmClient,
    secretsmanager_client: &SecretsManagerClient,
    arn: &str,
) -> Result<Vec<(String, String)>, Box<dyn Error>> {
    info!("Processing SSM parameter: {}", arn);
    let parameter_value = get_ssm_parameter(ssm_client, arn).await?;
    let json_value: Value = serde_json::from_str(&parameter_value)?;
    let mut results = Vec::new();

    if let Value::Object(obj) = json_value {
        for (key, value) in obj {
            if let Value::String(arn) = value {
                let stripped_key = key.strip_prefix("SECRET_").unwrap_or(&key);
                info!("Processing secret {} from SSM parameter", stripped_key);
                let secret_value = get_secret(secretsmanager_client, &arn).await?;
                results.push((stripped_key.to_string(), secret_value));
            } else {
                warn!("Unexpected value type for key {} in SSM parameter", key);
            }
        }
    } else {
        warn!("SSM parameter value is not an object");
    }

    Ok(results)
}
