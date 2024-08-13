use aws_sdk_ssm::error::SdkError;
use aws_sdk_ssm::operation::get_parameter::GetParameterError;
use aws_sdk_ssm::operation::get_parameter::GetParameterOutput;
use std::error::Error;
use tracing::{info, instrument};

#[async_trait::async_trait]
pub trait SsmClientTrait {
    async fn get_parameter(
        &self,
        name: &str,
        with_decryption: bool,
    ) -> Result<GetParameterOutput, SdkError<GetParameterError>>;
}

#[async_trait::async_trait]
impl SsmClientTrait for aws_sdk_ssm::Client {
    async fn get_parameter(
        &self,
        name: &str,
        with_decryption: bool,
    ) -> Result<GetParameterOutput, SdkError<GetParameterError>> {
        self.get_parameter()
            .name(name)
            .with_decryption(with_decryption)
            .send()
            .await
    }
}

#[instrument(skip(client))]
pub async fn get_ssm_parameter<T: SsmClientTrait + ?Sized>(
    client: &T,
    arn: &str,
) -> Result<String, Box<dyn Error>> {
    info!("Retrieving SSM parameter: {}", arn);
    let response = client.get_parameter(arn, true).await?;
    Ok(response
        .parameter()
        .and_then(|p| p.value())
        .unwrap_or_default()
        .to_string())
}
