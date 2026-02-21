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

{
    "command": "config",
    "arguments": {
        "action": "get",
        "key": "default_model"
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

For **each user message**, Ruster performs a **RAG-based search** over the metadata (name + description) of all available skills. It then:
1.  Identifies the most relevant skills (up to `rag_top_n` that are above `rag_threshold`).
2.  **Dynamically loads** these skills into the current session.
3.  Injects the full instructions of all loaded skills into the LLM's context.
4.  Persists the discovered skills in the message history for that turn.

Once a skill is loaded into a session (either via RAG or manually), it stays "active" for the duration of that session, ensuring the agent retains the capability as needed. You can manage these skills using the `skill` commands (add, list, search, remove, ban, unban).

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

## Tool Calling (Function Calling)

Ruster supports structured tool calling for LLMs that support it (Ollama, xAI, Gemini). Skills can define tools in their `SKILL.md` frontmatter, including execution logic.

### Example Tool Definition in `SKILL.md`

```yaml
---
name: clock
description: Fetches current date and time using system tools.
tools:
  - name: get_current_time
    description: Returns the current system time.
    parameters:
      type: object
      properties: {}
    exec: "date '+%A, %B %d, %Y %H:%M:%S %Z'"
---
```

### Execution and Logging

When an LLM requests a tool call, Ruster:
1. Assigns a unique **UUID** to the call.
2. Executes the command specified in the `exec` field (via `bash -c`).
3. Logs the call details, `stdout`, and `stderr` to `/tmp/ruster.run/tools/<uuid>/`.
4. Injects the first **10 lines** (configurable) of the output back into the conversation.
5. Emits a `tool_call` event over the socket:

```json
{
  "event": "tool_call",
  "session_id": "test",
  "tool": "get_current_time",
  "arguments": "{}",
  "call_id": "550e8400-e29b-41d4-a716-446655440000",
  "result_preview": "Friday, February 20, 2026 13:00:00 UTC"
}
```

### Pagination

If a tool produces large output, Ruster provides a built-in `paginate_tool_output` tool. The agent can use this tool to request more lines, search for specific terms, or view a different range of the captured `stdout`.

## Configuration

Configuration is located at `~/.config/ruster/config.toml`.
Defaults are created on first run.

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
