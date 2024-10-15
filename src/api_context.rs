use std::collections::HashMap;
use std::fmt::Display;
use std::fs;
use std::fs::{File, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{anyhow, Context};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::common::{BunqyyError, SetupContext, BUNQ_BASE_URL};
use crate::domains::oauth::get_access_token;
use crate::http::{
    get_unauthenticated_client, process_response_content, BunqResponse, WellKnownBunqHeaders,
};
use crate::signing::{create_signer, generate_keypair, Signer};

enum Endpoints {
    Installation,
    DeviceServer,
    SessionServer,
}

impl Display for Endpoints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            Endpoints::Installation => format!("{}/installation", BUNQ_BASE_URL),
            Endpoints::DeviceServer => format!("{}/device-server", BUNQ_BASE_URL),
            Endpoints::SessionServer => format!("{}/session-server", BUNQ_BASE_URL),
        };
        write!(f, "{}", str)
    }
}

/// A builder that makes it more ergonomic to create an api context while performing
/// the steps necessary.
///
/// ```
/// use bunqyy::api_context::ContextBuilder;
/// let mut builder = ContextBuilder::new();
/// //... get access token
/// builder.set_access_token(token);
/// ```
pub struct ContextBuilder {
    environment: Environment,
    api_key: Option<String>,
    installation_context: Option<InstallationContext>,
    device_id: Option<u64>,
    session_context: Option<SessionContext>,
}

impl ContextBuilder {
    /// Create a new instance for a given environment
    fn new_for_environment(environment: Environment) -> Self {
        ContextBuilder {
            environment,
            api_key: None,
            installation_context: None,
            device_id: None,
            session_context: None,
        }
    }

    fn set_access_token(&mut self, access_token: String) {
        self.api_key = Some(access_token.to_owned());
    }

    fn set_installation_context(&mut self, installation_context: InstallationContext) {
        self.installation_context = Some(installation_context);
    }

    fn set_device_id(&mut self, device_id: u64) {
        self.device_id = Some(device_id);
    }

    fn set_session_context(&mut self, session_context: SessionContext) {
        self.session_context = Some(session_context);
    }

    /// Build the collected data into an ApiContext instance
    fn build(self) -> anyhow::Result<ApiContext> {
        match (
            self.api_key,
            self.installation_context,
            self.session_context,
        ) {
            (Some(access_token), Some(installation_context), Some(session_context)) => {
                Ok(ApiContext {
                    api_key: access_token.to_string(),
                    environment: self.environment.clone(),
                    installation_context,
                    session_context,
                })
            }
            _ => Err(anyhow!(BunqyyError::MissingDataToBuildApiContext)),
        }
    }
}

/// Easily mark which environment is used
#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq)]
pub enum Environment {
    SANDBOX,
    PRODUCTION,
}

/// let environment = Environment::
impl Into<&'static str> for Environment {
    fn into(self) -> &'static str {
        match self {
            Environment::SANDBOX => "SANDBOX",
            Environment::PRODUCTION => "PRODUCTION",
        }
    }
}

impl FromStr for Environment {
    type Err = BunqyyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "SANDBOX" | "sandbox" | "" | "sb" => Ok(Environment::SANDBOX),
            "PRODUCTION" | "production" | "prod" | "PROD" => Ok(Environment::PRODUCTION),
            _ => Err(BunqyyError::InvalidEnvironment(s.to_string())),
        }
    }
}

pub type ManagedApiContext = Arc<Mutex<ApiContext>>;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ApiContext {
    pub api_key: String,
    pub environment: Environment,
    pub installation_context: InstallationContext,
    pub session_context: SessionContext,
}

impl ApiContext {
    /// Create a new instance of the api context with a session context
    /// Necessary when they expire
    /// ```
    /// use bunqyy::api_context::ApiContext;
    /// let mut api_context: ApiContext;
    /// // resolve a new session context
    /// api_context = api_context.with_session_context(new_session_context);
    /// ```
    pub fn with_session_context(self, session_context: SessionContext) -> Self {
        ApiContext {
            api_key: self.api_key,
            environment: self.environment,
            installation_context: self.installation_context,
            session_context,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct InstallationContext {
    pub token: String,
    pub private_key_client: String,
    pub public_key_client: String,
    pub public_key_server: String,
}

/// A session context is volatile; it expires over time and needs to be refreshed.
/// Its token is necessary for most requests
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionContext {
    pub token: String,
    pub valid_until: DateTime<Utc>,
    pub user_id: u64,
    pub user_api_key: SessionUserApiKey,
}

impl SessionContext {
    /// Check if this session context has expired
    /// has a buffer of 10 seconds to account for time between checking and using it
    pub fn needs_to_be_refreshed(&self) -> bool {
        self.valid_until.timestamp() < (Utc::now().timestamp() + 10)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionUserApiKey {
    pub id: u64,
    /// The user that manages the oauth application
    /// Likely to be you
    pub requested_by_user: UserInformation,
    /// The user that granted access to the application
    pub granted_by_user: UserInformation,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserInformation {
    pub id: u64,
    pub display_name: String,
    pub public_nick_name: String,
    pub session_timeout: u64,
}

/// Get the API context
/// If possible, fetch it from the setup context's storage location. If it is not available
/// attempt to set up a new context by communicating with bunqyy
pub async fn get_api_context(setup_context: &SetupContext) -> anyhow::Result<ApiContext> {
    let storage_path = setup_context.storage_path.as_str();

    let api_context: ApiContext;

    return if context_file_exists(storage_path) {
        debug!("context file exists, using that to recreate api context");
        let stored_config_json = fs::read_to_string(storage_path).unwrap();
        let api_context_from_storage =
            serde_json::from_str::<ApiContext>(stored_config_json.as_str()).unwrap();

        Ok(api_context_from_storage)
    } else {
        api_context = setup_api_context(setup_context).await?;
        persist_config(&api_context, setup_context.storage_path.as_str());

        Ok(api_context)
    };
}

pub fn persist_config(context: &ApiContext, path: &str) {
    // persist

    debug!("Persisting api context");

    fs::write(
        path,
        serde_json::to_string(&context).expect("Cannot serialize api context"),
    )
    .expect("Persisting failed");

    set_permissions(path);

    info!("Persisted api context to {}", path)
}

#[cfg(not(target_family = "unix"))]
fn set_permissions(path: &str) {
    let mut permissions = fs::metadata(path)
        .expect("Unable to get permissions for path")
        .permissions();

    let perms = Permissions::set_readonly(true);

    fs::set_permissions(path, permissions).expect("Failed to set permissions");
}

#[cfg(target_family = "unix")]
fn set_permissions(path: &str) {
    fs::set_permissions(path, Permissions::from_mode(0o400)).expect("Failed to set permissions");
}

pub async fn refresh_session(api_context: ManagedApiContext) -> anyhow::Result<()> {
    info!("Refreshing session");

    let local_api_context = api_context.lock().await.clone();

    let new_session = create_session(
        local_api_context.api_key.clone(),
        local_api_context.installation_context.token.clone(),
        create_signer(
            local_api_context
                .installation_context
                .private_key_client
                .clone(),
        ),
    )
    .await
    .with_context(|| "Failed to create a new session")?;

    *api_context.lock().await = local_api_context.with_session_context(new_session);

    Ok(())
}

pub async fn setup_api_context(setup_context: &SetupContext) -> anyhow::Result<ApiContext> {
    info!("Requesting access token");

    // Fetch an access token that will be used as the api_key – because we use oauth flow
    let api_key = get_access_token(setup_context).await?;
    let mut context_builder = ContextBuilder::new_for_environment(setup_context.environment);

    context_builder.set_access_token(api_key.clone());

    info!("Bunq gave us an access token ");
    info!("Now creating an installation context");

    let installation_context = get_installation_token().await?;

    context_builder.set_installation_context(installation_context.clone());

    info!("We\'ve got an installation context!");
    info!("Registering device server");

    let device_server_id = register_device(
        api_key.clone(),
        installation_context.token.clone(),
        create_signer(installation_context.private_key_client.clone()),
    )
    .await?;

    context_builder.set_device_id(device_server_id);

    info!("Also the device is registered for the installation context!");

    info!("Trying to create a session");

    // todo deserializer properly
    let session_context = create_session(
        api_key,
        installation_context.token,
        create_signer(installation_context.private_key_client.clone()),
    )
    .await?;

    context_builder.set_session_context(session_context);

    info!("Session created – all set!");

    context_builder.build()
}

/// Check if there is an earlier context file
fn context_file_exists(path: &str) -> bool {
    File::open(path).is_ok()
}

/// Register the server this application is running with bunqyy
/// they will provide a unique id for it
async fn register_device<'a>(
    api_key: String,
    session_token: String,
    signer: Signer,
) -> anyhow::Result<u64> {
    let client = get_unauthenticated_client()?;

    #[derive(Serialize, Debug)]
    struct Payload {
        description: String,
        secret: String,
        permitted_ips: Vec<String>,
    }

    // Bind the api key to the current external IP of the server
    // (implicitly let bunqyy decide, instead of forcing a value)
    let data = Payload {
        description: String::from("bunqyy"),
        secret: api_key,
        permitted_ips: Vec::new(),
    };

    let body_data = serde_json::to_string(&data).unwrap();

    let body_signature = signer(body_data.as_bytes());

    let response = client
        .post(Endpoints::DeviceServer.to_string())
        .header(
            WellKnownBunqHeaders::Authentication.to_string(),
            session_token,
        )
        .header(WellKnownBunqHeaders::Signature.to_string(), body_signature)
        .body(body_data.clone())
        .send()
        .await?
        .text()
        .await?;

    #[derive(Deserialize, Debug)]
    struct Id {
        id: u64,
    }

    #[derive(Deserialize, Debug)]
    struct Content {
        #[serde(rename = "Id")]
        id: Id,
    }

    let response = process_response_content::<Content>(response.as_str())?;

    let content = match response {
        BunqResponse::Success(data) => data.response,
        BunqResponse::Error(errors) => return Err(anyhow::anyhow!("Error: {:?}", errors.error)),
    };

    content
        .iter()
        .find_map(|content| Some(content.id.id))
        .ok_or(anyhow!("Id not found in response"))
}

/// Create a session in bunqyy. This session allows us to make authenticated
/// api calls; in other words, this is the final step before using their api fully.
async fn create_session(
    api_key: String,
    installation_token: String,
    signer: Signer,
) -> anyhow::Result<SessionContext> {
    let client = get_unauthenticated_client()?;

    #[derive(Serialize, Debug)]
    struct Payload {
        secret: String,
    }

    let data = Payload { secret: api_key };

    let body_data =
        serde_json::to_string(&data).with_context(|| "failed to serialize payload for session")?;

    let body_signature = signer(body_data.as_bytes());

    let response = client
        .post(Endpoints::SessionServer.to_string())
        .header(
            WellKnownBunqHeaders::Authentication.to_string(),
            installation_token,
        )
        .header(WellKnownBunqHeaders::Signature.to_string(), body_signature)
        .body(body_data.clone())
        .send()
        .await?
        .text()
        .await?;

    #[derive(Deserialize, Debug, Copy, Clone)]
    #[allow(dead_code)]
    struct Id {
        id: u64,
    }

    #[derive(Deserialize, Debug, Clone)]
    #[allow(dead_code)]
    struct Token {
        id: u64,
        created: String,
        updated: String,
        token: String,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RequestedByUser {
        #[serde(rename = "UserPerson")]
        pub user_person: UserPerson,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct GrantedByUser {
        #[serde(rename = "UserPerson")]
        pub user_person: UserPerson,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UserApiKey {
        pub id: u64,
        #[serde(rename = "requested_by_user")]
        pub requested_by_user: RequestedByUser,
        #[serde(rename = "granted_by_user")]
        pub granted_by_user: GrantedByUser,
    }

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UserPerson {
        pub id: u64,
        #[serde(rename = "display_name")]
        pub display_name: String,
        #[serde(rename = "public_nick_name")]
        pub public_nick_name: String,
        #[serde(rename = "session_timeout")]
        pub session_timeout: u64,
    }

    #[derive(Deserialize, Clone, Debug)]
    #[allow(dead_code)]
    enum Content {
        Token(Token),
        UserApiKey(UserApiKey),
        Id(Id),
    }

    impl Content {
        fn get_token(&self) -> Option<Token> {
            match self {
                Content::Token(token) => Some(token.clone()),
                _ => None,
            }
        }

        fn get_user_api_key(&self) -> Option<UserApiKey> {
            match self {
                Content::UserApiKey(user_api_key) => Some(user_api_key.clone()),
                _ => None,
            }
        }
    }

    let response = process_response_content::<Content>(response.as_ref())
        .with_context(|| "failed to process response content of session creation")?;

    let content = match response {
        BunqResponse::Success(data) => data.response,
        BunqResponse::Error(errors) => return Err(anyhow!("Error: {:?}", errors.error)),
    };

    let token = content.iter().find_map(|content| content.get_token());
    let user_api_key = content
        .iter()
        .find_map(|content| content.get_user_api_key());

    let (token, user_api_key) = match (token, user_api_key) {
        (Some(token), Some(user_api_key)) => (token, user_api_key),
        _ => return Err(anyhow!("Token or UserApiKey not found in response")),
    };

    Ok(SessionContext {
        token: token.token,
        valid_until: Utc::now()
            + ChronoDuration::seconds(
                user_api_key.requested_by_user.user_person.session_timeout as i64,
            ),
        user_id: user_api_key.id,
        user_api_key: SessionUserApiKey {
            id: user_api_key.id,
            requested_by_user: UserInformation {
                id: user_api_key.requested_by_user.user_person.id,
                display_name: user_api_key.requested_by_user.user_person.display_name,
                public_nick_name: user_api_key.requested_by_user.user_person.public_nick_name,
                session_timeout: user_api_key.requested_by_user.user_person.session_timeout,
            },
            granted_by_user: UserInformation {
                id: user_api_key.requested_by_user.user_person.id,
                display_name: user_api_key.granted_by_user.user_person.display_name,
                public_nick_name: user_api_key.granted_by_user.user_person.public_nick_name,
                session_timeout: user_api_key.granted_by_user.user_person.session_timeout,
            },
        },
    })
}

/// Get an installation context from bunqyy
pub async fn get_installation_token() -> anyhow::Result<InstallationContext> {
    log::info!("Attempting to register installation token");
    #[derive(Debug, Clone, PartialEq, Deserialize)]
    enum Content {
        Token(Token),
        ServerPublicKey(ServerPublicKey),
        #[serde(untagged)]
        Unknown(Value),
    }

    #[derive(Default, Debug, Clone, PartialEq, Deserialize)]
    pub struct Token {
        pub token: String,
    }

    #[derive(Default, Debug, Clone, PartialEq, Deserialize)]
    pub struct ServerPublicKey {
        pub server_public_key: String,
    }

    let client = get_unauthenticated_client()?;

    log::info!("generating new keys for installation token");
    let keypair = generate_keypair();

    let public_key_pem = String::from_utf8(keypair.public_key_to_pem()?)?;

    let data = HashMap::from([("client_public_key", public_key_pem.to_string())]);

    let response = client
        .post(Endpoints::Installation.to_string())
        .json(&data)
        .send()
        .await?
        .text()
        .await?;

    let response = process_response_content::<Content>(response.as_str())?;

    let content = match response {
        BunqResponse::Success(data) => data.response,
        BunqResponse::Error(errors) => return Err(anyhow::anyhow!("Error: {:?}", errors.error)),
    };

    let token = content.iter().find_map(|content| match content {
        Content::Token(token) => Some(token.clone()),
        _ => None,
    });

    let server_public_key = content.iter().find_map(|content| match content {
        Content::ServerPublicKey(server_public_key) => Some(server_public_key.clone()),
        _ => None,
    });

    let (token, server_public_key) = match (token, server_public_key) {
        (Some(token), Some(server_public_key)) => (token, server_public_key),
        _ => {
            return Err(anyhow::anyhow!(
                "Token or ServerPublicKey not found in response"
            ))
        }
    };

    Ok(InstallationContext {
        token: token.token,
        private_key_client: String::from_utf8(keypair.private_key_to_pem_pkcs8().unwrap()).unwrap(),
        public_key_client: public_key_pem,
        public_key_server: server_public_key.server_public_key.clone(),
    })
}
