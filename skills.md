**Agent Skills** (often stylized as AgentSkills) is an open standard for giving LLM-powered agents modular, on-demand capabilities. It packages domain-specific instructions, workflows, scripts, and resources into discoverable folders that agents can load only when relevant.

Originally created by Anthropic for Claude in 2025, it was released as a fully open format (see agentskills.io and the GitHub spec). It is now natively supported by Claude (Code, API, claude.ai), Cursor, VS Code + GitHub Copilot, OpenAI Codex, Spring AI, and many other agent runtimes. The core idea is **progressive disclosure**: give the agent lightweight metadata for every skill at startup, then inject full instructions + resources only when the task actually needs them. This solves context bloat while turning a general-purpose LLM into a reliable specialist.

### Core Parts of a Skill
A skill is simply a directory whose name matches the `name` field in its main file:

```
my-skill-name/
├─ SKILL.md                  # Required
├─ scripts/                  # Optional: executable code
├─ references/               # Optional: extra docs
└─ assets/                   # Optional: templates, images, data, schemas
```

**SKILL.md** (the only mandatory file) has two sections:

1. **YAML frontmatter** (metadata, loaded for every skill at startup, ~100 tokens):
   ```yaml
   ---
   name: my-skill-name                # must match folder name, lowercase + hyphens
   description: >                     # critical for discovery
     Creates professional PowerPoint decks following our brand guidelines.
     Use whenever the user asks for slides, presentations, or PPTX files.
   license: MIT
   compatibility: Requires bash and Python 3.11+
   allowed-tools: Bash(pptx) Bash(jq:*) Read   # experimental, agent-specific
   metadata:
     version: 1.2
     author: Your Team
   ---
   ```

2. **Markdown body** (full instructions, loaded only when activated, recommended ≤5 000 tokens):
   - Step-by-step workflow
   - Input/output examples
   - Edge-case handling
   - Best practices
   - Relative links to scripts/references/assets

Optional folders are referenced by relative paths inside SKILL.md and loaded on demand.

### How Agent Skills Work (the flow)

1. **Discovery**  
   The agent runtime scans standard locations (e.g. `~/.agent-skills/`, `.claude/skills/`, project subfolders, workspace settings). Every valid directory becomes a skill. Only the frontmatter (name + description) is read and kept in context.

2. **Matching & Activation**  
   When the user prompt or internal reasoning matches keywords in the description (or the skill name is explicitly referenced), the agent activates the skill and injects the full SKILL.md body.

3. **Progressive Disclosure**  
   - Metadata → always present  
   - Instructions (SKILL.md body) → loaded on activation  
   - Scripts / references / assets → fetched and executed or read only when the instructions explicitly tell the agent to do so (usually via bash or the agent’s file tools)

4. **Execution**  
   The LLM now follows the detailed instructions exactly as written. It can run scripts (output only returns to context), read reference files, use templates, etc. The skill can also declare `allowed-tools` so the agent knows it is permitted to call specific external capabilities.

5. **Deactivation**  
   Once the task is complete, the full skill content is usually dropped from context again (implementation-dependent).

This mechanism lets agents maintain thousands of skills with almost zero overhead until one is actually needed.

### How They Are Used in Practice

**For end users / developers**
- Drop a skill folder into the right directory → the agent instantly knows the new capability.
- Common patterns: company brand guidelines + slide templates, internal API conventions, legal review checklists, data-analysis pipelines, framework-specific coding rules, etc.

**For teams / enterprises**
- Store skills in Git → version-controlled, reviewable, shareable knowledge.
- One skill can be reused across Claude, Cursor, Codex, Copilot, local agents, etc., with zero rewriting.

**For skill authors**
- Write once → publish on GitHub → anyone using a compatible agent can `git clone` or copy the folder.
- Official/community collections already exist for PDF/Office handling, popular frameworks, security auditing, etc.

**Pre-built examples (Anthropic)**
- `pptx`, `xlsx`, `docx`, `pdf` — automatically available in Claude surfaces.

**Custom examples people actually build**
- “React-modernization” skill that knows exact migration steps + scripts.
- “ADK” skill (Google Agent Development Kit) that jumps from 29 % to 99 % success rate on ADK tasks.
- Internal “finance-report” skill that loads company chart templates, data schemas, and approval workflows.

### Why This Matters for LLM Agents

Traditional agents rely on huge system prompts or retrieval that still loads everything. Agent Skills give **composable, token-efficient specialization** without fine-tuning or long context. They are the “npm packages for agent capabilities”: write a skill once, drop it anywhere, and every compatible agent becomes better at that exact task.

In short: Agent Skills turn the “prompt engineering tax” into a reusable, discoverable asset library that any LLM agent can consume on demand. That is why the entire ecosystem (Claude, Cursor, Copilot, Codex, Spring AI, etc.) adopted the same folder + SKILL.md format in late 2025 / early 2026.

### How Agents Find the Right Skill

AgentSkills uses a **declarative, LLM-driven discovery and selection process** built on **progressive disclosure**. The agent doesn't "search" like a database query—it reasons over lightweight metadata to match skills to tasks. Here's the exact flow:

#### 1. **Discovery: Building the Skill Registry (Startup)**
   - **Scanning Directories**: On launch (or workspace load), the runtime (Claude, Cursor, Copilot, Codex, etc.) recursively scans predefined locations for skill folders:
     | Location Type | Paths (examples) |
     |---------------|------------------|
     | **Global** | `~/.agent-skills/`, `~/.claude/skills/` |
     | **Project** | `.claude/skills/`, `.cursor/skills/`, `.github/skills/`, `.agent-skills/` |
     | **Workspace** | Repo subdirs like `./skills/`, IDE settings |
     | **Installed** | Via marketplaces (e.g., VS Code extension pulls from GitHub) |
   - **Parsing**: For every folder with a `SKILL.md`, it extracts **only the YAML frontmatter** (name, description, version, etc.). Ignores the full body.
   - **Registry Creation**: This metadata (~30-100 tokens/skill) is injected into the agent's system prompt or as a "Skills" tool description. The LLM now "knows" every skill's purpose without bloat.
     - Example registry entry: `"pptx: Creates professional PowerPoint decks following brand guidelines. Use for slides or presentations."`
   - **Efficiency**: Thousands of skills fit in context. No full loads yet.

#### 2. **Matching: LLM Decides Relevance (Runtime Reasoning)**
   - **Implicit Matching** (most common):
     - During chain-of-thought or tool-use, the agent compares the user's task to skill descriptions.
     - Semantic/keyword fit: "Generate quarterly slides" → matches "pptx" description.
     - Agent self-prompts: "Does any skill description cover this? Yes → activate."
   - **Explicit Invocation**:
     - User types `/skill-name` in chat (e.g., `/pptx`).
     - Or mentions in prompt: "Use the finance-report skill for this."
   - **Advanced**: Some runtimes expose tools like `list_available_skills()` (returns metadata) or `invoke_skill(name)` for the agent to query/explore.

#### 3. **Activation: Loading the Skill**
   - Once matched, the runtime injects the **full SKILL.md body** (instructions) into the current context.
   - Agent now executes the workflow step-by-step.
   - Resources (scripts/, references/) load JIT via file tools—only when instructions reference them.

#### 4. **Edge Cases and Optimizations**
   - **Conflicts**: LLM picks the best match (or asks user). Descriptions should be precise to avoid this.
   - **Caching**: Metadata stays resident; full skills drop post-task.
   - **Cross-Platform**: Same skills work everywhere—runtime handles the rest.

This makes skills feel like "super-tools": the agent autonomously discovers and applies them, scaling from generalist to specialist without prompt hacks.
