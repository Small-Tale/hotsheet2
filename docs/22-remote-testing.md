# 22. Remote testing

How a Hot Sheet 2 user tests the software their project builds when they are not sitting
at the build machine: from a phone, from another laptop, or against a cloud build VM
running a platform they do not own (for example, a Windows user testing a macOS app).

Status: **Design only.** This doc records research and integration decisions; nothing
here is built yet. Origin: investigation `HS2-T8WKMG`; tier 4 research `HS2-V59H7C`
(2026-10). Vendor facts below were checked against vendor documentation in October 2026
and change often — re-verify before implementing a follow-up.

## 22.1 Strategy tiers

No single tool covers every case. `HS2-T8WKMG` settled on tiers matched to what is being
tested; most day-to-day checks need no live remote control at all.

| Tier | Solves                                                        | Decision                                    | Ticket                 |
| ---- | ------------------------------------------------------------- | ------------------------------------------- | ---------------------- |
| 1    | Web apps / dev servers on the build machine, from a phone     | Build: preview proxy via the HS2 server     | `HS2-PXB1BS`           |
| 2    | "Did my change work?" without driving the UI                  | Build: agent-captured evidence on tickets   | `HS2-6HMTQN`           |
| 3    | Interactive testing of one native window, no full desktop     | Spike (macOS, ScreenCaptureKit + WebRTC)    | `HS2-TVBGVZ`           |
| 4    | Interactive testing on cloud build VMs (this doc's main body) | Integrate existing tools; do not build      | `HS2-V59H7C` and §22.6 |
| 5    | Real devices / platforms nobody on the team owns              | Integrate device farms (BrowserStack, etc.) | — (not yet ticketed)   |

## 22.2 Why classic remote desktop feels slow, and what fixes it

VNC-style protocols send lossless framebuffer diffs, often over TCP. Meeting tools feel
faster because they use hardware video encoders (H.264/HEVC/AV1), adaptive bitrate, and
UDP transport. The tools below apply that same video pipeline to interactive control.
Three things dominate perceived latency, in order:

1. **Network round-trip time** to the VM (geography — see §22.5).
2. **Transport**: UDP-based (Moonlight/Sunshine, Parsec, DCV QUIC, RDP Shortpath, Apple
   High Performance) versus TCP-only fallbacks.
3. **Hardware encode/decode** on both ends. A VM without a GPU or hardware encoder falls
   back to slow software paths or, for some tools, cannot host at all.

## 22.3 Tool evaluation

### 22.3.1 Comparison

| Aspect                 | Moonlight + Sunshine                                                                                                         | Parsec                                                                          | Amazon DCV (formerly NICE DCV)                                                          | macOS High Performance Screen Sharing                                    | RDP RemoteApp / Azure Virtual Desktop                                                                              |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| Best at                | Lowest latency; launching **one app** as the session; free                                                                   | Polished low-latency desktop; easy NAT traversal; team admin                    | AWS-native; **EC2 Mac** and Windows/Linux; license included on EC2                      | Mac-to-Mac fidelity (4:4:4, HDR, stereo audio) with zero install         | **Native single-app windows** (RemoteApp) for Windows apps; enterprise identity                                    |
| Weak at                | macOS host is experimental; self-managed pairing/NAT; game-oriented UX                                                       | No Linux host; no iOS client; single-app streaming not offered                  | AWS-centric; macOS setup needs SIP off for unattended install and manual privacy grants | Apple silicon + macOS 14 **both ends** only; one session per Mac         | Windows hosts only; RemoteApp officially needs Windows Server RDS or AVD; video quality needs GPU VM sizes         |
| Host OS                | Windows, Linux, FreeBSD (full); macOS (experimental, no gamepads)                                                            | Windows, macOS (2019+ hardware, macOS 10.15+)                                   | Windows, Linux; macOS on EC2 Apple silicon instances only (DCV 2025.0+)                 | macOS 14+ on Apple silicon                                               | Windows Server (RDS), Windows multi-session (AVD); Windows 10/11 Pro single-session RemoteApp is unofficial        |
| Client OS              | Windows, macOS, Linux, iOS/iPadOS, Apple TV, Android, Vision Pro                                                             | Windows, macOS, Linux, Android, Chromium web client (no iOS/iPad)               | Windows, macOS, Linux native; HTML5 browser client                                      | macOS 14+ on Apple silicon                                               | Windows App on Windows/macOS/iOS/Android and web; Remote Desktop client (MSI) retired March 2026                   |
| Codec / transport      | H.264, HEVC, AV1; UDP                                                                                                        | H.264/H.265; UDP (peer-to-peer, relay fallback)                                 | Adaptive; QUIC (UDP) on by default since 2024.0, TCP fallback                           | Hardware video; UDP ports 5900–5902; ~75 Mbps for one 4K display         | AVC/H.264 (and HEVC on supported GPU sizes), AVC 4:4:4 option; RDP Shortpath (UDP) with TCP fallback               |
| Input fidelity         | Keyboard, mouse, gamepad, touch-as-mouse; Apple Pencil / stylus on mobile clients                                            | Keyboard, mouse, gamepad; Wacom tablets on paid tiers                           | Keyboard, mouse, touch, stylus, gamepad, USB redirection                                | Keyboard and mouse                                                       | Keyboard, mouse, touch, pen (Windows), redirection of many devices                                                 |
| Displays / HiDPI       | Streams one display; per-client resolution via a virtual display (Windows forks/drivers, Linux headless setups)              | Up to 3 monitors, virtual monitors, 4:4:4 on paid tiers                         | Up to 4 monitors, 4K; macOS server: 4K, multi-monitor, 60 FPS                           | Up to 2 virtual displays, max 4K (1920×1080 HiDPI)                       | Multi-monitor (`usemultimon`); RemoteApp windows integrate with the local desktop                                  |
| Audio                  | Yes                                                                                                                          | Yes                                                                             | Multi-channel audio; macOS server: audio output                                         | Stereo                                                                   | Yes                                                                                                                |
| Clipboard / files      | Limited (clipboard sync is not a core feature; verify per client)                                                            | Clipboard; file transfer on paid tiers (unverified exact tiering)               | Clipboard, file transfer / redirection                                                  | Clipboard, Finder drag-and-drop                                          | Clipboard, drive redirection                                                                                       |
| Headless / virtual GPU | Needs a hardware encoder; headless needs a virtual display (Windows: SudoVDA or the Apollo fork; Linux: headless compositor) | Needs a hardware encoder; virtual monitors on paid tiers                        | Works headless on EC2; GPU instances for 3D                                             | Creates virtual displays itself                                          | Session hosts are headless by design; GPU VM sizes for hardware encode                                             |
| Single-app launch      | **Yes**: each Sunshine "app" runs a command at stream start; the stream can target just that app                             | No                                                                              | No (full desktop; console or virtual sessions)                                          | No                                                                       | **Yes**: RemoteApp publishes individual application windows                                                        |
| Launchability          | CLI: `moonlight stream <host> "<app>"`, `moonlight quit <host>`                                                              | CLI: `parsecd peer_id=<id>` (after a prior login), plus `:`-separated settings  | URI `dcv://host[:port]/[?authToken][#sessionId]`; `.dcv` files; `dcvviewer <file>`      | `vnc://host` opens Screen Sharing; High Performance is chosen in the app | `ms-avd:connect?resourceid=…&user=…`; web `https://windows.cloud.microsoft/webclient/avd/<ws>/<res>`; `.rdp` files |
| Cost                   | Free, open source                                                                                                            | Free personal; Warp $9.99/mo; Teams $35/mo/user ($30 annual); Enterprise $45/mo | No charge on EC2 (pay for the instance); EC2 Mac has a 24-hour minimum host allocation  | Free with macOS                                                          | AVD: Azure compute + per-user access rights; RDS: CALs                                                             |

### 22.3.2 Notes per tool

- **Moonlight + Sunshine.** The best fit when a tester wants _just the app_: define one
  Sunshine app per built product (its command launches the app; prep/undo commands can
  reset state), and `moonlight stream <host> "<app>"` drops the tester directly into it.
  It is free and has the broadest client set, including iOS and Android with touch and
  stylus. Weaknesses: Sunshine on macOS is officially experimental, so for macOS-on-cloud
  prefer DCV. Pairing uses a PIN through Sunshine's web UI (port 47990), which is a
  one-time manual step per client. Reaching it across the internet needs a VPN or port
  forwarding; it has no relay.
- **Parsec.** Mature, easy NAT traversal, good team administration. It has no Linux
  hosting and no official iOS client. Its web client is Chromium-only and slower than the
  native app. Streaming one app is not offered. The CLI connects to a known `peer_id` but
  needs a prior interactive login. That works for a person's own machines but is awkward
  for project-shared VMs.
- **Amazon DCV.** The practical answer for **macOS on EC2** (Apple silicon instances,
  DCV 2025.0+) and a strong one for Windows/Linux GPU instances. No license fee on EC2.
  The `dcv://` URI and `.dcv` files make launching straightforward. Treat `authToken`
  as a secret, and never put it in the ticket store. macOS setup costs: unattended install
  requires SIP disabled, and Accessibility, Screen Recording and Remote Control
  permissions need a one-time GUI grant (AWS publishes an image-automation sample).
- **macOS High Performance Screen Sharing.** Excellent quality between two Apple silicon
  Macs on macOS 14+, with nothing to install. Not usable from Windows, Linux or mobile
  testers, so it does not solve the "Windows user tests a Mac app" case. Whether it works
  to an **EC2 Mac** instance is **unverified**: neither Apple's nor AWS's docs say, and
  EC2's default VNC path is limited to low resolutions. Treat it as a LAN/owned-Mac option.
- **RDP RemoteApp / Azure Virtual Desktop.** The only mainstream option that presents
  **individual remote windows as local windows** for Windows apps. It is good for testing
  a Windows desktop app from Windows, macOS, iOS or Android via Windows App. Officially it
  needs Windows Server RDS or AVD. Publishing RemoteApp from Windows 10/11 Pro works
  through an unofficial registry and `.rdp`-file technique limited to one session; use it
  only for personal setups. Video-heavy apps need GPU VM sizes and Shortpath (UDP) to feel
  responsive. The `ms-avd` URI cannot set display properties.

## 22.4 Recommendations by scenario

| Scenario                                         | First choice                                   | Alternative                                |
| ------------------------------------------------ | ---------------------------------------------- | ------------------------------------------ |
| Windows/Linux app on a cloud GPU VM, any client  | Moonlight + Sunshine with one app tile per app | Parsec (Windows host), DCV                 |
| macOS app on a cloud Mac (EC2 Mac)               | Amazon DCV                                     | Parsec (macOS host; unverified on EC2 Mac) |
| Windows desktop app, enterprise identity / Azure | AVD RemoteApp via Windows App                  | RDS RemoteApp `.rdp` file                  |
| Mac-to-Mac on owned hardware, LAN or fast link   | macOS High Performance Screen Sharing          | Moonlight + Sunshine (experimental host)   |
| Tester on iPhone/iPad                            | Moonlight (Sunshine host) or Windows App (AVD) | DCV web client; Parsec has no iOS client   |
| Tester in a browser only                         | AVD direct-launch URL, DCV HTML5 client        | Parsec web client (Chromium only)          |

## 22.5 Region placement and latency budget

- **Pick the VM region nearest the tester, not the developer or the repo.** Distance
  adds roughly 1–2 ms of latency per 100 miles. When testers are spread out, give each
  geography its own target rather than one central VM.
- **RTT budget for interactive app testing** (network round trip, client to VM; adds to
  roughly 10–20 ms of encode, decode and display time):

  | RTT       | Experience                                                                   |
  | --------- | ---------------------------------------------------------------------------- |
  | < 30 ms   | Good — feels close to local                                                  |
  | 30–60 ms  | Usable for UI and app testing; typing and dragging feel slightly heavy       |
  | 60–100 ms | Degraded — fine for checking state, poor for drag, scroll and animation work |
  | > 100 ms  | Poor — prefer tier 2 evidence capture, or move the VM                        |

  AWS's own guidance targets under 50 ms total round trip for interactive streaming.
  Local Zones or Wavelength can get below 10 ms for supported GPU instance types, but
  **EC2 Mac is only offered in a subset of regions**, which can force a farther region
  for macOS testers.

- **Measure from the tester's device.** The HS2 server's RTT to the VM is irrelevant.
  Use a user-initiated probe, never a polling loop (follow-up `HS2-YEH3R7`).
- **Prefer UDP paths.** Make sure DCV QUIC, RDP Shortpath, Sunshine's UDP ports, or
  Parsec peer-to-peer are actually in use. A TCP or relay fallback adds latency and
  makes loss far worse.

## 22.6 Integration decision: what Hot Sheet 2 integrates vs. documents

**Decision:** HS2 never implements or bundles a desktop-streaming protocol for VMs. (The
tier 3 single-window spike `HS2-TVBGVZ` is the separate place where HS2-owned streaming
is evaluated.) HS2 **integrates at the launch boundary**: it stores where a project's
remote test targets are and opens the vendor's own client with the right URI or command.
Everything else is documented here.

| Capability                                                                                                                                   | Decision      | Status                               | Ticket       |
| -------------------------------------------------------------------------------------------------------------------------------------------- | ------------- | ------------------------------------ | ------------ |
| Provider-neutral **remote test target** record on a project (provider, host/id, app, region, OS)                                             | Integrate     | Design only                          | `HS2-80077Y` |
| Per-provider launch builders: `moonlight stream`, `parsecd peer_id=`, `dcv://` / `.dcv`, `ms-avd:` / AVD web URL, `.rdp` RemoteApp, `vnc://` | Integrate     | Design only                          | `HS2-80077Y` |
| Headless `hotsheet-cli remote-target add/ls/rm/open/print` (CLAUDE.md headless parity)                                                       | Integrate     | Design only                          | `HS2-80077Y` |
| Client "open / copy link" action with per-device availability (e.g. no Parsec on iOS)                                                        | Integrate     | Design only                          | `HS2-80077Y` |
| Client-side RTT probe and nearest-region hint                                                                                                | Integrate     | Design only                          | `HS2-YEH3R7` |
| Preinstalled streaming hosts in HS2-managed cloud build VM images                                                                            | Integrate     | Deferred until cloud build VMs exist | `HS2-5AHG9Y` |
| Vendor setup: pairing, accounts, NAT/VPN, licensing, GPU sizing, macOS privacy grants                                                        | Document only | This doc                             | —            |
| Building a VM desktop protocol, relays, or mirroring vendor session state                                                                    | Out of scope  | —                                    | —            |

Integration constraints:

- **Secrets stay out of shared state.** Project settings hold non-secret fields only. DCV
  `authToken`s, RDP passwords and vendor logins belong in machine-local settings or the
  OS keychain, never in git or the ticket store.
- **Explicit capability failures.** Each provider declares which client OSes can open it;
  an unsupported combination fails with "install X" guidance rather than a dead link.
- **No polling.** Launching is user-initiated. Session liveness is not tracked.

## 22.7 Sources

Checked 2026-10. Facts marked _unverified_ in the text could not be confirmed from a
primary source.

- Parsec plans and pricing — <https://parsec.app/pricing>
- Parsec command line — <https://support.parsec.app/hc/en-us/articles/32381836182932-Run-Parsec-App-From-Command-Line>
- Parsec hardware and software compatibility (fetch blocked; summarized via search) — <https://support.parsec.app/hc/en-us/articles/32381568346644-Hardware-and-Software-Compatibility>
- Parsec web app — <https://support.parsec.app/hc/en-us/articles/32381650129300-Use-the-Web-App-browser>
- Sunshine getting started (platform support, macOS experimental) — <https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2getting__started.html>
- Sunshine repository — <https://github.com/LizardByte/Sunshine>
- Moonlight setup guide — <https://github.com/moonlight-stream/moonlight-docs/wiki/Setup-Guide>
- Moonlight-qt CLI `stream`/`quit` (issue tracker; no formal reference page) — <https://github.com/moonlight-stream/moonlight-qt/issues/92>
- SudoVDA virtual display (Windows) — <https://github.com/SudoMaker/SudoVDA>; Apollo fork — <https://github.com/ClassicOldSong/Apollo>
- Amazon DCV on EC2 Mac (install, privacy grants) — <https://docs.aws.amazon.com/dcv/latest/adminguide/setting-up-installing-macosinstall.html>
- Amazon DCV macOS prerequisites — <https://docs.aws.amazon.com/dcv/latest/adminguide/setting-up-installing-macosprereq.html>
- Amazon DCV licensing (no charge on EC2) — <https://docs.aws.amazon.com/dcv/latest/adminguide/setting-up-license.html>
- Amazon DCV URI — <https://docs.aws.amazon.com/dcv/latest/userguide/using-connecting-uri.html>; connection files — <https://docs.aws.amazon.com/dcv/latest/userguide/using-connection-file.html>
- Amazon DCV product page (QUIC, clients, monitors, input) — <https://aws.amazon.com/hpc/dcv/>
- DCV on EC2 Mac announcement (2025-11) — <https://aws.amazon.com/about-aws/whats-new/2025/11/amazon-dcv-ed2-mac-instances>
- EC2 Mac instances — <https://aws.amazon.com/ec2/instance-types/mac/>
- Apple screen sharing type options (High Performance requirements) — <https://support.apple.com/guide/mac-help/screen-sharing-type-options-on-mac-mchl1883115d/mac>
- Apple Remote Desktop High Performance — <https://support.apple.com/guide/remote-desktop/use-high-performance-screen-sharing-apdf8e09f5a9/mac>
- AVD URI schemes (`ms-avd`, `ms-rd`) — <https://learn.microsoft.com/en-us/azure/virtual-desktop/uri-scheme>
- Windows App direct launch URLs — <https://learn.microsoft.com/en-us/windows-app/direct-launch-urls>
- AVD GPU acceleration (H.264/HEVC, 4:4:4) — <https://learn.microsoft.com/en-us/azure/virtual-desktop/graphics-enable-gpu-acceleration>
- RemoteApp on Windows 10/11 without Windows Server (unofficial) — <https://woshub.com/run-remoteapps-desktop-windows/>
- AWS cloud gaming latency guidance — <https://aws.amazon.com/blogs/compute/use-amazon-ec2-for-cost-efficient-cloud-gaming-with-pay-as-you-go-pricing/>

## 22.8 Cross-references

- Distributed execution, mobile access, and security posture:
  [08-distributed-and-remote.md](08-distributed-and-remote.md)
- Clients (where the remote-target action would surface): [06-clients.md](06-clients.md)
- Headless setup parity and secrets handling: [04-core-server-cli.md](04-core-server-cli.md)
