# PromptProxy General Specifications

## 1. System Overview

### 1.1 Purpose
PromptProxy is a GUI application designed to seamlessly bridge (proxy) "Aider," a powerful local AI coding tool, with advanced LLMs available through web interfaces (e.g., Gemini 1.5 Pro, Claude 3.5 Sonnet). By leveraging Aider's sophisticated repository analysis and patching capabilities while handling all AI interaction and source code transfer via Rust/Tauri, it aims to integrate the massive context windows of Web LLMs into local development environments.

### 1.2 Key Features
* **Fully Modular Design**: Three independent blocks (Repo Map Generation, Target File XML Packing, and Patch Application) allow for an incremental context-building workflow.
* **Hybrid Context Generation**: Aider is tasked only with analyzing the project structure (Repo Map) without being passed file contents. Instead, Rust directly reads and packs large source code files into XML format.
* **Bypassing Upload Limits**: For Web LLMs with strict file size limits, the tool automatically splits files into safe chunks (configurable in KB) by line, enabling them to be dragged and dropped as multiple files simultaneously.
* **Portable Application**: All settings are saved in `config.json` within the same directory as the executable, making it easy to carry on a USB drive or migrate between environments.

---

## 2. System Architecture

The system is built using Tauri (Rust) + React (TypeScript).

### 2.1 Frontend (View Layer)
* **Technology**: React, TypeScript, Vanilla CSS
* **Role**: Handles user input for instructions, manages Global Settings, provides single/multiple file drag-and-drop functionality to the OS, provides a UI for AI responses, and displays real-time stdout logs from the Aider process.

### 2.2 Backend (Controller/Proxy Layer)
* **Technology**: Rust, Tauri, Axum (Web Server), Tokio (Async)
* **Role**:
  * Runs an OpenAI-compatible mock API server (`/v1/chat/completions`) at `localhost:8080` (or a dynamic port).
  * Manages the Aider child process (launching, argument construction, piping stdout, graceful shutdown).
  * Intercepts API requests from Aider and converts them into pure prompts for the user.
  * Provides high-speed local file system access and chunked XML packing.
  * Completely isolates Aider from the network during patch application (offline mode) to prevent external checks or model initializations.

---

## 3. UI Composition and Specifications (Modular Workflow)

The app avoids restrictive state lockdowns. Users can use any of the three independent functional blocks in any order.

### 3.1 Screen Layout
1. **Top Header**: Step indicators and a "🔄 Reload" button for forcing process re-initialization.
2. **Center Main Area**: Global settings and three independent functional blocks (A, B, and C).
3. **Bottom Terminal Area**: A persistent black terminal displaying app logs and the Aider process output.

### 3.2 Three Independent Functional Blocks
* **[Block A] Generate Repo Map & Prompt**: 
  Uses Aider to generate the overall project structure (Repo Map) along with strict output rules for the AI.
* **[Block B] Pack Target Files to XML**: 
  Rust reads the specified files directly and outputs them as XML files to be passed to the Web LLM.
* **[Block C] Apply Patch**: 
  Accepts AI-generated code output (SEARCH/REPLACE blocks) copied from the Web UI and applies it to local files.

### 3.3 Configuration Management
* **Global Settings**: Target directory, file encoding, max chunk size (KB), Map Tokens, and output extension (e.g., `.xml`, `.txt`) can be set from the main screen.
* **Prompt Settings**: From the ⚙️ Settings menu, users can configure the Aider path, API port, chat language, and custom prompts (for Edit/Ask modes).
All settings are loaded from `config.json` at startup and saved immediately upon change.

---

## 4. Core Features and Rules

### 4.1 Disabling Aider Prompts and Enforcing Single Code Blocks
Aider's complex native system prompts are disabled (`--edit-format ask`), replaced by PromptProxy's own strict instructions. To facilitate easy copy-pasting, the LLM is instructed to **"wrap all file changes within a single code block, with the file path written on the line immediately preceding each SEARCH/REPLACE marker."**

### 4.2 Safe Line-Based File Splitting (Chunking)
To avoid upload restrictions of Web LLMs, Target Files are split into chunks based on a specified size limit (e.g., 80KB). Each tag in the XML includes line number attributes (e.g., `<file path="src/main.rs" lines="1-850">`) to prevent the LLM from losing context or hallucinating.

### 4.3 OS-Agnostic Bulk Drag-and-Drop
Multiple file chunks can be dropped into a browser at once via a single "Drag All" icon.
* **Linux (Wayland/X11)**: Uses the native `text/uri-list` protocol for multiple files.
* **Mac/Windows**: Handled as OS drag events via the Tauri native plugin.

### 4.4 Offline Patching and Line Ending Protection
During patch application (Block C), internal API settings are redirected to the local port to prevent Aider from communicating externally (e.g., with openrouter.ai). This ensures a **completely offline** process. Additionally, the tool automatically detects the original file's line endings (LF vs. CRLF) and preserves them to prevent unintended changes.

### 4.5 Graceful Shutdown of Aider Processes
To prevent zombie processes and Git lock issues, the app follows these steps when closing:
1. Sends a `SIGINT` (equivalent to Ctrl+C) to the process ID.
2. Polls for a clean exit for up to 2 seconds.
3. If it times out, it terminates the process with `SIGKILL`.
