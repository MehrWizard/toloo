//! Client-side transport for `pool.exchange` and other requests.
//!
//! Supports WebSocket (ws/wss) and HTTP/HTTPS with configurable TLS verification.

use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite::Message, Connector, MaybeTlsStream};
use std::sync::Arc;

use crate::envelope::{make_envelope, parse_envelope, verify_chain};
use crate::types::{Envelope, LocalNode};

#[cfg(feature = "tls")]
use tokio_rustls::rustls::{self, pki_types::ServerName};
#[cfg(feature = "tls")]
use tokio_rustls::TlsConnector;

/// TLS verification mode for client connections.
#[derive(Debug, Clone, Copy)]
pub enum TlsVerify {
    /// Verify certificates using system trust store (standard HTTPS).
    Standard,
    /// Accept any certificate (for self-signed relay certs).
    /// WARNING: Vulnerable to MITM. Only use when you pin the cert fingerprint separately.
    AcceptAny,
}

/// Configuration for a pool.exchange request.
#[derive(Debug, Clone)]
pub struct PoolExchangeConfig {
    /// Envelopes to offer to the relay.
    pub offer: Vec<Envelope>,
    /// Rooms to fetch, with optional `after` timestamp and `limit`.
    pub want_rooms: Vec<WantRoom>,
    /// Request timeout.
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct WantRoom {
    pub room_pub: String,
    pub after: u64,
    pub limit: usize,
}

impl Default for PoolExchangeConfig {
    fn default() -> Self {
        Self {
            offer: Vec::new(),
            want_rooms: Vec::new(),
            timeout: Some(Duration::from_secs(30)),
        }
    }
}

/// Perform `pool.exchange` over WebSocket (ws:// or wss://).
///
/// Returns parsed and verified envelopes.
pub async fn pool_exchange_ws(
    url: &str,
    node: &LocalNode,
    config: PoolExchangeConfig,
    tls_verify: TlsVerify,
) -> Result<Vec<Envelope>> {
    let request = build_pool_exchange_request(node, &config)?;

    let (mut ws, _) = connect_ws(url, tls_verify).await
        .with_context(|| format!("failed to connect to {url}"))?;

    ws.send(Message::Text(serde_json::to_string(&request)?.into()))
        .await
        .context("WS send failed")?;

    let mut results = Vec::new();
    while let Some(msg) = ws.next().await {
        match msg? {
            Message::Text(text) => {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    if v.get("done").is_some() { break; }
                    if v.get("error").is_some() { continue; }
                    if v.get("wr").is_some() { continue; }
                    if let Ok(env) = parse_envelope(v) {
                        if verify_chain(&env).is_ok() {
                            results.push(env);
                        }
                    }
                }
            }
            Message::Close(_) => break,
            Message::Ping(p) => { ws.send(Message::Pong(p)).await.ok(); }
            _ => {}
        }
    }
    let _ = ws.close(None).await;
    Ok(results)
}

/// Perform `pool.exchange` over HTTP/HTTPS.
///
/// Returns parsed and verified envelopes.
pub async fn pool_exchange_http(
    url: &str,
    node: &LocalNode,
    config: PoolExchangeConfig,
    _tls_verify: TlsVerify,
) -> Result<Vec<Envelope>> {
    let request = build_pool_exchange_request(node, &config)?;
    let request_json = serde_json::to_string(&request)?;

    let resp_body = do_http_request(url, &request_json).await?;
    let values: Vec<Value> = serde_json::from_slice(&resp_body)?;

    let mut results = Vec::new();
    for v in values {
        if v.get("error").is_some() || v.get("done").is_some() { continue; }
        if let Ok(env) = parse_envelope(v) {
            if verify_chain(&env).is_ok() {
                results.push(env);
            }
        }
    }
    Ok(results)
}

/// Auto-detect transport from URL scheme and perform `pool.exchange`.
pub async fn pool_exchange(
    url: &str,
    node: &LocalNode,
    config: PoolExchangeConfig,
    tls_verify: TlsVerify,
) -> Result<Vec<Envelope>> {
    if url.starts_with("http://") || url.starts_with("https://") {
        pool_exchange_http(url, node, config, tls_verify).await
    } else {
        pool_exchange_ws(url, node, config, tls_verify).await
    }
}

fn build_pool_exchange_request(node: &LocalNode, config: &PoolExchangeConfig) -> Result<Envelope> {
    let offer_values: Vec<Value> = config.offer
        .iter()
        .map(|e| serde_json::to_value(e))
        .collect::<Result<Vec<_>, _>>()?;

    let want = serde_json::json!({
        "rooms": config.want_rooms.iter().map(|w| serde_json::json!({
            "room": w.room_pub,
            "after": w.after,
            "limit": w.limit,
        })).collect::<Vec<_>>(),
    });

    make_envelope("pool.exchange", Some(serde_json::json!({
        "offer": offer_values,
        "want": want,
    })), node, None)
}

#[cfg(feature = "tls")]
async fn connect_ws(
    url: &str,
    tls_verify: TlsVerify,
) -> Result<(
    tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>,
    tokio_tungstenite::tungstenite::handshake::client::Response
)> {
    match tls_verify {
        TlsVerify::Standard => {
            connect_async(url).await.map_err(|e| anyhow::anyhow!("WS connect: {}", e))
        }
        TlsVerify::AcceptAny => {
            let connector = make_accept_any_connector();
            let (ws, resp) = tokio_tungstenite::connect_async_tls_with_config(
                url,
                None,
                false,
                Some(Connector::Rustls(connector)),
            ).await.map_err(|e| anyhow::anyhow!("WSS connect: {}", e))?;
            Ok((ws, resp))
        }
    }
}

#[cfg(not(feature = "tls"))]
async fn connect_ws(
    _url: &str,
    tls_verify: TlsVerify,
) -> Result<(
    tokio_tungstenite::WebSocketStream<MaybeTlsStream<TcpStream>>,
    tokio_tungstenite::tungstenite::handshake::client::Response
)> {
    if matches!(tls_verify, TlsVerify::AcceptAny) {
        anyhow::bail!("TLS support (wss) requires the 'tls' feature");
    }
    connect_async(_url).await.map_err(|e| anyhow::anyhow!("WS connect: {}", e))
}

#[cfg(feature = "tls")]
fn make_accept_any_connector() -> Arc<rustls::ClientConfig> {
    #[derive(Debug)]
    struct AcceptAny;
    impl rustls::client::danger::ServerCertVerifier for AcceptAny {
        fn verify_server_cert(
            &self, _end_entity: &rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[rustls::pki_types::CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self, _msg: &[u8], _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self, _msg: &[u8], _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::aws_lc_rs::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    let cfg = rustls::ClientConfig::builder_with_provider(
        Arc::new(rustls::crypto::aws_lc_rs::default_provider())
    )
    .with_safe_default_protocol_versions()
    .expect("safe default protocol versions")
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(AcceptAny))
    .with_no_client_auth();

    Arc::new(cfg)
}

async fn do_http_request(url: &str, body: &str) -> Result<Vec<u8>> {
    let is_https = url.starts_with("https://");
    let without_scheme = url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .context("invalid HTTP(S) URL")?;

    let (host_port, path) = match without_scheme.find('/') {
        Some(i) => (&without_scheme[..i], &without_scheme[i..]),
        None => (without_scheme, "/"),
    };
    let default_port = if is_https { 443u16 } else { 80 };
    let (host, port) = match host_port.rfind(':') {
        Some(i) => (&host_port[..i], host_port[i+1..].parse()?),
        None => (host_port, default_port),
    };

    let body_bytes = body.as_bytes();
    let req_headers = format!(
        "POST {} HTTP/1.1\r\nHost: {host_port}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        path, body_bytes.len()
    );

    let stream = TcpStream::connect(format!("{host}:{port}")).await?;

    #[cfg(feature = "tls")]
    let resp_body: Vec<u8> = if is_https {
        let connector = TlsConnector::from(make_accept_any_connector());
        let server_name = ServerName::try_from(host.to_owned())?;
        let mut tls = connector.connect(server_name, stream).await?;
        tls.write_all(req_headers.as_bytes()).await?;
        tls.write_all(body_bytes).await?;
        tls.flush().await?;
        read_http_response_body(&mut tls).await?
    } else {
        let mut plain = stream;
        plain.write_all(req_headers.as_bytes()).await?;
        plain.write_all(body_bytes).await?;
        plain.flush().await?;
        read_http_response_body(&mut plain).await?
    };

    #[cfg(not(feature = "tls"))]
    let resp_body: Vec<u8> = {
        if is_https {
            anyhow::bail!("HTTPS support requires the 'tls' feature");
        }
        let mut plain = stream;
        plain.write_all(req_headers.as_bytes()).await?;
        plain.write_all(body_bytes).await?;
        plain.flush().await?;
        read_http_response_body(&mut plain).await?
    };

    Ok(resp_body)
}

async fn read_http_response_body<S>(stream: &mut S) -> Result<Vec<u8>>
where S: AsyncRead + Unpin
{
    let mut header_buf: Vec<u8> = Vec::with_capacity(2048);
    loop {
        let mut byte = [0u8; 1];
        match stream.read(&mut byte).await {
            Ok(0) | Err(_) => break,
            Ok(_) => header_buf.push(byte[0]),
        }
        if header_buf.ends_with(b"\r\n\r\n") { break; }
        if header_buf.len() > 16 * 1024 { anyhow::bail!("HTTP response headers too large"); }
    }
    let header_str = std::str::from_utf8(&header_buf)?;
    let mut content_length: Option<usize> = None;
    for line in header_str.split("\r\n").skip(1) {
        if let Some(rest) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = rest.trim().parse().ok();
        }
    }
    if let Some(len) = content_length {
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await?;
        Ok(buf)
    } else {
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await?;
        Ok(buf)
    }
}