<div align="center">

<img src="docs/images/logo.svg" alt="Beayy" width="72">

# Beayy

**Your day, written down automatically: when, where, and what you were doing**

Your computer, phone and watch together, plus the places you went.<br>
Summaries as you go, a journal every evening, rolled up into weekly and monthly reviews.

*Be Authentic. Your Yesterdays.*

[简体中文](README.md) · **English**

[![Release](https://img.shields.io/github/v/release/hengfengliya/beayy?label=download&color=0b0b0b)](https://github.com/hengfengliya/beayy/releases/latest)
[![Downloads](https://img.shields.io/github/downloads/hengfengliya/beayy/total?label=downloads&color=0b0b0b)](https://github.com/hengfengliya/beayy/releases)
[![Stars](https://img.shields.io/github/stars/hengfengliya/beayy?color=0b0b0b)](https://github.com/hengfengliya/beayy/stargazers)
[![License](https://img.shields.io/github/license/hengfengliya/beayy?color=0b0b0b)](LICENSE)
![Platform](https://img.shields.io/badge/macOS%2014%2B%20%C2%B7%20Windows%2010%2F11%20%C2%B7%20iPhone%20%C2%B7%20Android-0b0b0b)

[Download for Mac / Windows / Android](https://github.com/hengfengliya/beayy/releases/latest) · [iPhone & Apple Watch (TestFlight)](https://testflight.apple.com/join/Ptzqu4q8) · [beayy.com](https://beayy.com)

<img src="docs/images/en/devices.png" alt="Beayy on the computer and the phone" width="860">

</div>

At the end of a busy day it's hard to remember what you actually did. Screen Time tells you that you spent three hours in Chrome, not what you were doing in it. Beayy runs in the background on your computer, phone and watch, and has a language model write the log for you. You just read it.

The interface comes in English and Simplified Chinese (Settings → General → Language; it follows your system by default). Summaries and journals follow the interface language, or set it under Settings → AI → Language. The screenshots below use made-up sample data.

## One interface on your computer and your phone

The desktop app (Mac, Windows) records and writes the summaries. The iPhone and Android apps record what happens on your phone and show the day, timeline and journals synced from your computer. Apple Watch records audio all day.

<p align="center"><img src="docs/images/en/phone.png" alt="Phone app: Today, Places, daily journal" width="860"></p>

## The shape of your day

How long you were at the computer, how long on your phone, how many times you switched apps, how many places you went. The day strip lays out places, sleep, computer, phone and conversations hour by hour, and your time is sorted by project and by type of work.

With the iPhone app you can also bring in Apple Health: when you fell asleep and woke up, how long you slept, how many steps you took and how long you worked out.

<p align="center"><img src="docs/images/en/today.png" alt="Today view" width="860"></p>

## What you were doing, piece by piece

The timeline lists what you were doing each hour. Open an hour to see the summary of each stretch, the apps you used, and what you did when you picked up your phone. The official model writes one summary an hour by default; with your own model it can go down to every 10 minutes.

<p align="center"><img src="docs/images/en/timeline.png" alt="Timeline" width="860"></p>

## Where you went

With the iPhone app, Beayy records the places you stayed, when you arrived and left, and guesses which shop or café it was. Name the places you go often. It keeps stops only, never the route in between.

<p align="center"><img src="docs/images/en/places.png" alt="Places" width="860"></p>

## Daily, weekly and monthly reviews

Every evening Beayy writes a journal entry: one line about the day, a short remark that opens with a quote from a book or a well-known person, what got done, where you drifted, and what's next, plus where you went and where your time went. A weekly review follows each week and a monthly review each month. If your iPhone data arrives late, hit "regenerate".

Write rules in your own words, such as "no computer after 22:00" or "less than 3 hours of phone a day", and the journal checks the day against them.

<p align="center"><img src="docs/images/en/summary-day.png" alt="Daily journal" width="860"></p>

## Ask your own past

Beayy ships a local MCP server. Connect it to Claude Code, Claude Desktop or Cursor and ask things like "what was I working on last Wednesday afternoon?" or "which night did I stay up latest this month?". It's read-only, and the data never leaves your computer.

```bash
claude mcp add beayy -- "/Applications/Beayy.app/Contents/MacOS/beayy" --mcp
```

The config snippet for Claude Desktop and Cursor is under Settings → AI → Connections. Tools: `list_days` (an overview of recent days), `get_day` (time spent, what you did each hour, where the time went, and the journal for a day), `search` (keyword search over the 10-minute summaries), `get_rules_and_projects`.

<p align="center"><img src="docs/images/en/mcp.png" alt="Connect to AI" width="860"></p>

## Your data stays yours

Everything is stored on your computer. To sync between devices, pick one, or don't sync at all:

- **iCloud**: all-Apple devices, no sign-up
- **Cloud drive (WebDAV)**: Nextcloud, a NAS and the like; works with Android and Windows too, no sign-up
- **Official server**: open sign-up, no invite code, 100 MB of sync space per account, 50 accounts for now; on-screen text from your computer is never uploaded
- **Self-hosted**: the server lives in [`server/`](server/README.md); `docker compose up -d` gets it running
- **Sync folder** (advanced): a folder OneDrive, Dropbox or Syncthing already syncs, between computers

Either way, records are compressed and encrypted on your device before they leave it, so the server or cloud drive only ever sees ciphertext. The official model is separate from sync: with iCloud or WebDAV you can still sign in to a Beayy account for the official model (50,000 tokens a month free).

<p align="center"><img src="docs/images/en/sync.png" alt="Sync options" width="860"></p>

## What it records and where it lives

| | |
|---|---|
| Computer | Which app and window is in front, what you type and send, clicks, scrolling, on-screen text (via macOS Accessibility or Windows UI Automation), whether you're at the computer, sleep and lock. Password fields are never recorded. You can exclude apps entirely, pause at any time, and delete the last 15 minutes, hour or day |
| Phone | iPhone Screen Time as synced to your Mac through iCloud: when each app was in the foreground and for how long. App contents are not visible. On Android, the Beayy Android app reads it itself (usage access) and syncs it to your computer, encrypted |
| Location (optional) | Stops recorded by the iPhone app (arrival, departure, place name), encrypted and synced to your computer. Routes are not recorded |
| Health (optional) | The iPhone app reads Apple Health (sleep, steps, activity, workouts, heart rate, mood and more) read-only, and syncs it to your computer encrypted. The Today view shows last night's sleep and the day's activity; the journal leaves out details like heart rate and mood |
| Watch (optional) | All-day Apple Watch audio, sent to a transcription service (the official one or your own) that transcribes it and separates speakers. Your computer only gets the text: who said what, and when |
| Storage | All local, in `~/Library/Application Support/Beayy/`, one append-only file per device per 10 minutes |
| Summaries | Each 10-minute record goes to **the model you configure** (any OpenAI-compatible API, or the official server) and the result is written back locally. The interval can be 10, 20, 30 or 60 minutes; with the official model it defaults to 60. Settings → AI → Model → Usage shows the tokens spent per day |
| Sync (optional) | Compressed and encrypted with XChaCha20-Poly1305 on your device. Your master password never leaves the device (key derived with Argon2id); the server stores only ciphertext and a hash of the login verifier. With the official server, on-screen text is never uploaded; it stays on your device |

## Install

Download `Beayy-<version>.dmg` from [Releases](https://github.com/hengfengliya/beayy/releases/latest) and drag it into Applications. On Windows, run `Beayy-<version>-windows-x64-setup.exe`: it adds desktop and Start menu shortcuts and starts Beayy at login. The first launch walks you through permissions (Accessibility, Input Monitoring, optionally Full Disk Access), the model, and whether to sync. Beayy checks for updates on its own; once a new version is downloaded it asks you, and "Restart to update" takes a few seconds.

The iPhone and Apple Watch apps are in public TestFlight: https://testflight.apple.com/join/Ptzqu4q8 (source in [`ios/`](ios/)).

Android app (preview): download `Beayy-<version>-android.apk` from Releases, sign in with the same account as your computer (or connect the same WebDAV drive), and allow usage access under Settings → Phone and watch. It records how long you use each app; location is not recorded yet (source in [`android/`](android/)).

Requires macOS 14 or later, or Windows 10/11 (64-bit).

## Not supported yet

- Linux computers; the Android app does not record location yet, is not release-signed and doesn't update itself
- iPhone Screen Time can only be read on a Mac. With only a Windows computer you won't see which phone apps you used (location still works)
- The Windows build is still a preview. It isn't code-signed, so SmartScreen blocks the first run; choose "More info → Run anyway". Antivirus software may flag the keyboard hook as a keylogger; allow it. Apps running as administrator can't be read (Beayy doesn't elevate)
- The official model allowance is 50,000 tokens a month, enough to try Beayy out (the official model summarizes once an hour by default, which still takes tens of thousands of tokens a day). For daily use, bring your own model. Transcription and maps aren't included

## Build from source

```bash
# Rust (rustup) and the Xcode command line tools
git clone https://github.com/hengfengliya/beayy && cd beayy
./build.sh            # test build: compile, sign (Developer ID if available, ad-hoc otherwise), install as Beayy-rs.app and launch; uses a separate shadow data directory, so it can run next to the release
./build.sh --install  # install as ~/Applications/Beayy.app (release build, real data directory; replaces an installed Beayy)
./build.sh --dmg      # build target/Beayy-<version>.dmg; notarized automatically when signed with Developer ID and notary credentials are set
```

iPhone and Apple Watch apps (Xcode 26, [XcodeGen](https://github.com/yonaskolb/XcodeGen), and the Rust targets `aarch64-apple-ios` and `aarch64-apple-ios-sim`):

```bash
cd ios && xcodegen            # generates BeayyRecorder.xcodeproj; set DEVELOPMENT_TEAM in project.yml to your own
open BeayyRecorder.xcodeproj  # run BeayyPhone; the Rust core is built before compiling
```

Android app (JDK 17, Android SDK 35 and NDK 27, [cargo-ndk](https://github.com/bbqsrc/cargo-ndk), and the Rust targets `aarch64-linux-android` and `x86_64-linux-android`):

```bash
cd android && ./build.sh      # builds app/build/outputs/apk/debug/app-debug.apk; ./build.sh release for a release build
```

UI only: open `ui/index.html` in a browser. It reads the made-up sample data in `ui/mock/` (regenerate with `python3 ui/mock/make_mock.py`).

<details>
<summary>Code layout</summary>

| Directory | Contents |
|---|---|
| `crates/beayy-core` | Event format and writing (per device, UTC 10-minute segments, append-only), device names, data directory |
| `crates/beayy-capture` | Capture: keyboard and mouse, typing and sending, on-screen text, app switches, presence and heartbeat, sleep and lock. macOS uses Accessibility and event taps, Windows uses low-level hooks and UI Automation; the decision logic is shared (`common.rs`) |
| `crates/beayy-phone` | Reads iPhone Screen Time synced through iCloud (macOS) |
| `crates/beayy-remote` | Pulls watch transcripts and iPhone stops from the recording server (HTTPS + token) |
| `crates/beayy-summary` | 10-minute and hourly summaries, two-level classification (type; area · project), project name merging |
| `crates/beayy-sync` | Multi-device sync client: account, encryption, upload and download; Beayy server, iCloud, WebDAV, sync folder |
| `src-tauri` | Tauri 2 shell: menu bar, main window, commands for the web UI (`src-tauri/COMMANDS.md`), scheduled jobs, daily/weekly/monthly reviews, MCP server |
| `ui` | Web UI in plain HTML, CSS and JS, shared by the computer, iPhone and Android; English and Simplified Chinese from one translation table (`i18n.js`, `locales/en.js`) |
| `mobile` | Rust core for iPhone (static library), built from the same data code as `src-tauri` |
| `android` | Kotlin shell for the Android app: reads per-app usage and calls the same Rust core over JNI; the UI is a web view loading `ui/` |
| `ios` | Swift shells for the iPhone and Apple Watch apps (XcodeGen): location stops, watch recording, encrypted upload; the UI is a web view loading `ui/` |
| `server` | Server: sync (ciphertext only) + transcription + optional model and map proxies, in Python (FastAPI + SQLite + local files). Run it with Docker on your own computer or server, see [`server/README.md`](server/README.md); the official instance runs the same code |
| `sync-server` | The old Cloudflare sync server (Workers + R2 + D1), same API as `server`, no longer maintained |
| `docs/images` | Images for the READMEs (English ones in `en/`), showing made-up sample data |

</details>

## Star history

<p align="center">
<a href="https://www.star-history.com/#hengfengliya/beayy&Date">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=hengfengliya/beayy&type=Date&theme=dark">
    <img alt="Star History" src="https://api.star-history.com/svg?repos=hengfengliya/beayy&type=Date" width="640">
  </picture>
</a>
</p>

## Feedback

Found a bug or want a feature? Open an [issue](https://github.com/hengfengliya/beayy/issues).

## License

MIT, see [LICENSE](LICENSE). Third-party code is listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
