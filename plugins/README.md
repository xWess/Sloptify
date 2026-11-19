# Sloptify plugins

People publish plugins as normal internet packages — not special repos that Sloptify clones.
Sloptify discovers them from a **catalog JSON**, and installs a **zip** into
`~/.local/share/sloptify/plugins/<id>/`.

## Package shape

```
my-plugin/
  manifest.json
  main.js
  (optional assets)
```

`manifest.json` example:

```json
{
  "id": "community.hello",
  "name": "Hello",
  "version": "0.1.0",
  "author": "You",
  "description": "Example plugin",
  "kind": "action",
  "tags": ["example"]
}
```

## Best way to ship on the internet

1. Put the folder in any Git host (GitHub, Codeberg, …).
2. Create a **Release** and attach `plugin.zip`  
   (zip the folder contents so `manifest.json` is at the zip root — not a nested parent dir).
3. Users install with either:
   - **Plugins → Install from URL** (paste the release zip URL, or a GitHub archive URL like `…/archive/refs/heads/main.zip`), or
   - a PR adding your listing to [`catalog.json`](./catalog.json) with `downloadUrl` set to that zip.

The in-app marketplace loads catalog JSON over HTTPS (`SLOPTIFY_PLUGIN_CATALOG`, defaulting to this repo’s `plugins/catalog.json` when published). Listings without `downloadUrl` install the bundled sample from the curated catalog.

## Local / developer install

Copy the folder to `~/.local/share/sloptify/plugins/<id>/`, or use Install for curated sample listings.
