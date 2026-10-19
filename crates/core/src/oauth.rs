//! Google OAuth2: Authorization Code + PKCE with a loopback redirect.

use std::time::{Duration, Instant};

use oauth2::basic::BasicClient;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge, RedirectUrl,
    RefreshToken, Scope, TokenResponse, TokenUrl,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::{Error, Result};

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://mail.google.com/";

#[derive(Clone)]
pub struct AccessToken {
    pub token: String,
    pub expires: Instant,
}

impl AccessToken {
    /// Still good for at least another minute.
    pub fn fresh(&self) -> bool {
        Instant::now() + Duration::from_secs(60) < self.expires
    }
}

fn http() -> Result<oauth2::reqwest::Client> {
    oauth2::reqwest::ClientBuilder::new()
        .redirect(oauth2::reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| Error::Auth(e.to_string()))
}

fn auth_err(e: impl std::fmt::Display) -> Error {
    Error::Auth(e.to_string())
}

/// Pulls `code` and `state` out of `GET /callback?code=…&state=… HTTP/1.1`.
pub fn parse_callback(request: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    let line = request.lines().next()?;
    let target = line.split_whitespace().nth(1)?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/callback" {
        return None;
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = percent_decode(v);
        match k {
            "code" => code = Some(v),
            "state" => state = Some(v),
            "error" => error = Some(v),
            _ => {}
        }
    }
    Some((code, state, error))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const PAGE: &str = "<!doctype html><meta charset=utf-8><title>lettera</title>\
<body style=\"font-family:monospace;background:#1a1b26;color:#c0caf5;padding:48px\">";

/// Runs the whole browser flow and returns the refresh token.
/// `open` receives the URL to show in the system browser.
pub async fn authorize(
    client_id: &str,
    client_secret: &str,
    email: &str,
    open: impl FnOnce(String),
) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let client = BasicClient::new(ClientId::new(client_id.to_string()))
        .set_client_secret(ClientSecret::new(client_secret.to_string()))
        .set_auth_uri(AuthUrl::new(AUTH_URL.into()).map_err(auth_err)?)
        .set_token_uri(TokenUrl::new(TOKEN_URL.into()).map_err(auth_err)?)
        .set_redirect_uri(RedirectUrl::new(redirect).map_err(auth_err)?);
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, csrf) = client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new(SCOPE.into()))
        .set_pkce_challenge(challenge)
        .add_extra_param("access_type", "offline")
        .add_extra_param("prompt", "consent")
        .add_extra_param("login_hint", email)
        .url();
    open(url.to_string());

    let deadline = Instant::now() + Duration::from_secs(300);
    let code = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let (mut sock, _) = tokio::time::timeout(remaining, listener.accept())
            .await
            .map_err(|_| Error::Auth("timed out waiting for the browser".into()))??;
        let mut buf = vec![0u8; 8192];
        let n = sock.read(&mut buf).await.unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]).into_owned();
        let Some((code, state, error)) = parse_callback(&request) else {
            let _ = sock
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            continue;
        };
        let (status, msg, result) = match (code, state, error) {
            (_, _, Some(e)) => ("400 Bad Request", "access denied.", Err(Error::Auth(e))),
            (Some(c), Some(s), None) if s == *csrf.secret() => {
                ("200 OK", "signed in. you can close this window.", Ok(c))
            }
            _ => (
                "400 Bad Request",
                "invalid response.",
                Err(Error::Auth("missing state or code".into())),
            ),
        };
        let body = format!("{PAGE}lettera · {msg}</body>");
        let resp = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = sock.write_all(resp.as_bytes()).await;
        let _ = sock.shutdown().await;
        break result?;
    };

    let token = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(verifier)
        .request_async(&http()?)
        .await
        .map_err(|e| Error::Auth(format!("code exchange failed: {e}")))?;
    token
        .refresh_token()
        .map(|t| t.secret().clone())
        .ok_or_else(|| Error::Auth("google returned no refresh token".into()))
}

/// Exchanges the refresh token for a fresh access token.
pub async fn refresh(
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<AccessToken> {
    let client = BasicClient::new(ClientId::new(client_id.to_string()))
        .set_client_secret(ClientSecret::new(client_secret.to_string()))
        .set_token_uri(TokenUrl::new(TOKEN_URL.into()).map_err(auth_err)?);
    let token = client
        .exchange_refresh_token(&RefreshToken::new(refresh_token.to_string()))
        .request_async(&http()?)
        .await
        .map_err(|e| Error::Auth(format!("token refresh failed: {e}")))?;
    let lifetime = token.expires_in().unwrap_or(Duration::from_secs(3600));
    Ok(AccessToken {
        token: token.access_token().secret().clone(),
        expires: Instant::now() + lifetime,
    })
}

/// Plain GET returning the body (used by tests to talk to local services).
pub async fn http_get(url: &str) -> Result<String> {
    http()?
        .get(url)
        .send()
        .await
        .map_err(auth_err)?
        .text()
        .await
        .map_err(auth_err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_parsing() {
        let (c, s, e) =
            parse_callback("GET /callback?state=ab%2Fc&code=4%2F0Ad HTTP/1.1\r\nHost: x\r\n")
                .unwrap();
        assert_eq!(c.as_deref(), Some("4/0Ad"));
        assert_eq!(s.as_deref(), Some("ab/c"));
        assert!(e.is_none());
        let (_, _, e) = parse_callback("GET /callback?error=access_denied HTTP/1.1").unwrap();
        assert_eq!(e.as_deref(), Some("access_denied"));
        assert!(parse_callback("GET /favicon.ico HTTP/1.1").is_none());
        assert_eq!(percent_decode("a%2"), "a%2");
    }
}
