# rustycord

Native Discord client written in Rust with [GPUI](https://gpui.rs) (Zed's UI framework). No webview, no Electron.

> [!WARNING]
> rustycord signs in with a **user token**. Automating user accounts ("self-bots") violates
> [Discord's Terms of Service](https://support.discord.com/hc/en-us/articles/115002192352) and can get the account
> terminated. Use at your own risk.

## Status

Working client, Discord-like dark UI with Inter typography: guild rail with unread pills, channel categories,
DM list with avatars and presence, **virtualized** message transcript (tail-follow, jump-to-latest, lazy history),
sending, replies, editing/deleting, live reactions, markdown with clickable links/mentions, unread/mention badges,
date/unread separators, typing indicator, member list with roles and role colors, user profile panel,
file uploads (picker and drag & drop), file cards with download, image attachments with a full-size viewer.
Token is stored in the OS keyring. Voice channels and DM calls use a custom Discord voice client (UDP, DAVE end-to-end encryption, Opus). Search is local; OS notifications are not implemented yet.

## Build

Requires the pinned Rust nightly `nightly-2026-11-26` (1.102.0-nightly; stable 1.102 is expected on 2027-02-04).

### Windows

- Visual Studio 2022 Build Tools with the C++ workload (MSVC)
- CMake, Ninja, NASM, LLVM (libclang)

```powershell
choco install cmake ninja nasm llvm -y
$env:CMAKE_GENERATOR = "Ninja"
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
cargo build --release -p rustycord
```

The binary is `target\release\rustycord.exe`.

### Linux

```bash
sudo apt-get install pkg-config libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-dev libxcb1-dev \
  libfontconfig1-dev libfreetype-dev libclang-dev libvulkan-dev libegl-dev libasound2-dev
cargo build --release -p rustycord
```

Vulkan 1.3 capable driver is required (llvmpipe works).

## Voice

- Join guild voice channels from the sidebar; join or start DM/group calls from the call button in the header.
- Microphone uses voice activity detection (VAD). Mute and deafen from the user panel.
- Input/output devices are selected in the settings dialog; the choice is stored in `audio.conf` next to the app config.
- Media uses Opus over Discord's UDP transport with DAVE end-to-end encryption (MLS).
- Not implemented: screen share/Go Live, push-to-talk, noise suppression, voice activity sensitivity.

## Usage

Paste a Discord user token into the login screen (browser DevTools: `F12` -> Application -> Local Storage -> `token`).
`DISCORD_TOKEN` pre-fills the field.

## CI

![CI](https://github.com/drawiks/rustycord/actions/workflows/ci.yml/badge.svg)

Voice handshake can be tested from the `Voice smoke` workflow: add a `DISCORD_TOKEN` repository secret and run it
with a voice channel ID (and optional guild ID). The test joins the channel, waits for the DAVE MLS group, and leaves.
