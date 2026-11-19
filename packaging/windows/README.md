# Windows package

CI builds a standalone `Sloptify-<ver>-x86_64.exe` (and zip) on `windows-latest`.

Requirements on the user machine:

- [WebView2 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (preinstalled on Windows 11)
- Google Chrome or Chromium for the Spotify Web Player engine (Widevine)

The release workflow publishes the `.exe` / `.zip` on each `v*` tag. An MSI can be added later with WiX (`cargo-wix`) once code signing is available.
