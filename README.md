# PromptProxy

**A local AI coding tool proxy for Web LLMs.**

PromptProxy is a GUI application that bridges the powerful local AI coding tool **Aider** with the massive context windows of **Web LLMs** (e.g., Gemini 1.5 Pro, Claude 3.5 Sonnet). 

It allows you to leverage Aider's repository analysis and patching capabilities while using the browser-based AI interface as your "LLM backend."

## Key Features

- **Modular Workflow**: Independent blocks for Repo Map generation, XML file packing, and patch application.
- **Bypass Upload Limits**: Automatically splits large source files into line-based chunks for easy drag-and-drop into Web UIs.
- **Offline Patching**: Applies AI-generated SEARCH/REPLACE blocks to your local files without requiring an external API connection.
- **Preserve Line Endings**: Automatically detects and maintains your files' original line endings (LF/CRLF).
- **Portable**: Settings are stored locally in `config.json`.

## Requirements

- **Node.js** (v18 or later)
- **Rust** (Stable)
- **pnpm** (Recommended)
- **Aider** (Installed and accessible via CLI)

## Getting Started

### 1. Installation

Clone the repository and install dependencies:

```bash
pnpm install
```

### 2. Development

Run the application in development mode:

```bash
pnpm tauri dev
```

### 3. Build

To create a production-ready executable:

```bash
pnpm tauri build
```

The executable will be generated in the `src-tauri/target/release` directory.

## License

This project is licensed under the **GPL-3.0-or-later** license. See the [LICENSE](LICENSE) file for details.
