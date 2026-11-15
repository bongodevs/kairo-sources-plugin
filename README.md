# kairo-sources-plugin

The bundled service-provider library for Kairo. One cdylib advertises only the providers enabled in
`[plugin.config]`; audio never crosses the plugin boundary.

## Build

The crate fetches `kairo-plugin-api` from the Kairo repository, so it builds on its own:

```bash
git clone https://github.com/bongodevs/kairo-sources-plugin.git
cargo build --release --manifest-path kairo-sources-plugin/Cargo.toml
```

Copy the built library into a Kairo checkout. The `.kai` extension is the same on every OS; the
contents stay native to the machine that built them.

```bash
cp kairo-sources-plugin/target/release/libkairo_plugin_sources.so kairo/plugins/sources.kai
```

## Install

```toml
[[plugin]]
name = "sources"
repo = "bongodevs/kairo-sources-plugin"
tag = "v1.0.0"

[plugin.config.jiosaavn]
enabled = true
secretKey = "your-own-eight-byte-key"
artistTopTracks = 20
mirror = true
```

The host stages the file under `plugins/.run/` and checks its ELF, PE or Mach-O header before loading.
A build for another OS is rejected.

## GitHub Actions

- `.github/workflows/ci.yml` runs fmt, clippy and tests on `main` and pull requests.
- `.github/workflows/release.yml` builds the four release assets on a `v*` tag:
  `sources-{linux,windows,macos}-{x86_64,aarch64}.kai` plus `.sha256` files.

Both workflows read `kairo-plugin-api` from the private `bongodevs/baka-susi` repository. Add a
repository secret named `KAIRO_REPO_TOKEN` with read access to that repository; a fine-grained token
with `Contents: read` is enough.

The dependency currently follows the `chore/cleanup` branch. Switch `Cargo.toml` to `main` after the
plugin API rename is merged.
