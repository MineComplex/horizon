//! `config/paper-global.yml`, as Paper and Leaf keep it: only `proxies.velocity`
//! is read (`enabled`, `secret`), the setting that puts the server behind a
//! Velocity proxy. A missing file is written with Paper's defaults (off).
use std::{fs, io, path::Path};

/// `proxies.velocity`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VelocitySettings {
    pub enabled: bool,
    /// Paper's `online-mode`: the proxy authenticated the player (kept for the
    /// file; this server asks nobody).
    pub online_mode: bool,
    pub secret: String,
}

const DEFAULT: &str = "\
# This is the global configuration file for Paper.
# Horizon reads proxies.velocity from it: set enabled to true and the secret
# to the proxy's forwarding secret to put this server behind Velocity.
_version: 31
proxies:
  velocity:
    enabled: false
    online-mode: true
    secret: ''
";

/// Read `path`, writing Paper's default file first when it is missing.
pub fn load(path: &Path) -> io::Result<VelocitySettings> {
    if !path.exists() {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, DEFAULT)?;
    }
    Ok(parse(&fs::read_to_string(path)?))
}

/// The `proxies.velocity` keys of a YAML document (block mappings only, which
/// is all Paper writes there).
pub fn parse(text: &str) -> VelocitySettings {
    let mut out = VelocitySettings {
        online_mode: true,
        ..VelocitySettings::default()
    };
    // The open mappings: (indent, key).
    let mut path: Vec<(usize, String)> = Vec::new();
    for line in text.lines() {
        let content = line.split(" #").next().unwrap_or("");
        if content.trim().is_empty() || content.trim_start().starts_with('#') {
            continue;
        }
        let indent = content.len() - content.trim_start().len();
        let Some((key, value)) = content.trim().split_once(':') else {
            continue;
        };
        while path.last().is_some_and(|(i, _)| *i >= indent) {
            path.pop();
        }
        let (key, value) = (unquote(key.trim()), value.trim());
        let at: Vec<&str> = path.iter().map(|(_, k)| k.as_str()).collect();
        if value.is_empty() {
            path.push((indent, key));
            continue;
        }
        if at == ["proxies", "velocity"] {
            match key.as_str() {
                "enabled" => out.enabled = value == "true",
                "online-mode" => out.online_mode = value == "true",
                "secret" => out.secret = unquote(value),
                _ => {}
            }
        }
    }
    out
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    for q in ['\'', '"'] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            let inner = &s[1..s.len() - 1];
            return if q == '\'' {
                inner.replace("''", "'")
            } else {
                inner.replace("\\\"", "\"")
            };
        }
    }
    s.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_file_is_off() {
        assert_eq!(
            parse(DEFAULT),
            VelocitySettings {
                enabled: false,
                online_mode: true,
                secret: String::new()
            }
        );
    }

    #[test]
    fn papers_layout_is_read() {
        let text = "\
_version: 31
chunk-loading-basic:
  player-max-chunk-generate-rate: -1.0
proxies:
  bungee-cord:
    online-mode: true
  proxy-protocol: false
  velocity:
    enabled: true # behind the proxy
    online-mode: true
    secret: 'it''s a secret'
scoreboards:
  save-empty-scoreboard-teams: true
";
        let v = parse(text);
        assert!(v.enabled);
        assert_eq!(v.secret, "it's a secret");
    }
}
