#![forbid(unsafe_code)]

mod jiosaavn;

use kairo_plugin_api::{
    Host, LoadRequest, LoadResult, Manifest, Open, OpenRequest, Plugin, PluginError, export_plugin,
};
use serde::Deserialize;
use serde_json::Value;

const PLUGIN_NAME: &str = "sources";

#[derive(Default)]
struct SourcesPlugin {
    jiosaavn: Option<jiosaavn::JioSaavn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    jiosaavn: Option<Value>,
}

impl Plugin for SourcesPlugin {
    fn init(&mut self, host: &Host, config: Value) -> Result<Manifest, PluginError> {
        let config: Config = serde_json::from_value(config)
            .map_err(|error| PluginError::common(format!("invalid sources config: {error}")))?;
        self.jiosaavn = match config.jiosaavn {
            Some(config) => jiosaavn::JioSaavn::new(host, config)?,
            None => None,
        };
        let mut manifest = Manifest::new(PLUGIN_NAME, env!("CARGO_PKG_VERSION"));
        if self.jiosaavn.is_some() {
            manifest = manifest.source(jiosaavn::JioSaavn::source());
        }
        host.info("All sources plugin ready");
        Ok(manifest)
    }

    async fn load(&self, host: &Host, request: LoadRequest) -> Result<LoadResult, PluginError> {
        match (request.source.as_str(), &self.jiosaavn) {
            (jiosaavn::SOURCE, Some(source)) => source.load(host, &request.identifier).await,
            _ => Err(unknown_source(&request.source)),
        }
    }

    async fn open(&self, host: &Host, request: OpenRequest) -> Result<Open, PluginError> {
        match (request.source.as_str(), &self.jiosaavn) {
            (jiosaavn::SOURCE, Some(source)) => source.open(host, &request).await,
            _ => Err(unknown_source(&request.source)),
        }
    }
}

fn unknown_source(source: &str) -> PluginError {
    PluginError::common(format!("sources plugin does not provide {source:?}"))
}

export_plugin!(SourcesPlugin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_enables_no_sources() {
        let config: Config = serde_json::from_value(serde_json::json!({})).expect("config");
        assert!(config.jiosaavn.is_none());
    }

    #[test]
    fn unknown_sources_are_reported() {
        assert!(unknown_source("missing").message.contains("missing"));
    }
}
