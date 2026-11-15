use std::sync::LazyLock;

use kairo_plugin_api::{Host, LoadResult, Open, OpenRequest, Playlist, PluginError, Source, Track};
use regex::Regex;
use serde_json::Value;

use super::api;
use super::config::{Config, MAX_TRACKS};
use super::model::{self, ApiResponse};
use super::track;

pub(crate) const PREVIEW_PREFIX: &str = "jsprev:";
const SEARCH_PREFIX: &str = "jssearch:";
const SOURCE_PREFIX: &str = "jiosaavn:";
const RECOMMENDATIONS_PREFIX: &str = "jsrec:";
static ITEM_URL: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"^https://www\.jiosaavn\.com/(album|featured|song|s/playlist|artist)/[^/?#]+/([A-Za-z0-9_,\-]+)(?:[?#].*)?$",
    )
    .ok()
});

pub(crate) struct JioSaavn {
    config: Config,
}

impl JioSaavn {
    pub(crate) fn new(host: &Host, config: Value) -> Result<Option<Self>, PluginError> {
        if !Config::enabled(&config)? {
            host.debug("jiosaavn source disabled");
            return Ok(None);
        }
        host.info("loading jiosaavn source");
        let config = Config::parse(config)?;
        Ok(Some(Self { config }))
    }

    pub(crate) fn source() -> Source {
        Source::new(model::SOURCE)
            .prefix(SEARCH_PREFIX.trim_end_matches(':'))
            .prefix(PREVIEW_PREFIX.trim_end_matches(':'))
            .prefix(RECOMMENDATIONS_PREFIX.trim_end_matches(':'))
            .prefix(SOURCE_PREFIX.trim_end_matches(':'))
            .host("www.jiosaavn.com")
    }

    pub(crate) async fn load(
        &self,
        host: &Host,
        identifier: &str,
    ) -> Result<LoadResult, PluginError> {
        let Some(route) = route(identifier) else {
            return Ok(LoadResult::empty());
        };
        match route.kind {
            RouteKind::Search(query) => search(host, &self.config, &query, route.preview).await,
            RouteKind::Recommendations(id) => recommendations(host, &self.config, &id).await,
            RouteKind::Song(id) => song(host, &self.config, &id, route.preview).await,
            RouteKind::Album(id) => {
                collection(host, &self.config, &id, "album", Collection::Album, route.preview).await
            }
            RouteKind::Playlist(id) => {
                collection(host, &self.config, &id, "playlist", Collection::Playlist, route.preview)
                    .await
            }
            RouteKind::Artist(id) => {
                artist(host, &self.config, &id, self.config.artist_top_tracks, route.preview).await
            }
        }
    }

    pub(crate) async fn open(
        &self,
        host: &Host,
        request: &OpenRequest,
    ) -> Result<Open, PluginError> {
        track::open(host, &self.config, request).await
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Route {
    kind: RouteKind,
    preview: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum RouteKind {
    Search(String),
    Recommendations(String),
    Song(String),
    Album(String),
    Playlist(String),
    Artist(String),
}

fn route(identifier: &str) -> Option<Route> {
    if let Some(inner) = identifier.strip_prefix(PREVIEW_PREFIX) {
        let mut route = route_inner(inner)?;
        route.preview = true;
        return Some(route);
    }
    route_inner(identifier)
}

fn route_inner(identifier: &str) -> Option<Route> {
    for prefix in [SEARCH_PREFIX, SOURCE_PREFIX] {
        if let Some(query) = identifier.strip_prefix(prefix).filter(|query| !query.is_empty()) {
            return Some(Route { kind: RouteKind::Search(query.to_owned()), preview: false });
        }
    }
    if let Some(id) = identifier.strip_prefix(RECOMMENDATIONS_PREFIX).filter(|id| !id.is_empty()) {
        return Some(Route { kind: RouteKind::Recommendations(id.to_owned()), preview: false });
    }
    let captures = ITEM_URL.as_ref()?.captures(identifier)?;
    let kind = captures.get(1)?.as_str();
    let id = captures.get(2)?.as_str().to_owned();
    let kind = match kind {
        "song" => RouteKind::Song(id),
        "album" => RouteKind::Album(id),
        "artist" => RouteKind::Artist(id),
        "featured" | "s/playlist" => RouteKind::Playlist(id),
        _ => return None,
    };
    Some(Route { kind, preview: false })
}

async fn search(
    host: &Host,
    config: &Config,
    query: &str,
    preview: bool,
) -> Result<LoadResult, PluginError> {
    let response: ApiResponse = api::get(
        host,
        config.endpoint(),
        "search.getResults",
        &[("includeMetaTags", "1"), ("q", query)],
    )
    .await?;
    let tracks = model::tracks(response.results, false, preview)?;
    if tracks.is_empty() {
        return Ok(LoadResult::empty());
    }
    Ok(LoadResult::Search(tracks))
}

async fn song(
    host: &Host,
    config: &Config,
    id: &str,
    preview: bool,
) -> Result<LoadResult, PluginError> {
    let response = metadata(host, config, id, "song", &[]).await?;
    let Some(track) = response.songs.into_iter().next() else {
        return Ok(LoadResult::empty());
    };
    Ok(LoadResult::Track(model::track(track, true, preview)?))
}

async fn collection(
    host: &Host,
    config: &Config,
    id: &str,
    kind: &str,
    collection: Collection,
    preview: bool,
) -> Result<LoadResult, PluginError> {
    let limit = MAX_TRACKS.to_string();
    let response = if kind == "playlist" {
        metadata(host, config, id, kind, &[("n", limit.as_str())]).await?
    } else {
        metadata(host, config, id, kind, &[]).await?
    };
    let tracks = model::tracks(response.list, true, preview)?;
    if tracks.is_empty() {
        return Ok(LoadResult::empty());
    }
    let fallback = match collection {
        Collection::Album => "JioSaavn Album",
        Collection::Playlist => "JioSaavn Playlist",
    };
    let name = model::collection_name(response.title, fallback)?;
    Ok(LoadResult::Playlist(Playlist { name, selected_track: -1, tracks }))
}

async fn artist(
    host: &Host,
    config: &Config,
    id: &str,
    artist_top_tracks: usize,
    preview: bool,
) -> Result<LoadResult, PluginError> {
    let limit = artist_top_tracks.to_string();
    let response = metadata(host, config, id, "artist", &[("n_song", limit.as_str())]).await?;
    let tracks = model::tracks(response.top_songs, true, preview)?;
    if tracks.is_empty() {
        return Ok(LoadResult::empty());
    }
    let artist = model::collection_name(response.name, "JioSaavn Artist")?;
    Ok(LoadResult::Playlist(Playlist {
        name: format!("{artist}'s Top Tracks"),
        selected_track: -1,
        tracks,
    }))
}

async fn recommendations(
    host: &Host,
    config: &Config,
    id: &str,
) -> Result<LoadResult, PluginError> {
    let entity = format!(r#"["{id}"]"#);
    let station: Value = api::get(
        host,
        config.endpoint(),
        "webradio.createEntityStation",
        &[
            ("api_version", "4"),
            ("ctx", "android"),
            ("entity_id", entity.as_str()),
            ("entity_type", "queue"),
        ],
    )
    .await?;
    let tracks = if let Some(station) = station.get("stationid").and_then(Value::as_str) {
        let response: Value = api::get(
            host,
            config.endpoint(),
            "webradio.getSong",
            &[("api_version", "4"), ("ctx", "android"), ("stationid", station), ("k", "20")],
        )
        .await?;
        model::tracks_from_values(response, true)?
    } else {
        artist_recommendations(host, config, id).await?
    };
    if tracks.is_empty() {
        return Ok(LoadResult::empty());
    }
    Ok(LoadResult::Playlist(Playlist {
        name: String::from("JioSaavn Recommendations"),
        selected_track: -1,
        tracks,
    }))
}

async fn artist_recommendations(
    host: &Host,
    config: &Config,
    id: &str,
) -> Result<Vec<Track>, PluginError> {
    let seed: Value = api::get(host, config.endpoint(), "song.getDetails", &[("pids", id)]).await?;
    let artist_ids = seed
        .get(id)
        .and_then(|track| track.get("primary_artists_id"))
        .and_then(Value::as_str)
        .ok_or_else(|| model::missing("recommendation artist ids"))?;
    let response: Value = api::get(
        host,
        config.endpoint(),
        "search.artistOtherTopSongs",
        &[
            ("api_version", "4"),
            ("ctx", "wap6dot0"),
            ("artist_ids", artist_ids),
            ("song_id", id),
            ("language", "unknown"),
        ],
    )
    .await?;
    model::tracks_from_values(response, false)
}

async fn metadata(
    host: &Host,
    config: &Config,
    id: &str,
    kind: &str,
    extra: &[(&str, &str)],
) -> Result<ApiResponse, PluginError> {
    let mut parameters =
        vec![("api_version", "4"), ("ctx", "web6dot0"), ("token", id), ("type", kind)];
    parameters.extend_from_slice(extra);
    api::get(host, config.endpoint(), "webapi.get", &parameters).await
}

#[derive(Debug, Clone, Copy)]
enum Collection {
    Album,
    Playlist,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_cover_prefixes_previews_and_urls() {
        assert_eq!(
            route("jssearch:apna bana"),
            Some(Route { kind: RouteKind::Search("apna bana".into()), preview: false })
        );
        assert_eq!(
            route("jiosaavn:apna bana"),
            Some(Route { kind: RouteKind::Search("apna bana".into()), preview: false })
        );
        assert_eq!(
            route("jsprev:jssearch:apna"),
            Some(Route { kind: RouteKind::Search("apna".into()), preview: true })
        );
        assert_eq!(
            route("jsrec:song-1"),
            Some(Route { kind: RouteKind::Recommendations("song-1".into()), preview: false })
        );
        assert_eq!(
            route("https://www.jiosaavn.com/song/apna-bana-le/ATIfejZ9bWw"),
            Some(Route { kind: RouteKind::Song("ATIfejZ9bWw".into()), preview: false })
        );
        assert_eq!(
            route("https://www.jiosaavn.com/album/bhediya/wSM2AOubajk?x=1"),
            Some(Route { kind: RouteKind::Album("wSM2AOubajk".into()), preview: false })
        );
        assert_eq!(
            route("https://www.jiosaavn.com/s/playlist/demo/list-id"),
            Some(Route { kind: RouteKind::Playlist("list-id".into()), preview: false })
        );
        assert_eq!(
            route("jsprev:https://www.jiosaavn.com/artist/demo/artist-id"),
            Some(Route { kind: RouteKind::Artist("artist-id".into()), preview: true })
        );
    }

    #[test]
    fn routes_reject_foreign_hosts_and_bad_ids() {
        assert!(route("https://example.test/song/apna/id").is_none());
        assert!(route("http://www.jiosaavn.com/song/apna/id").is_none());
        assert!(route("https://jiosaavn.com/song/apna/id").is_none());
        assert!(route("https://www.jiosaavn.com/song/apna/bad id").is_none());
        assert!(route("https://www.jiosaavn.com/unknown/apna/id").is_none());
        assert!(route("not a route").is_none());
    }

    #[test]
    fn the_manifest_names_the_expected_claims() {
        let source = JioSaavn::source();
        assert_eq!(source.name, "jiosaavn");
        assert_eq!(source.prefixes, ["jssearch", "jsprev", "jsrec", "jiosaavn"]);
        assert_eq!(source.hosts, ["www.jiosaavn.com"]);
    }
}
