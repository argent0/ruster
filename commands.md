# Ruster Commands Documentation

Ruster uses a DSL-style command format for all interactions.

## Command Format
All commands are sent as JSON objects with `command` and `arguments` keys.
Example: `{"command": "session", "arguments": {"action": "list"}}`

---

### Session Commands
These are used with `command: "session"`.

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

#### `history`
Retrieve paginated history for a session.
- `action`: "history"
- `session_id`: (string) The ID of the session.
- `limit`: (optional, integer) Number of messages to return (default: 20).
- `offset`: (optional, integer) Number of messages to skip (default: 0).
- **Example:** `{"command": "session", "arguments": {"action": "history", "session_id": "main", "limit": 10, "offset": 0}}`
- **Response includes:** `skills`: (array of strings) List of skills active when the message was sent/received.

---

### Skill Commands
These are used with `command: "skill"`.

#### `add`
Add a skill permanently to the current session's context.
- `action`: "add"
- `session_id`: (string) The ID of the session.
- `skill`: (string) The name of the skill to add.
- **Example:** `{"command": "skill", "arguments": {"action": "add", "session_id": "main", "skill": "joke-teller"}}`

#### `list`
List all skills currently active in a session.
- `action`: "list"
- `session_id`: (string) The ID of the session.
- **Example:** `{"command": "skill", "arguments": {"action": "list", "session_id": "main"}}`

#### `search`
Search for available skills using RAG.
- `action`: "search"
- `session_id`: (string) The ID of the session.
- `query`: (string) The search query.
- **Example:** `{"command": "skill", "arguments": {"action": "search", "session_id": "main", "query": "funny jokes"}}`

#### `remove`
Remove a skill from the session's active list and its message history.
- `action`: "remove"
- `session_id`: (string) The ID of the session.
- `skill`: (string) The name of the skill to remove.
- **Example:** `{"command": "skill", "arguments": {"action": "remove", "session_id": "main", "skill": "joke-teller"}}`

#### `ban`
Globally prevent a skill from being loaded or dynamically selected.
- `action`: "ban"
- `session_id`: (string) The ID of the session (required for context).
- `skill`: (string) The name of the skill to ban.
- **Example:** `{"command": "skill", "arguments": {"action": "ban", "session_id": "main", "skill": "clock"}}`

#### `unban`
Remove a skill from the global ban list.
- `action`: "unban"
- `session_id`: (string) The ID of the session.
- `skill`: (string) The name of the skill to unban.
- **Example:** `{"command": "skill", "arguments": {"action": "unban", "session_id": "main", "skill": "clock"}}`

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
