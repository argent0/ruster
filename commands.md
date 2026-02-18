# Ruster Commands Documentation

Ruster supports two command formats: DSL format and Legacy format.

## DSL Format
All commands are sent as JSON objects with `command` and `arguments` keys.
Example: `{"command": "session", "arguments": {"action": "list"}}`

## Legacy Format
Legacy commands are sent as JSON objects with `action` and any other required arguments as top-level keys.
Example: `{"action": "list"}`

---

### Session Commands
These are used with `command: "session"` or as Legacy actions.

#### `create`
Create a new session or switch to an existing one.
- `action`: "create"
- `session_id`: (string) Unique ID for the session.
- `model`: (optional, string) Override the default model.
- **Example:** `{"command": "session", "arguments": {"action": "create", "session_id": "main", "model": "openai/gpt-4"}}`

#### `send`
Send a message to a session and get a response.
- `action`: "send"
- `session_id`: (string) The ID of the session.
- `message`: (string) The user message.
- **Example:** `{"command": "session", "arguments": {"action": "send", "session_id": "main", "message": "Hello!"}}`

#### `list`
List all currently loaded or stored sessions.
- `action`: "list"
- **Example:** `{"command": "session", "arguments": {"action": "list"}}`

#### `delete`
Delete a session and its associated data.
- `action`: "delete"
- `session_id`: (string) The ID of the session to delete.
- **Example:** `{"command": "session", "arguments": {"action": "delete", "session_id": "main"}}`

#### `history` (New)
Retrieve paginated history for a session.
- `action`: "history"
- `session_id`: (string) The ID of the session.
- `limit`: (optional, integer) Number of messages to return (default: 20).
- `offset`: (optional, integer) Number of messages to skip (default: 0).
- **Example:** `{"command": "session", "arguments": {"action": "history", "session_id": "main", "limit": 10, "offset": 0}}`

---

### Config Commands
These are used with `command: "config"`.

#### `set`
Set a configuration value.
- `action`: "set"
- `key`: (string) The configuration key.
- `value`: (any) The new value.
- **Example:** `{"command": "config", "arguments": {"action": "set", "key": "log_level", "value": "debug"}}`

#### `get`
Get a configuration value.
- `action`: "get"
- `key`: (string) The configuration key.
- **Example:** `{"command": "config", "arguments": {"action": "get", "key": "default_model"}}`

#### `list`
List all configuration options and their current values.
- `action`: "list"
- **Example:** `{"command": "config", "arguments": {"action": "list"}}`
