use kairo_plugin_api::PluginError;
use serde::Deserialize;
use serde_json::Value;

pub(crate) const DEFAULT_ENDPOINT: &str = "https://www.jiosaavn.com/api.php";
pub(crate) const MAX_TRACKS: usize = 1_000;
const DEFAULT_ARTIST_TRACKS: usize = 20;

#[derive(Debug)]
pub(crate) struct Config {
    pub(crate) secret_key: [u8; 8],
    endpoint: Option<String>,
    pub(crate) artist_top_tracks: usize,
    pub(crate) mirror: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WireConfig {
    #[serde(default = "default_true", rename = "enabled")]
    _enabled: bool,
    secret_key: String,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default = "default_artist_tracks")]
    artist_top_tracks: usize,
    #[serde(default = "default_true")]
    mirror: bool,
    #[serde(default)]
    algorithm: Option<String>,
    #[serde(default)]
    transformation: Option<String>,
}

impl Config {
    pub(crate) fn enabled(value: &Value) -> Result<bool, PluginError> {
        match value.get("enabled") {
            None => Ok(true),
            Some(Value::Bool(enabled)) => Ok(*enabled),
            Some(_) => Err(PluginError::common("jiosaavn enabled must be a boolean")),
        }
    }

    pub(crate) fn parse(value: Value) -> Result<Self, PluginError> {
        let config: WireConfig = serde_json::from_value(value)
            .map_err(|error| PluginError::common(format!("invalid jiosaavn config: {error}")))?;
        let secret_key = config.secret_key.as_bytes().try_into().map_err(|_| {
            PluginError::common("jiosaavn secretKey must contain exactly 8 UTF-8 bytes")
        })?;
        if config.algorithm.as_deref().is_some_and(|value| value != "DES") {
            return Err(PluginError::common("jiosaavn algorithm must be DES"));
        }
        if config.transformation.as_deref().is_some_and(|value| value != "DES/ECB/PKCS5Padding") {
            return Err(PluginError::common(
                "jiosaavn transformation must be DES/ECB/PKCS5Padding",
            ));
        }
        if let Some(endpoint) = &config.endpoint
            && (!(endpoint.starts_with("http://") || endpoint.starts_with("https://"))
                || endpoint.chars().any(char::is_whitespace))
        {
            return Err(PluginError::common("jiosaavn endpoint must be an http or https URL"));
        }
        Ok(Self {
            secret_key,
            endpoint: config.endpoint,
            artist_top_tracks: config.artist_top_tracks.min(MAX_TRACKS),
            mirror: config.mirror,
        })
    }

    pub(crate) fn endpoint(&self) -> &str {
        self.endpoint.as_deref().unwrap_or(DEFAULT_ENDPOINT)
    }
}

const fn default_artist_tracks() -> usize {
    DEFAULT_ARTIST_TRACKS
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_requires_an_eight_byte_secret() {
        let error = Config::parse(serde_json::json!({ "secretKey": "short" })).unwrap_err();
        assert!(error.message.contains("8 UTF-8 bytes"));

        let config = Config::parse(serde_json::json!({ "secretKey": "12345678" })).unwrap();
        assert_eq!(config.endpoint(), DEFAULT_ENDPOINT);
        assert_eq!(config.artist_top_tracks, DEFAULT_ARTIST_TRACKS);
        assert!(config.mirror);
    }

    #[test]
    fn config_accepts_a_local_endpoint_and_clamps_artist_tracks() {
        let config = Config::parse(serde_json::json!({
            "secretKey": "12345678",
            "endpoint": "http://127.0.0.1:9/api.php",
            "artistTopTracks": 99_999,
            "enabled": true,
            "algorithm": "DES",
            "transformation": "DES/ECB/PKCS5Padding",
            "mirror": false
        }))
        .unwrap();

        assert_eq!(config.endpoint(), "http://127.0.0.1:9/api.php");
        assert_eq!(config.artist_top_tracks, MAX_TRACKS);
        assert!(!config.mirror);
    }

    #[test]
    fn config_rejects_unknown_keys_and_bad_endpoints() {
        assert!(
            Config::parse(serde_json::json!({ "secretKey": "12345678", "extra": true })).is_err()
        );
        let error = Config::parse(serde_json::json!({
            "secretKey": "12345678",
            "endpoint": "ftp://example.test/api.php"
        }))
        .unwrap_err();
        assert!(error.message.contains("http or https"));
    }

    #[test]
    fn disabled_source_does_not_require_a_secret() {
        assert!(!Config::enabled(&serde_json::json!({ "enabled": false })).unwrap());
        assert!(Config::enabled(&serde_json::json!({ "enabled": "no" })).is_err());
    }
}
