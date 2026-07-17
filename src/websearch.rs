//! Stage-3 escalation tools: web and GitHub search for best practices, plus a
//! page fetcher. Web search uses DuckDuckGo's HTML endpoint (no key); GitHub
//! search uses the public REST API (no key, or GITHUB_TOKEN for higher limits
//! and code search).

use std::env;

use anyhow::{Result, anyhow, bail};
use regex::Regex;

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("bbarit-agent")
        .build()?)
}

/// Run a read-only blocking web operation on a helper thread while the caller
/// polls the agent cancel flag. Dropping the receiver abandons the in-flight
/// request safely; the helper owns no mutable project state.
fn run_cancellable<T, F>(operation: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    run_cancellable_with(operation, crate::commands::cancel_requested)
}

fn run_cancellable_with<T, F, C>(operation: F, cancelled: C) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
    C: Fn() -> bool,
{
    if cancelled() {
        bail!("web request cancelled");
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(operation());
    });
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(50)) {
            Ok(result) => return result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if cancelled() => {
                bail!("web request cancelled");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow!("web request thread ended unexpectedly"));
            }
        }
    }
}

fn github_token() -> Option<String> {
    env::var("GITHUB_TOKEN")
        .or_else(|_| env::var("GH_TOKEN"))
        .ok()
        .filter(|value| !value.trim().is_empty())
}

/// Web search via DuckDuckGo's HTML endpoint. Returns the top results as
/// title / url / snippet lines.
pub fn web_search(query: &str, limit: usize) -> Result<String> {
    if query.trim().is_empty() {
        bail!("web_search requires a query");
    }
    let query_owned = query.to_string();
    let body = run_cancellable(move || {
        Ok(client()?
            .post("https://html.duckduckgo.com/html/")
            .form(&[("q", query_owned)])
            .send()?
            .error_for_status()?
            .text()?)
    })?;

    let link = Regex::new(r#"(?s)<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)?;
    let snippet = Regex::new(r#"(?s)<a[^>]*class="result__snippet"[^>]*>(.*?)</a>"#)?;
    let snippets: Vec<String> = snippet
        .captures_iter(&body)
        .map(|cap| clean_html(&cap[1]))
        .collect();

    let mut out = Vec::new();
    for (index, cap) in link.captures_iter(&body).take(limit.max(1)).enumerate() {
        let url = decode_ddg_url(&cap[1]);
        let title = clean_html(&cap[2]);
        let mut entry = format!("{}. {}\n   {}", index + 1, title, url);
        if let Some(snip) = snippets.get(index)
            && !snip.is_empty()
        {
            entry.push_str(&format!("\n   {snip}"));
        }
        out.push(entry);
    }
    if out.is_empty() {
        return Ok(format!("No web results for {query}"));
    }
    Ok(out.join("\n\n"))
}

/// GitHub search. `kind` is "repositories" (default, keyless) or "code"
/// (requires GITHUB_TOKEN). Returns the top matches.
pub fn github_search(query: &str, kind: &str, limit: usize) -> Result<String> {
    if query.trim().is_empty() {
        bail!("github_search requires a query");
    }
    let code = kind.eq_ignore_ascii_case("code");
    if code && github_token().is_none() {
        bail!(
            "GitHub code search requires GITHUB_TOKEN. Use kind=repositories, or set GITHUB_TOKEN."
        );
    }
    let path = if code {
        "search/code"
    } else {
        "search/repositories"
    };
    let per_page = limit.clamp(1, 10);
    let mut url = format!(
        "https://api.github.com/{path}?q={}&per_page={per_page}",
        urlencoding::encode(query)
    );
    if !code {
        url.push_str("&sort=stars&order=desc");
    }
    let token = github_token();
    let json: serde_json::Value = run_cancellable(move || {
        let mut request = client()?
            .get(url)
            .header("Accept", "application/vnd.github+json");
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let response = request.send()?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            bail!("GitHub search failed ({status}): {}", body.trim());
        }
        Ok(response.json()?)
    })?;
    let items = json["items"].as_array().cloned().unwrap_or_default();
    if items.is_empty() {
        return Ok(format!("No GitHub {kind} results for {query}"));
    }
    let mut out = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if code {
            let repo = item["repository"]["full_name"].as_str().unwrap_or("?");
            let file = item["path"].as_str().unwrap_or("?");
            let link = item["html_url"].as_str().unwrap_or("");
            out.push(format!("{}. {repo} — {file}\n   {link}", index + 1));
        } else {
            let name = item["full_name"].as_str().unwrap_or("?");
            let stars = item["stargazers_count"].as_u64().unwrap_or(0);
            let desc = item["description"].as_str().unwrap_or("");
            let link = item["html_url"].as_str().unwrap_or("");
            out.push(format!(
                "{}. {name}  ★{stars}\n   {desc}\n   {link}",
                index + 1
            ));
        }
    }
    Ok(out.join("\n\n"))
}

/// Fetch a URL and return its readable text (tags stripped, truncated).
pub fn web_fetch(url: &str, max_chars: usize) -> Result<String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        bail!("web_fetch requires an http(s) URL");
    }
    let url = url.to_string();
    let body =
        run_cancellable(move || Ok(client()?.get(url).send()?.error_for_status()?.text()?))?;
    let text = clean_html(&body);
    if text.chars().count() > max_chars {
        let truncated: String = text.chars().take(max_chars).collect();
        Ok(format!("{truncated}\n\n[truncated]"))
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn stalled_http_body_returns_promptly_when_cancelled() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let release_server = Arc::new(AtomicBool::new(false));
        let release = Arc::clone(&release_server);
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0u8; 2048];
            let _ = socket.read(&mut request);
            // Headers arrive, but the advertised body deliberately stalls.
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n")
                .unwrap();
            while !release.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let _ = socket.write_all(b"late");
        });

        let cancelled = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&cancelled);
        let trigger = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            signal.store(true, Ordering::Relaxed);
        });
        let started = std::time::Instant::now();
        let result = run_cancellable_with(
            move || Ok(client()?.get(format!("http://{address}")).send()?.text()?),
            || cancelled.load(Ordering::Relaxed),
        );
        release_server.store(true, Ordering::Relaxed);
        trigger.join().unwrap();
        server.join().unwrap();
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "cancel took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn completed_web_operation_returns_its_value() {
        let value = run_cancellable_with(|| Ok::<_, anyhow::Error>("done"), || false).unwrap();
        assert_eq!(value, "done");
    }

    #[test]
    fn pre_cancelled_web_operation_does_not_start() {
        let ran = Arc::new(AtomicBool::new(false));
        let marker = Arc::clone(&ran);
        let result = run_cancellable_with(
            move || {
                marker.store(true, Ordering::Relaxed);
                Ok(())
            },
            || true,
        );
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert!(!ran.load(Ordering::Relaxed));
    }
}

/// DuckDuckGo wraps result links as //duckduckgo.com/l/?uddg=<encoded-url>.
fn decode_ddg_url(href: &str) -> String {
    let candidate = href
        .strip_prefix("//")
        .map(|s| s.to_string())
        .unwrap_or_else(|| href.to_string());
    if let Some(index) = candidate.find("uddg=") {
        let rest = &candidate[index + 5..];
        let encoded = rest.split('&').next().unwrap_or(rest);
        if let Ok(decoded) = urlencoding::decode(encoded) {
            return decoded.into_owned();
        }
    }
    if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    }
}

/// Strip script/style blocks and HTML tags, decode a few entities, and collapse
/// whitespace to produce readable text.
fn clean_html(input: &str) -> String {
    let without_blocks = Regex::new(r"(?is)<(script|style)[^>]*>.*?</(script|style)>")
        .map(|re| re.replace_all(input, " ").into_owned())
        .unwrap_or_else(|_| input.to_string());
    let without_tags = Regex::new(r"(?s)<[^>]+>")
        .map(|re| re.replace_all(&without_blocks, " ").into_owned())
        .unwrap_or(without_blocks);
    let decoded = without_tags
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}
