//! API クライアント。応答は `payload` の型でそのまま受ける。
//!
//! 手書きの型を挟まないのは、フィールド名の食い違いを実行時ではなくコンパイル時に
//! 出したいため（#647 で手書き `paths.ts` が実際の応答とずれていた）。

use reqwest::{Method, StatusCode, Url};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::config::RuntimeConfig;
use crate::error::{CliError, Result};

pub struct ApiClient {
    http: reqwest::Client,
    base: Url,
    tenant_id: String,
}

impl ApiClient {
    pub fn new(config: RuntimeConfig) -> Result<Self> {
        let base = Url::parse(&config.api_url).map_err(|err| {
            CliError::validation(format!("Invalid api_url {}: {err}", config.api_url))
        })?;
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", config.token).parse().map_err(|_| {
                CliError::validation("The configured token is not a valid header value")
            })?,
        );
        headers.insert(reqwest::header::ACCEPT, "application/json".parse().unwrap());
        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(CliError::from)?;
        Ok(Self {
            http,
            base,
            tenant_id: config.tenant_id,
        })
    }

    pub fn tenant_id(&self) -> &str {
        &self.tenant_id
    }

    pub async fn get<T: DeserializeOwned>(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
    ) -> Result<T> {
        self.send(Method::GET, segments, query, None::<&()>).await
    }

    pub async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        segments: &[&str],
        body: &B,
    ) -> Result<T> {
        self.send(Method::POST, segments, &[], Some(body)).await
    }

    pub async fn put<B: Serialize, T: DeserializeOwned>(
        &self,
        segments: &[&str],
        body: &B,
    ) -> Result<T> {
        self.send(Method::PUT, segments, &[], Some(body)).await
    }

    pub async fn patch<B: Serialize, T: DeserializeOwned>(
        &self,
        segments: &[&str],
        body: &B,
    ) -> Result<T> {
        self.send(Method::PATCH, segments, &[], Some(body)).await
    }

    /// 本文を返さない削除。204 を空応答の失敗と取り違えないよう別扱いにする。
    pub async fn delete(&self, segments: &[&str]) -> Result<()> {
        let url = self.url(segments, &[])?;
        let response = self.http.request(Method::DELETE, url).send().await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status.is_success() {
            Ok(())
        } else {
            Err(CliError::http(status.as_u16(), &body))
        }
    }

    async fn send<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        segments: &[&str],
        query: &[(&str, String)],
        body: Option<&B>,
    ) -> Result<T> {
        let url = self.url(segments, query)?;
        let path = url.path().to_string();
        let mut request = self.http.request(method, url);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(CliError::http(status.as_u16(), &text));
        }
        if status == StatusCode::NO_CONTENT || text.trim().is_empty() {
            return Err(CliError::new("API returned empty response"));
        }
        serde_json::from_str(&text).map_err(|err| {
            // 応答の形が型と合わないのは、たいてい API 側の変更に追いついていない状態。
            // 握り潰すと「なぜか空」に見えるので、どこで何が合わないかを出す
            CliError::new(format!("Cannot parse the API response for {path}: {err}"))
        })
    }

    fn url(&self, segments: &[&str], query: &[(&str, String)]) -> Result<Url> {
        let mut url = self.base.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| CliError::validation("api_url must be an http(s) URL"))?;
            // ベース URL がパス付き（`https://host/api`）でも繋がるよう、末尾の空要素を落とす
            path.pop_if_empty();
            path.extend(segments);
        }
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        Ok(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(api_url: &str) -> ApiClient {
        ApiClient::new(RuntimeConfig {
            api_url: api_url.into(),
            token: "token-1".into(),
            tenant_id: "tenant-1".into(),
        })
        .unwrap()
    }

    #[test]
    fn builds_urls_below_a_base_that_already_has_a_path() {
        let url = client("https://api.invalid/api")
            .url(&["v1", "auth", "me"], &[])
            .unwrap();
        assert_eq!(url.as_str(), "https://api.invalid/api/v1/auth/me");
    }

    #[test]
    fn percent_encodes_path_parameters_so_a_key_cannot_escape_its_segment() {
        let url = client("https://api.invalid")
            .url(
                &["v1", "tenants", "tenant-1", "projects", "../../admin"],
                &[],
            )
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.invalid/v1/tenants/tenant-1/projects/..%2F..%2Fadmin"
        );
    }

    #[test]
    fn appends_query_parameters_in_the_given_order() {
        let url = client("https://api.invalid")
            .url(
                &["v1", "reviews"],
                &[("pr", "618".into()), ("repo", "acme/old".into())],
            )
            .unwrap();
        assert_eq!(url.query(), Some("pr=618&repo=acme%2Fold"));
    }

    #[test]
    fn keeps_an_empty_repo_filter_distinguishable_from_an_absent_one() {
        // 空文字は「連携を張る前のラウンド」を指す。落とすと到達手段が無くなる
        let url = client("https://api.invalid")
            .url(&["v1", "reviews"], &[("repo", String::new())])
            .unwrap();
        assert_eq!(url.query(), Some("repo="));
    }
}
