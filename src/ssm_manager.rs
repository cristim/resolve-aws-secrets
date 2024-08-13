use aws_sdk_ssm::Client as SsmClient;
use std::error::Error;
use tracing::{info, instrument};

#[instrument(skip(client))]
pub async fn get_ssm_parameter(client: &SsmClient, arn: &str) -> Result<String, Box<dyn Error>> {
    info!("Retrieving SSM parameter: {}", arn);
    let response = client
        .get_parameter()
        .name(arn)
        .with_decryption(true)
        .send()
        .await?;
    Ok(response
        .parameter()
        .and_then(|p| p.value())
        .unwrap_or_default()
        .to_string())
}
