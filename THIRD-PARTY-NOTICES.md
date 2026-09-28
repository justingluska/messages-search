# Third-party notices

Messages Search is licensed under GPL-3.0-or-later (see [LICENSE](LICENSE)). It includes or builds on the open-source software below. Every license listed is compatible with distributing the app under GPL-3.0. Full license texts ship inside each package on crates.io and npm; `cargo deny check licenses` (see [deny.toml](deny.toml)) verifies the Rust dependency tree.

## Core components

| Component | Use | License |
|---|---|---|
| [imessage-database](https://github.com/ReagentX/imessage-exporter) and crabstep, by Christopher Sardegna | Reading and decoding the Messages database | GPL-3.0-or-later |
| [Tauri](https://tauri.app) (tauri, tao, wry, muda, tauri-plugin-opener, @tauri-apps/api, @tauri-apps/plugin-opener) | App shell | Apache-2.0 OR MIT |
| [ONNX Runtime](https://onnxruntime.ai) 1.28, Microsoft (statically linked) | Running the embedding model | MIT; its own third-party notices: https://github.com/microsoft/onnxruntime/blob/main/ThirdPartyNotices.txt |
| [ort / ort-sys](https://ort.pyke.io), pyke | Rust bindings for ONNX Runtime | MIT OR Apache-2.0 |
| [fastembed-rs](https://github.com/Anush008/fastembed-rs), Qdrant | Embedding pipeline | Apache-2.0 |
| [tokenizers](https://github.com/huggingface/tokenizers), [hf-hub](https://github.com/huggingface/hf-hub), Hugging Face | Tokenizer, model download | Apache-2.0 |
| Oniguruma (onig, onig_sys); esaxx-rs | Used by tokenizers | BSD-2-Clause; Apache-2.0 |
| [SQLite](https://sqlite.org) via rusqlite / libsqlite3-sys | Index storage and full-text search (FTS5) | Public domain; MIT |
| [sqlite-vec](https://github.com/asg017/sqlite-vec), Alex Garcia | Vector search | MIT OR Apache-2.0 |
| objc2, block2, objc2-foundation, objc2-app-kit, objc2-contacts | macOS APIs (Contacts, Trash, clipboard) | MIT OR Zlib OR Apache-2.0 |
| [notify](https://github.com/notify-rs/notify) | Watching for new messages | CC0-1.0 |
| protobuf, plist, chrono, serde, serde_json, thiserror | Parsing and serialization | MIT / MIT OR Apache-2.0 |
| rustls, webpki, ring, webpki-roots | TLS in the dependency tree | Apache-2.0 / ISC / MIT; ring: Apache-2.0 AND ISC; webpki-roots: CDLA-Permissive-2.0 |
| ICU4X crates | Unicode handling | Unicode-3.0 |
| cssparser, selectors, dtoa-short, option-ext | Used by the app shell | MPL-2.0 (unmodified; source on crates.io) |
| [React](https://react.dev), react-dom, scheduler, Meta | UI | MIT |
| [TanStack Virtual](https://tanstack.com/virtual), Tanner Linsley | Virtualized lists | MIT |

## Models (downloaded on first launch, not bundled)

| Model | License |
|---|---|
| [BAAI/bge-small-en-v1.5](https://huggingface.co/BAAI/bge-small-en-v1.5), and its int8 ONNX export [Qdrant/bge-small-en-v1.5-onnx-Q](https://huggingface.co/Qdrant/bge-small-en-v1.5-onnx-Q) (default) | MIT |
| [intfloat/multilingual-e5-small](https://huggingface.co/intfloat/multilingual-e5-small) (optional, not yet exposed in the app) | MIT |

## Test data

`scripts/make-fixture.py` builds a fictional Messages database from the schema of imessage-database's test database (GPL-3.0-or-later). All people, numbers (555-01xx) and emails (`.example`) in fixtures and mock data are invented.

## Artwork

The app icon, UI icons and the water background are original to this project.
