# Hot Sheet 2 desktop build and macOS signing

HS2-5R706Q adds a macOS Tauri 2 host for the production Kerf web client. The app
bundles its web files, local bridge, Node runtime, Rust server and CLI binaries,
and HS1 migration assets. It does not load Vite at runtime. The bridge listens
only on an ephemeral loopback port, keeps the server credential outside web
content, and exits when the app closes. The standalone Rust server and its work
continue after the window closes.

Desktop projects and device preferences survive changes to the ephemeral bridge
port (HS2-RKZKX7). Before the client starts, the release shell restores Hot Sheet's
localStorage keys from private per-key records in the native app-data directory
under `browser-preferences-v1`, then saves subsequent changes through its local
bridge. An explicit `HOTSHEET_DESKTOP_STORAGE_DIRECTORY` overrides that directory
for isolated native verification; use an absolute path and an isolated
`HOTSHEET_HOME` with a synthetic application identifier. Separate app-data directories remain isolated; simultaneous bridges save
unrelated keys independently. Ordinary browser and development sessions retain
their existing origin-scoped storage. Credentials remain owned by the server.

Values visible in an older origin are imported when no desktop record exists.
WebKit origins abandoned by earlier random-port launches cannot be enumerated
from JavaScript, so previously forgotten projects may need to be reopened once.
Staged ticket-composer and Not working attachment bytes also survive the port change
(HS2-791W19). The desktop client sends them to the bridge, which stores each one as a single
private `<sha256 key>.draft` record under `draft-files-v1` in the same directory: a 4-byte
metadata length, the metadata JSON, then the bytes. A save writes a temporary and renames it
over the record, so a reader gets the complete old record or the complete new one, never old
metadata with new bytes. The directory is 0700 and records are 0600. Saves must declare a
`Content-Length` (browsers always do for a File) and are refused with 411 otherwise, so every
accepted save joins its key's queue on arrival and a later removal cannot be overtaken. Uploads
keep the server's 100 MiB attachment limit. Keys stay scoped by draft kind, project, and draft
file id; removing, discarding, or submitting a draft deletes its record.

A failed recovery copy never silently drops a staged file. If a save fails (for example a full
disk), the file stays attached with its bytes in memory, the composer or Not working dialog
shows a recovery warning that names the affected files, and the next change retries the save.
The warning clears only when every current recovery failure is resolved. If one copy is missing or cannot be read at relaunch, the others restore, the dialog says how many
could not, and their removable references stay in the draft for the next reopen. Create and
Submit remain disabled, including direct form submissions, until every unsaved or unrestored
file recovers or is explicitly removed. Cancel discards the current draft. Successful submission
cleans only the submitted origin references; delayed staging, restore, close-animation or
submission responses cannot overwrite a later project draft or Not working target. Ordinary browser sessions keep these bytes in origin-scoped IndexedDB with
the same per-file failure handling. Files staged by an earlier build in a now-abandoned
origin's IndexedDB cannot be recovered.
Small close-time writes use ordered keepalive retries; large or aggregate writes
beyond the browser keepalive budget need a native flush handshake (HS2-E0H7S0).
`npm run desktop:storage:test` checks the rebuilt production client and bridge
across fresh origins and reloads in Chromium and WebKit using synthetic projects and
synthetic draft attachment bytes.

## Build locally

Install Xcode command line tools, Rust, and Node 22. From the repository root,
install the web and migrator dependencies:

```sh
cd clients/web && npm ci
cd ../../migrator && npm ci
cd ../clients/web && npm run desktop:build -- --no-sign
```

The unsigned app is at
`clients/web/src-tauri/target/release/bundle/macos/Hot Sheet 2.app` from the
repository root. `npm run desktop:dev` starts the Vite client for iteration;
`npm run desktop:build` packages the production client. The build copies the
current machine's Node executable and release Rust binaries into Tauri sidecars,
so build separately for Apple Silicon and Intel. Do not commit the generated
`src-tauri/binaries/`, `src-tauri/resources/`, or `src-tauri/target/` contents.
The Node sidecar must be a standalone executable; the build rejects a Node
installation that links to package-manager libraries. The 1Password shortcut
selects Node from the configured login shell. For a direct build, set
`HOTSHEET_DESKTOP_NODE` to a standalone Node executable when npm uses another
installation. The signed Node sidecar uses the macOS JIT entitlement in
`src-tauri/Entitlements.plist` so V8 can start under the hardened runtime.
The staged migrator contains its installed production packages only; development
tools such as esbuild, Rollup, and fsevents must not enter the signed app.

If the signing variables are `op://` references in the repository-root `.env`,
install and sign in to the 1Password CLI, then run `npm run op:desktop:build`
from `clients/web`. The shortcut resolves those references only for the build
process and invokes `desktop:build`. `APPLE_API_KEY_PATH` must resolve to a
filesystem path to a readable `.p8` key, not the key's contents.

An unsigned local app can trigger a macOS Keychain approval for the server's
credential master when opening a project with a ticket store. Use a development
signing identity for repeatable local verification, and use Developer ID signing
before distributing the app. The first approved read may migrate legacy items;
subsequent launches should use the same signing identity.

## Sign for direct download

1. In Keychain Access, create a certificate signing request. In your Apple
   Developer account, create a **Developer ID Application** certificate under
   Certificates, IDs & Profiles, then download and install its `.cer` in the
   login keychain. Apple limits creation of this certificate to the account
   holder. Confirm the installed identity with
   `security find-identity -v -p codesigning`.
2. In App Store Connect, open **Users and Access → Integrations**, create a team
   API key with Developer access, and download its `.p8` private key. Keep it
   outside this repository and restrict its file permissions. Record its Key ID
   and the Issuer ID shown on that page.
3. In the build shell, set `APPLE_SIGNING_IDENTITY` to the installed Developer ID
   Application identity, `APPLE_API_KEY` to the Key ID, `APPLE_API_ISSUER` to the
   Issuer ID, and `APPLE_API_KEY_PATH` to the private key file's local path.
   These values belong in the shell or a secure credential manager, never in a
   committed file or a command argument. Run `npm run desktop:build` from
   `clients/web`. Tauri signs the app and bundled sidecars, submits it for
   notarization, and staples the result when Apple accepts it.
4. From `clients/web`, verify the app, notarization ticket, and Gatekeeper
   assessment. Test a fresh download on another Mac before publishing it.

   ```sh
   codesign --verify --deep --strict --verbose=2 "src-tauri/target/release/bundle/macos/Hot Sheet 2.app"
   xcrun stapler validate "src-tauri/target/release/bundle/macos/Hot Sheet 2.app"
   spctl --assess --type execute --verbose "src-tauri/target/release/bundle/macos/Hot Sheet 2.app"
   ```

The Tauri identifier is `com.smalltale.hotsheet2`. Signing and notarization
require an active paid Apple Developer Program membership. This repository
does not store the certificate, API key, or notarization credentials.

See the [Tauri macOS signing guide](https://v2.tauri.app/distribute/sign/macos/)
and [Apple's Developer ID certificate guide](https://developer.apple.com/help/account/certificates/create-developer-id-certificates/)
for the current account screens and credential requirements.

### Unattended backend smoke tests (HS2-SFVWN5)

After staging the desktop resources, `npm run desktop:test` checks the staged bridge
and runtime using capability-verified synthetic debug server/CLI backends. It does
not open the operating-system credential store. `npm run desktop:test:native` is an
explicit, optional native integration test of the staged release backends; run it
only when native credential access is available. Production packaging retains
native credentials and never includes the synthetic backend.
