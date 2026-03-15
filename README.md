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
It supports a DSL-style command format for session management and configuration.

### Basic Interaction with `nc` (netcat)

```bash
# Connect to the socket
nc -U /tmp/ruster.sock
```

#### Session Management

```json
{
    "command": "session",
    "arguments": {
        "action": "create",
        "session_id": "test",
        "model": "ollama/llama3.2"
    }
}

{
    "command": "session",
    "arguments": {
        "action": "send",
        "session_id": "test",
        "message": "Hello, who are you?"
    }
}
```

#### External Servers

Ruster can connect to external "servers" that provide real-time events or tools.

```json
// List discovered servers
{
    "command": "server",
    "arguments": { "action": "list" }
}

// Attach a session to a server
{
    "command": "server",
    "arguments": {
        "action": "attach",
        "session_id": "test",
        "server_name": "system-monitor",
        "event_delivery": "next-turn"
    }
}
```

#### Configuration

You can configure `ruster` dynamically through the socket:

```json
{
    "command": "config",
    "arguments": {
        "action": "list"
    }
}

{
    "command": "config",
    "arguments": {
        "action": "set",
        "key": "log_level",
        "value": "debug"
    }
}
```

### Python Client Example

```python
import socket
import json

sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect("/tmp/ruster.sock")

def send(cmd):
    sock.sendall((json.dumps(cmd) + "\n").encode())

def listen():
    buffer = ""
    while True:
        chunk = sock.recv(4096).decode()
        if not chunk: break
        buffer += chunk
        while "\n" in buffer:
            line, buffer = buffer.split("\n", 1)
            if not line: continue
            print("Received:", line)

# Create session
send({
    "command": "session",
    "arguments": {
        "action": "create",
        "session_id": "py-session"
    }
})

# Send message
send({
    "command": "session",
    "arguments": {
        "action": "send",
        "session_id": "py-session",
        "message": "Tell me a joke"
    }
})

listen()
```

## Skills

Ruster follows the **Agent Skills** open standard. It automatically discovers skills in `~/.config/ruster/skills/` and other configured directories. A skill is a directory containing a `SKILL.md` file with metadata and instructions.

### Automatic Skill Selection (RAG)

For **each user message**, Ruster performs a **RAG-based search** over the metadata (name + description) of all available skills. It then identifies the most relevant skills, dynamically loads them, and injects their instructions into the LLM's context.

## Servers

Ruster supports connecting to external processes called "Servers" via Unix sockets. These servers can push events to sessions or respond to direct queries.

### Discovery

Ruster automatically scans `/tmp` for sockets matching the pattern `ruster-srv-<name>.sock`. Once discovered, a server appears in the `server list` command.

### Event Delivery Modes

When attaching a session to a server, you can specify how events from that server are delivered:

- **`immediate`**: The event is added to the session history, and the LLM is immediately triggered to generate a response.
- **`proactive`**: The event is broadcasted to the proactive loop, which may decide to trigger the LLM based on internal logic.
- **`next-turn`** (Default): The event is queued and injected as a system message during the next user-initiated turn.

### Rate Limiting

To prevent runaway loops or spam, Ruster applies a per-server, per-session rate limit (default: 5 events/sec with a burst of 10). If a server exceeds this limit, events are dropped, and a `rate_limited` event is emitted.

## Tool Calling (Function Calling)

Ruster supports structured tool calling for LLMs that support it (Ollama, xAI, Gemini). Skills can define tools in their `SKILL.md` frontmatter.

### Execution and Logging

When an LLM requests a tool call, Ruster executes the command and logs the call details, `stdout`, and `stderr` to `/tmp/ruster.run/tools/<uuid>/`. The first **10 lines** of output are injected back into the conversation.

## Configuration

Configuration is located at `~/.config/ruster/config.toml`.

```toml
socket_path = "/tmp/ruster.sock"
default_model = "ollama/llama3.1:8b"
rag_model = "ollama/nomic-embed-text"
rag_top_n = 3
rag_threshold = 0.4
skills_dirs = ["~/.config/ruster/skills", "/usr/share/ruster/skills"]
proactive_interval_secs = 300
log_level = "info"
tool_run_dir = "/tmp/ruster.run"
tool_output_lines = 10
proxy_url = "http://localhost:8080"
```


## Proactivity

Ruster runs a background loop that can trigger proactive events. Currently, this is a loop that emits keepalive signals and processes events marked with the `proactive` delivery mode.

## Logs

Logs are written to `~/.var/app/ruster/logs/ruster.log` (rotating daily) and `activity.log` inside each session folder (`~/.local/share/ruster/sessions/<id>/`).
