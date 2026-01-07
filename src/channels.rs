use anyhow::Result;
use regex::Regex;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Channel {
    pub name: String,
    pub url: String,
}

pub fn parse_m3u(content: &str) -> Result<Vec<Channel>> {
    let mut channels = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut current_name = None;

    let re_extinf = Regex::new(r"#EXTINF:\d+,(.*)").unwrap();

    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some(caps) = re_extinf.captures(line) {
            current_name = Some(caps[1].trim().to_string());
        } else if line.starts_with("rtsp://") {
            if let Some(name) = current_name.take() {
                channels.push(Channel {
                    name,
                    url: line.to_string(),
                });
            }
        }
    }

    Ok(channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_m3u() {
        let data = r#"#EXTM3U
#EXTINF:0,3sat SD
#EXTVLCOPT:network-caching=1000
rtsp://192.168.178.1:554/?avm=1&freq=450&bw=8&msys=dvbc&mtype=256qam&sr=6900&specinv=1&pids=0,16,17,18,20,200,210,220,221,222,231,250
#EXTINF:0,KiKA SD
#EXTVLCOPT:network-caching=1000
rtsp://192.168.178.1:554/?avm=1&freq=450&bw=8&msys=dvbc&mtype=256qam&sr=6900&specinv=1&pids=0,16,17,18,20,300,310,320,321,322,331"#;

        let channels = parse_m3u(data).unwrap();
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "3sat SD");
        assert!(channels[0].url.starts_with("rtsp://"));
        assert_eq!(channels[1].name, "KiKA SD");
    }
}
