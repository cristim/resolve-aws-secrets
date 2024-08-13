use crate::environment_processor::process_environment;
use crate::secret_manager::SecretsManagerClientTrait;
use crate::ssm_manager::SsmClientTrait;
use aws_sdk_secretsmanager::error::SdkError;
use aws_sdk_secretsmanager::operation::get_secret_value::{
    GetSecretValueError, GetSecretValueOutput,
};
use aws_sdk_ssm::error::SdkError as SsmSdkError;
use aws_sdk_ssm::operation::get_parameter::{GetParameterError, GetParameterOutput};
use aws_sdk_ssm::types::Parameter;
use mockall::mock;
use mockall::predicate::*;
use serial_test::serial;
use std::collections::HashMap;
use std::sync::Once;
use std::time::Duration;

static INIT: Once = Once::new();

fn initialize() {
    INIT.call_once(|| {
        env_logger::init();
    });
}

fn reset_environment() {
    for (key, _) in std::env::vars().collect::<Vec<(String, String)>>() {
        if key.starts_with("SECRET_")
            || key == "SECRETS_PARAMETER_ARN"
            || key == "SECRETS_PARAMETER_NAME"
        {
            std::env::remove_var(&key);
        }
    }
}

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

fn setup_mock_secrets_client() -> MockSecretsManagerClient {
    let mut client = MockSecretsManagerClient::new();
    client.expect_get_secret_value().returning(|secret_id| {
        Ok(GetSecretValueOutput::builder()
            .secret_string(format!("secret-value-{}", secret_id))
            .build())
    });
    client
}

macro_rules! async_test {
    ($name:ident, $body:expr) => {
        #[tokio::test]
        #[serial]
        async fn $name() {
            initialize();
            reset_environment();
            $body;
        }
    };
}
async_test!(test_get_secret_success, {
    let mut mock_client = MockSecretsManagerClient::new();
    mock_client
        .expect_get_secret_value()
        .with(eq("test-arn"))
        .times(1)
        .returning(|_| {
            Ok(GetSecretValueOutput::builder()
                .secret_string("test-secret")
                .build())
        });

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        crate::secret_manager::get_secret(&mock_client, "test-arn"),
    )
    .await
    .expect("Test timed out")
    .expect("Failed to get secret");

    assert_eq!(result, "test-secret");
});

async_test!(test_get_secret_error, {
    let mut mock_client = MockSecretsManagerClient::new();
    mock_client
        .expect_get_secret_value()
        .with(eq("test-arn"))
        .times(1)
        .returning(|_| {
            Err(SdkError::service_error(
                GetSecretValueError::InvalidParameterException(
                    aws_sdk_secretsmanager::types::error::InvalidParameterException::builder()
                        .message("Invalid parameter")
                        .build(),
                ),
                aws_smithy_runtime_api::http::Response::new(
                    aws_smithy_runtime_api::http::StatusCode::try_from(400).unwrap(),
                    aws_smithy_types::body::SdkBody::empty(),
                ),
            ))
        });

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        crate::secret_manager::get_secret(&mock_client, "test-arn"),
    )
    .await
    .expect("Test timed out");

    assert!(result.is_err());
    assert!(matches!(
        result
            .unwrap_err()
            .downcast_ref::<SdkError<GetSecretValueError>>(),
        Some(SdkError::ServiceError(_))
    ));
});

async_test!(test_get_ssm_parameter_success, {
    let mut mock_client = MockSsmClient::new();
    mock_client
        .expect_get_parameter()
        .with(eq("test-arn"), eq(true))
        .times(1)
        .returning(|_, _| {
            Ok(GetParameterOutput::builder()
                .parameter(Parameter::builder().value("test-parameter").build())
                .build())
        });

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        crate::ssm_manager::get_ssm_parameter(&mock_client, "test-arn"),
    )
    .await
    .expect("Test timed out")
    .expect("Failed to get SSM parameter");

    assert_eq!(result, "test-parameter");
});

async_test!(test_get_ssm_parameter_error, {
    let mut mock_client = MockSsmClient::new();
    mock_client
        .expect_get_parameter()
        .with(eq("test-arn"), eq(true))
        .times(1)
        .returning(|_, _| {
            Err(SsmSdkError::service_error(
                GetParameterError::ParameterNotFound(
                    aws_sdk_ssm::types::error::ParameterNotFound::builder()
                        .message("Parameter not found")
                        .build(),
                ),
                aws_smithy_runtime_api::http::Response::new(
                    aws_smithy_runtime_api::http::StatusCode::try_from(404).unwrap(),
                    aws_smithy_types::body::SdkBody::empty(),
                ),
            ))
        });

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        crate::ssm_manager::get_ssm_parameter(&mock_client, "test-arn"),
    )
    .await
    .expect("Test timed out");

    assert!(result.is_err());
    assert!(matches!(
        result
            .unwrap_err()
            .downcast_ref::<SsmSdkError<GetParameterError>>(),
        Some(SsmSdkError::ServiceError(_))
    ));
});

async_test!(test_process_environment_no_secrets, {
    let mock_secrets_client = MockSecretsManagerClient::new();
    let mock_ssm_client = MockSsmClient::new();

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        process_environment(&mock_secrets_client, &mock_ssm_client),
    )
    .await
    .expect("Test timed out")
    .expect("Failed to process environment");

    assert!(result.is_empty());
});

async_test!(test_process_environment_invalid_json, {
    let mock_secrets_client = MockSecretsManagerClient::new();

    let mut mock_ssm_client = MockSsmClient::new();
    mock_ssm_client
        .expect_get_parameter()
        .with(eq("arn:ssm:parameter"), eq(true))
        .times(1)
        .returning(|_, _| {
            Ok(GetParameterOutput::builder()
                .parameter(Parameter::builder().value("invalid-json").build())
                .build())
        });

    std::env::set_var("SECRETS_PARAMETER_ARN", "arn:ssm:parameter");

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        process_environment(&mock_secrets_client, &mock_ssm_client),
    )
    .await
    .expect("Test timed out");

    assert!(result.is_err());
});

async_test!(test_process_environment_success, {
    let mock_secrets_client = setup_mock_secrets_client();
    let mut mock_ssm_client = MockSsmClient::new();

    mock_ssm_client
        .expect_get_parameter()
        .with(eq("arn:ssm:parameter"), eq(true))
        .times(1)
        .returning(|_, _| {
            Ok(GetParameterOutput::builder()
                .parameter(
                    Parameter::builder()
                        .value(r#"{"SECRET_PARAM1":"arn:secret1","SECRET_PARAM2":"arn:secret2"}"#)
                        .build(),
                )
                .build())
        });

    std::env::set_var("SECRET_TEST1", "arn:test1");
    std::env::set_var("SECRET_TEST2", "arn:test2");
    std::env::set_var("SECRETS_PARAMETER_ARN", "arn:ssm:parameter");

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        process_environment(&mock_secrets_client, &mock_ssm_client),
    )
    .await
    .expect("Test timed out")
    .expect("Failed to process environment");

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
});

async_test!(test_process_environment_ssm_parameter_name, {
    let mock_secrets_client = setup_mock_secrets_client();
    let mut mock_ssm_client = MockSsmClient::new();

    mock_ssm_client
        .expect_get_parameter()
        .with(eq("test-parameter-name"), eq(true))
        .times(1)
        .returning(|_, _| {
            Ok(GetParameterOutput::builder()
                .parameter(
                    Parameter::builder()
                        .value(r#"{"SECRET_PARAM1":"arn:secret1","SECRET_PARAM2":"arn:secret2"}"#)
                        .build(),
                )
                .build())
        });

    std::env::set_var("SECRETS_PARAMETER_NAME", "test-parameter-name");

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        process_environment(&mock_secrets_client, &mock_ssm_client),
    )
    .await
    .expect("Test timed out")
    .expect("Failed to process environment");

    let result_map: HashMap<_, _> = result.into_iter().collect();

    assert_eq!(
        result_map.get("PARAM1"),
        Some(&"secret-value-arn:secret1".to_string())
    );
    assert_eq!(
        result_map.get("PARAM2"),
        Some(&"secret-value-arn:secret2".to_string())
    );
});
