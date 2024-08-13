use crate::secret_manager::{get_secret, SecretsManagerClientTrait};
use crate::ssm_manager::{get_ssm_parameter, SsmClientTrait};
use serde_json::Value;
use std::error::Error;
use tracing::{info, instrument, warn};

#[instrument(skip(secretsmanager_client, ssm_client))]
pub async fn process_environment<S, T>(
    secretsmanager_client: &S,
    ssm_client: &T,
) -> Result<Vec<(String, String)>, Box<dyn Error>>
where
    S: SecretsManagerClientTrait + ?Sized,
    T: SsmClientTrait + ?Sized,
{
    let mut results = Vec::new();

    for (key, value) in std::env::vars() {
        if key.starts_with("SECRET_") && value.starts_with("arn:") {
            let secret_value = get_secret(secretsmanager_client, &value).await?;
            results.push((key.trim_start_matches("SECRET_").to_string(), secret_value));
        }
    }

    if let Ok(ssm_arn) = std::env::var("SECRETS_PARAMETER_ARN") {
        let ssm_secrets =
            process_ssm_parameter(ssm_client, secretsmanager_client, &ssm_arn).await?;
        results.extend(ssm_secrets);
    }

    if let Ok(ssm_name) = std::env::var("SECRETS_PARAMETER_NAME") {
        let ssm_secrets =
            process_ssm_parameter(ssm_client, secretsmanager_client, &ssm_name).await?;
        results.extend(ssm_secrets);
    }

    Ok(results)
}

#[instrument(skip(ssm_client, secretsmanager_client))]
async fn process_ssm_parameter<
    S: SecretsManagerClientTrait + ?Sized,
    T: SsmClientTrait + ?Sized,
>(
    ssm_client: &T,
    secretsmanager_client: &S,
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
