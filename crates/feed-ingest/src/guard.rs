//! Invariant И1 in code: the program never requests a page on iherb.com.
//! Feed URLs, and every redirect hop, pass through [`check_feed_url`].

use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuardError {
    #[error("feed URL is not a valid URL")]
    Invalid,
    #[error("feed URL must use https")]
    NotHttps,
    #[error("refusing to request {0}: the program never reads iherb.com (invariant И1)")]
    StoreHost(String),
}

/// True for `iherb.com` and any of its subdomains.
pub fn is_store_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "iherb.com" || host.ends_with(".iherb.com")
}

pub fn check_feed_url(raw: &str) -> Result<Url, GuardError> {
    let url = Url::parse(raw).map_err(|_| GuardError::Invalid)?;
    if url.scheme() != "https" {
        return Err(GuardError::NotHttps);
    }
    let host = url.host_str().ok_or(GuardError::Invalid)?;
    if is_store_host(host) {
        return Err(GuardError::StoreHost(host.to_owned()));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_the_store_in_every_spelling() {
        for u in [
            "https://www.iherb.com/pr/x/1",
            "https://iherb.com/",
            "https://KR.IHERB.COM/c/magnesium",
            "https://www.iherb.com./feed.csv",
            "https://user@www.iherb.com/feed",
        ] {
            assert!(matches!(check_feed_url(u), Err(GuardError::StoreHost(_))), "{u}");
        }
    }

    #[test]
    fn accepts_network_hosts_and_requires_https() {
        assert!(check_feed_url("https://feeds.network.example/catalog.csv.gz?key=1").is_ok());
        // A tracking domain that merely contains the word is not the store.
        assert!(check_feed_url("https://iherb.sjv.io/feed").is_ok());
        assert!(check_feed_url("https://notiherb.com/feed").is_ok());
        assert_eq!(check_feed_url("http://feeds.network.example/x"), Err(GuardError::NotHttps));
        assert_eq!(check_feed_url("file:///etc/passwd"), Err(GuardError::NotHttps));
    }
}
