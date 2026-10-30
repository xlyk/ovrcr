use base64::Engine;
use ovrcr::operator::auth::{AuthCancel, AuthEndpoints, AuthError, AuthManager};
use ovrcr_protocol::operator::OperatorProvider::{ChatGpt, Grok};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}
type Handler = Box<dyn FnOnce(String) -> (u16, Value) + Send>;
fn serve(handlers: Vec<Handler>) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        for handler in handlers {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let n = socket.read(&mut buf).unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    let len = headers
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length: ")
                                .and_then(|n| n.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let (status, body) = handler(String::from_utf8(bytes).unwrap());
            let body = if body.is_null() {
                String::new()
            } else {
                body.to_string()
            };
            write!(socket,"HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (base, handle)
}
fn response(status: u16, body: Value) -> Handler {
    Box::new(move |_| (status, body))
}
fn device() -> Value {
    json!({"device_code":"private-device","user_code":"ABCD","verification_uri":"https://accounts.x.ai/activate","expires_in":60,"interval":1})
}
fn tokens(lifetime: u64) -> Value {
    json!({"access_token":"synthetic-access-secret","refresh_token":"synthetic-refresh-secret","expires_in":lifetime,"token_type":"Bearer"})
}
fn endpoints(base: &str) -> AuthEndpoints {
    AuthEndpoints {
        grok_device: format!("{base}/device"),
        grok_token: format!("{base}/token"),
        chat_token: format!("{base}/token"),
        chat_jwks: format!("{base}/jwks"),
        chat_models: format!("{base}/models"),
        chat_revoke: format!("{base}/revoke"),
        ..Default::default()
    }
}
fn record_path(root: &Path, provider: &str, profile: &str) -> PathBuf {
    root.join("operator-auth").join(format!(
        "{provider}-{:x}.json",
        Sha256::digest(profile.as_bytes())
    ))
}
fn seed(root: &Path, provider: &str, mut body: Value) {
    body["client_id"] = json!(if provider == "chatgpt" {
        "oaiapp_test"
    } else {
        "b1a00492-073a-47ea-816f-4c329264a828"
    });
    body["subject"] = json!("subject-1");
    body["issuer"] = json!("https://auth.openai.com");
    body["expires_at"] = body
        .get("expires_at")
        .cloned()
        .unwrap_or(json!(now() + 3600));
    body["scopes"] = json!(["chatgpt.tokens.use.direct"]);
    let path = record_path(root, provider, "default");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    write!(file, "{body}").unwrap();
}
#[test]
fn grok_device_request_uses_subscription_contract() {
    runtime().block_on(async {
        let (base, server) = serve(vec![Box::new(|request| {
            assert!(request.starts_with("POST /device "));
            assert!(request.contains("b1a00492-073a-47ea-816f-4c329264a828"));
            assert!(request.contains("grok-cli%3Aaccess"));
            assert!(request.contains("referrer=ovrcr"));
            (200, device())
        })]);
        let temp = tempfile::tempdir().unwrap();
        let auth = AuthManager::new(temp.path())
            .unwrap()
            .with_endpoints(endpoints(&base));
        let login = auth.begin_grok(&AuthCancel::default()).await.unwrap();
        assert_eq!(login.user_code, "ABCD");
        assert!(!format!("{login:?}").contains("private-device"));
        server.join().unwrap();
    });
}
#[test]
fn grok_pending_slowdown_success_persists_private_tokens() {
    runtime().block_on(async {
        let (base, server) =
            serve(vec![
                response(200, device()),
                response(400, json!({"error":"authorization_pending"})),
                response(400, json!({"error":"slow_down","interval":1})),
                Box::new(|request| {
                    assert!(request.contains(
                        "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"
                    ));
                    assert!(request.contains("device_code=private-device"));
                    (200, tokens(3600))
                }),
            ]);
        let temp = tempfile::tempdir().unwrap();
        let auth = AuthManager::new(temp.path())
            .unwrap()
            .with_endpoints(endpoints(&base));
        let cancel = AuthCancel::default();
        let login = auth.begin_grok(&cancel).await.unwrap();
        auth.finish_grok(login, &cancel).await.unwrap();
        assert!(auth.status(Grok).await.unwrap().signed_in);
        let token = auth.resolve(Grok, &cancel).await.unwrap();
        assert_eq!(token.expose(), "synthetic-access-secret");
        assert!(!format!("{token:?}").contains("synthetic"));
        let path = record_path(temp.path(), "grok", "default");
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(temp.path().join("operator-auth"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        server.join().unwrap();
    });
}
#[test]
fn grok_denied_expired_cancelled_and_malformed_do_not_save() {
    runtime().block_on(async{
 for(error,expected)in[("access_denied",AuthError::Denied),("authorization_denied",AuthError::Denied),("expired_token",AuthError::Expired),("unknown-secret-remote-body",AuthError::Protocol)]{
 let(base,server)=serve(vec![response(200,device()),response(400,json!({"error":error,"error_description":"synthetic-secret"}))]);let temp=tempfile::tempdir().unwrap();let auth=AuthManager::new(temp.path()).unwrap().with_endpoints(endpoints(&base));let cancel=AuthCancel::default();let login=auth.begin_grok(&cancel).await.unwrap();assert_eq!(auth.finish_grok(login,&cancel).await.unwrap_err(),expected);assert!(!auth.status(Grok).await.unwrap().signed_in);server.join().unwrap();}
 let(base,server)=serve(vec![response(200,device())]);let temp=tempfile::tempdir().unwrap();let auth=AuthManager::new(temp.path()).unwrap().with_endpoints(endpoints(&base));let cancel=AuthCancel::default();let login=auth.begin_grok(&cancel).await.unwrap();cancel.cancel();assert_eq!(auth.finish_grok(login,&cancel).await.unwrap_err(),AuthError::Cancelled);server.join().unwrap();
 let(base,server)=serve(vec![response(200,json!({"device_code":"secret","user_code":"A","verification_uri":"file:///etc/passwd","expires_in":1}))]);let auth=auth.with_endpoints(endpoints(&base));assert_eq!(auth.begin_grok(&AuthCancel::default()).await.unwrap_err(),AuthError::Protocol);server.join().unwrap();
});
}
#[test]
fn local_device_deadline_expires_before_first_poll() {
    runtime().block_on(async {
        let mut d = device();
        d["expires_in"] = json!(1);
        d["interval"] = json!(10);
        let (base, server) = serve(vec![response(200, d)]);
        let temp = tempfile::tempdir().unwrap();
        let auth = AuthManager::new(temp.path())
            .unwrap()
            .with_endpoints(endpoints(&base));
        let cancel = AuthCancel::default();
        let login = auth.begin_grok(&cancel).await.unwrap();
        assert_eq!(
            auth.finish_grok(login, &cancel).await.unwrap_err(),
            AuthError::Expired
        );
        server.join().unwrap();
    });
}
#[test]
fn concurrent_refresh_serializes_rotating_credentials_and_releases_lock() {
    runtime().block_on(async{
 let(base,server)=serve(vec![Box::new(|request|{assert!(request.contains("refresh_token=old-refresh"));(200,json!({"access_token":"new-access","refresh_token":"rotated-refresh","expires_in":3600}))})]);let temp=tempfile::tempdir().unwrap();let auth=AuthManager::new(temp.path()).unwrap().with_endpoints(endpoints(&base));seed(temp.path(),"grok",json!({"access_token":"old-access","refresh_token":"old-refresh","expires_at":1}));let other=AuthManager::new(temp.path()).unwrap().with_endpoints(endpoints(&base));let cancel=AuthCancel::default();let(a,b)=tokio::join!(auth.resolve(Grok,&cancel),other.resolve(Grok,&cancel));let a=a.unwrap();assert_eq!(a.expose(),"new-access");assert_eq!(b.unwrap().expose(),"new-access");let stored:Value=serde_json::from_slice(&fs::read(record_path(temp.path(),"grok","default")).unwrap()).unwrap();assert_eq!(stored["refresh_token"],"rotated-refresh");auth.logout(Grok,&cancel).await.unwrap();assert_eq!(a.expose(),"new-access");assert!(!auth.status(Grok).await.unwrap().signed_in);server.join().unwrap();
});
}
#[test]
fn refresh_without_replacement_retains_prior_refresh_and_chat_grant() {
    runtime().block_on(async {
        for (provider, name) in [(Grok, "grok"), (ChatGpt, "chatgpt")] {
            let (base, server) = serve(vec![Box::new(move |request| {
                assert!(request.contains("refresh_token=old-refresh"));
                if name == "chatgpt" {
                    assert!(request.contains("client_id=oaiapp_test"));
                    assert!(request.contains("resource=https%3A%2F%2Fapi.openai.com%2Fv1"));
                    assert!(!request.contains("scope="));
                }
                (200, json!({"access_token":"new-access","expires_in":3600}))
            })]);
            let temp = tempfile::tempdir().unwrap();
            let auth = AuthManager::new(temp.path())
                .unwrap()
                .with_endpoints(endpoints(&base));
            seed(
                temp.path(),
                name,
                json!({"access_token":"old-access","refresh_token":"old-refresh","expires_at":1}),
            );
            assert_eq!(
                auth.resolve(provider, &AuthCancel::default())
                    .await
                    .unwrap()
                    .expose(),
                "new-access"
            );
            let stored: Value = serde_json::from_slice(
                &fs::read(record_path(temp.path(), name, "default")).unwrap(),
            )
            .unwrap();
            assert_eq!(stored["refresh_token"], "old-refresh");
            server.join().unwrap();
        }
    });
}
#[test]
fn refresh_persistence_failure_returns_no_bearer_and_keeps_original() {
    runtime().block_on(async{
 let temp=tempfile::tempdir().unwrap();let root=temp.path().to_owned();let(base,server)=serve(vec![Box::new(move|_|{let path=record_path(&root,"grok","default");fs::set_permissions(path,fs::Permissions::from_mode(0o644)).unwrap();(200,json!({"access_token":"new-access","refresh_token":"rotated-refresh","expires_in":3600}))})]);let auth=AuthManager::new(temp.path()).unwrap().with_endpoints(endpoints(&base));seed(temp.path(),"grok",json!({"access_token":"old-access","refresh_token":"old-refresh","expires_at":1}));assert_eq!(auth.resolve(Grok,&AuthCancel::default()).await.unwrap_err(),AuthError::Storage);let stored:Value=serde_json::from_slice(&fs::read(record_path(temp.path(),"grok","default")).unwrap()).unwrap();assert_eq!(stored["refresh_token"],"old-refresh");server.join().unwrap();
});
}
#[test]
fn missing_auth_and_ungranted_chat_never_fall_back() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let auth = AuthManager::new(temp.path()).unwrap();
        for provider in [Grok, ChatGpt] {
            assert!(!auth.status(provider).await.unwrap().signed_in);
            assert_eq!(
                auth.resolve(provider, &AuthCancel::default())
                    .await
                    .unwrap_err(),
                AuthError::NotSignedIn
            );
        }
        seed(temp.path(), "chatgpt", tokens(3600));
        let path = record_path(temp.path(), "chatgpt", "default");
        let mut stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        stored["scopes"] = json!(["openid"]);
        fs::write(path, stored.to_string()).unwrap();
        assert_eq!(
            auth.resolve(ChatGpt, &AuthCancel::default())
                .await
                .unwrap_err(),
            AuthError::Permission
        );
    });
}
fn jwt(nonce: &str, changes: Value) -> String {
    let mut claims = json!({"iss":"https://auth.openai.com","sub":"subject-1","aud":"oaiapp_test","exp":now()+3600,"nonce":nonce});
    for (k, v) in changes.as_object().unwrap() {
        claims[k] = v.clone();
    }
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some("test-key".into());
    jsonwebtoken::encode(
        &header,
        &claims,
        &jsonwebtoken::EncodingKey::from_rsa_der(
            &base64::engine::general_purpose::STANDARD
                .decode(
                    include_str!("fixtures/operator-auth/rsa-test-only.pem")
                        .lines()
                        .filter(|line| !line.starts_with("-----"))
                        .collect::<String>(),
                )
                .unwrap(),
        ),
    )
    .unwrap()
}
fn query(url: &str) -> HashMap<String, String> {
    reqwest::Url::parse(url)
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
}
async fn callback(
    auth: &AuthManager,
    login: ovrcr::operator::auth::BrowserLogin,
    query: &str,
) -> Result<(), AuthError> {
    let redirect = query_map_redirect(login.authorization_url());
    let client = reqwest::Client::new();
    let request = client.get(format!("{redirect}?{query}"));
    let cancel = AuthCancel::default();
    let finish = auth.finish_chatgpt(login, &cancel);
    let (result, _) = tokio::join!(finish, async {
        let _ = request.send().await;
    });
    result
}
fn query_map_redirect(url: &str) -> String {
    query(url)["redirect_uri"].clone()
}
#[test]
fn siwc_validates_signature_state_nonce_identity_and_discovers_account_models() {
    runtime().block_on(async{
 for(changes,success)in[(json!({}),true),(json!({"nonce":"wrong"}),false),(json!({"iss":"https://evil.example"}),false),(json!({"aud":"other-client"}),false),(json!({"exp":1}),false)]{
  let temp=tempfile::tempdir().unwrap();let auth=AuthManager::new(temp.path()).unwrap();let login=auth.begin_chatgpt().await.unwrap();let fields=query(login.authorization_url());assert_eq!(fields["client_id"],"dynamic_agent_client");assert_eq!(fields["agent_name_hint"],"OVRCR");assert_eq!(fields["resource"],"https://api.openai.com/v1");assert_eq!(fields["code_challenge_method"],"S256");assert!(fields["redirect_uri"].starts_with("http://127.0.0.1:"));assert!(!format!("{login:?}").contains(&fields["nonce"]));
  let token=jwt(&fields["nonce"],changes);let jwks:Value=serde_json::from_str(include_str!("fixtures/operator-auth/jwks.json")).unwrap();let mut handlers=vec![Box::new(move|request:String|{assert!(request.contains("client_id=oaiapp_test"));assert!(request.contains("grant_type=authorization_code"));assert!(request.contains("code_verifier="));let mut t=tokens(3600);t["id_token"]=json!(token);t["scope"]=json!("openid chatgpt.tokens.use.direct");(200_u16,t)})as Box<dyn FnOnce(String)->(u16,Value)+Send>,response(200,jwks)];if success{handlers.push(Box::new(|request|{assert!(request.to_ascii_lowercase().contains("authorization: bearer synthetic-access-secret"));(200,json!({"models":[{"slug":"allowed-1","display_name":"Allowed One","visibility":"list"},{"slug":"hidden","display_name":"Hidden","visibility":"hidden"},{"slug":"allowed-2","display_name":"Allowed Two","visibility":"list"}]}))}));handlers.push(response(200,Value::Null));}
  let(base,server)=serve(handlers);let auth=auth.with_endpoints(endpoints(&base));let result=callback(&auth,login,&format!("state={}&code=auth-code&client_id=oaiapp_test",fields["state"])).await;if success{result.unwrap();let models=auth.models(ChatGpt,&AuthCancel::default()).await.unwrap();assert_eq!(models.iter().map(|m|m.id.as_str()).collect::<Vec<_>>(),vec!["allowed-1","allowed-2"]);let again=auth.begin_chatgpt().await.unwrap();let again_fields=query(again.authorization_url());assert_eq!(again_fields["client_id"],"oaiapp_test");assert_eq!(again_fields["ext_agent_host_id"],fields["ext_agent_host_id"]);assert!(!again_fields.contains_key("agent_name_hint"));assert!(again_fields.contains_key("id_token_hint"));assert!(auth.logout(ChatGpt,&AuthCancel::default()).await.unwrap().remote_revocation_confirmed);assert!(!auth.status(ChatGpt).await.unwrap().signed_in);let after=auth.begin_chatgpt().await.unwrap();let after_fields=query(after.authorization_url());assert_eq!(after_fields["client_id"],"oaiapp_test");assert!(!after_fields.contains_key("id_token_hint"));}else{assert_eq!(result.unwrap_err(),AuthError::Identity);assert!(!auth.status(ChatGpt).await.unwrap().signed_in);}server.join().unwrap();
 }
});
}
#[test]
fn siwc_rejects_bad_state_missing_client_and_duplicate_fields_before_exchange() {
    runtime().block_on(async {
        for kind in ["state", "client", "duplicate", "denied"] {
            let temp = tempfile::tempdir().unwrap();
            let auth = AuthManager::new(temp.path()).unwrap();
            let login = auth.begin_chatgpt().await.unwrap();
            let fields = query(login.authorization_url());
            let q = match kind {
                "state" => "state=wrong&code=code&client_id=oaiapp_test".into(),
                "client" => format!("state={}&code=code", fields["state"]),
                "duplicate" => format!(
                    "state={}&state={}&code=code&client_id=oaiapp_test",
                    fields["state"], fields["state"]
                ),
                _ => format!("state={}&error=access_denied", fields["state"]),
            };
            let error = callback(&auth, login, &q).await.unwrap_err();
            assert_eq!(
                error,
                match kind {
                    "duplicate" => AuthError::Protocol,
                    "denied" => AuthError::Denied,
                    _ => AuthError::Identity,
                }
            );
            assert!(!auth.status(ChatGpt).await.unwrap().signed_in);
        }
    });
}
#[test]
fn storage_refuses_insecure_directory_and_symlink_credentials() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("operator-auth");
        fs::create_dir(&dir).unwrap();
        assert_eq!(
            AuthManager::new(temp.path()).unwrap_err(),
            AuthError::Storage
        );
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let auth = AuthManager::new(temp.path()).unwrap();
        let outside = temp.path().join("outside");
        fs::write(&outside, "sensitive").unwrap();
        std::os::unix::fs::symlink(&outside, record_path(temp.path(), "grok", "default")).unwrap();
        assert_eq!(auth.status(Grok).await.unwrap_err(), AuthError::Storage);
        assert_eq!(fs::read_to_string(outside).unwrap(), "sensitive");
    });
}
#[test]
fn dangling_credential_links_are_rejected() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let auth = AuthManager::new(temp.path()).unwrap();
        std::os::unix::fs::symlink(
            temp.path().join("missing"),
            record_path(temp.path(), "grok", "default"),
        )
        .unwrap();
        assert_eq!(auth.status(Grok).await.unwrap_err(), AuthError::Storage);
    });
}
#[test]
fn login_cancellation_and_profile_isolation() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let auth = AuthManager::new(temp.path()).unwrap();
        seed(temp.path(), "grok", tokens(3600));
        let other = AuthManager::profile(temp.path(), "other").unwrap();
        assert!(!other.status(Grok).await.unwrap().signed_in);
        let login = auth.begin_chatgpt().await.unwrap();
        let cancel = AuthCancel::default();
        cancel.cancel();
        assert_eq!(
            auth.finish_chatgpt(login, &cancel).await.unwrap_err(),
            AuthError::Cancelled
        );
    });
}
#[test]
fn device_slowdown_without_interval_adds_five_seconds() {
    runtime().block_on(async {
        let start = std::sync::Arc::new(std::sync::Mutex::new(None));
        let first = start.clone();
        let second = start.clone();
        let (base, server) = serve(vec![
            response(200, device()),
            Box::new(move |_| {
                *first.lock().unwrap() = Some(std::time::Instant::now());
                (400, json!({"error":"slow_down"}))
            }),
            Box::new(move |_| {
                assert!(second.lock().unwrap().unwrap().elapsed() >= Duration::from_secs(6));
                (200, tokens(3600))
            }),
        ]);
        let temp = tempfile::tempdir().unwrap();
        let auth = AuthManager::new(temp.path())
            .unwrap()
            .with_endpoints(endpoints(&base));
        let cancel = AuthCancel::default();
        let login = auth.begin_grok(&cancel).await.unwrap();
        auth.finish_grok(login, &cancel).await.unwrap();
        server.join().unwrap();
    });
}
#[test]
fn device_expiry_interrupts_inflight_poll() {
    runtime().block_on(async {
        let mut d = device();
        d["expires_in"] = json!(2);
        let (base, server) = serve(vec![
            response(200, d),
            Box::new(move |_| {
                thread::sleep(Duration::from_secs(2));
                (200, tokens(3600))
            }),
        ]);
        let temp = tempfile::tempdir().unwrap();
        let auth = AuthManager::new(temp.path())
            .unwrap()
            .with_endpoints(endpoints(&base));
        let cancel = AuthCancel::default();
        let login = auth.begin_grok(&cancel).await.unwrap();
        assert_eq!(
            auth.finish_grok(login, &cancel).await.unwrap_err(),
            AuthError::Expired
        );
        assert!(!auth.status(Grok).await.unwrap().signed_in);
        server.join().unwrap();
    });
}
#[test]
fn siwc_retains_issued_registration_when_exchange_fails() {
    runtime().block_on(async {
        let (base, server) = serve(vec![response(
            400,
            json!({"error":"invalid_grant","error_description":"do-not-log-secret"}),
        )]);
        let temp = tempfile::tempdir().unwrap();
        let auth = AuthManager::new(temp.path())
            .unwrap()
            .with_endpoints(endpoints(&base));
        let login = auth.begin_chatgpt().await.unwrap();
        let fields = query(login.authorization_url());
        let error = callback(
            &auth,
            login,
            &format!(
                "state={}&code=expired-code&client_id=oaiapp_test",
                fields["state"]
            ),
        )
        .await
        .unwrap_err();
        assert!(!format!("{error:?} {error}").contains("do-not-log"));
        let status = auth.status(ChatGpt).await.unwrap();
        assert!(!status.signed_in);
        assert_eq!(status.client_id.as_deref(), Some("oaiapp_test"));
        let retry = auth.begin_chatgpt().await.unwrap();
        assert_eq!(query(retry.authorization_url())["client_id"], "oaiapp_test");
        server.join().unwrap();
    });
}
#[test]
fn instance_host_id_is_shared_and_has_uuid_form() {
    runtime().block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let a = AuthManager::profile(temp.path(), "a").unwrap();
        let b = AuthManager::profile(temp.path(), "b").unwrap();
        let (a, b) = tokio::join!(a.begin_chatgpt(), b.begin_chatgpt());
        let a = a.unwrap();
        let b = b.unwrap();
        let af = query(a.authorization_url());
        let bf = query(b.authorization_url());
        assert_eq!(af["ext_agent_host_id"], bf["ext_agent_host_id"]);
        let uuid = af["ext_agent_host_id"].strip_prefix("urn:uuid:").unwrap();
        assert!(ovrcr_protocol::bridge::canonical_uuid(uuid));
    });
}
#[test]
fn siwc_rejects_bad_signature_but_saves_identity_only_consent() {
    runtime().block_on(async {
        for tamper in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let auth = AuthManager::new(temp.path()).unwrap();
            let login = auth.begin_chatgpt().await.unwrap();
            let fields = query(login.authorization_url());
            let mut token = jwt(&fields["nonce"], json!({}));
            if tamper {
                let index = token.rfind('.').unwrap() + 1;
                let ch = if token.as_bytes()[index] == b'A' {
                    "B"
                } else {
                    "A"
                };
                token.replace_range(index..index + 1, ch);
            }
            let mut t = tokens(3600);
            t["id_token"] = json!(token);
            t["scope"] = json!("openid profile");
            let (base, server) = serve(vec![
                response(200, t),
                response(
                    200,
                    serde_json::from_str(include_str!("fixtures/operator-auth/jwks.json")).unwrap(),
                ),
            ]);
            let auth = auth.with_endpoints(endpoints(&base));
            let result = callback(
                &auth,
                login,
                &format!("state={}&code=code&client_id=oaiapp_test", fields["state"]),
            )
            .await;
            if tamper {
                assert_eq!(result.unwrap_err(), AuthError::Identity);
                assert!(!auth.status(ChatGpt).await.unwrap().signed_in);
            } else {
                result.unwrap();
                let status = auth.status(ChatGpt).await.unwrap();
                assert!(status.signed_in);
                assert!(!status.plan_usage);
                assert_eq!(
                    auth.resolve(ChatGpt, &AuthCancel::default())
                        .await
                        .unwrap_err(),
                    AuthError::Permission
                );
            }
            server.join().unwrap();
        }
    });
}
#[test]
fn refresh_revoked_grant_saves_rotated_token_and_logout_reports_unconfirmed_revocation() {
    runtime().block_on(async{
 let(base,server)=serve(vec![response(200,json!({"access_token":"new-access","refresh_token":"rotated-refresh","expires_in":3600,"scope":"openid"})),response(500,json!({"error_description":"do-not-log-secret"}))]);let temp=tempfile::tempdir().unwrap();let auth=AuthManager::new(temp.path()).unwrap().with_endpoints(endpoints(&base));seed(temp.path(),"chatgpt",json!({"access_token":"old-access","refresh_token":"old-refresh","expires_at":1}));assert_eq!(auth.resolve(ChatGpt,&AuthCancel::default()).await.unwrap_err(),AuthError::Permission);let status=auth.status(ChatGpt).await.unwrap();assert!(!status.plan_usage);let record:Value=serde_json::from_slice(&fs::read(record_path(temp.path(),"chatgpt","default")).unwrap()).unwrap();assert_eq!(record["refresh_token"],"rotated-refresh");assert!(!auth.logout(ChatGpt,&AuthCancel::default()).await.unwrap().remote_revocation_confirmed);assert!(!auth.status(ChatGpt).await.unwrap().signed_in);assert_eq!(auth.status(ChatGpt).await.unwrap().client_id.as_deref(),Some("oaiapp_test"));server.join().unwrap();
});
}
#[test]
fn reauthorization_rejects_changed_client_and_verified_subject() {
    runtime().block_on(async {
        for mismatch_client in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let auth = AuthManager::new(temp.path()).unwrap();
            seed(temp.path(), "chatgpt", tokens(3600));
            let before = fs::read(record_path(temp.path(), "chatgpt", "default")).unwrap();
            let login = auth.begin_chatgpt().await.unwrap();
            let fields = query(login.authorization_url());
            let auth = if mismatch_client {
                auth
            } else {
                let mut t = tokens(3600);
                t["id_token"] = json!(jwt(&fields["nonce"], json!({"sub":"different-subject"})));
                t["scope"] = json!("chatgpt.tokens.use.direct");
                let (base, server) = serve(vec![
                    response(200, t),
                    response(
                        200,
                        serde_json::from_str(include_str!("fixtures/operator-auth/jwks.json"))
                            .unwrap(),
                    ),
                ]);
                let auth = auth.with_endpoints(endpoints(&base));
                let error = callback(
                    &auth,
                    login,
                    &format!("state={}&code=code", fields["state"]),
                )
                .await
                .unwrap_err();
                assert_eq!(error, AuthError::Identity);
                assert_eq!(
                    before,
                    fs::read(record_path(temp.path(), "chatgpt", "default")).unwrap()
                );
                server.join().unwrap();
                continue;
            };
            let error = callback(
                &auth,
                login,
                &format!(
                    "state={}&code=code&client_id=different-client",
                    fields["state"]
                ),
            )
            .await
            .unwrap_err();
            assert_eq!(error, AuthError::Identity);
            assert_eq!(
                before,
                fs::read(record_path(temp.path(), "chatgpt", "default")).unwrap()
            );
        }
    });
}
#[test]
fn released_rig_oauth_constructors_use_public_responses_dialects() {
    use rig::providers::{
        openai::{OpenAIConfig, Route},
        xai,
    };
    let openai = OpenAIConfig::new("synthetic-oauth-bearer")
        .with_route(Route::Responses)
        .client();
    let grok = xai::new("synthetic-oauth-bearer");
    assert_eq!(openai.config().base_url, "https://api.openai.com/v1");
    assert_eq!(grok.config().base_url, "https://api.x.ai");
    assert!(!format!("{openai:?} {grok:?}").contains("synthetic-oauth-bearer"));
}
