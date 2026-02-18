# Ruster

Ruster is a persistent, proactive LLM agent that runs as a background daemon.
It exposes a single UNIX socket (`/tmp/ruster.sock`) for all interactions, managing multiple named sessions with memory and skill capabilities.

## Installation

### Arch Linux (AUR)

Clone the repository and build the package:

```bash
git clone https://github.com/argent0/ruster.git
cd ruster
makepkg -si
```

Enable the user service:

```bash
systemctl --user enable --now ruster
```

### Manual Installation (Cargo)

```bash
git clone https://github.com/argent0/ruster.git
cd ruster
cargo install --path .
```

To run manually:

```bash
ruster
```

## Usage

Ruster communicates via JSON Lines over a UNIX socket.

### Basic Interaction with `nc` (netcat)

```bash
# Connect to the socket
nc -U /tmp/ruster.sock
```

Send a command (one JSON object per line):

```json
{"action":"create","session_id":"test","model":"ollama/llama3.2"}
{"action":"send","session_id":"test","message":"Hello, who are you?"}
```

You will receive streaming responses:

```json
{"event":"created","session_id":"test","model":"ollama/llama3.2"}
{"event":"response","session_id":"test","delta":"I am ","done":false}
{"event":"response","session_id":"test","delta":"Ruster.","done":true}
```

### Python Client Example

```python
import socket
import json

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect("/tmp/ruster.sock")

def send(cmd):
    sock.sendall((json.dumps(cmd) + "
").encode())

def listen():
    buffer = ""
    while True:
        chunk = sock.recv(4096).decode()
        if not chunk: break
        buffer += chunk
        while "
" in buffer:
            line, buffer = buffer.split("
", 1)
            if not line: continue
            print("Received:", line)

# Create session
send({"action": "create", "session_id": "py-session"})

# Send message
send({"action": "send", "session_id": "py-session", "message": "Tell me a joke"})

listen()
```

## Skills

Ruster automatically discovers skills in `~/.config/ruster/skills/`.
A skill is a directory containing a `SKILL.md` file.

### Example: `~/.config/ruster/skills/joke-teller/SKILL.md`

```markdown
---
name: joke-teller
description: Tells funny programming jokes. Use when user asks for a laugh.
---

# Joke Teller Instructions

You are a comedian specialized in programming humor.
When the user asks for a joke, provide one related to:
- Rust borrowing checker
- Python whitespace
- Java verbosity

Keep it short and punchy.
```

When you ask Ruster for a joke, it will detect this skill, inject these instructions into the context, and the LLM will follow them.

## Configuration

Configuration is located at `~/.config/ruster/config.toml`.
Defaults are created on first run.

```toml
socket_path = "/tmp/ruster.sock"
default_model = "ollama/llama3.2"
skills_dirs = ["~/.config/ruster/skills", "/usr/share/ruster/skills"]
proactive_interval_secs = 300
log_level = "info"
proxy_url = "http://localhost:8080" # Optional: URL to LLM proxy or provider
```

## Proactivity

Ruster runs a background loop that can trigger proactive events.
Clients connected to the socket will receive:

```json
{"event":"proactive","session_id":"...","message":"Reminder: meeting in 30 min"}
```

Currently, this is a placeholder loop that emits a keepalive signal.

## Logs

Logs are written to `~/.var/app/ruster/logs/ruster.log` (rotating daily) and `activity.log` inside each session folder.
