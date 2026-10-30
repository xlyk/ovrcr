//! Instance-owned subscription OAuth. No personal profiles or API-key fallback.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use ovrcr_protocol::operator::OperatorProvider;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::watch,
    time::Instant,
};
const XAI_CLIENT: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const MAX_HTTP_BODY: usize = 1024 * 1024;
type Result<T> = std::result::Result<T, AuthError>;
/// Errors exclude remote bodies, URLs, credentials and OS paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    Storage,
    Transport,
    Protocol,
    Identity,
    Permission,
    NotSignedIn,
    Denied,
    Expired,
    Cancelled,
}
impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Storage => "operator credential storage failed",
            Self::Transport => "subscription authentication request failed",
            Self::Protocol => "invalid subscription authentication response",
            Self::Identity => "subscription identity validation failed",
            Self::Permission => "ChatGPT plan usage permission is not enabled",
            Self::NotSignedIn => "sign in to the operator provider first",
            Self::Denied => "subscription sign-in was denied",
            Self::Expired => "subscription sign-in expired",
            Self::Cancelled => "subscription sign-in cancelled",
        })
    }
}
impl std::error::Error for AuthError {}
#[derive(Clone)]
pub struct AuthCancel(watch::Sender<bool>);
impl Default for AuthCancel {
    fn default() -> Self {
        Self(watch::channel(false).0)
    }
}
impl AuthCancel {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
    async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        loop {
            if *receiver.borrow_and_update() {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
    async fn run<T>(&self, future: impl std::future::Future<Output = Result<T>>) -> Result<T> {
        tokio::select! {biased;_=self.cancelled()=>Err(AuthError::Cancelled),result=future=>result}
    }
}
/// Owned snapshot: no credential lock survives resolution into inference.
pub struct AccessToken(String);
impl AccessToken {
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessToken([REDACTED])")
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthStatus {
    pub signed_in: bool,
    pub needs_refresh: bool,
    pub plan_usage: bool,
    pub subject: Option<String>,
    pub client_id: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelChoice {
    pub id: String,
    pub display_name: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogoutResult {
    pub remote_revocation_confirmed: bool,
}
/// Injectable for controlled fixtures. Defaults are fixed production endpoints.
#[derive(Clone)]
pub struct AuthEndpoints {
    pub grok_device: String,
    pub grok_token: String,
    pub chat_authorize: String,
    pub chat_token: String,
    pub chat_jwks: String,
    pub chat_revoke: String,
    pub chat_models: String,
}
impl Default for AuthEndpoints {
    fn default() -> Self {
        Self {
            grok_device: "https://auth.x.ai/oauth2/device/code".into(),
            grok_token: "https://auth.x.ai/oauth2/token".into(),
            chat_authorize: "https://auth.openai.com/api/accounts/authorize".into(),
            chat_token: "https://auth.openai.com/api/accounts/oauth/token".into(),
            chat_jwks: "https://auth.openai.com/.well-known/jwks.json".into(),
            chat_revoke: "https://auth.openai.com/api/accounts/oauth/revoke".into(),
            chat_models: "https://api.openai.com/v1/models".into(),
        }
    }
}
#[derive(Clone)]
pub struct AuthManager {
    dir: PathBuf,
    profile: String,
    client: Client,
    endpoints: AuthEndpoints,
}
impl fmt::Debug for AuthManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AuthManager([PRIVATE])")
    }
}
#[derive(Serialize, Deserialize, Default)]
struct Record {
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    issuer: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    id_token: String,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    expires_at: u64,
}
impl Record {
    fn plan_usage(&self) -> bool {
        self.scopes.iter().any(|s| s == "chatgpt.tokens.use.direct")
    }
}
pub struct DeviceLogin {
    pub user_code: String,
    pub verification_uri: String,
    device_code: String,
    interval: Duration,
    deadline: Instant,
}
impl fmt::Debug for DeviceLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceLogin([REDACTED])")
    }
}
pub struct BrowserLogin {
    authorization_url: Url,
    listener: TcpListener,
    redirect_uri: String,
    state: String,
    nonce: String,
    verifier: String,
    previous: Record,
}
impl BrowserLogin {
    pub fn authorization_url(&self) -> &str {
        self.authorization_url.as_str()
    }
}
impl fmt::Debug for BrowserLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BrowserLogin([REDACTED])")
    }
}
impl AuthManager {
    pub fn new(instance_dir: &Path) -> Result<Self> {
        Self::profile(instance_dir, "default")
    }
    /// Separate named registrations; profile names are hashed before constructing paths.
    pub fn profile(instance_dir: &Path, profile: &str) -> Result<Self> {
        let dir = instance_dir.join("operator-auth");
        match fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err(AuthError::Storage),
        }
        check_private_dir(&dir)?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| AuthError::Transport)?;
        Ok(Self {
            dir,
            profile: format!("{:x}", Sha256::digest(profile.as_bytes())),
            client,
            endpoints: AuthEndpoints::default(),
        })
    }
    pub fn with_endpoints(mut self, endpoints: AuthEndpoints) -> Self {
        self.endpoints = endpoints;
        self
    }
    fn path(&self, provider: OperatorProvider) -> PathBuf {
        self.dir
            .join(format!("{}-{}.json", provider_name(provider), self.profile))
    }
    async fn lock(&self, provider: OperatorProvider, cancel: &AuthCancel) -> Result<File> {
        self.lock_path(&self.path(provider).with_extension("lock"), cancel)
            .await
    }
    async fn lock_path(&self, path: &Path, cancel: &AuthCancel) -> Result<File> {
        loop {
            check_private_dir(&self.dir)?;
            let file = private_open(path, true)?;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(file);
            }
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::WouldBlock {
                return Err(AuthError::Storage);
            }
            cancel
                .run(async {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    Ok(())
                })
                .await?;
        }
    }
    fn read(&self, provider: OperatorProvider) -> Result<Record> {
        check_private_dir(&self.dir)?;
        let path = self.path(provider);
        if !exists(&path)? {
            return Ok(Record::default());
        }
        let file = private_open(&path, false)?;
        serde_json::from_reader(file.take(MAX_HTTP_BODY as u64)).map_err(|_| AuthError::Storage)
    }
    fn save(&self, provider: OperatorProvider, record: &Record) -> Result<()> {
        atomic_json(&self.dir, &self.path(provider), record)
    }
    pub async fn status(&self, provider: OperatorProvider) -> Result<AuthStatus> {
        let r = self.read(provider)?;
        Ok(AuthStatus {
            signed_in: !r.access_token.is_empty(),
            needs_refresh: !r.access_token.is_empty() && r.expires_at <= now().saturating_add(300),
            plan_usage: if provider == OperatorProvider::Grok {
                !r.access_token.is_empty()
            } else {
                r.plan_usage()
            },
            subject: nonempty(r.subject),
            client_id: nonempty(r.client_id),
        })
    }
    pub async fn resolve(
        &self,
        provider: OperatorProvider,
        cancel: &AuthCancel,
    ) -> Result<AccessToken> {
        let _lock = self.lock(provider, cancel).await?;
        let old = self.read(provider)?;
        if old.access_token.is_empty() {
            return Err(AuthError::NotSignedIn);
        }
        if provider == OperatorProvider::ChatGpt && !old.plan_usage() {
            return Err(AuthError::Permission);
        }
        if old.expires_at > now().saturating_add(300) {
            return Ok(AccessToken(old.access_token));
        }
        if old.refresh_token.is_empty() {
            return Err(AuthError::NotSignedIn);
        }
        let mut fields = vec![
            ("grant_type", "refresh_token"),
            ("client_id", old.client_id.as_str()),
            ("refresh_token", old.refresh_token.as_str()),
        ];
        let url = if provider == OperatorProvider::Grok {
            &self.endpoints.grok_token
        } else {
            fields.push(("resource", RESOURCE));
            &self.endpoints.chat_token
        };
        let body = self.post(url, &fields, cancel).await?;
        let mut refreshed = token_record(&body, Some(&old), provider)?;
        if provider == OperatorProvider::ChatGpt
            && !refreshed.id_token.is_empty()
            && refreshed.id_token != old.id_token
        {
            let subject = self
                .validate_id(&refreshed.id_token, &old.client_id, None, cancel)
                .await?;
            if subject != old.subject {
                return Err(AuthError::Identity);
            }
        }
        refreshed.subject = old.subject;
        refreshed.issuer = old.issuer;
        refreshed.client_id = old.client_id;
        self.save(provider, &refreshed)?;
        if provider == OperatorProvider::ChatGpt && !refreshed.plan_usage() {
            return Err(AuthError::Permission);
        }
        Ok(AccessToken(refreshed.access_token))
    }
    pub async fn begin_grok(&self, cancel: &AuthCancel) -> Result<DeviceLogin> {
        let body = self
            .post(
                &self.endpoints.grok_device,
                &[
                    ("client_id", XAI_CLIENT),
                    (
                        "scope",
                        "openid profile email offline_access grok-cli:access api:access",
                    ),
                    ("referrer", "ovrcr"),
                ],
                cancel,
            )
            .await?;
        let uri = body
            .get("verification_uri_complete")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or(required(&body, "verification_uri")?);
        let url = Url::parse(uri).map_err(|_| AuthError::Protocol)?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(AuthError::Protocol);
        }
        let interval = body
            .get("interval")
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
            .unwrap_or(5);
        let expires = positive(&body, "expires_in")?;
        Ok(DeviceLogin {
            user_code: required(&body, "user_code")?.into(),
            verification_uri: url.into(),
            device_code: required(&body, "device_code")?.into(),
            interval: Duration::from_secs(interval.clamp(1, 3600)),
            deadline: Instant::now() + Duration::from_secs(expires.min(86400)),
        })
    }
    pub async fn finish_grok(&self, mut login: DeviceLogin, cancel: &AuthCancel) -> Result<()> {
        loop {
            let remaining = login.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(AuthError::Expired);
            }
            cancel
                .run(async {
                    tokio::time::sleep(login.interval.min(remaining)).await;
                    Ok(())
                })
                .await?;
            if Instant::now() >= login.deadline {
                return Err(AuthError::Expired);
            }
            let (success, body) = tokio::time::timeout_at(
                login.deadline,
                self.form(
                    &self.endpoints.grok_token,
                    &[
                        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                        ("client_id", XAI_CLIENT),
                        ("device_code", &login.device_code),
                    ],
                    cancel,
                ),
            )
            .await
            .map_err(|_| AuthError::Expired)??;
            if success {
                let mut r = token_record(&body, None, OperatorProvider::Grok)?;
                r.client_id = XAI_CLIENT.into();
                let _lock = self.lock(OperatorProvider::Grok, cancel).await?;
                return self.save(OperatorProvider::Grok, &r);
            }
            match body.get("error").and_then(Value::as_str) {
                Some("authorization_pending") => (),
                Some("slow_down") => {
                    login.interval = body
                        .get("interval")
                        .and_then(Value::as_u64)
                        .filter(|n| *n > 0)
                        .map(Duration::from_secs)
                        .unwrap_or(login.interval.saturating_add(Duration::from_secs(5)))
                        .clamp(Duration::from_secs(1), Duration::from_secs(3600));
                }
                Some("access_denied" | "authorization_denied") => return Err(AuthError::Denied),
                Some("expired_token") => return Err(AuthError::Expired),
                _ => return Err(AuthError::Protocol),
            }
        }
    }
    pub async fn begin_chatgpt(&self) -> Result<BrowserLogin> {
        let cancel = AuthCancel::default();
        let _lock = self.lock(OperatorProvider::ChatGpt, &cancel).await?;
        let previous = self.read(OperatorProvider::ChatGpt)?;
        let _host_lock = self.lock_path(&self.dir.join("host.lock"), &cancel).await?;
        let host_path = self.dir.join("host.json");
        let host: String = match private_open(&host_path, false) {
            Ok(file) => serde_json::from_reader(file).map_err(|_| AuthError::Storage)?,
            Err(_) if !exists(&host_path)? => {
                let host = host_id()?;
                atomic_json(&self.dir, &host_path, &host)?;
                host
            }
            Err(e) => return Err(e),
        };
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| AuthError::Transport)?;
        let redirect_uri = format!(
            "http://127.0.0.1:{}/auth/callback",
            listener
                .local_addr()
                .map_err(|_| AuthError::Transport)?
                .port()
        );
        let state = random()?;
        let nonce = random()?;
        let verifier = random()?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url =
            Url::parse(&self.endpoints.chat_authorize).map_err(|_| AuthError::Protocol)?;
        let client_id = if previous.client_id.is_empty() {
            "dynamic_agent_client"
        } else {
            &previous.client_id
        };
        {
            let mut query = url.query_pairs_mut();
            query.extend_pairs([
                ("client_id", client_id),
                ("ext_agent_host_id", host.as_str()),
                ("response_type", "code"),
                ("redirect_uri", redirect_uri.as_str()),
                ("scope", SCOPE),
                ("resource", RESOURCE),
                ("state", state.as_str()),
                ("nonce", nonce.as_str()),
                ("code_challenge_method", "S256"),
                ("code_challenge", challenge.as_str()),
            ]);
            if previous.client_id.is_empty() {
                query.append_pair("agent_name_hint", "OVRCR");
            } else if !previous.id_token.is_empty() {
                query.append_pair("id_token_hint", &previous.id_token);
            }
        }
        Ok(BrowserLogin {
            authorization_url: url,
            listener,
            redirect_uri,
            state,
            nonce,
            verifier,
            previous,
        })
    }
    /// Caller opens authorization_url() in the system browser before awaiting this.
    pub async fn finish_chatgpt(&self, login: BrowserLogin, cancel: &AuthCancel) -> Result<()> {
        cancel
            .run(async {
                tokio::time::timeout(Duration::from_secs(600), self.chat_callback(login, cancel))
                    .await
                    .map_err(|_| AuthError::Expired)?
            })
            .await
    }
    async fn chat_callback(&self, login: BrowserLogin, cancel: &AuthCancel) -> Result<()> {
        let (mut socket, _) = login
            .listener
            .accept()
            .await
            .map_err(|_| AuthError::Transport)?;
        let mut bytes = Vec::new();
        loop {
            let mut buf = [0; 1024];
            let n = socket
                .read(&mut buf)
                .await
                .map_err(|_| AuthError::Transport)?;
            if n == 0 {
                return Err(AuthError::Protocol);
            }
            bytes.extend_from_slice(&buf[..n]);
            if bytes.len() > 16384 {
                return Err(AuthError::Protocol);
            }
            if bytes.windows(4).any(|b| b == b"\r\n\r\n") {
                break;
            }
        }
        let request = std::str::from_utf8(&bytes).map_err(|_| AuthError::Protocol)?;
        let mut line = request
            .lines()
            .next()
            .ok_or(AuthError::Protocol)?
            .split_whitespace();
        if line.next() != Some("GET") {
            return Err(AuthError::Protocol);
        }
        let target = line.next().ok_or(AuthError::Protocol)?;
        if !target.starts_with("/auth/callback?") {
            return Err(AuthError::Protocol);
        }
        let url =
            Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| AuthError::Protocol)?;
        let mut params = std::collections::HashMap::new();
        for (key, value) in url.query_pairs() {
            if params
                .insert(key.into_owned(), value.into_owned())
                .is_some()
            {
                return Err(AuthError::Protocol);
            }
        }
        if params.get("state") != Some(&login.state) {
            return Err(AuthError::Identity);
        }
        if params.contains_key("error") {
            return Err(AuthError::Denied);
        }
        let code = params
            .get("code")
            .filter(|s| !s.is_empty())
            .ok_or(AuthError::Protocol)?;
        let client_id = params.get("client_id").unwrap_or(&login.previous.client_id);
        if client_id.is_empty()
            || client_id == "dynamic_agent_client"
            || (!login.previous.client_id.is_empty() && *client_id != login.previous.client_id)
        {
            return Err(AuthError::Identity);
        }
        let _lock = self.lock(OperatorProvider::ChatGpt, cancel).await?;
        let current = self.read(OperatorProvider::ChatGpt)?;
        if current.client_id != login.previous.client_id {
            return Err(AuthError::Identity);
        }
        if login.previous.client_id.is_empty() {
            // Preserve issued registration even if code exchange requires another attempt.
            self.save(
                OperatorProvider::ChatGpt,
                &Record {
                    client_id: client_id.clone(),
                    ..Default::default()
                },
            )?;
        }
        let body = self
            .post(
                &self.endpoints.chat_token,
                &[
                    ("grant_type", "authorization_code"),
                    ("client_id", client_id),
                    ("code", code),
                    ("code_verifier", &login.verifier),
                    ("redirect_uri", &login.redirect_uri),
                    ("resource", RESOURCE),
                ],
                cancel,
            )
            .await?;
        let mut r = token_record(&body, None, OperatorProvider::ChatGpt)?;
        let subject = self
            .validate_id(&r.id_token, client_id, Some(&login.nonce), cancel)
            .await?;
        if !login.previous.subject.is_empty() && subject != login.previous.subject {
            return Err(AuthError::Identity);
        }
        r.client_id = client_id.clone();
        r.subject = subject;
        r.issuer = "https://auth.openai.com".into();
        self.save(OperatorProvider::ChatGpt, &r)?;
        let body = "OVRCR connected. You may close this.";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        socket
            .write_all(response.as_bytes())
            .await
            .map_err(|_| AuthError::Transport)?;
        Ok(())
    }
    async fn validate_id(
        &self,
        token: &str,
        client_id: &str,
        nonce: Option<&str>,
        cancel: &AuthCancel,
    ) -> Result<String> {
        let header = jsonwebtoken::decode_header(token).map_err(|_| AuthError::Identity)?;
        if header.alg != Algorithm::RS256 {
            return Err(AuthError::Identity);
        }
        let jwks: JwkSet =
            serde_json::from_value(self.get(&self.endpoints.chat_jwks, None, cancel).await?)
                .map_err(|_| AuthError::Identity)?;
        let jwk = jwks
            .find(header.kid.as_deref().ok_or(AuthError::Identity)?)
            .ok_or(AuthError::Identity)?;
        let key = DecodingKey::from_jwk(jwk).map_err(|_| AuthError::Identity)?;
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&["https://auth.openai.com"]);
        validation.set_audience(&[client_id]);
        validation.leeway = 0;
        #[derive(Clone, Deserialize)]
        struct Claims {
            sub: String,
            nonce: Option<String>,
        }
        let claims = jsonwebtoken::decode::<Claims>(token, &key, &validation)
            .map_err(|_| AuthError::Identity)?
            .claims;
        if claims.sub.is_empty() || nonce.is_some_and(|n| claims.nonce.as_deref() != Some(n)) {
            return Err(AuthError::Identity);
        }
        Ok(claims.sub)
    }
    pub async fn models(
        &self,
        provider: OperatorProvider,
        cancel: &AuthCancel,
    ) -> Result<Vec<ModelChoice>> {
        let token = self.resolve(provider, cancel).await?;
        if provider == OperatorProvider::Grok {
            return Ok([
                ("grok-4.7", "Grok 4.7"),
                ("grok-4.3", "Grok 4.3"),
                ("grok-4.20-0309-reasoning", "Grok 4.20 Reasoning"),
            ]
            .into_iter()
            .map(|(id, name)| ModelChoice {
                id: id.into(),
                display_name: name.into(),
            })
            .collect());
        }
        let body = self
            .get(&self.endpoints.chat_models, Some(token.expose()), cancel)
            .await?;
        let models = body
            .get("models")
            .and_then(Value::as_array)
            .ok_or(AuthError::Protocol)?;
        models
            .iter()
            .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
            .map(|m| {
                Ok(ModelChoice {
                    id: required(m, "slug")?.into(),
                    display_name: required(m, "display_name")?.into(),
                })
            })
            .collect()
    }
    pub async fn logout(
        &self,
        provider: OperatorProvider,
        cancel: &AuthCancel,
    ) -> Result<LogoutResult> {
        let _lock = self.lock(provider, cancel).await?;
        let mut r = self.read(provider)?;
        let confirmed = if provider == OperatorProvider::ChatGpt && !r.refresh_token.is_empty() {
            let request = self.client.post(&self.endpoints.chat_revoke).form(&[
                ("token", r.refresh_token.as_str()),
                ("token_type_hint", "refresh_token"),
                ("client_id", r.client_id.as_str()),
            ]);
            matches!(cancel.run(async{request.send().await.map_err(|_|AuthError::Transport)}).await,Ok(response)if response.status().as_u16()==200)
        } else {
            false
        };
        r.access_token.clear();
        r.refresh_token.clear();
        r.id_token.clear();
        r.scopes.clear();
        r.expires_at = 0;
        self.save(provider, &r)?;
        Ok(LogoutResult {
            remote_revocation_confirmed: confirmed,
        })
    }
    async fn post(&self, url: &str, fields: &[(&str, &str)], cancel: &AuthCancel) -> Result<Value> {
        let (success, body) = self.form(url, fields, cancel).await?;
        if !success {
            return Err(match body.get("error").and_then(Value::as_str) {
                Some("access_denied" | "authorization_denied") => AuthError::Denied,
                Some("expired_token") => AuthError::Expired,
                _ => AuthError::Protocol,
            });
        }
        Ok(body)
    }
    async fn form(
        &self,
        url: &str,
        fields: &[(&str, &str)],
        cancel: &AuthCancel,
    ) -> Result<(bool, Value)> {
        cancel
            .run(async {
                let response = self
                    .client
                    .post(url)
                    .header("Accept", "application/json")
                    .form(fields)
                    .send()
                    .await
                    .map_err(|_| AuthError::Transport)?;
                let success = response.status().is_success();
                Ok((success, read_body(response).await?))
            })
            .await
    }
    async fn get(&self, url: &str, token: Option<&str>, cancel: &AuthCancel) -> Result<Value> {
        cancel
            .run(async {
                let mut request = self.client.get(url);
                if let Some(token) = token {
                    request = request.bearer_auth(token);
                }
                let response = request.send().await.map_err(|_| AuthError::Transport)?;
                if !response.status().is_success() {
                    return Err(AuthError::Transport);
                }
                read_body(response).await
            })
            .await
    }
}
async fn read_body(mut response: reqwest::Response) -> Result<Value> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| AuthError::Transport)? {
        if bytes.len() + chunk.len() > MAX_HTTP_BODY {
            return Err(AuthError::Protocol);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| AuthError::Protocol)
}
fn token_record(body: &Value, old: Option<&Record>, provider: OperatorProvider) -> Result<Record> {
    if body
        .get("token_type")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.eq_ignore_ascii_case("bearer"))
    {
        return Err(AuthError::Protocol);
    }
    let access_token = required(body, "access_token")?.into();
    let refresh_token = match body.get("refresh_token") {
        None if old.is_some() => old.unwrap().refresh_token.clone(),
        _ => required(body, "refresh_token")?.into(),
    };
    let lifetime = if body.get("expires_in").is_none() && provider == OperatorProvider::Grok {
        3600
    } else {
        positive(body, "expires_in")?
    };
    let scopes = match body.get("scope") {
        Some(Value::String(scope)) => scope.split_whitespace().map(str::to_owned).collect(),
        None => old.map(|r| r.scopes.clone()).unwrap_or_default(),
        _ => return Err(AuthError::Protocol),
    };
    let id_token = match body.get("id_token") {
        Some(Value::String(token)) => token.clone(),
        None => old.map(|r| r.id_token.clone()).unwrap_or_default(),
        _ => return Err(AuthError::Protocol),
    };
    if provider == OperatorProvider::ChatGpt && old.is_none() && id_token.is_empty() {
        return Err(AuthError::Identity);
    }
    Ok(Record {
        access_token,
        refresh_token,
        id_token,
        scopes,
        expires_at: now().checked_add(lifetime).ok_or(AuthError::Protocol)?,
        ..Default::default()
    })
}
fn required<'a>(body: &'a Value, key: &str) -> Result<&'a str> {
    body.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(AuthError::Protocol)
}
fn positive(body: &Value, key: &str) -> Result<u64> {
    body.get(key)
        .and_then(Value::as_u64)
        .filter(|n| *n > 0)
        .ok_or(AuthError::Protocol)
}
fn nonempty(s: String) -> Option<String> {
    if s.is_empty() { None } else { Some(s) }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn random() -> Result<String> {
    let mut bytes = [0; 32];
    File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|_| AuthError::Storage)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
fn provider_name(provider: OperatorProvider) -> &'static str {
    match provider {
        OperatorProvider::ChatGpt => "chatgpt",
        OperatorProvider::Grok => "grok",
    }
}
fn check_private_dir(dir: &Path) -> Result<()> {
    let m = fs::symlink_metadata(dir).map_err(|_| AuthError::Storage)?;
    if !m.is_dir() || m.mode() & 0o077 != 0 || m.uid() != unsafe { libc::geteuid() } {
        return Err(AuthError::Storage);
    }
    Ok(())
}
fn private_open(path: &Path, create: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(create)
        .create(create)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| AuthError::Storage)?;
    let m = file.metadata().map_err(|_| AuthError::Storage)?;
    if !m.is_file()
        || m.mode() & 0o077 != 0
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
    {
        return Err(AuthError::Storage);
    }
    Ok(file)
}
fn atomic_json(dir: &Path, path: &Path, value: &impl Serialize) -> Result<()> {
    check_private_dir(dir)?;
    if exists(path)? {
        private_open(path, false)?;
    }
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|_| AuthError::Storage)?;
    serde_json::to_writer(&mut temp, value).map_err(|_| AuthError::Storage)?;
    temp.flush()
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|_| AuthError::Storage)?;
    temp.persist(path).map_err(|_| AuthError::Storage)?;
    File::open(dir)
        .and_then(|f| f.sync_all())
        .map_err(|_| AuthError::Storage)
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(AuthError::Storage),
    }
}
fn host_id() -> Result<String> {
    let mut bytes = [0u8; 16];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|_| AuthError::Storage)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}
