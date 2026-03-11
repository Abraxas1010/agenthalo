use std::time::Duration;

pub fn agent_for_url_with_timeout(_url: &str, timeout: Duration) -> Result<ureq::Agent, String> {
    Ok(ureq::Agent::config_builder()
        .proxy(None)
        .timeout_global(Some(timeout))
        .build()
        .into())
}

pub fn get_with_timeout(
    url: &str,
    timeout: Duration,
) -> Result<ureq::RequestBuilder<ureq::typestate::WithoutBody>, String> {
    Ok(agent_for_url_with_timeout(url, timeout)?.get(url))
}

pub fn post_with_timeout(
    url: &str,
    timeout: Duration,
) -> Result<ureq::RequestBuilder<ureq::typestate::WithBody>, String> {
    Ok(agent_for_url_with_timeout(url, timeout)?.post(url))
}
