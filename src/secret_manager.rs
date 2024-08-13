use aws_sdk_secretsmanager::Client as SecretsManagerClient;
use std::error::Error;
use tracing::{info, instrument};

#[instrument(skip(client))]
pub async fn get_secret(
    client: &SecretsManagerClient,
    arn: &str,
) -> Result<String, Box<dyn Error>> {
    info!("Retrieving secret from Secrets Manager: {}", arn);
    let response = client.get_secret_value().secret_id(arn).send().await?;
    Ok(response.secret_string().unwrap_or_default().to_string())
}
