use std::sync::Arc;

use http::{Extensions, HeaderMap, HeaderValue};
use reqwest::header::{ACCEPT, ACCEPT_ENCODING, ACCEPT_LANGUAGE, CONNECTION, REFERER, USER_AGENT};
use reqwest::{cookie::Jar, Client, Error, Request, Response, Url};
use reqwest_middleware::{Middleware, Next};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::utils::ntfy;


pub struct FlaresolverrMiddleware {
    instance: String,
    refresh_client: Client,
    cookie_jar: Arc<Jar>,
    headers: RwLock<HeaderMap>,
}

#[derive(Debug)]
enum FlaresolverrError {
    ConnectionProblem,
    _MissingResponse,
    MIssingUA,
    MissingSolution,
    MissingCookie,
    CookiesNotAnArray,
}

#[derive(Deserialize, Debug)]
struct CookieData {
    name: String,
    value: String,
    domain: String,
    path: String,
    expires: Option<f64>,
    #[serde(default)]
    http_only: bool,
    #[serde(default)]
    secure: bool,
    #[serde(default)]
    _session: bool,
    #[serde(default)]
    same_site: Option<String>,
}

impl FlaresolverrMiddleware {
    async fn list_sessions(&self) {
        let data: Value = json!({
            "cmd": "sessions.list",
        });
        if let Ok(resp) = self
            .refresh_client
            .post(&self.instance)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&data)
            .send()
            .await
        {
            println!("{:?}", resp.json::<Value>().await.unwrap())
        }
    }

    async fn create_session(&self) -> Result<Response, Error> {
        let data: Value = json!({
            "cmd": "sessions.create",
            "session": "forum-stream"
        });
        self.refresh_client
            .post(&self.instance)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&data)
            .send()
            .await
    }

    fn parse_cookie(cookie: &Value) -> (String, Url) {
        let data = serde_json::from_value::<CookieData>(cookie.clone()).unwrap();
        let mut cookie_str = format!("{}={}", data.name, data.value);
        if !data.path.is_empty() {
            cookie_str.push_str(&format!("; Path={}", data.path));
        }
        if !data.domain.is_empty() {
            cookie_str.push_str(&format!("; Domain={}", data.domain));
        }
        if data.secure {
            cookie_str.push_str("; Secure");
        }
        if data.http_only {
            cookie_str.push_str("; HttpOnly");
        }
        if let Some(ref same_site) = data.same_site {
            cookie_str.push_str(&format!("; SameSite={}", same_site));
        }

        if let Some(exp) = data.expires {
            use chrono::{TimeZone, Utc};
            let date = Utc.timestamp_opt(exp as i64, 0).unwrap();
            let cookie_date = date.to_rfc2822(); // e.g. "Wed, 21 Oct 2015 07:28:00 GMT"
            cookie_str.push_str(&format!("; Expires={}", cookie_date));
        }

        let domain_url =
            Url::parse(&format!("https://{}", data.domain.trim_start_matches('.'))).unwrap();
        (cookie_str, domain_url)
    }

    async fn resolve_cloudflare(&self, req: &mut Request) -> Result<(), FlaresolverrError> {
        let data: Value = json!({
          "cmd": "request.get",
          "url": req.url().to_string(),
          "maxTimeout": 60000
        });

        if let Ok(resp) = self
            .refresh_client
            .post(&self.instance)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&data)
            .send()
            .await
        {
            let json: Value = resp.json().await.unwrap();
            let cookies = json
                .get("solution")
                .ok_or(FlaresolverrError::MissingSolution)?
                .get("cookies")
                .ok_or(FlaresolverrError::MissingCookie)?
                .as_array()
                .ok_or(FlaresolverrError::CookiesNotAnArray)?;

            for c in cookies {
                let parsed = Self::parse_cookie(c);
                self.cookie_jar.add_cookie_str(parsed.0.as_str(), &parsed.1);
            }

            let ua = json
                .get("solution")
                .ok_or(FlaresolverrError::MissingSolution)?
                .get("userAgent")
                .ok_or(FlaresolverrError::MIssingUA)?
                .as_str()
                .unwrap();

            {
                let mut headers = self.headers.write().await;
                headers.insert(USER_AGENT, HeaderValue::from_str(&ua).unwrap());
                // Add common browser headers too (best-effort)
                headers.entry(ACCEPT).or_insert(HeaderValue::from_static(
                    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
                ));
                headers
                    .entry(ACCEPT_LANGUAGE)
                    .or_insert(HeaderValue::from_static("en-US,en;q=0.9"));
                headers
                    .entry(ACCEPT_ENCODING)
                    .or_insert(HeaderValue::from_static("gzip, deflate, br"));
                headers
                    .entry(CONNECTION)
                    .or_insert(HeaderValue::from_static("keep-alive"));
                headers
                    .entry(REFERER)
                    .or_insert(HeaderValue::from_static("https://example.com/"));
                headers.insert("Sec-Fetch-Mode", HeaderValue::from_static("navigate"));
                headers.insert("Sec-Fetch-Site", HeaderValue::from_static("none"));
            }
        } else {
            return Err(FlaresolverrError::ConnectionProblem);
        }
        Ok(())
    }

    pub async fn new(
        client: Client,
        cookie_jar: Arc<Jar>,
        proxy_url: String
    ) -> Result<FlaresolverrMiddleware, reqwest::Error> {
        let middleware = FlaresolverrMiddleware {
            instance: proxy_url,
            refresh_client: client,
            cookie_jar,
            headers: RwLock::new(HeaderMap::default()),
        };
        ntfy("Constructed", "forum-stream-errors").await;
        middleware.create_session().await?;
        let _ = middleware.list_sessions().await;
        Ok(middleware)
    }
}

#[async_trait::async_trait]
impl Middleware for FlaresolverrMiddleware {
    async fn handle(
        &self,
        mut req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        // println!("Request started {:?}", req);
        let h = req.headers_mut();
        {
            let real_h = self.headers.read().await;
            for (k, v) in real_h.iter() {
                h.insert(k, v.clone());
            }
        }
        let mut res = next
            .clone()
            .run(req.try_clone().unwrap(), extensions)
            .await?;
        if res.status() == 403 {
            ntfy("Status code 403 :(", "forum-stream-errors").await;
            // println!("{:?}", self.cookie_jar);
            self.resolve_cloudflare(&mut req).await.unwrap();
            let h = req.headers_mut();
            {
                let real_h = self.headers.read().await;
                for (k, v) in real_h.iter() {
                    h.insert(k, v.clone());
                }
            }
            res = next.run(req, extensions).await?;
        }
        // println!("Result: {:?}", res);
        Ok(res)
    }
}
