use anyhow::{anyhow, Result};
use bytes::Bytes;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use url::Url;

pub struct OcisClient {
    pub(crate) client: Client,
    base_url: Url,
    pub space_id: String,
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct DrivesResponse {
    value: Vec<Drive>,
}

#[derive(Deserialize)]
struct Drive {
    id: String,
    #[serde(rename = "driveType")]
    drive_type: String,
}

impl OcisClient {
    pub async fn from_credentials(base_url: Url, username: &str, password: &str) -> Result<Self> {
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .build()?;

        let drives_url = base_url.join("/graph/v1.0/me/drives")?;
        let resp = client
            .get(drives_url)
            .basic_auth(username, Some(password))
            .send()
            .await?
            .error_for_status()?;

        let drives: DrivesResponse = resp.json().await?;
        let personal = drives
            .value
            .into_iter()
            .find(|d| d.drive_type == "personal")
            .ok_or_else(|| anyhow!("no personal drive found"))?;

        Ok(Self {
            client,
            base_url,
            space_id: personal.id,
            username: username.to_owned(),
            password: password.to_owned(),
        })
    }

    pub(crate) fn webdav_url(&self, path: &str) -> Result<Url> {
        // Build the URL by pushing path segments so each component is
        // percent-encoded. `Url::join` would treat characters like `?` in a
        // filename (e.g. "测试 file?name.txt") as a query delimiter, sending
        // the PUT to the wrong path — which is exactly what made the down-sync
        // test silently store the file under the wrong name.
        let mut url = self.base_url.clone();
        {
            let mut segments = url
                .path_segments_mut()
                .map_err(|_| anyhow!("base_url cannot be a base"))?;
            segments.push("dav");
            segments.push("spaces");
            segments.push(&self.space_id);
            for component in path.trim_start_matches('/').split('/') {
                segments.push(component);
            }
        }
        Ok(url)
    }

    pub async fn put(&self, path: &str, content: &[u8]) -> Result<()> {
        self.client
            .put(self.webdav_url(path)?)
            .basic_auth(&self.username, Some(&self.password))
            .body(content.to_vec())
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn get(&self, path: &str) -> Result<Bytes> {
        let bytes = self
            .client
            .get(self.webdav_url(path)?)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        Ok(bytes)
    }

    pub async fn exists(&self, path: &str) -> Result<bool> {
        let resp = self
            .client
            .head(self.webdav_url(path)?)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await?;
        Ok(resp.status() == StatusCode::OK)
    }

    pub async fn collection_exists(&self, path: &str) -> Result<bool> {
        let resp = self
            .client
            .request(
                reqwest::Method::from_bytes(b"PROPFIND").unwrap(),
                self.webdav_url(path)?,
            )
            .basic_auth(&self.username, Some(&self.password))
            .header("Depth", "0")
            .send()
            .await?;
        Ok(resp.status().as_u16() == 207)
    }
}
