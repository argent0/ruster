**Ruster Specification**  
*For implementation by coding agent. Target: Arch Linux. Rust 2024 edition. Repo: `https://github.com/argent0/ruster`. Deliver clean, documented, production-grade code.*

### 1. Project Goals
Persistent, proactive LLM agent that:
- Runs as background daemon.
- Exposes one world-writable UNIX socket for all user interaction.
- Manages named chat sessions with per-session model, history and memory.
- Detects and invokes relevant skills automatically.
- Logs everything.
- Zero command-line arguments.

### 2. File System Layout (created on first run)
```
~/.config/ruster/
├── config.toml                  # main config (populated from defaults)
├── skills/                      # user skills (Rust .rs files or .toml descriptors)
└── extra_skills/                # optional additional dirs from config

~/.var/app/ruster/
├── logs/ruster.log              # detailed runtime logs (rotating)
├── sessions/
│   └── {session_id}/
│       ├── history.jsonl        # full conversation + skill calls
│       ├── memory/              # long-term memory files
│       └── activity.log
└── state/                       # global agent state, proactive tasks
```

### 3. Configuration (`~/.config/ruster/config.toml`)
```toml
socket_path = "/tmp/ruster.sock"   # default
default_model = "ollama/llama3.2"  # proxy prefix
skills_dirs = ["~/.config/ruster/skills", "/usr/share/ruster/skills"]
proactive_interval_secs = 300
log_level = "info"
```
On startup if folder missing: create it + copy defaults from embedded assets.

### 4. UNIX Socket Server (`/tmp/ruster.sock`, mode 0666)
- Single listener, concurrent clients via `tokio::net::UnixListener`.
- Protocol: **JSON Lines** (one JSON object per line, UTF-8).
- Any user can connect (`nc -U /tmp/ruster.sock` works).
- Server never closes connection unless client does; supports multiple sessions per client.

**Client → Server commands**
```json
{"action":"create","session_id":"work","model":"ollama/phi3"} 
{"action":"send","session_id":"work","message":"Summarize my emails"}
{"action":"list"} 
{"action":"delete","session_id":"old"}
```

**Server → Client events**
```json
{"event":"created","session_id":"work","model":"ollama/phi3"}
{"event":"response","session_id":"work","delta":"Thinking...","done":false}
{"event":"response","session_id":"work","delta":"Final answer.","done":true}
{"event":"proactive","session_id":"work","message":"Reminder: meeting in 30 min"}
{"event":"skill_used","session_id":"work","skill":"email_fetch","result":"3 new emails"}
```

Streaming deltas for LLM responses. All lines end with `\n`.

### 5. Sessions
- In-memory + persisted to `~/.var/app/ruster/sessions/{session_id}/history.jsonl`.
- Each session has its own message history, model override, and memory vector store (simple file-based embeddings or SQLite).
- Agent prepends system prompt + relevant memory + detected skills to every LLM call.

### 6. LLM Interaction
Implement **exactly** as defined in `proxy.md` (already in repo root).  
Must support:
- Streaming responses.
- Model switching per session (format: `provider/model`).
- Default = value from config (`ollama` family first).

### 7. Skills System
Implement **exactly** as defined in `skills.md`.  
- Auto-discovery at startup from all `skills_dirs`.
- On every user message, agent asks LLM (cheap model) “which skills are relevant?”.
- If relevant, execute skill(s), inject results into context, continue.
- Skills live in `~/.config/ruster/skills/*.rs` or descriptors; compile-time registration preferred for speed.

### 8. Memory & Proactivity
- Short-term: session history.
- Long-term: simple key-value + embedding search in session folder.
- Proactive loop (tokio task): every `proactive_interval_secs` checks internal tasks and pushes `{"event":"proactive", ...}` to any client connected to the affected session.

### 9. Logging
- `tracing` + `tracing-appender` with daily rotation.
- Every socket message, LLM call, skill execution, and proactive action logged at appropriate level.
- Per-session activity log for user inspection.

### 10. Packaging – PKGBUILD (Arch Linux)
```bash
# PKGBUILD
pkgname=ruster
pkgver=0.1.0
pkgrel=1
pkgdesc="Persistent LLM agent with UNIX socket IPC"
arch=('x86_64')
url="https://github.com/argent0/ruster"
license=('MIT')
depends=('gcc-libs')
makedepends=('git' 'rust' 'cargo')
source=("git+https://github.com/argent0/ruster.git")
sha256sums=('SKIP')

pkgver() {
  cd "$srcdir/ruster"
  git describe --tags | sed 's/^v//;s/-/./g'
}

build() {
  cd "$srcdir/ruster"
  cargo build --release --locked
}

package() {
  cd "$srcdir/ruster"
  install -Dm755 "target/release/ruster" "$pkgdir/usr/bin/ruster"
  install -Dm644 "README.md" "$pkgdir/usr/share/doc/$pkgname/README.md"
  # optional: install systemd user unit
  install -Dm644 "ruster.service" "$pkgdir/usr/lib/systemd/user/ruster.service"
}
```
`makepkg -si` installs from latest git. `systemctl --user enable --now ruster` starts daemon.

### 11. Code Quality Mandates
- `cargo fmt`, `clippy --all-targets -- -D warnings`.
- Full Rustdoc + module-level docs.
- Comprehensive `README.md` covering:
  - Install (AUR + manual).
  - Socket usage examples (`socat`, `nc`, Python client).
  - How to write/add skills.
  - Config reference.
  - Proactivity examples.
- `Cargo.toml` with workspace if skills become separate crates later.
- Graceful shutdown on SIGTERM.
- No panics in main paths.

### 12. Git & Repo
- Repo already exists at `github.com/argent0/ruster`.
- `main` branch only.
- Every commit must pass `cargo test`, `cargo clippy`, `cargo fmt --check`.
- Initial commit: complete spec + skeleton.

**Deliverables for coding agent**:
1. Full working Rust codebase.
2. `PKGBUILD` + `ruster.service`.
3. `README.md` (user-facing).
4. `proxy.md` and `skills.md` respected verbatim (do not alter).
5. Ready for `cargo install --path .` and AUR submission.

