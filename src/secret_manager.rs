use aws_sdk_secretsmanager::error::SdkError;
use aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError;
use aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueOutput;
use std::error::Error;
use tracing::{info, instrument};

#[async_trait::async_trait]
pub trait SecretsManagerClientTrait {
    async fn get_secret_value(
        &self,
        secret_id: &str,
    ) -> Result<GetSecretValueOutput, SdkError<GetSecretValueError>>;
}

#[async_trait::async_trait]
impl SecretsManagerClientTrait for aws_sdk_secretsmanager::Client {
    async fn get_secret_value(
        &self,
        secret_id: &str,
    ) -> Result<GetSecretValueOutput, SdkError<GetSecretValueError>> {
        self.get_secret_value().secret_id(secret_id).send().await
    }
}

#[instrument(skip(client))]
pub async fn get_secret<T: SecretsManagerClientTrait + ?Sized>(
    client: &T,
    arn: &str,
) -> Result<String, Box<dyn Error>> {
    info!("Retrieving secret from Secrets Manager: {}", arn);
    let response = client.get_secret_value(arn).await?;
    Ok(response.secret_string().unwrap_or_default().to_string())
}
