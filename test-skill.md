When a user writes **"check if the browser is active?"**, the following process occurs within **Ruster**:

1.  **Skill Selection (RAG):**
    *   The `SkillsManager` analyzes the message and, using the RAG model (e.g., `nomic-embed-text`), identifies that the **`web-browsing`** skill is highly relevant because its description is "Uses a web browser."
    *   The skill is dynamically activated for this message context.

2.  **Context Injection:**
    *   The instructions from `skills/web-browsing/SKILL.md` ("Check if the broswer is active using `./browser-active.sh`") are appended to the LLM's **system prompt**.
    *   The built-in **`run_skill_script`** tool is added to the list of available tools. This tool is designed to execute scripts from a skill's `scripts/` directory.

3.  **LLM Reasoning & Tool Call:**
    *   The LLM (e.g., `llama3.1:8b`) receives the context and the user's request.
    *   Seeing the instruction to use `./browser-active.sh` and having the `run_skill_script` tool available, it generates a **tool call**:
        ```json
        {
          "name": "run_skill_script",
          "arguments": {
            "skill_name": "web-browsing",
            "script_name": "browser-active.sh"
          }
        }
        ```

4.  **Tool Execution:**
    *   The Ruster server catches the tool call and executes `execute_tool` in `src/server.rs`.
    *   It locates the script at `/path/to/ruster/skills/web-browsing/scripts/browser-active.sh`.
    *   It runs the script via `bash -c "./scripts/browser-active.sh"` within the skill's root directory.
    *   The script uses `curl` to check if a Chromium debug instance is listening on port 9222.

5.  **Observation & Response:**
    *   The script's output (e.g., *"Chromium debug is running at localhost:9222"* or an error message) is returned to the LLM as a tool result.
    *   The LLM then generates a final natural language response to the user, such as: *"Yes, the browser is active and running at localhost:9222."*

## Testing Steps

Follow these steps to verify the `web-browsing` skill is working correctly.

### 1. Preparation
Ensure you have the necessary environment set up:
*   **Chromium with Remote Debugging:** Start Chromium (or any Chrome-based browser) with the remote debugging port enabled:
    ```bash
    chromium --remote-debugging-port=9222 --headless
    ```
    *(Note: `--headless` is optional but useful for testing.)*
*   **Skill Location:** Verify the `web-browsing` skill is in a directory Ruster scans. By default, it looks in `~/.config/ruster/skills`. You can symlink the local `skills/web-browsing` directory:
    ```bash
    mkdir -p ~/.config/ruster/skills
    ln -s $(pwd)/skills/web-browsing ~/.config/ruster/skills/
    ```

### 2. Start the Ruster Server
Run the server from the project root:
```bash
cargo run
```
Verify the server is listening by checking the logs or the existence of `/tmp/ruster.sock`.

### 3. Send the Test Message
Use `nc` (netcat) or `socat` to send a message to the Ruster socket.

**Using `nc`:**
1.  Connect to the socket:
    ```bash
    nc -U /tmp/ruster.sock
    ```
2.  Paste the following JSON to create a session and send the message:
    ```json
    {"command": "session", "arguments": {"action": "create", "session_id": "browser-test", "model": "ollama/llama3.1:8b"}}
    {"command": "session", "arguments": {"action": "send", "session_id": "browser-test", "message": "check if the browser is active?"}}
    ```

### 4. What Should Happen at Each Step
1.  **After sending "create":** You should receive an `{"event": "created", ...}` response confirming the session is ready.
2.  **After sending "send":**
    *   **Skill Used:** You should see an `{"event": "skill_used", "skill": "web-browsing", ...}` event. This confirms RAG successfully selected the skill.
    *   **Tool Call:** You should see an `{"event": "tool_call", "tool": "run_skill_script", ...}` event. This confirms the LLM correctly identified it needs to run the script.
    *   **Tool Execution:** Ruster will execute `./skills/web-browsing/scripts/browser-active.sh`.
    *   **Final Response:** You will receive a series of `{"event": "response", ...}` chunks, concluding with a message like: *"The browser is active and running at localhost:9222."*

### 5. Troubleshooting
*   **Skill not selected:** If the `skill_used` event doesn't appear, ensure the `web-browsing` folder contains a valid `SKILL.md` with the correct frontmatter and that it's in a scanned directory.
*   **Script fails:** Check the tool execution logs at `/tmp/ruster.run/tools/<call_id>/stderr` to see why `curl` failed.
*   **LLM doesn't call tool:** Ensure the model you are using (e.g., `llama3.1:8b`) supports tool calling.
