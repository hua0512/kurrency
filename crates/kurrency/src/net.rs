//! The app's HTTP client: GPUI's [`HttpClient`] over [ureq], a small
//! blocking client, in place of gpui-pre's reqwest client (which brings
//! tokio, hyper and HTTP/2 for a handful of GETs a day).
//!
//! Each request runs on a thread of its own, so the UI never waits on the
//! network; the returned future completes when the thread has the whole
//! response. TLS is rustls (ring) with the platform's certificate verifier,
//! falling back to the bundled webpki roots on a system without CA
//! certificates, like GPUI's own client. Proxies come from `HTTPS_PROXY`,
//! `ALL_PROXY` and `HTTP_PROXY`, minus `NO_PROXY`. Responses are decoded
//! from gzip.

use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::Duration;

use anyhow::{Context as _, anyhow};
use futures::AsyncReadExt as _;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use gpui_kit::http_client::http::{HeaderValue, header};
use gpui_kit::http_client::{
    self, AsyncBody, HttpClient, RedirectPolicy, Request, RequestTimeout, Response, Url,
};
use ureq::tls::{RootCerts, TlsConfig};

/// The `User-Agent` of every request.
const USER_AGENT: &str = concat!("kurrency/", env!("CARGO_PKG_VERSION"));
/// Time to establish a connection (DNS, TCP and TLS).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Time for a whole request, from connecting to the end of the body, unless
/// the request sets its own [`RequestTimeout`].
const TOTAL_TIMEOUT: Duration = Duration::from_secs(30);
/// Redirects followed when the request does not say ([`RedirectPolicy`]).
const DEFAULT_REDIRECTS: u32 = 10;
/// The most [`RedirectPolicy::FollowAll`] follows.
const MAX_REDIRECTS: u32 = 100;
/// The largest response body read; the rate tables are a few kilobytes.
const BODY_LIMIT: u64 = 32 * 1024 * 1024;

pub(crate) struct UreqClient {
    /// Built on the first request, on its thread: the platform verifier
    /// reads the system's certificates.
    agent: Arc<OnceLock<ureq::Agent>>,
    user_agent: HeaderValue,
}

impl UreqClient {
    pub(crate) fn new() -> Self {
        Self {
            agent: Arc::default(),
            user_agent: HeaderValue::from_static(USER_AGENT),
        }
    }
}

impl HttpClient for UreqClient {
    fn user_agent(&self) -> Option<&HeaderValue> {
        Some(&self.user_agent)
    }

    /// The proxy is ureq's, from the environment, so GPUI has none to add.
    fn proxy(&self) -> Option<&Url> {
        None
    }

    fn send(
        &self,
        request: Request<AsyncBody>,
    ) -> BoxFuture<'static, anyhow::Result<Response<AsyncBody>>> {
        let agent = self.agent.clone();
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let body = match body.0 {
                http_client::Inner::Empty => None,
                http_client::Inner::Bytes(cursor) => Some(cursor.into_inner().to_vec()),
                http_client::Inner::AsyncReader(mut reader) => {
                    let mut bytes = Vec::new();
                    reader
                        .read_to_end(&mut bytes)
                        .await
                        .context("reading the request body")?;
                    Some(bytes)
                }
            };
            let request = Request::from_parts(parts, body);
            let (sender, receiver) = oneshot::channel();
            thread::Builder::new()
                .name("http".into())
                .spawn(move || {
                    let agent = agent.get_or_init(new_agent);
                    // The caller may have given up and dropped the receiver.
                    sender.send(run(agent, request)).ok();
                })
                .context("starting an HTTP thread")?;
            receiver
                .await
                .map_err(|_| anyhow!("the HTTP thread quit"))?
        })
    }
}

/// An agent with the timeouts, the User-Agent, gzip and the environment's
/// proxy (ureq's default), which returns error statuses as responses.
fn new_agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(TOTAL_TIMEOUT))
        .user_agent(USER_AGENT)
        .http_status_as_error(false)
        .max_redirects(DEFAULT_REDIRECTS)
        .tls_config(TlsConfig::builder().root_certs(root_certs()).build())
        .build();
    if let Some(proxy) = config.proxy() {
        // The host only: the URL may hold credentials.
        log::info!("HTTP proxy from the environment: {}", proxy.host());
    }
    ureq::Agent::new_with_config(config)
}

/// The platform's verifier (the system's certificates, or the OS's own
/// verification on macOS and Windows), or the bundled webpki roots when it
/// cannot start: a Linux system with no CA certificates installed.
fn root_certs() -> RootCerts {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    match rustls_platform_verifier::Verifier::new(provider) {
        Ok(_) => RootCerts::PlatformVerifier,
        Err(err) => {
            log::warn!("no platform certificate verifier, using the bundled roots: {err}");
            RootCerts::WebPki
        }
    }
}

/// Applies a request's redirect limit and total timeout.
fn configure<S: ureq::AsSendBody>(
    agent: &ureq::Agent,
    request: Request<S>,
    redirects: u32,
    timeout: Duration,
) -> Request<S> {
    agent
        .configure_request(request)
        .max_redirects(redirects)
        // With no redirects left, return the 3xx rather than an error.
        .max_redirects_will_error(false)
        .timeout_global(Some(timeout))
        .build()
}

/// Runs one request to the end of its body (blocking).
fn run(
    agent: &ureq::Agent,
    request: Request<Option<Vec<u8>>>,
) -> anyhow::Result<Response<AsyncBody>> {
    let redirects = match request.extensions().get::<RedirectPolicy>() {
        None => DEFAULT_REDIRECTS,
        Some(RedirectPolicy::NoFollow) => 0,
        Some(RedirectPolicy::FollowLimit(limit)) => *limit,
        Some(RedirectPolicy::FollowAll) => MAX_REDIRECTS,
    };
    let timeout = request
        .extensions()
        .get::<RequestTimeout>()
        .map_or(TOTAL_TIMEOUT, |timeout| timeout.0);
    let url = request.uri().to_string();
    let (parts, body) = request.into_parts();
    let response = match body {
        // No body, so ureq sends no `Content-Length: 0` with a GET.
        None => agent.run(configure(
            agent,
            Request::from_parts(parts, ()),
            redirects,
            timeout,
        )),
        Some(body) => agent.run(configure(
            agent,
            Request::from_parts(parts, body),
            redirects,
            timeout,
        )),
    }
    .with_context(|| format!("{url}: request failed"))?;

    let (mut parts, mut body) = response.into_parts();
    // ureq decodes gzip while reading; the length and encoding headers
    // describe what came over the wire.
    if parts
        .headers
        .get(header::CONTENT_ENCODING)
        .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"gzip"))
    {
        parts.headers.remove(header::CONTENT_ENCODING);
        parts.headers.remove(header::CONTENT_LENGTH);
    }
    let bytes = body
        .with_config()
        .limit(BODY_LIMIT)
        .read_to_vec()
        .with_context(|| format!("{url}: reading the response"))?;
    Ok(Response::from_parts(parts, AsyncBody::from(bytes)))
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead as _, BufReader, Read as _, Write as _};
    use std::net::{TcpListener, TcpStream};
    use std::time::Instant;

    use gpui_kit::http_client::{HttpRequestExt as _, StatusCode, http};

    use super::*;

    /// A one-connection-per-response HTTP/1.1 server on localhost: for each
    /// response in `responses`, accepts a connection, reads one request and
    /// answers. Returns its base URL and the requests it read (head only).
    fn serve(responses: Vec<Vec<u8>>) -> (String, thread::JoinHandle<Vec<String>>) {
        serve_on(TcpListener::bind("127.0.0.1:0").unwrap(), responses)
    }

    fn serve_on(
        listener: TcpListener,
        responses: Vec<Vec<u8>>,
    ) -> (String, thread::JoinHandle<Vec<String>>) {
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (stream, _) = listener.accept().unwrap();
                requests.push(read_head(&stream));
                (&stream).write_all(&response).unwrap();
            }
            requests
        });
        (url, server)
    }

    fn read_head(stream: &TcpStream) -> String {
        let mut reader = BufReader::new(stream);
        let mut head = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                return head;
            }
            head.push_str(&line);
        }
    }

    fn response(status: &str, headers: &[&str], body: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for header in headers {
            out.push_str(header);
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
        let mut out = out.into_bytes();
        out.extend_from_slice(body);
        out
    }

    fn get(
        client: &UreqClient,
        url: &str,
        follow: bool,
    ) -> anyhow::Result<(StatusCode, http::HeaderMap, Vec<u8>)> {
        futures::executor::block_on(async {
            let mut response = client.get(url, AsyncBody::empty(), follow).await?;
            let mut body = Vec::new();
            response.body_mut().read_to_end(&mut body).await?;
            Ok((response.status(), response.headers().clone(), body))
        })
    }

    #[test]
    fn gets_a_gzipped_body_with_the_user_agent() {
        let json = br#"{"base":"USD","rates":{"EUR":0.92}}"#;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(json).unwrap();
        let gzipped = gz.finish().unwrap();
        let (url, server) = serve(vec![response(
            "200 OK",
            &["Content-Encoding: gzip"],
            &gzipped,
        )]);

        let (status, headers, body) =
            get(&UreqClient::new(), &format!("{url}/rates"), true).unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json);
        assert!(headers.get(header::CONTENT_ENCODING).is_none());

        let requests = server.join().unwrap().join("\n").to_ascii_lowercase();
        assert!(
            requests.starts_with("get /rates http/1.1\r\n"),
            "{requests}"
        );
        let agent = format!("user-agent: kurrency/{}\r\n", env!("CARGO_PKG_VERSION"));
        assert!(requests.contains(&agent), "{requests}");
        assert!(requests.contains("accept-encoding: gzip"), "{requests}");
        assert!(!requests.contains("content-length"), "{requests}");
    }

    #[test]
    fn follows_redirects_when_asked() {
        // The Location needs the port, so the server binds first.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let redirect = response("302 Found", &[&format!("Location: {url}/b")], b"");
        let (url, server) = serve_on(
            listener,
            vec![
                redirect.clone(),
                response("200 OK", &[], b"moved"),
                redirect,
            ],
        );

        let client = UreqClient::new();
        let (status, _, body) = get(&client, &format!("{url}/a"), true).unwrap();
        assert_eq!((status, body.as_slice()), (StatusCode::OK, &b"moved"[..]));
        // Not following returns the redirect itself.
        let (status, headers, _) = get(&client, &format!("{url}/a"), false).unwrap();
        assert_eq!(status, StatusCode::FOUND);
        assert_eq!(headers[header::LOCATION], format!("{url}/b").as_str());
        let paths: Vec<String> = server
            .join()
            .unwrap()
            .iter()
            .map(|head| head.split(' ').nth(1).unwrap().to_string())
            .collect();
        assert_eq!(paths, ["/a", "/b", "/a"]);
    }

    #[test]
    fn error_statuses_are_responses() {
        let (url, server) = serve(vec![response("503 Service Unavailable", &[], b"busy")]);
        let (status, _, body) = get(&UreqClient::new(), &url, true).unwrap();
        assert_eq!(
            (status, body.as_slice()),
            (StatusCode::SERVICE_UNAVAILABLE, &b"busy"[..])
        );
        server.join().unwrap();
    }

    #[test]
    fn a_silent_server_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            // Accept and read, then never answer until the client gives up.
            let (mut stream, _) = listener.accept().unwrap();
            read_head(&stream);
            let mut rest = Vec::new();
            stream.read_to_end(&mut rest).ok();
        });
        let request = Request::get(&url)
            .timeout(Duration::from_millis(300))
            .body(AsyncBody::empty())
            .unwrap();
        let started = Instant::now();
        let result = futures::executor::block_on(UreqClient::new().send(request));
        assert!(result.is_err());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        server.join().unwrap();
    }

    #[test]
    fn a_closed_port_is_a_transport_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        drop(listener);
        assert!(get(&UreqClient::new(), &url, true).is_err());
    }
}
