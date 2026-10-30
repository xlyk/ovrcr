//! Open a secret-free loopback URL; only browser HTTP receives the provider redirect.
use anyhow::{Result, anyhow, bail};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    process::Command,
    task::JoinHandle,
};

pub struct BrowserRedirect {
    local_url: String,
    task: JoinHandle<()>,
}
impl BrowserRedirect {
    pub async fn new(provider_url: &str) -> Result<Self> {
        let provider = reqwest::Url::parse(provider_url)
            .map_err(|_| anyhow!("invalid sign-in destination"))?;
        if provider.scheme() != "https" || provider_url.contains(['\r', '\n']) {
            bail!("invalid sign-in destination");
        }
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| anyhow!("sign-in redirect unavailable"))?;
        let token = ovrcr_runtime::agent_runner::private_identifier()
            .map_err(|_| anyhow!("sign-in redirect unavailable"))?;
        let path = format!("/sign-in/{token}");
        let local_url = format!(
            "http://{}{path}",
            listener
                .local_addr()
                .map_err(|_| anyhow!("sign-in redirect unavailable"))?
        );
        let provider_url = provider.as_str().to_owned();
        let task = tokio::spawn(async move {
            let serve = async {
                for _ in 0..16 {
                    let Ok((mut socket, peer)) = listener.accept().await else {
                        break;
                    };
                    if !peer.ip().is_loopback() {
                        continue;
                    }
                    let mut request = Vec::new();
                    let read = tokio::time::timeout(Duration::from_secs(5), async {
                        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                            let mut block = [0u8; 1024];
                            let n = socket.read(&mut block).await?;
                            if n == 0 || request.len() + n > 8192 {
                                return Err(std::io::Error::other("invalid sign-in request"));
                            }
                            request.extend_from_slice(&block[..n]);
                        }
                        Ok::<_, std::io::Error>(())
                    })
                    .await;
                    if !matches!(read, Ok(Ok(()))) {
                        continue;
                    }
                    let matched = std::str::from_utf8(&request)
                        .ok()
                        .and_then(|s| s.lines().next())
                        .is_some_and(|line| line == format!("GET {path} HTTP/1.1"));
                    let response = if matched {
                        format!(
                            "HTTP/1.1 302 Found\r\nLocation: {provider_url}\r\nCache-Control: no-store\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        )
                    } else {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .into()
                    };
                    let _ = socket.write_all(response.as_bytes()).await;
                    if matched {
                        break;
                    }
                }
            };
            let _ = tokio::time::timeout(Duration::from_secs(600), serve).await;
        });
        Ok(Self { local_url, task })
    }
    fn launcher(&self) -> Command {
        let mut command = Command::new(if cfg!(target_os = "macos") {
            "/usr/bin/open"
        } else {
            "xdg-open"
        });
        command
            .arg(&self.local_url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }
    pub async fn open(&self) -> Result<()> {
        if !self
            .launcher()
            .status()
            .await
            .map_err(|_| anyhow!("could not open sign-in browser"))?
            .success()
        {
            bail!("could not open sign-in browser");
        }
        Ok(())
    }
}
impl Drop for BrowserRedirect {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn browser_launcher_exposes_only_loopback_and_redirects_hint_in_http() {
        let secret = "synthetic-private-id-token";
        let redirect = BrowserRedirect::new(&format!(
            "https://auth.openai.com/authorize?id_token_hint={secret}"
        ))
        .await
        .unwrap();
        let args: Vec<_> = redirect
            .launcher()
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args.len(), 1);
        assert!(args[0].starts_with("http://127.0.0.1:"));
        assert!(!args[0].contains(secret));
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        let response = client.get(&args[0]).send().await.unwrap();
        assert_eq!(response.status(), 302);
        assert!(
            response.headers()["location"]
                .to_str()
                .unwrap()
                .contains(secret)
        );
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    #[tokio::test]
    async fn redirect_waits_for_complete_request_and_refuses_other_paths() {
        let redirect = BrowserRedirect::new("https://auth.openai.com/authorize")
            .await
            .unwrap();
        let url = reqwest::Url::parse(&redirect.local_url).unwrap();
        let wrong = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{}/wrong", url.port().unwrap()))
            .send()
            .await
            .unwrap();
        assert_eq!(wrong.status(), 404);
        let mut socket = tokio::net::TcpStream::connect(("127.0.0.1", url.port().unwrap()))
            .await
            .unwrap();
        socket.write_all(b"GET ").await.unwrap();
        tokio::task::yield_now().await;
        socket
            .write_all(format!("{} HTTP/1.1\r\nHost: localhost\r\n\r\n", url.path()).as_bytes())
            .await
            .unwrap();
        let mut bytes = Vec::new();
        socket.read_to_end(&mut bytes).await.unwrap();
        assert!(
            String::from_utf8(bytes)
                .unwrap()
                .starts_with("HTTP/1.1 302")
        );
    }
}
