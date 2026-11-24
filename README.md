# rustycord

Native Discord client written in Rust with [GPUI](https://gpui.rs) (Zed's UI framework). No webview, no Electron.

> [!WARNING]
> rustycord signs in with a **user token**. Automating user accounts ("self-bots") violates
> [Discord's Terms of Service](https://support.discord.com/hc/en-us/articles/115002192352) and can get the account
> terminated. Use at your own risk.

## Status

Early development: guilds/channels, DM list, message history, sending, reactions, unread state, typing indicator.

## Build

Requires Rust 1.98.1.

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
sudo apt-get install pkg-config libxkbcommon-dev libwayland-dev libx11-dev libxcb1-dev \
  libfontconfig1-dev libfreetype-dev libclang-dev libvulkan-dev libegl-dev
cargo build --release -p rustycord
```

Vulkan 1.3 capable driver is required (llvmpipe works).

## Usage

Paste a Discord user token into the login screen (browser DevTools: `F12` -> Application -> Local Storage -> `token`).
`DISCORD_TOKEN` pre-fills the field.

## CI

![CI](https://github.com/drawiks/rustycord/actions/workflows/ci.yml/badge.svg)
