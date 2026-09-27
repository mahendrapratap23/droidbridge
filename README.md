# DroidBridge

Universal Android-to-macOS file transfer over USB. Open source. No cloud, no Wi-Fi, no Bluetooth — just a USB cable.

## What it does

DroidBridge connects to **any MTP-compatible Android phone** (Samsung, Google Pixel, OnePlus, Xiaomi, Redmi, Vivo, Oppo, Motorola, Nothing, and more) via USB and lets you browse, download, upload, and delete files with a native macOS interface.

## How it works

- **USB device discovery** via [nusb](https://crates.io/crates/nusb) (pure Rust, no libusb)
- **MTP protocol** implemented directly on top of PTP/USB bulk transport
- **Desktop app** built with [Tauri 2](https://v2.tauri.app/) (Rust backend + web frontend)
- **UI** in vanilla HTML, CSS, and JavaScript — no framework overhead

Device detection is based on USB interface class codes (PTP class `0x06` and vendor-specific MTP `0xFF/0x01/0x01`), **not** on hardcoded vendor or product IDs. Any standards-compliant MTP device will work.

## Features

- [x] USB device scanning and identification
- [x] MTP session management (open / close)
- [x] Storage volume listing
- [x] Folder browsing with breadcrumb navigation
- [x] File download to macOS
- [x] File upload from macOS
- [x] File deletion on device
- [x] Transfer progress reporting
- [x] Transfer cancellation
- [x] Keyboard shortcuts (⌘A select all, Backspace navigate back, Escape deselect)
- [ ] Drag-and-drop transfers
- [ ] USB hotplug detection (currently requires manual scan)
- [ ] Folder download/upload (recursive)

## Project status

> **Early development.** The MTP protocol implementation is complete but has not yet been tested end-to-end with a physical Android device. USB device discovery works immediately. The UI is fully functional as a scaffold.

## Prerequisites

- **macOS** 11.0+ (Apple Silicon or Intel)
- **Rust** 1.70+ ([install](https://rustup.rs/))
- **Node.js** 20+ ([install](https://nodejs.org/))

## Getting started

```bash
# Clone
git clone https://github.com/YOUR_USERNAME/droidbridge.git
cd droidbridge

# Install frontend dependencies
npm install

# Run in development mode
npm run tauri dev
```

The first build will compile all Rust dependencies (~2-3 minutes). Subsequent builds are fast.

## Project structure

```
droidbridge/
├── index.html                  # App shell
├── src/
│   ├── main.js                 # State management + event wiring
│   ├── modules/
│   │   ├── api.js              # Tauri IPC wrappers
│   │   └── ui.js               # DOM rendering functions
│   └── styles/
│       └── index.css           # Design system (dark mode, macOS-native)
├── src-tauri/
│   ├── Cargo.toml              # Rust dependencies
│   ├── tauri.conf.json         # Tauri config
│   ├── capabilities/
│   │   └── default.json        # Permission grants
│   └── src/
│       ├── main.rs             # Entry point
│       ├── lib.rs              # Plugin + command registration
│       ├── commands.rs         # Tauri command handlers
│       ├── state.rs            # Shared app state
│       └── mtp/
│           ├── mod.rs          # Module root
│           ├── protocol.rs     # PTP/MTP wire format + opcodes
│           ├── device.rs       # USB device discovery (nusb)
│           ├── session.rs      # MTP session + operations
│           ├── transfer.rs     # Download/upload with progress
│           └── types.rs        # Shared data types
├── README.md
├── LICENSE
└── .gitignore
```

## Architecture

```
┌─────────────────────────────────┐
│   Vanilla HTML/CSS/JS Frontend  │
│   (Tauri WebView)               │
└─────────┬───────────────────────┘
          │ Tauri IPC (invoke / events)
┌─────────▼───────────────────────┐
│   Tauri Commands (Rust)         │
│   commands.rs                   │
└─────────┬───────────────────────┘
          │
┌─────────▼───────────────────────┐
│   MTP Protocol Layer            │
│   PTP containers over USB bulk  │
│   session.rs + protocol.rs      │
└─────────┬───────────────────────┘
          │
┌─────────▼───────────────────────┐
│   nusb (pure Rust USB)          │
│   Device discovery + I/O        │
└─────────┬───────────────────────┘
          │
      USB Cable
          │
    Android Phone
```

## Dependencies and licenses

| Crate | Version | License | Purpose |
|-------|---------|---------|---------|
| [tauri](https://crates.io/crates/tauri) | 2.x | MIT/Apache-2.0 | Desktop app framework |
| [nusb](https://crates.io/crates/nusb) | 0.2.x | Apache-2.0/MIT | Pure-Rust USB access |
| [serde](https://crates.io/crates/serde) | 1.x | MIT/Apache-2.0 | Serialisation |
| [tokio](https://crates.io/crates/tokio) | 1.x | MIT | Async runtime |
| [thiserror](https://crates.io/crates/thiserror) | 2.x | MIT/Apache-2.0 | Error types |
| [uuid](https://crates.io/crates/uuid) | 1.x | MIT/Apache-2.0 | Transfer IDs |
| [log](https://crates.io/crates/log) | 0.4.x | MIT/Apache-2.0 | Logging facade |
| [env_logger](https://crates.io/crates/env_logger) | 0.11.x | MIT/Apache-2.0 | Log output |
| [vite](https://www.npmjs.com/package/vite) | 6.x | MIT | Frontend dev server |

All dependencies are MIT or Apache-2.0 licensed. **No C dependencies** — the entire stack is pure Rust and JavaScript.

## Contributing

Contributions welcome. Please open an issue before starting major work.

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run `cargo test` and `cargo clippy`
5. Open a pull request

## License

[MIT](LICENSE)
