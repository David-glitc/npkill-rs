use std::time::Duration;

use ureq::Agent;

const REPO: &str = "David-glitc/npkill-rs";

pub struct UpdateInfo {
    pub current: &'static str,
    pub latest: String,
}

pub fn check_for_update() -> Option<UpdateInfo> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");

    let config = Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(3)))
        .build();
    let agent: Agent = config.into();

    let resp = agent
        .get(&url)
        .header("User-Agent", "npkill-rs")
        .header("Accept", "application/vnd.github.v3+json")
        .call();

    let resp = match resp {
        Ok(r) => r,
        Err(_) => return None,
    };

    let body_text: String = match resp.into_body().read_to_string() {
        Ok(v) => v,
        Err(_) => return None,
    };

    let parsed: serde_json::Value = match serde_json::from_str(&body_text) {
        Ok(v) => v,
        Err(_) => return None,
    };

    let tag = parsed["tag_name"].as_str()?;
    let latest = tag.strip_prefix('v').unwrap_or(tag).to_string();
    let current = env!("CARGO_PKG_VERSION");

    if is_newer(current, &latest) {
        Some(UpdateInfo {
            current,
            latest: format!("v{latest}"),
        })
    } else {
        None
    }
}

fn is_newer(current: &str, latest: &str) -> bool {
    fn parse(s: &str) -> Vec<u32> {
        s.split('.')
            .filter_map(|p| p.parse::<u32>().ok())
            .collect()
    }
    let cur = parse(current);
    let lat = parse(latest);
    for (c, l) in cur.iter().zip(lat.iter()) {
        if l > c {
            return true;
        }
        if l < c {
            return false;
        }
    }
    lat.len() > cur.len()
}
