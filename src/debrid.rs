//! Explicit opt-in cloud providers. Tokens stay in memory and are never
//! forwarded to generated download URLs or persisted with queue entries.
//!
//! Phase A (provider port): the providers are reachable and tested, but
//! nothing calls them yet. The UI and queue wiring is a later phase, so
//! the public API is intentionally unused for now.
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The cloud provider a job belongs to.
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    #[default]
    RealDebrid,
    TorBox,
}

impl Provider {
    /// Human-readable provider name.
    pub fn label(self) -> &'static str {
        match self {
            Self::RealDebrid => "Real-Debrid",
            Self::TorBox => "TorBox",
        }
    }

    /// Provider order used by the (later) settings UI.
    pub fn index(self) -> usize {
        match self {
            Self::RealDebrid => 0,
            Self::TorBox => 1,
        }
    }
}

/// What the provider was asked to do.
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    #[default]
    Torrent,
    Hosting,
}

/// A cloud task the provider runs on our behalf.
#[derive(Clone, Serialize, Deserialize)]
pub struct CloudJob {
    pub provider: Provider,
    pub id: String,
    #[serde(default)]
    pub kind: Kind,
}

/// One file inside a cloud task.
#[derive(Clone)]
pub struct CloudFile {
    pub id: u64,
    pub name: String,
    pub bytes: u64,
    pub selected: bool,
    pub link: String,
}

/// The files of a cloud task plus its provider status.
pub struct Listing {
    pub files: Vec<CloudFile>,
    pub status: String,
}

/// A hosting link the provider resolved to a direct download.
///
/// This is a local DTO: the provider module must not depend on the
/// static mirror subsystem, so the link shape is declared here instead
/// of borrowing the mirror's discovery record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedFile {
    pub url: String,
    pub name: String,
    #[serde(default)]
    pub connections: Option<usize>,
}

/// A provider client. The token lives in memory only.
#[derive(Clone)]
pub struct Api {
    provider: Provider,
    token: String,
    client: reqwest::Client,
    #[cfg(test)]
    test_base: Option<String>,
}

impl Api {
    /// Build a client for `provider`. The token is trimmed and kept in
    /// memory; it is never written to disk or to a generated URL.
    pub fn new(provider: Provider, token: String) -> Result<Self> {
        if token.trim().is_empty() {
            return Err(anyhow!("Enter your provider API token first"));
        }
        Ok(Self {
            #[cfg(test)]
            test_base: None,
            provider,
            token: token.trim().into(),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(30))
                .build()?,
        })
    }

    /// The provider endpoint for `path`.
    fn url(&self, path: &str) -> String {
        #[cfg(test)]
        if let Some(base) = &self.test_base {
            return format!("{base}/{path}");
        }
        format!(
            "{}{}",
            match self.provider {
                Provider::RealDebrid => "https://api.real-debrid.com/rest/1.0/",
                Provider::TorBox => "https://api.torbox.app/v1/api/",
            },
            path
        )
    }

    /// Send a request and decode the JSON answer.
    ///
    /// Errors never echo the request URL: the token is part of it.
    async fn json(&self, request: reqwest::RequestBuilder) -> Result<Value> {
        let mut response = request
            .send()
            .await
            .map_err(|_| anyhow!("Provider request failed or timed out"))?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!(
                "Provider returned HTTP {}{}",
                status.as_u16(),
                if status.as_u16() == 429 {
                    "; wait before retrying"
                } else {
                    "; check account access and token"
                }
            ));
        }
        if status == reqwest::StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| anyhow!("Provider response interrupted"))?
        {
            if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
                return Err(anyhow!("Provider response too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|_| anyhow!("Invalid provider response"))?;
        if value.get("success").and_then(Value::as_bool) == Some(false)
            || value.get("error_code").is_some()
        {
            return Err(anyhow!(
                "Provider refused the request; check account limits and permissions"
            ));
        }
        Ok(value)
    }

    /// Attach the bearer token.
    fn auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.bearer_auth(&self.token)
    }

    /// Hand a magnet link to the provider. The provider runs the swarm;
    /// this app has no local torrent engine.
    pub async fn add(&self, magnet: &str) -> Result<CloudJob> {
        let u = url::Url::parse(magnet.trim())?;
        if u.scheme() != "magnet"
            || !u
                .query_pairs()
                .any(|(k, v)| k == "xt" && v.starts_with("urn:bt"))
        {
            return Err(anyhow!("Enter a magnet link"));
        }
        let value = match self.provider {
            Provider::RealDebrid => {
                self.json(
                    self.auth(self.client.post(self.url("torrents/addMagnet")))
                        .form(&[("magnet", magnet)]),
                )
                .await?
            }
            Provider::TorBox => {
                self.json(
                    self.auth(self.client.post(self.url("torrents/createtorrent")))
                        .multipart(
                            reqwest::multipart::Form::new()
                                .text("magnet", magnet.to_string())
                                .text("seed", "1"),
                        ),
                )
                .await?
            }
        };
        let id = match self.provider {
            Provider::RealDebrid => value.get("id"),
            Provider::TorBox => value.pointer("/data/torrent_id"),
        }
        .ok_or_else(|| anyhow!("Provider did not return a cloud task ID"))?;
        let id = id
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| id.to_string());
        if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(anyhow!("Invalid cloud task ID"));
        }
        Ok(CloudJob {
            provider: self.provider,
            id,
            kind: Kind::Torrent,
        })
    }

    /// Upload `.torrent` metadata for the provider to fetch.
    pub async fn torrent_file(&self, name: String, bytes: Vec<u8>) -> Result<CloudJob> {
        if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
            return Err(anyhow!("Choose a nonempty .torrent file under 16 MiB"));
        }
        let value = match self.provider {
            Provider::RealDebrid => {
                self.json(
                    self.auth(self.client.put(self.url("torrents/addTorrent")))
                        .header("Content-Type", "application/x-bittorrent")
                        .body(bytes),
                )
                .await?
            }
            Provider::TorBox => {
                self.json(
                    self.auth(self.client.post(self.url("torrents/createtorrent")))
                        .multipart(
                            reqwest::multipart::Form::new()
                                .part(
                                    "file",
                                    reqwest::multipart::Part::bytes(bytes)
                                        .file_name(name)
                                        .mime_str("application/x-bittorrent")?,
                                )
                                .text("seed", "1"),
                        ),
                )
                .await?
            }
        };
        let id = match self.provider {
            Provider::RealDebrid => value.get("id"),
            Provider::TorBox => value.pointer("/data/torrent_id"),
        }
        .ok_or_else(|| anyhow!("Provider did not return a torrent task ID"))?;
        let id = id
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| id.to_string());
        if !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(anyhow!("Invalid torrent task ID"));
        }
        Ok(CloudJob {
            provider: self.provider,
            id,
            kind: Kind::Torrent,
        })
    }

    /// Hand a hosting link to the provider.
    ///
    /// Real-Debrid resolves supported hosts to direct downloads right
    /// away; TorBox creates a hosting task that is polled like a torrent
    /// task.
    pub async fn hosting(
        &self,
        link: &str,
        password: &str,
    ) -> Result<(Option<CloudJob>, Vec<HostedFile>)> {
        let u = url::Url::parse(link.trim())?;
        if !matches!(u.scheme(), "http" | "https")
            || !u.username().is_empty()
            || u.password().is_some()
        {
            return Err(anyhow!("Enter an HTTP or HTTPS hosting link"));
        }
        match self.provider {
            Provider::RealDebrid => {
                let value = self
                    .json(
                        self.auth(self.client.post(self.url("unrestrict/link")))
                            .form(&[("link", link), ("password", password)]),
                    )
                    .await?;
                let values = if let Some(values) = value.as_array() {
                    values.clone()
                } else {
                    vec![value]
                };
                let mut found = Vec::new();
                for value in values {
                    let text = value["download"]
                        .as_str()
                        .ok_or_else(|| anyhow!("Host did not provide a downloadable file"))?;
                    self.checked_link(text)?;
                    found.push(HostedFile {
                        url: text.into(),
                        name: value["filename"].as_str().unwrap_or("download").into(),
                        connections: value["chunks"]
                            .as_u64()
                            .filter(|n| *n > 0)
                            .map(|n| (n as usize).clamp(1, 16)),
                    });
                }
                Ok((None, found))
            }
            Provider::TorBox => {
                let value = self
                    .json(
                        self.auth(self.client.post(self.url("webdl/createwebdownload")))
                            .form(&[("link", link), ("password", password)]),
                    )
                    .await?;
                let id = value
                    .pointer("/data/webdownload_id")
                    .or_else(|| value.pointer("/data/web_id"))
                    .ok_or_else(|| anyhow!("Provider did not return a hosting task ID"))?;
                let id = id
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| id.to_string());
                if !id.chars().all(|c| c.is_ascii_digit()) {
                    return Err(anyhow!("Invalid hosting task ID"));
                }
                Ok((
                    Some(CloudJob {
                        provider: self.provider,
                        id,
                        kind: Kind::Hosting,
                    }),
                    Vec::new(),
                ))
            }
        }
    }

    /// The provider's current list of supported hosts.
    pub async fn hosters(&self) -> Result<String> {
        let path = if self.provider == Provider::RealDebrid {
            "hosts/domains"
        } else {
            "webdl/hosters"
        };
        let value = self
            .json(self.auth(self.client.get(self.url(path))))
            .await?;
        let value = if self.provider == Provider::TorBox {
            value.get("data").unwrap_or(&value)
        } else {
            &value
        };
        let rows = value
            .as_array()
            .ok_or_else(|| anyhow!("Provider returned an unknown host list format"))?;
        let mut lines = Vec::new();
        for row in rows.iter().take(500) {
            if let Some(domain) = row.as_str() {
                lines.push(domain.to_owned());
                continue;
            }
            let name = row["name"].as_str().unwrap_or("Host");
            let status = if row["status"].as_bool() == Some(true) {
                "available"
            } else {
                "unavailable"
            };
            let domains = row["domains"]
                .as_array()
                .map(|d| {
                    d.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let note = row["note"].as_str().unwrap_or("");
            lines.push(format!(
                "{name} — {status}\n{domains}{}",
                if note.is_empty() {
                    String::new()
                } else {
                    format!("\n{note}")
                }
            ));
        }
        Ok(lines.join("\n\n"))
    }

    /// Accept a provider-generated download address.
    ///
    /// Only HTTPS, no credentials, and never an address that exposes the
    /// API token.
    fn checked_link(&self, text: &str) -> Result<String> {
        let u = url::Url::parse(text).map_err(|_| anyhow!("Invalid cloud download address"))?;
        if u.scheme() != "https"
            || !u.username().is_empty()
            || u.password().is_some()
            || text.contains(&self.token)
        {
            return Err(anyhow!("Provider returned an unsafe download address"));
        }
        Ok(text.into())
    }

    /// Select every file of a torrent job (Real-Debrid only).
    pub async fn select_all(&self, job: &CloudJob) -> Result<()> {
        if self.provider == Provider::RealDebrid && job.kind == Kind::Torrent {
            self.json(
                self.auth(
                    self.client
                        .post(self.url(&format!("torrents/selectFiles/{}", job.id))),
                )
                .form(&[("files", "all")]),
            )
            .await?;
        }
        Ok(())
    }

    /// The current status and file list of a cloud task.
    pub async fn list(&self, job: &CloudJob) -> Result<Listing> {
        let value = match self.provider {
            Provider::RealDebrid => {
                self.json(
                    self.auth(
                        self.client
                            .get(self.url(&format!("torrents/info/{}", job.id))),
                    ),
                )
                .await?
            }
            Provider::TorBox => {
                self.json(
                    self.auth(self.client.get(self.url(if job.kind == Kind::Hosting {
                        "webdl/mylist"
                    } else {
                        "torrents/mylist"
                    })))
                    .query(&[("id", job.id.as_str())]),
                )
                .await?
            }
        };
        parse_listing(self.provider, &value)
    }

    /// The direct download address of one cloud file.
    pub async fn link(&self, job: &CloudJob, file: &CloudFile) -> Result<(String, Option<usize>)> {
        let value = match self.provider {
            Provider::RealDebrid => {
                if file.link.is_empty() {
                    return Err(anyhow!("Cloud file is not ready"));
                }
                self.json(
                    self.auth(self.client.post(self.url("unrestrict/link")))
                        .form(&[("link", file.link.as_str())]),
                )
                .await?
            }
            Provider::TorBox => {
                self.json(
                    self.client
                        .get(self.url(if job.kind == Kind::Hosting {
                            "webdl/requestdl"
                        } else {
                            "torrents/requestdl"
                        }))
                        .query(&[
                            ("token", self.token.clone()),
                            (
                                if job.kind == Kind::Hosting {
                                    "web_id"
                                } else {
                                    "torrent_id"
                                },
                                job.id.clone(),
                            ),
                            ("file_id", file.id.to_string()),
                            ("redirect", "false".into()),
                        ]),
                )
                .await?
            }
        };
        let text = match self.provider {
            Provider::RealDebrid => value.get("download"),
            Provider::TorBox => value.get("data"),
        }
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Cloud file is not ready for download"))?;
        let link = self.checked_link(text)?;
        let connections = value["chunks"]
            .as_u64()
            .filter(|n| *n > 0)
            .map(|n| (n as usize).clamp(1, 16));
        Ok((link, connections))
    }
}

/// Read the file list and status out of a provider answer.
fn parse_listing(provider: Provider, value: &Value) -> Result<Listing> {
    let data = if provider == Provider::TorBox {
        let d = &value["data"];
        if d.is_array() {
            d.get(0).unwrap_or(&Value::Null)
        } else {
            d
        }
    } else {
        value
    };
    let status = if provider == Provider::TorBox {
        data["download_state"]
            .as_str()
            .unwrap_or("Waiting for cloud files")
    } else {
        data["status"].as_str().unwrap_or("Waiting for cloud files")
    }
    .to_string();
    let links = data["links"].as_array();
    let mut at = 0;
    let files = data["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(|f| {
                    let selected =
                        provider == Provider::TorBox || f["selected"].as_u64() == Some(1);
                    let link = if selected && provider == Provider::RealDebrid {
                        let l = links
                            .and_then(|l| l.get(at))
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        at += 1;
                        l
                    } else {
                        String::new()
                    };
                    Some(CloudFile {
                        id: f["id"].as_u64()?,
                        name: f[if provider == Provider::TorBox {
                            "name"
                        } else {
                            "path"
                        }]
                        .as_str()?
                        .to_string(),
                        bytes: f[if provider == Provider::TorBox {
                            "size"
                        } else {
                            "bytes"
                        }]
                        .as_u64()
                        .unwrap_or(0),
                        selected: false,
                        link,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Listing { files, status })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A local provider fixture: records the raw requests and answers
    /// with canned provider payloads. No provider account is contacted.
    async fn mock(
        provider: Provider,
    ) -> (
        Api,
        std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let record = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let record = record.clone();
                tokio::spawn(async move {
                    let mut bytes = Vec::new();
                    let mut b = [0; 1];
                    while bytes.len() < 65536 {
                        if socket.read_exact(&mut b).await.is_err() {
                            return;
                        }
                        bytes.push(b[0]);
                        if bytes.ends_with(b"\r\n\r\n") {
                            break;
                        }
                    }
                    let headers = String::from_utf8_lossy(&bytes).into_owned();
                    let len = headers
                        .lines()
                        .find_map(|l| {
                            l.split_once(':')
                                .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                                .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    let mut body = vec![0; len];
                    if len > 0 {
                        if socket.read_exact(&mut body).await.is_err() {
                            return;
                        }
                    }
                    bytes.extend_from_slice(&body);
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    record.lock().unwrap().push(text.clone());
                    let path = headers.lines().next().unwrap_or("");
                    let payload = if path.contains("unrestrict/link") {
                        r#"{"download":"https://cdn.example.test/file.bin","filename":"file.bin"}"#
                    } else if path.contains("createwebdownload") {
                        r#"{"success":true,"data":{"webdownload_id":42}}"#
                    } else if path.contains("webdl/mylist") {
                        r#"{"success":true,"data":{"download_state":"cached","files":[{"id":3,"name":"file.bin","size":123}]}}"#
                    } else if path.contains("requestdl") {
                        r#"{"success":true,"data":"https://cdn.example.test/file.bin"}"#
                    } else if path.contains("addMagnet") || path.contains("addTorrent") {
                        r#"{"id":"rd123"}"#
                    } else if path.contains("createtorrent") {
                        r#"{"success":true,"data":{"torrent_id":71}}"#
                    } else {
                        r#"{"success":true}"#
                    };
                    let reply=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",payload.len(),payload);
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        let mut api = Api::new(provider, "private-api-token".into()).unwrap();
        api.test_base = Some(base);
        (api, requests, task)
    }

    #[tokio::test]
    async fn hosting_flows_preserve_mega_keys_and_use_correct_provider_endpoints() {
        let (api, requests, task) = mock(Provider::RealDebrid).await;
        let (job, files) = api
            .hosting("https://mega.nz/file/abc#decryption-key", "file-password")
            .await
            .unwrap();
        assert!(job.is_none());
        assert_eq!(files[0].url, "https://cdn.example.test/file.bin");
        let text = requests.lock().unwrap()[0].clone();
        assert!(text.contains("authorization: Bearer private-api-token"));
        assert!(text.contains("%23decryption-key"));
        assert!(text.contains("password=file-password"));
        task.abort();
        let (api, requests, task) = mock(Provider::TorBox).await;
        let (job, files) = api
            .hosting("https://mega.nz/file/abc#key", "")
            .await
            .unwrap();
        assert!(files.is_empty());
        let job = job.unwrap();
        assert!(job.kind == Kind::Hosting);
        let listing = api.list(&job).await.unwrap();
        let (link, _) = api.link(&job, &listing.files[0]).await.unwrap();
        assert_eq!(link, "https://cdn.example.test/file.bin");
        let requests = requests.lock().unwrap();
        assert!(requests[1].starts_with("GET /webdl/mylist?id=42 "));
        assert!(requests[2].contains("web_id=42"));
        assert!(!requests[2].contains("torrent_id="));
        assert!(!link.contains("private-api-token"));
        task.abort();
    }

    #[tokio::test]
    async fn cloud_magnets_are_submitted_to_each_provider_without_local_swarm_code() {
        for provider in [Provider::RealDebrid, Provider::TorBox] {
            let (api, requests, task) = mock(provider).await;
            let job = api
                .add("magnet:?xt=urn:btih:0123456789abcdef")
                .await
                .unwrap();
            assert!(job.kind == Kind::Torrent);
            let request = requests.lock().unwrap()[0].clone();
            assert!(request.contains("authorization: Bearer private-api-token"));
            if provider == Provider::TorBox {
                assert!(request.contains("multipart/form-data"));
                assert!(request.contains("name=\"magnet\""));
            } else {
                assert!(request.contains("magnet=magnet%3A"));
            }
            task.abort();
        }
    }

    #[tokio::test]
    async fn torrent_metadata_upload_uses_raw_put_or_multipart_as_documented() {
        for provider in [Provider::RealDebrid, Provider::TorBox] {
            let (api, requests, task) = mock(provider).await;
            api.torrent_file("example.torrent".into(), b"d4:infodee".to_vec())
                .await
                .unwrap();
            let request = requests.lock().unwrap()[0].clone();
            if provider == Provider::RealDebrid {
                assert!(request.starts_with("PUT /torrents/addTorrent "));
            } else {
                assert!(request.contains("name=\"file\""));
                assert!(request.contains("filename=\"example.torrent\""));
            }
            assert!(request.contains("d4:infodee"));
            task.abort();
        }
    }

    #[test]
    fn generated_downloads_reject_token_leaks_and_plaintext_links() {
        let api = Api::new(Provider::TorBox, "private-token".into()).unwrap();
        assert!(api.checked_link("http://cdn.example.test/file").is_err());
        assert!(api
            .checked_link("https://cdn.example.test/file?token=private-token")
            .is_err());
        assert!(api
            .checked_link("https://user:pass@cdn.example.test/file")
            .is_err());
    }

    #[test]
    fn links_follow_selected_files_only() {
        let j = serde_json::json!({"status":"downloaded","links":["https://cdn/a","https://cdn/c"],"files":[{"id":1,"path":"a","selected":1},{"id":2,"path":"b","selected":0},{"id":3,"path":"c","selected":1}]});
        let l = parse_listing(Provider::RealDebrid, &j).unwrap();
        assert_eq!(l.files[2].link, "https://cdn/c");
        assert!(l.files[1].link.is_empty());
    }

    #[test]
    fn torbox_list_can_be_an_object_or_array() {
        for data in [
            serde_json::json!({"files":[{"id":9,"name":"a","size":42}]}),
            serde_json::json!([{"files":[{"id":9,"name":"a","size":42}]}]),
        ] {
            let l = parse_listing(Provider::TorBox, &serde_json::json!({"data":data})).unwrap();
            assert_eq!(l.files[0].bytes, 42);
        }
    }
}
