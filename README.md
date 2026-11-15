# kairo-sources-plugin

The bundled service-provider library for Kairo. One cdylib advertises only the providers enabled in
`[plugin.config]`; audio never crosses the plugin boundary.

## Build

The crate depends on `kairo-plugin-api` by path, so it is checked out at `plugins/sources` inside a
Kairo checkout:

```bash
git clone https://github.com/appujet/baka-susi.git kairo
git clone https://github.com/appujet/kairo-sources-plugin.git kairo/plugins/sources
cargo build --release --manifest-path kairo/plugins/sources/Cargo.toml
```

Copy the built library to the install path. The `.kai` extension is the same on every OS; the
contents stay native to the machine that built them.

```bash
cp kairo/plugins/sources/target/release/libkairo_plugin_sources.so kairo/plugins/sources.kai
```

## Install

```toml
[[plugin]]
name = "sources"
repo = "appujet/kairo-sources-plugin"
tag = "v1.0.0"

[plugin.config.jiosaavn]
enabled = true
secretKey = "your-own-eight-byte-key"
artistTopTracks = 20
mirror = true
```

The host stages the file under `plugins/.run/` and checks its ELF, PE or Mach-O header before loading.
A build for another OS is rejected.
