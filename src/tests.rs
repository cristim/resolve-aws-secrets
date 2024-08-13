use aws_sdk_secretsmanager::error::SdkError;
use aws_sdk_secretsmanager::operation::get_secret_value::{
    GetSecretValueError, GetSecretValueOutput,
};
use aws_sdk_ssm::error::SdkError as SsmSdkError;
use aws_sdk_ssm::operation::get_parameter::{GetParameterError, GetParameterOutput};
use aws_sdk_ssm::types::Parameter;
use mockall::mock;
use mockall::predicate::*;
use std::collections::HashMap;

use crate::environment_processor::process_environment;
use crate::secret_manager::{get_secret, SecretsManagerClientTrait};
use crate::ssm_manager::{get_ssm_parameter, SsmClientTrait};

// Create mock implementations
mock! {
    pub SecretsManagerClient {}

    #[async_trait::async_trait]
    impl SecretsManagerClientTrait for SecretsManagerClient {
        async fn get_secret_value(&self, secret_id: &str) -> Result<GetSecretValueOutput, SdkError<GetSecretValueError>>;
    }
}

mock! {
    pub SsmClient {}

    #[async_trait::async_trait]
    impl SsmClientTrait for SsmClient {
        async fn get_parameter(&self, name: &str, with_decryption: bool) -> Result<GetParameterOutput, SsmSdkError<GetParameterError>>;
    }
}

#[tokio::test]
async fn test_get_secret() {
    let mut mock_client = MockSecretsManagerClient::new();
    mock_client
        .expect_get_secret_value()
        .with(eq("test-arn"))
        .returning(|_| {
            Ok(GetSecretValueOutput::builder()
                .secret_string("test-secret")
                .build())
        });

    let result = get_secret(&mock_client, "test-arn").await.unwrap();
    assert_eq!(result, "test-secret");
}

#[tokio::test]
async fn test_get_ssm_parameter() {
    let mut mock_client = MockSsmClient::new();
    mock_client
        .expect_get_parameter()
        .with(eq("test-arn"), eq(true))
        .returning(|_, _| {
            Ok(GetParameterOutput::builder()
                .parameter(Parameter::builder().value("test-parameter").build())
                .build())
        });

    let result = get_ssm_parameter(&mock_client, "test-arn").await.unwrap();
    assert_eq!(result, "test-parameter");
}

#[tokio::test]
async fn test_process_environment() {
    let mut mock_secrets_client = MockSecretsManagerClient::new();
    mock_secrets_client
        .expect_get_secret_value()
        .returning(|secret_id| {
            Ok(GetSecretValueOutput::builder()
                .secret_string(format!("secret-value-{}", secret_id))
                .build())
        });

    let mut mock_ssm_client = MockSsmClient::new();
    mock_ssm_client.expect_get_parameter().returning(|name, _| {
        Ok(GetParameterOutput::builder()
            .parameter(
                Parameter::builder()
                    .value(format!(
                        "{{\"SECRET_PARAM1\":\"arn:secret1\",\"SECRET_PARAM2\":\"arn:secret2\"}}"
                    ))
                    .build(),
            )
            .build())
    });

    // Set up environment variables
    std::env::set_var("SECRET_TEST1", "arn:test1");
    std::env::set_var("SECRET_TEST2", "arn:test2");
    std::env::set_var("SECRETS_PARAMETER_ARN", "arn:ssm:parameter");

    let result = process_environment(&mock_secrets_client, &mock_ssm_client)
        .await
        .unwrap();

    // Convert result to HashMap for easier assertion
    let result_map: HashMap<_, _> = result.into_iter().collect();

    assert_eq!(
        result_map.get("TEST1"),
        Some(&"secret-value-arn:test1".to_string())
    );
    assert_eq!(
        result_map.get("TEST2"),
        Some(&"secret-value-arn:test2".to_string())
    );
    assert_eq!(
        result_map.get("PARAM1"),
        Some(&"secret-value-arn:secret1".to_string())
    );
    assert_eq!(
        result_map.get("PARAM2"),
        Some(&"secret-value-arn:secret2".to_string())
    );

    // Clean up environment variables
    std::env::remove_var("SECRET_TEST1");
    std::env::remove_var("SECRET_TEST2");
    std::env::remove_var("SECRETS_PARAMETER_ARN");
}
