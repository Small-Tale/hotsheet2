# Hot Sheet 2 desktop build and macOS signing

HS2-5R706Q adds a macOS Tauri 2 host for the production Kerf web client. The app
bundles its web files, local bridge, Node runtime, Rust server and CLI binaries,
and HS1 migration assets. It does not load Vite at runtime. The bridge listens
only on an ephemeral loopback port, keeps the server credential outside web
content, and exits when the app closes. The standalone Rust server and its work
continue after the window closes.

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
