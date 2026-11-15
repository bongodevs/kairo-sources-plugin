use kairo_plugin_api::{PluginError, Track, TrackInfo};
use serde::Deserialize;
use serde_json::{Map, Value};

pub(crate) const SOURCE: &str = "jiosaavn";
pub(crate) const MAX_TEXT_BYTES: usize = 8 * 1024;
const DEFAULT_LENGTH_SECONDS: u64 = 30;
const PREVIEW_LENGTH_SECONDS: u64 = 30;
const UNKNOWN_TITLE: &str = "Unknown title";
const UNKNOWN_AUTHOR: &str = "Unknown artist";

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ApiResponse {
    #[serde(default)]
    pub(crate) results: Vec<ApiTrack>,
    #[serde(default)]
    pub(crate) songs: Vec<ApiTrack>,
    #[serde(default)]
    pub(crate) list: Vec<ApiTrack>,
    #[serde(default, rename = "topSongs")]
    pub(crate) top_songs: Vec<ApiTrack>,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ApiTrack {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) song: Option<String>,
    #[serde(default)]
    pub(crate) primary_artists: Option<String>,
    #[serde(default)]
    pub(crate) singers: Option<String>,
    #[serde(default)]
    pub(crate) image: Option<String>,
    #[serde(default)]
    pub(crate) duration: Value,
    #[serde(default)]
    pub(crate) perma_url: Option<String>,
    #[serde(default)]
    pub(crate) album: Option<String>,
    #[serde(default)]
    pub(crate) album_url: Option<String>,
    #[serde(default)]
    pub(crate) media_preview_url: Option<String>,
    #[serde(default)]
    pub(crate) vlink: Option<String>,
    #[serde(default)]
    pub(crate) more_info: Option<MoreInfo>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct MoreInfo {
    #[serde(default)]
    pub(crate) duration: Value,
    #[serde(default)]
    pub(crate) music: Option<String>,
    #[serde(default)]
    pub(crate) album: Option<String>,
    #[serde(default)]
    pub(crate) album_url: Option<String>,
    #[serde(default)]
    pub(crate) vlink: Option<String>,
    #[serde(default, rename = "artistMap")]
    pub(crate) artist_map: Option<ArtistMap>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ArtistMap {
    #[serde(default)]
    pub(crate) primary_artists: Vec<Artist>,
    #[serde(default)]
    pub(crate) artists: Vec<Artist>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Artist {
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) perma_url: Option<String>,
    #[serde(default)]
    pub(crate) image: Option<String>,
}

pub(crate) fn missing(field: &'static str) -> PluginError {
    PluginError::common(format!("JioSaavn: response is missing {field}"))
}

pub(crate) fn clean(value: Option<&str>) -> Option<String> {
    value.map(|value| value.replace("&quot;", "\"").replace("&#039;", "'").replace("&amp;", "&"))
}

pub(crate) fn upgrade_artwork(value: String) -> String {
    value.replace("150x150", "500x500")
}

pub(crate) fn bounded_optional(
    value: Option<String>,
    field: &'static str,
) -> Result<Option<String>, PluginError> {
    value.map(|value| bounded(value, field)).transpose()
}

fn bounded_required(value: String, field: &'static str) -> Result<String, PluginError> {
    if value.is_empty() {
        return Err(missing(field));
    }
    bounded(value, field)
}

fn bounded(value: String, field: &'static str) -> Result<String, PluginError> {
    if value.len() > MAX_TEXT_BYTES {
        return Err(PluginError::suspicious(format!(
            "JioSaavn field {field} exceeds {MAX_TEXT_BYTES} bytes"
        )));
    }
    Ok(value)
}

fn number(value: &Value) -> Option<u64> {
    match value {
        Value::Number(value) => value.as_u64(),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

pub(crate) fn tracks(
    values: Vec<ApiTrack>,
    metadata_shape: bool,
    preview: bool,
) -> Result<Vec<Track>, PluginError> {
    values
        .into_iter()
        .take(super::config::MAX_TRACKS)
        .map(|value| track(value, metadata_shape, preview))
        .collect()
}

pub(crate) fn tracks_from_values(
    value: Value,
    nested_song: bool,
) -> Result<Vec<Track>, PluginError> {
    let values = match value {
        Value::Array(values) => values,
        Value::Object(values) => values.into_values().collect(),
        _ => Vec::new(),
    };
    values
        .into_iter()
        .filter_map(|value| if nested_song { value.get("song").cloned() } else { Some(value) })
        .take(super::config::MAX_TRACKS)
        .map(|value| {
            let value: ApiTrack = serde_json::from_value(value).map_err(|error| {
                PluginError::suspicious(format!("invalid JioSaavn track: {error}"))
            })?;
            track(value, true, false)
        })
        .collect()
}

pub(crate) fn track(
    value: ApiTrack,
    metadata_shape: bool,
    preview: bool,
) -> Result<Track, PluginError> {
    let author = bounded(author(&value, metadata_shape), "author")?;
    let plugin_info = extras(&value, metadata_shape, preview)?.into_map(value.perma_url.clone());
    let identifier = bounded_required(value.id, "track id")?;
    let title = bounded(
        clean(if metadata_shape { value.title.as_deref() } else { value.song.as_deref() })
            .unwrap_or_else(|| UNKNOWN_TITLE.to_owned()),
        "title",
    )?;
    let duration = if preview {
        PREVIEW_LENGTH_SECONDS
    } else {
        let value = if metadata_shape {
            value.more_info.as_ref().map(|info| &info.duration)
        } else {
            Some(&value.duration)
        };
        value.and_then(number).unwrap_or(DEFAULT_LENGTH_SECONDS)
    };
    let uri = value.perma_url.map(|value| bounded(value, "track URI")).transpose()?;
    let artwork_url =
        value.image.map(|value| bounded(upgrade_artwork(value), "artwork URL")).transpose()?;
    let info = TrackInfo {
        identifier,
        title,
        author,
        length: duration.saturating_mul(1_000),
        is_stream: false,
        is_seekable: true,
        uri,
        artwork_url,
        isrc: None,
        source_name: String::from(SOURCE),
    };
    Ok(Track::new(info).plugin_info(plugin_info))
}

pub(crate) fn collection_name(
    value: Option<String>,
    fallback: &str,
) -> Result<String, PluginError> {
    bounded(
        value
            .as_deref()
            .and_then(|value| clean(Some(value)))
            .unwrap_or_else(|| fallback.to_owned()),
        "collection title",
    )
}

fn author(value: &ApiTrack, metadata_shape: bool) -> String {
    if !metadata_shape {
        return clean(value.primary_artists.as_deref())
            .filter(|author| !author.is_empty())
            .or_else(|| clean(value.singers.as_deref()))
            .filter(|author| !author.is_empty())
            .unwrap_or_else(|| UNKNOWN_AUTHOR.to_owned());
    }
    let more_info = value.more_info.as_ref();
    let artist_map = more_info.and_then(|info| info.artist_map.as_ref());
    let artists = artist_map
        .map(|map| if map.primary_artists.is_empty() { &map.artists } else { &map.primary_artists })
        .into_iter()
        .flatten()
        .filter_map(|artist| clean(Some(&artist.name)))
        .filter(|artist| !artist.is_empty())
        .collect::<Vec<_>>();
    if !artists.is_empty() {
        return artists.join(", ");
    }
    more_info
        .and_then(|info| clean(info.music.as_deref()))
        .filter(|author| !author.is_empty())
        .unwrap_or_else(|| UNKNOWN_AUTHOR.to_owned())
}

#[derive(Debug, Default)]
struct Extras {
    album_name: Option<String>,
    album_url: Option<String>,
    artist_url: Option<String>,
    artist_artwork_url: Option<String>,
    preview_url: Option<String>,
    is_preview: bool,
}

impl Extras {
    fn into_map(self, save_uri: Option<String>) -> Map<String, Value> {
        let mut info = Map::new();
        insert_optional(&mut info, "save_uri", save_uri);
        insert_optional(&mut info, "albumUrl", self.album_url);
        insert_optional(&mut info, "albumName", self.album_name);
        insert_optional(&mut info, "previewUrl", self.preview_url);
        info.insert(String::from("isPreview"), Value::Bool(self.is_preview));
        insert_optional(&mut info, "artistUrl", self.artist_url);
        insert_optional(&mut info, "artistArtworkUrl", self.artist_artwork_url);
        info.insert(String::from("isLocal"), Value::Bool(false));
        info
    }
}

fn extras(value: &ApiTrack, metadata_shape: bool, preview: bool) -> Result<Extras, PluginError> {
    if !metadata_shape {
        return Ok(Extras {
            album_name: bounded_optional(clean(value.album.as_deref()), "album name")?,
            album_url: bounded_optional(value.album_url.clone(), "album URL")?,
            preview_url: bounded_optional(
                value.media_preview_url.clone().or_else(|| value.vlink.clone()),
                "preview URL",
            )?,
            is_preview: preview,
            ..Extras::default()
        });
    }

    let more_info = value.more_info.as_ref();
    let artist = more_info
        .and_then(|info| info.artist_map.as_ref())
        .and_then(|map| map.primary_artists.first().or_else(|| map.artists.first()));
    Ok(Extras {
        album_name: bounded_optional(
            more_info.and_then(|info| clean(info.album.as_deref())),
            "album name",
        )?,
        album_url: bounded_optional(
            more_info.and_then(|info| info.album_url.clone()),
            "album URL",
        )?,
        artist_url: bounded_optional(
            artist.and_then(|artist| artist.perma_url.clone()),
            "artist URL",
        )?,
        artist_artwork_url: bounded_optional(
            artist.and_then(|artist| artist.image.clone()).map(upgrade_artwork),
            "artist artwork URL",
        )?,
        preview_url: bounded_optional(
            more_info.and_then(|info| info.vlink.clone()).or_else(|| value.vlink.clone()),
            "preview URL",
        )?,
        is_preview: preview,
    })
}

fn insert_optional(info: &mut Map<String, Value>, name: &str, value: Option<String>) {
    info.insert(name.to_owned(), value.map_or(Value::Null, Value::String));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata_track() -> ApiTrack {
        serde_json::from_value(serde_json::json!({
            "id": "song-1",
            "title": "Apna &amp; Bana Le",
            "image": "https://c.saavncdn.com/150x150.jpg",
            "perma_url": "https://www.jiosaavn.com/song/apna/song-1",
            "more_info": {
                "duration": "240",
                "album": "Bhediya",
                "album_url": "https://www.jiosaavn.com/album/bhediya/album-1",
                "vlink": "https://media.example.test/preview.mp3",
                "artistMap": {
                    "primary_artists": [{
                        "name": "Arijit Singh",
                        "perma_url": "https://www.jiosaavn.com/artist/arijit/artist-1",
                        "image": "https://c.saavncdn.com/150x150-artist.jpg"
                    }]
                }
            }
        }))
        .unwrap()
    }

    #[test]
    fn metadata_tracks_use_the_plugin_info_keys() {
        let track = track(metadata_track(), true, false).unwrap();
        assert_eq!(track.info.identifier, "song-1");
        assert_eq!(track.info.title, "Apna & Bana Le");
        assert_eq!(track.info.author, "Arijit Singh");
        assert_eq!(track.info.length, 240_000);
        assert_eq!(track.info.source_name, SOURCE);
        assert_eq!(track.info.artwork_url.as_deref(), Some("https://c.saavncdn.com/500x500.jpg"));
        assert_eq!(track.plugin_info["albumName"], "Bhediya");
        assert_eq!(
            track.plugin_info["artistArtworkUrl"],
            "https://c.saavncdn.com/500x500-artist.jpg"
        );
        assert_eq!(track.plugin_info["isPreview"], false);
        assert_eq!(track.plugin_info["isLocal"], false);
    }

    #[test]
    fn preview_tracks_have_a_short_length_and_the_flag() {
        let track = track(metadata_track(), true, true).unwrap();
        assert_eq!(track.info.length, 30_000);
        assert_eq!(track.plugin_info["isPreview"], true);
        assert_eq!(track.plugin_info["previewUrl"], "https://media.example.test/preview.mp3");
    }

    #[test]
    fn search_tracks_use_the_flat_api_shape() {
        let value: ApiTrack = serde_json::from_value(serde_json::json!({
            "id": "song-2",
            "song": "Kesariya",
            "primary_artists": "Arijit Singh, Pritam",
            "singers": "Arijit Singh",
            "duration": 268,
            "image": "https://c.saavncdn.com/150x150-2.jpg"
        }))
        .unwrap();
        let track = track(value, false, false).unwrap();
        assert_eq!(track.info.title, "Kesariya");
        assert_eq!(track.info.author, "Arijit Singh, Pritam");
        assert_eq!(track.info.length, 268_000);
    }

    #[test]
    fn missing_ids_and_oversized_fields_are_rejected() {
        let mut missing = metadata_track();
        missing.id.clear();
        assert!(track(missing, true, false).unwrap_err().message.contains("track id"));

        let mut long = metadata_track();
        long.title = Some("x".repeat(MAX_TEXT_BYTES + 1));
        assert!(track(long, true, false).unwrap_err().message.contains("title"));
    }
}
