use futures::future::join_all;
use std::collections::HashMap;
use std::env;
use std::process::{Command, ExitStatus};
use std::sync::Arc;
use tokio::sync::Mutex;

use aws_config::meta::region::RegionProviderChain;
use aws_sdk_secretsmanager::Client as SecretsManagerClient;
use aws_sdk_ssm::Client as SsmClient;
use aws_types::region::Region;

use async_trait::async_trait;

#[cfg(test)]
use aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueError;

#[cfg(test)]
use mockall::mock;

#[cfg(test)]
use mockall::predicate::*;

#[cfg(test)]
use aws_sdk_ssm::operation::get_parameter::GetParameterError;

#[derive(Debug, Clone)]
enum SecretType {
    SecretsManagerArn(String),
    SsmArn(String),
    Name(String),
}

fn parse_region_from_arn(arn: &str) -> Option<String> {
    arn.split(':').nth(3).map(String::from)
}

fn determine_secret_type(value: &str) -> SecretType {
    if value.starts_with("arn:") {
        let parts: Vec<&str> = value.split(':').collect();
        if parts.len() >= 6 {
            match parts[2] {
                "secretsmanager" => SecretType::SecretsManagerArn(value.to_string()),
                "ssm" => SecretType::SsmArn(value.to_string()),
                _ => SecretType::Name(value.to_string()),
            }
        } else {
            SecretType::Name(value.to_string())
        }
    } else {
        SecretType::Name(value.to_string())
    }
}

async fn setup_aws_clients(region: Option<&str>) -> (Arc<SecretsManagerClient>, Arc<SsmClient>) {
    let region_provider = region
        .map(|r| Region::new(r.to_string()))
        .map(|r| RegionProviderChain::first_try(r))
        .unwrap_or_else(RegionProviderChain::default_provider);

    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(region_provider)
        .load()
        .await;
    (
        Arc::new(SecretsManagerClient::new(&config)),
        Arc::new(SsmClient::new(&config)),
    )
}

fn collect_secrets() -> Vec<(String, SecretType)> {
    env::vars()
        .filter_map(|(key, value)| {
            if key.starts_with("SECRET_ARN_") {
                Some((key[11..].to_string(), determine_secret_type(&value)))
            } else if key.starts_with("SECRET_NAME_") {
                Some((key[12..].to_string(), SecretType::Name(value)))
            } else if key.starts_with("SECRET_") {
                Some((key[7..].to_string(), determine_secret_type(&value)))
            } else {
                None
            }
        })
        .collect()
}

async fn get_secret_value(
    sm_client: &dyn SecretsManagerClientTrait,
    ssm_client: &dyn SsmClientTrait,
    secret_type: &SecretType,
) -> Result<String, Box<dyn std::error::Error>> {
    match secret_type {
        SecretType::SecretsManagerArn(arn) => sm_client.get_secret_value(arn.to_string()).await,
        SecretType::SsmArn(arn) | SecretType::Name(arn) => {
            ssm_client.get_parameter(arn.to_string(), true).await
        }
    }
}

async fn fetch_secret_values(
    secrets: Vec<(String, SecretType)>,
) -> Result<Vec<(String, String)>, Box<dyn std::error::Error>> {
    let (default_sm_client, default_ssm_client) = setup_aws_clients(None).await;
    let default_sm_client: Arc<dyn SecretsManagerClientTrait> = Arc::new(default_sm_client);
    let default_ssm_client: Arc<dyn SsmClientTrait> = Arc::new(default_ssm_client);
    let region_clients: Arc<
        Mutex<HashMap<String, (Arc<dyn SecretsManagerClientTrait>, Arc<dyn SsmClientTrait>)>>,
    > = Arc::new(Mutex::new(HashMap::new()));

    let futures = secrets.into_iter().map(|(key, secret_type)| {
        let default_sm_client = Arc::clone(&default_sm_client);
        let default_ssm_client = Arc::clone(&default_ssm_client);
        let region_clients = Arc::clone(&region_clients);
        async move {
            let (sm_client, ssm_client) = match &secret_type {
                SecretType::SecretsManagerArn(arn) | SecretType::SsmArn(arn) => {
                    if let Some(region) = parse_region_from_arn(arn) {
                        let mut clients = region_clients.lock().await;
                        let (sm_client, ssm_client) =
                            clients.entry(region.clone()).or_insert_with(|| {
                                let (sm, ssm) = tokio::runtime::Handle::current()
                                    .block_on(setup_aws_clients(Some(&region)));
                                (
                                    Arc::new(sm) as Arc<dyn SecretsManagerClientTrait>,
                                    Arc::new(ssm) as Arc<dyn SsmClientTrait>,
                                )
                            });
                        (Arc::clone(sm_client), Arc::clone(ssm_client))
                    } else {
                        (
                            Arc::clone(&default_sm_client),
                            Arc::clone(&default_ssm_client),
                        )
                    }
                }
                SecretType::Name(_) => (
                    Arc::clone(&default_sm_client),
                    Arc::clone(&default_ssm_client),
                ),
            };

            let secret_value = get_secret_value(&*sm_client, &*ssm_client, &secret_type).await?;
            Ok((key, secret_value))
        }
    });

    join_all(futures)
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
}

fn run_program(
    program: &str,
    args: &[String],
    env_vars: Vec<(String, String)>,
) -> Result<ExitStatus, std::io::Error> {
    Command::new(program).args(args).envs(env_vars).status()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <program> [args...]", args[0]);
        std::process::exit(1);
    }

    let program = &args[1];
    let program_args = &args[2..];

    let secrets = collect_secrets();
    let env_vars = fetch_secret_values(secrets).await?;

    let status = run_program(program, program_args, env_vars)?;

    std::process::exit(status.code().unwrap_or(1))
}

#[cfg(test)]
mock! {
    pub SecretsManagerClient {
        pub fn get_secret_value(&self, secret_id: String) -> Result<String, Box<dyn std::error::Error>>;
    }
}

#[cfg(test)]
mock! {
    pub SsmClient {
        pub fn get_parameter(&self, name: String, with_decryption: bool) -> Result<String, Box<dyn std::error::Error>>;
    }
}

#[cfg(test)]
mock! {
    pub GetSecretValue<T: Into<String>> {
        pub fn secret_id(self, secret_id: T) -> Self;
        pub async fn send(self) -> Result<aws_sdk_secretsmanager::operation::get_secret_value::GetSecretValueOutput, aws_sdk_secretsmanager::error::SdkError<GetSecretValueError>>;
    }
}

#[cfg(test)]
mock! {
    pub GetParameter<T: Into<String>> {
        pub fn name(self, name: T) -> Self;
        pub fn with_decryption(self, decryption: bool) -> Self;
        pub async fn send(self) -> Result<aws_sdk_ssm::operation::get_parameter::GetParameterOutput, aws_sdk_ssm::error::SdkError<GetParameterError>>;
    }
}

#[async_trait]
pub trait SecretsManagerClientTrait {
    async fn get_secret_value(
        &self,
        secret_id: String,
    ) -> Result<String, Box<dyn std::error::Error>>;
}

#[async_trait]
pub trait SsmClientTrait {
    async fn get_parameter(
        &self,
        name: String,
        with_decryption: bool,
    ) -> Result<String, Box<dyn std::error::Error>>;
}

// Implement for the actual AWS clients
#[async_trait]
impl SecretsManagerClientTrait for SecretsManagerClient {
    async fn get_secret_value(
        &self,
        secret_id: String,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let resp = self.get_secret_value().secret_id(secret_id).send().await?;
        resp.secret_string()
            .ok_or_else(|| "Secret value is not a string".into())
            .map(|s| s.to_string())
    }
}

#[async_trait]
impl SsmClientTrait for SsmClient {
    async fn get_parameter(
        &self,
        name: String,
        with_decryption: bool,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let resp = self
            .get_parameter()
            .name(name)
            .with_decryption(with_decryption)
            .send()
            .await?;
        resp.parameter()
            .and_then(|p| p.value())
            .ok_or_else(|| "Parameter value is empty".into())
            .map(|s| s.to_string())
    }
}

#[async_trait::async_trait]
impl SecretsManagerClientTrait for Arc<SecretsManagerClient> {
    async fn get_secret_value(
        &self,
        secret_id: String,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let resp = self
            .as_ref()
            .get_secret_value()
            .secret_id(secret_id)
            .send()
            .await?;
        resp.secret_string()
            .ok_or_else(|| "Secret value is not a string".into())
            .map(|s| s.to_string())
    }
}

#[async_trait::async_trait]
impl SsmClientTrait for Arc<SsmClient> {
    async fn get_parameter(
        &self,
        name: String,
        with_decryption: bool,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let resp = self
            .as_ref()
            .get_parameter()
            .name(name)
            .with_decryption(with_decryption)
            .send()
            .await?;
        resp.parameter()
            .and_then(|p| p.value())
            .ok_or_else(|| "Parameter value is empty".into())
            .map(|s| s.to_string())
    }
}
#[cfg(test)]
#[async_trait::async_trait]
impl SecretsManagerClientTrait for MockSecretsManagerClient {
    async fn get_secret_value(
        &self,
        secret_id: String,
    ) -> Result<String, Box<dyn std::error::Error>> {
        self.get_secret_value(secret_id)
    }
}

#[cfg(test)]
#[async_trait::async_trait]
impl SsmClientTrait for MockSsmClient {
    async fn get_parameter(
        &self,
        name: String,
        with_decryption: bool,
    ) -> Result<String, Box<dyn std::error::Error>> {
        self.get_parameter(name, with_decryption)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // use mockall::predicate::*;

    #[test]
    fn test_parse_region_from_arn() {
        let arn = "arn:aws:secretsmanager:us-west-2:123456789012:secret:mydbpassword";
        assert_eq!(parse_region_from_arn(arn), Some("us-west-2".to_string()));

        let invalid_arn = "invalid:arn:format";
        assert_eq!(parse_region_from_arn(invalid_arn), None);
    }

    #[test]
    fn test_determine_secret_type() {
        let sm_arn = "arn:aws:secretsmanager:us-west-2:123456789012:secret:mydbpassword";
        let ssm_arn = "arn:aws:ssm:us-west-2:123456789012:parameter/myparameter";
        let name = "mysecret";

        match determine_secret_type(sm_arn) {
            SecretType::SecretsManagerArn(_) => {}
            _ => panic!("Expected SecretsManagerArn"),
        }

        match determine_secret_type(ssm_arn) {
            SecretType::SsmArn(_) => {}
            _ => panic!("Expected SsmArn"),
        }

        match determine_secret_type(name) {
            SecretType::Name(_) => {}
            _ => panic!("Expected Name"),
        }
    }

    #[tokio::test]
    async fn test_setup_aws_clients() {
        let (sm_client, ssm_client) = setup_aws_clients(Some("us-west-2")).await;
        // We can't access config directly, so let's just check if the clients are created
        assert!(Arc::strong_count(&sm_client) == 1);
        assert!(Arc::strong_count(&ssm_client) == 1);

        let (default_sm_client, default_ssm_client) = setup_aws_clients(None).await;
        assert!(Arc::strong_count(&default_sm_client) == 1);
        assert!(Arc::strong_count(&default_ssm_client) == 1);
    }

    #[test]
    fn test_collect_secrets() {
        // Clear any existing environment variables that might interfere with the test
        for (key, _) in env::vars() {
            if key.starts_with("SECRET_") {
                env::remove_var(&key);
            }
        }

        env::set_var(
            "SECRET_ARN_DB_PASSWORD",
            "arn:aws:secretsmanager:us-west-2:123456789012:secret:mydbpassword",
        );
        env::set_var("SECRET_NAME_API_KEY", "myapikey");
        env::set_var(
            "SECRET_MIXED_ARN",
            "arn:aws:ssm:us-east-1:123456789012:parameter/mixedarn",
        );
        env::set_var("SECRET_MIXED_NAME", "mixedname");
        env::set_var("SECRET_MIXED", "mixed_secret");
        env::set_var("NOT_A_SECRET", "this should be ignored");

        let secrets = collect_secrets();

        assert_eq!(secrets.len(), 5, "Expected 5 secrets, found: {:?}", secrets);
        assert!(secrets.iter().any(|(key, secret_type)| key == "DB_PASSWORD"
            && matches!(secret_type, SecretType::SecretsManagerArn(_))));
        assert!(secrets.iter().any(
            |(key, secret_type)| key == "API_KEY" && matches!(secret_type, SecretType::Name(_))
        ));
        assert!(secrets
            .iter()
            .any(|(key, secret_type)| key == "MIXED_ARN"
                && matches!(secret_type, SecretType::SsmArn(_))));
        assert!(secrets
            .iter()
            .any(|(key, secret_type)| key == "MIXED_NAME"
                && matches!(secret_type, SecretType::Name(_))));
        assert!(
            secrets
                .iter()
                .any(|(key, secret_type)| key == "MIXED"
                    && matches!(secret_type, SecretType::Name(_)))
        );

        // Clean up environment variables
        env::remove_var("SECRET_ARN_DB_PASSWORD");
        env::remove_var("SECRET_NAME_API_KEY");
        env::remove_var("SECRET_MIXED_ARN");
        env::remove_var("SECRET_MIXED_NAME");
        env::remove_var("SECRET_MIXED");
        env::remove_var("NOT_A_SECRET");
    }

    #[tokio::test]
    async fn test_get_secret_value_secretsmanager() {
        let mut mock_sm_client = MockSecretsManagerClient::new();
        let mock_ssm_client = MockSsmClient::new();

        let secret_id =
            "arn:aws:secretsmanager:us-west-2:123456789012:secret:mydbpassword".to_string();

        mock_sm_client
            .expect_get_secret_value()
            .with(eq(secret_id.clone()))
            .times(1)
            .returning(|_| Ok("mock_secret_value".to_string()));

        let secret_type = SecretType::SecretsManagerArn(secret_id);
        let result = get_secret_value(&mock_sm_client, &mock_ssm_client, &secret_type).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "mock_secret_value");
    }

    #[tokio::test]
    async fn test_get_secret_value_ssm() {
        let mock_sm_client = MockSecretsManagerClient::new();
        let mut mock_ssm_client = MockSsmClient::new();

        let parameter_name = "arn:aws:ssm:us-west-2:123456789012:parameter/myparameter".to_string();

        mock_ssm_client
            .expect_get_parameter()
            .with(eq(parameter_name.clone()), eq(true))
            .times(1)
            .returning(|_, _| Ok("mock_parameter_value".to_string()));

        let secret_type = SecretType::SsmArn(parameter_name);
        let result = get_secret_value(&mock_sm_client, &mock_ssm_client, &secret_type).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "mock_parameter_value");
    }

    #[test]
    fn test_collect_secrets_with_different_prefixes() {
        env::set_var(
            "SECRET_ARN_DB_PASSWORD",
            "arn:aws:secretsmanager:us-west-2:123456789012:secret:mydbpassword",
        );
        env::set_var("SECRET_NAME_API_KEY", "myapikey");
        env::set_var("SECRET_MIXED", "mixed_secret");

        let secrets = collect_secrets();

        assert_eq!(secrets.len(), 3);
        assert!(secrets.iter().any(|(key, secret_type)| key == "DB_PASSWORD"
            && matches!(secret_type, SecretType::SecretsManagerArn(_))));
        assert!(secrets.iter().any(
            |(key, secret_type)| key == "API_KEY" && matches!(secret_type, SecretType::Name(_))
        ));
        assert!(
            secrets
                .iter()
                .any(|(key, secret_type)| key == "MIXED"
                    && matches!(secret_type, SecretType::Name(_)))
        );

        // Clean up
        env::remove_var("SECRET_ARN_DB_PASSWORD");
        env::remove_var("SECRET_NAME_API_KEY");
        env::remove_var("SECRET_MIXED");
    }

    #[test]
    fn test_parse_region_from_arn_edge_cases() {
        assert_eq!(
            parse_region_from_arn(
                "arn:aws:secretsmanager:us-west-2:123456789012:secret:mydbpassword"
            ),
            Some("us-west-2".to_string())
        );
        assert_eq!(
            parse_region_from_arn("arn:aws:secretsmanager::123456789012:secret:mydbpassword"),
            Some("".to_string())
        );
        assert_eq!(parse_region_from_arn("arn:aws:secretsmanager"), None);
        assert_eq!(parse_region_from_arn("invalid:arn"), None);
    }

    #[test]
    fn test_run_program_error_handling() {
        let program = "non_existent_program";
        let args = vec![];
        let env_vars = vec![];

        let result = run_program(program, &args, env_vars);
        assert!(result.is_err());
    }

    #[test]
    fn test_run_program() {
        let program = "echo";
        let args = vec!["Hello, World!".to_string()];
        let env_vars = vec![("TEST_ENV".to_string(), "test_value".to_string())];

        let status = run_program(program, &args, env_vars).unwrap();
        assert!(status.success());
    }
    // #[test]
    // fn test_determine_secret_type_with_invalid_arn() {
    //     let invalid_arn = "arn:aws:invalid:us-west-2:123456789012:secret:mydbpassword";
    //     match determine_secret_type(invalid_arn) {
    //         Ok(_) => panic!("Expected an error for invalid ARN"),
    //         Err(e) => assert_eq!(e, "Invalid ARN: unsupported service"),
    //     }
    // }

    // #[tokio::test]
    // async fn test_fetch_secret_values_concurrent() {
    //     // Setup mock clients and test concurrent fetching of multiple secrets
    // }

    // #[test]
    // fn test_collect_secrets_with_invalid_env_vars() {
    //     env::set_var("SECRET_INVALID_PREFIX_TEST", "invalid_secret");
    //     let secrets = collect_secrets();
    //     assert!(!secrets.iter().any(|(key, _)| key == "INVALID_PREFIX_TEST"));
    //     env::remove_var("SECRET_INVALID_PREFIX_TEST");
    // }

    // #[tokio::test]
    // async fn test_end_to_end_workflow() {
    //     // Simulate the entire process from secret collection to program execution
    // }
}
