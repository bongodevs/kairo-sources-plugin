use kairo_plugin_api::{Headers, Host, Open, OpenRequest, PluginError, Track, TrackInfo};
use serde_json::Value;

use super::api::{self, MediaResponse};
use super::config::Config;
use super::model;

pub(crate) async fn open(
    host: &Host,
    config: &Config,
    request: &OpenRequest,
) -> Result<Open, PluginError> {
    let info = &request.track.info;
    if preview(&request.track)? {
        let url = preview_url(&request.track)?;
        return Ok(url_open(url, info.length));
    }
    if config.mirror && !request.direct {
        return Ok(mirror_request(info));
    }
    let url = full_media_url(host, config, &info.identifier).await?;
    Ok(url_open(url, info.length))
}

fn url_open(url: String, length: u64) -> Open {
    Open::Url {
        url,
        headers: Headers::new(),
        seekable: None,
        length_ms: Some(length),
        content_type: None,
    }
}

fn preview(track: &Track) -> Result<bool, PluginError> {
    match track.plugin_info.get("isPreview") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(PluginError::suspicious("JioSaavn track has a non-boolean isPreview field")),
    }
}

fn preview_url(track: &Track) -> Result<String, PluginError> {
    track
        .plugin_info
        .get("previewUrl")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| model::missing("preview URL"))
}

fn mirror_request(info: &TrackInfo) -> Open {
    Open::Mirror {
        isrc: info.isrc.clone(),
        title: info.title.clone(),
        author: info.author.clone(),
        length_ms: (!info.is_stream).then_some(info.length),
    }
}

async fn full_media_url(
    host: &Host,
    config: &Config,
    identifier: &str,
) -> Result<String, PluginError> {
    let response: MediaResponse =
        api::get(host, config.endpoint(), "song.getDetails", &[("pids", identifier)]).await?;
    let details = response.get(identifier).ok_or_else(|| model::missing("media details"))?;
    let mut media_url = api::decrypt_media_url(&details.encrypted_media_url, &config.secret_key)?;
    if details.high_quality() {
        media_url = media_url.replace("_96.mp4", "_320.mp4");
    }
    Ok(media_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn info() -> TrackInfo {
        let mut info =
            TrackInfo::new("song-1", "Apna Bana Le", "Arijit Singh", model::SOURCE, 240_000);
        info.isrc = Some("IN-Z03-22-01234".into());
        info
    }

    fn track(plugin_info: Map<String, Value>) -> Track {
        Track::new(info()).plugin_info(plugin_info)
    }

    #[test]
    fn mirror_requests_keep_the_track_identity() {
        let Open::Mirror { isrc, title, author, length_ms } = mirror_request(&info()) else {
            panic!("expected a mirror");
        };
        assert_eq!(isrc.as_deref(), Some("IN-Z03-22-01234"));
        assert_eq!(title, "Apna Bana Le");
        assert_eq!(author, "Arijit Singh");
        assert_eq!(length_ms, Some(240_000));
    }

    #[test]
    fn preview_tracks_use_their_saved_url() {
        let mut plugin_info = Map::new();
        plugin_info.insert("isPreview".into(), Value::Bool(true));
        plugin_info.insert(
            "previewUrl".into(),
            Value::String("https://media.example.test/preview.mp3".into()),
        );
        let track = track(plugin_info);
        assert!(preview(&track).unwrap());
        assert_eq!(preview_url(&track).unwrap(), "https://media.example.test/preview.mp3");
    }

    #[test]
    fn a_missing_or_malformed_preview_is_an_error() {
        let mut plugin_info = Map::new();
        plugin_info.insert("isPreview".into(), Value::Bool(true));
        assert!(preview_url(&track(plugin_info)).unwrap_err().message.contains("preview URL"));

        let mut plugin_info = Map::new();
        plugin_info.insert("isPreview".into(), Value::String("yes".into()));
        assert!(preview(&track(plugin_info)).unwrap_err().message.contains("isPreview"));
    }

    #[test]
    fn url_opens_carry_the_track_length() {
        let open = url_open("https://media.example.test/song.mp3".into(), 240_000);
        let Open::Url { length_ms, .. } = open else {
            panic!("expected a URL");
        };
        assert_eq!(length_ms, Some(240_000));
    }
}
