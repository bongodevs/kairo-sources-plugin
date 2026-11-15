use std::collections::HashMap;
use std::fmt::Write as _;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use des::Des;
use des::cipher::generic_array::GenericArray;
use des::cipher::{BlockDecrypt, KeyInit};
use kairo_plugin_api::{Host, PluginError};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub(crate) type MediaResponse = HashMap<String, MediaDetails>;

#[derive(Debug, Deserialize)]
pub(crate) struct MediaDetails {
    pub(crate) encrypted_media_url: String,
    #[serde(default, rename = "320kbps")]
    high_quality: Value,
}

impl MediaDetails {
    pub(crate) fn high_quality(&self) -> bool {
        match &self.high_quality {
            Value::Bool(value) => *value,
            Value::String(value) => value.eq_ignore_ascii_case("true") || value == "1",
            Value::Number(value) => value.as_u64().is_some_and(|value| value != 0),
            _ => false,
        }
    }
}

pub(crate) async fn get<T>(
    host: &Host,
    endpoint: &str,
    call: &str,
    parameters: &[(&str, &str)],
) -> Result<T, PluginError>
where
    T: DeserializeOwned,
{
    host.get(api_url(endpoint, call, parameters)).json().await
}

fn api_url(endpoint: &str, call: &str, parameters: &[(&str, &str)]) -> String {
    let mut url = String::with_capacity(endpoint.len() + 64);
    url.push_str(endpoint);
    url.push(if endpoint.contains('?') { '&' } else { '?' });
    write!(url, "__call={}&_format=json&_marker=0&cc=in", encode(call))
        .expect("writing to a String cannot fail");
    for (name, value) in parameters {
        url.push('&');
        url.push_str(&encode(name));
        url.push('=');
        url.push_str(&encode(value));
    }
    url
}

fn encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => {
                write!(encoded, "%{byte:02X}").expect("writing to a String cannot fail");
            }
        }
    }
    encoded
}

pub(crate) fn decrypt_media_url(encrypted: &str, key: &[u8; 8]) -> Result<String, PluginError> {
    let mut bytes = STANDARD
        .decode(encrypted)
        .map_err(|_| PluginError::common("JioSaavn media URL is not valid Base64"))?;
    if bytes.is_empty() || bytes.len() % 8 != 0 {
        return Err(PluginError::common("JioSaavn media ciphertext is not a whole DES block"));
    }

    let cipher = Des::new(GenericArray::from_slice(key));
    for block in bytes.as_chunks_mut::<8>().0 {
        cipher.decrypt_block(GenericArray::from_mut_slice(block));
    }
    let padding = usize::from(
        bytes
            .last()
            .copied()
            .ok_or_else(|| PluginError::common("JioSaavn media URL has invalid PKCS#7 padding"))?,
    );
    if !(1..=8).contains(&padding)
        || !bytes[bytes.len() - padding..].iter().all(|byte| usize::from(*byte) == padding)
    {
        return Err(PluginError::common("JioSaavn media URL has invalid PKCS#7 padding"));
    }
    bytes.truncate(bytes.len() - padding);
    String::from_utf8(bytes).map_err(|_| PluginError::common("JioSaavn media URL is not UTF-8"))
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use des::Des;
    use des::cipher::generic_array::GenericArray;
    use des::cipher::{BlockEncrypt, KeyInit};

    use super::*;

    fn encrypted(url: &str, key: &[u8; 8]) -> String {
        let mut bytes = url.as_bytes().to_vec();
        let padding = 8 - bytes.len() % 8;
        bytes.extend(std::iter::repeat_n(padding as u8, padding));
        let cipher = Des::new(GenericArray::from_slice(key));
        for block in bytes.as_chunks_mut::<8>().0 {
            cipher.encrypt_block(GenericArray::from_mut_slice(block));
        }
        STANDARD.encode(bytes)
    }

    #[test]
    fn query_values_are_percent_encoded() {
        let url =
            api_url("http://localhost/api.php", "search.getResults", &[("q", "apna bana & le")]);
        assert!(url.contains("__call=search.getResults"));
        assert!(url.contains("q=apna%20bana%20%26%20le"));
    }

    #[test]
    fn an_existing_query_uses_an_ampersand() {
        let url = api_url("http://localhost/api.php?x=1", "song.getDetails", &[]);
        assert!(url.starts_with("http://localhost/api.php?x=1&__call="));
    }

    #[test]
    fn des_urls_round_trip_and_reject_bad_input() {
        let key = *b"12345678";
        let url = "https://media.example.test/song.mp3";
        assert_eq!(decrypt_media_url(&encrypted(url, &key), &key).unwrap(), url);

        assert!(decrypt_media_url("not base64!", &key).is_err());
        assert!(decrypt_media_url(&STANDARD.encode([1, 2, 3]), &key).is_err());
    }
}
