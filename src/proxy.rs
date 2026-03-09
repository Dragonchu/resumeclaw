//! Proxy configuration for outbound HTTP traffic.
//!
//! Two modes:
//! 1. **Native proxy** — set `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY`, reqwest handles it
//! 2. **External proxy** (proxychains etc.) — set `PROXY_MODE=external`, reqwest won't
//!    touch proxy at all, letting the external tool intercept connections
//!
//! Default: native proxy mode (reads env vars).

use reqwest::ClientBuilder;

/// Call once at startup to configure proxy environment for all HTTP clients
/// (including third-party libraries like serenity that create their own reqwest clients).
pub fn init() {
    if is_external_proxy() {
        // Set NO_PROXY=* so ALL reqwest clients (including ones we don't control,
        // like serenity's internal client) skip proxy auto-detection.
        // Proxychains handles routing at the TCP level instead.
        std::env::set_var("NO_PROXY", "*");
        std::env::set_var("no_proxy", "*");
        // Clear any leftover proxy env vars to be safe.
        for key in ["http_proxy", "HTTP_PROXY", "https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"] {
            std::env::remove_var(key);
        }
        tracing::info!("PROXY_MODE=external: cleared proxy env vars, set NO_PROXY=*");
    }
}

/// Build a reqwest Client with proxy configuration.
pub fn build_client() -> Result<reqwest::Client, reqwest::Error> {
    apply_proxy(reqwest::Client::builder()).build()
}

/// Apply proxy settings to a ClientBuilder.
///
/// - `PROXY_MODE=external` → disables reqwest proxy entirely (for proxychains)
/// - Otherwise → reads `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY` natively
pub fn apply_proxy(builder: ClientBuilder) -> ClientBuilder {
    if is_external_proxy() {
        // Disable reqwest's automatic proxy detection.
        // Let proxychains (or other external tool) handle TCP connections.
        tracing::debug!("PROXY_MODE=external, reqwest proxy disabled");
        return builder.no_proxy();
    }

    // Native mode: let reqwest read HTTP_PROXY/HTTPS_PROXY/ALL_PROXY automatically.
    // reqwest::ClientBuilder does this by default, no manual work needed.
    builder
}

fn is_external_proxy() -> bool {
    std::env::var("PROXY_MODE")
        .ok()
        .is_some_and(|v| v.eq_ignore_ascii_case("external"))
}
