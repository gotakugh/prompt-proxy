import { useState, useEffect, DragEvent, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import "./App.css";

const DEFAULT_EDIT_PROMPT = "Read the attached context.xml, understand the context, and modify the code according to the instructions below.\n\n" +
  "=== Instructions ===\n{instruction}\n================\n\n" +
  "[IMPORTANT] Strict Output Format:\n" +
  "1. Omit all greetings and explanations.\n" +
  "2. Wrap the entire output in a markdown code block.\n" +
  "3. You must write the 'target file path' on a single line at the very beginning of the block so Aider can recognize it.\n" +
  "4. ONLY if you determine that necessary files are missing from context.xml, DO NOT output the code modification block. Instead, politely tell the user which files are missing, output the missing file paths in a single markdown code block (so the user can easily copy them), and ask the user to add them to the 'Target Files' input and try again.";

const DEFAULT_ASK_PROMPT = "Read the attached context.xml, understand the repository context, and answer the following question.\n\n=== Question ===\n{instruction}\n==============\n\n[IMPORTANT]\nIf you determine that necessary files are missing from context.xml to answer the question, please tell the user which files are missing. Output the missing file paths in a single markdown code block (so the user can easily copy them), and ask the user to add them to the 'Target Files' input and run again.";

interface RepoMapPayload {
  repo_map_file_path: string;
  icon_file_path: string;
  prompt: string;
}

function App() {
  const [mode, setMode] = useState<"edit" | "ask">("edit");
  const [targetDir, setTargetDir] = useState("");
  const [files, setFiles] = useState("");
  const [instruction, setInstruction] = useState("");
  const [fileEncoding, setFileEncoding] = useState(() => localStorage.getItem("fileEncoding") || "");
  const [mapTokens, setMapTokens] = useState(() => localStorage.getItem("mapTokens") || "");
  const [gitPath, setGitPath] = useState(() => localStorage.getItem("gitPath") || "");
  const [chatLanguage, setChatLanguage] = useState(() => localStorage.getItem("chatLanguage") || "English");
  const [aiderPath, setAiderPath] = useState(() => localStorage.getItem("aiderPath") || "aider");
  const [apiPort, setApiPort] = useState(() => Number(localStorage.getItem("apiPort") || 8080));
  const [repoMapData, setRepoMapData] = useState<RepoMapPayload | null>(null);
  const [packedFilesPath, setPackedFilesPath] = useState<string | null>(null);
  const [aiResponse, setAiResponse] = useState("");
  const [logs, setLogs] = useState<string[]>([]);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  // Settings states
  const [showSettings, setShowSettings] = useState(false);
  const [useCustomPrompt, setUseCustomPrompt] = useState(false);
  const [customEditPrompt, setCustomEditPrompt] = useState(DEFAULT_EDIT_PROMPT);
  const [customAskPrompt, setCustomAskPrompt] = useState(DEFAULT_ASK_PROMPT);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  // Load settings on mount
  useEffect(() => {
    const savedSettings = localStorage.getItem("promptSettings");
    if (savedSettings) {
      try {
        const settings = JSON.parse(savedSettings);
        setUseCustomPrompt(settings.useCustomPrompt ?? false);
        setCustomEditPrompt(settings.customEditPrompt ?? "");
        setCustomAskPrompt(settings.customAskPrompt ?? "");
      } catch (e) {
        console.error("Failed to parse settings from localStorage", e);
      }
    }
  }, []);

  // Reset backend state on component mount (e.g., page refresh)
  useEffect(() => {
    invoke("reset_aider_state").catch(console.error);
  }, []);

  // Sync settings with backend and save to localStorage on change
  useEffect(() => {
    const settingsToSave = { useCustomPrompt, customEditPrompt, customAskPrompt };
    localStorage.setItem("promptSettings", JSON.stringify(settingsToSave));
    localStorage.setItem("aiderPath", aiderPath);
    localStorage.setItem("gitPath", gitPath);
    localStorage.setItem("apiPort", String(apiPort));

    const settingsForRust = {
      use_custom: useCustomPrompt,
      custom_edit_prompt: customEditPrompt,
      custom_ask_prompt: customAskPrompt,
    };
    invoke("update_prompt_settings", { settings: settingsForRust }).catch(
      console.error
    );
  }, [useCustomPrompt, customEditPrompt, customAskPrompt, aiderPath, apiPort, gitPath]);

  useEffect(() => {
    invoke("start_api_server", { port: Number(apiPort) }).catch(console.error);
  }, [apiPort]);

  useEffect(() => {
    localStorage.setItem("chatLanguage", chatLanguage);
  }, [chatLanguage]);

  useEffect(() => {
    localStorage.setItem("fileEncoding", fileEncoding);
  }, [fileEncoding]);

  useEffect(() => {
    localStorage.setItem("mapTokens", mapTokens);
  }, [mapTokens]);

  useEffect(() => {
    const unlisten = listen<RepoMapPayload>("repo_map_ready", (event) => {
      setLogs(prev => [...prev, `--- PromptProxy: Received Repo Map from Aider [${new Date().toLocaleTimeString()}] ---`]);
      setRepoMapData(event.payload);
    });

    const unlistenAiderLog = listen<string>("aider_log", (event) => {
      setLogs(prev => [...prev, event.payload]);
    });

    return () => {
      unlisten.then((fn) => fn());
      unlistenAiderLog.then((fn) => fn());
    };
  }, []);

  const handleCopyToClipboard = () => {
    if (repoMapData?.prompt) {
      navigator.clipboard.writeText(repoMapData.prompt).catch((err) => {
        console.error("Failed to copy text: ", err);
      });
    }
  };

  const handleReset = async () => {
    await invoke("reset_aider_state");
    setRepoMapData(null);
    setPackedFilesPath(null);
    setAiResponse("");
  };

  const handleApplyPatch = async () => {
    setLogs(prev => [...prev, `--- PromptProxy: Applying patch via Aider [${new Date().toLocaleTimeString()}] ---`]);
    await invoke("apply_patch", {
      targetDir,
      response: aiResponse,
      aiderPath,
      fileEncoding,
      gitPath
    });
    setAiResponse(""); // Clear response after applying
  };

  const handlePackFiles = async () => {
    setLogs(prev => [...prev, `--- PromptProxy: Packing target files... [${new Date().toLocaleTimeString()}] ---`]);
    try {
      const path = await invoke<string>("pack_target_files", {
        targetDir,
        files,
        fileEncoding,
      });
      setPackedFilesPath(path);
      setLogs(prev => [...prev, `--- PromptProxy: Successfully packed files to XML ---`]);
    } catch (error) {
      setLogs(prev => [...prev, `--- PromptProxy: Error packing files: ${error} ---`]);
      console.error("Failed to pack files:", error);
    }
  };

  const handleSelectDirectory = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
    });
    if (typeof selected === "string") {
      setTargetDir(selected);
    }
  };

  const handleLaunchAider = async () => {
    // Aiderをクラッシュさせる /ask の代わりに、Rustだけが解釈できる独自タグを付与
    const finalMessage = mode === "ask" ? `[MODE:ASK]\n${instruction}` : instruction;
    await invoke("launch_aider_batch", {
      targetDir,
      files,
      message: finalMessage,
      chatLanguage,
      aiderPath,
      fileEncoding,
      gitPath,
      mapTokens,
      apiPort: Number(apiPort),
    });
  };

  const handleDragFile = async (e: DragEvent<HTMLDivElement>, filePath: string, iconPath?: string) => {
    const isLinux = navigator.userAgent.toLowerCase().includes("linux");

    if (isLinux) {
      // Linux (X11): Use native drag protocol
      const uri = 'file://' + filePath;
      e.dataTransfer?.setData('text/uri-list', uri + '\r\n');
      e.dataTransfer?.setData('text/plain', filePath);
    } else {
      // Windows/Mac: Use Tauri native plugin
      e.preventDefault();
      
      // TS2322エラー回避: iconPath が undefined の場合は処理を中断する
      if (!iconPath) {
        console.error("Drag failed: iconPath is missing.");
        return;
      }

      try {
        await startDrag({
          item: [filePath],
          icon: iconPath,
        });
      } catch (error) {
        console.error("Drag failed:", error);
      }
    }
  };

  return (
    <div className="app-container">
      <header className="status-header">
        <div className="stepper">
          <span>A. RepoMap</span> | <span>B. Target Files</span> | <span>C. Apply Patch</span>
        </div>
        <button onClick={handleReset} className="reset-button">🔄 Reload</button>
      </header>
      <main className="main-content">
        <div className="main-header">
          <h1>LLM Prompt Proxy</h1>
          <button className="settings-button" onClick={() => setShowSettings(true)}>⚙️ Settings</button>
        </div>

        {showSettings ? (
          <div className="settings-container">
            <h2>Prompt Settings</h2>
            <div className="form-group">
              <label>Aider Path (Executable Path)</label>
              <input
                type="text"
                value={aiderPath}
                onChange={(e) => setAiderPath(e.target.value)}
                placeholder="aider"
              />
            </div>
            <div className="form-group">
              <label>API Port (Proxy Server Port)</label>
              <input
                type="number"
                value={apiPort}
                onChange={(e) => setApiPort(Number(e.target.value))}
              />
            </div>
            <div className="form-group">
              <label>Git Path (Optional. e.g. C:\Program Files\Git\cmd)</label>
              <input type="text" value={gitPath} onChange={(e) => setGitPath(e.target.value)} placeholder="Leave blank if git is in PATH" />
            </div>
            <div className="form-group">
              <label>Chat Language (Aider's thinking/response language)</label>
              <input
                type="text"
                value={chatLanguage}
                onChange={(e) => setChatLanguage(e.target.value)}
                placeholder="e.g. English, Japanese (Leave blank for OS default)"
              />
            </div>
            <div className="form-group">
              <div className="mode-selector">
                <label>
                  <input
                    type="radio"
                    checked={!useCustomPrompt}
                    onChange={() => setUseCustomPrompt(false)}
                  />
                  Use tool's default settings
                </label>
                <label>
                  <input
                    type="radio"
                    checked={useCustomPrompt}
                    onChange={() => setUseCustomPrompt(true)}
                  />
                  Use custom prompts
                </label>
              </div>
            </div>

            {useCustomPrompt && (
              <>
                <div className="form-group">
                  <label>Prompt for Code Edit (Edit)</label>
                  <textarea
                    value={customEditPrompt}
                    onChange={(e) => setCustomEditPrompt(e.target.value)}
                  />
                  <p className="prompt-hint">* User instruction will be inserted at {`{instruction}`}</p>
                </div>
                <div className="form-group">
                  <label>Prompt for Repository Q&A (Ask)</label>
                  <textarea
                    value={customAskPrompt}
                    onChange={(e) => setCustomAskPrompt(e.target.value)}
                  />
                  <p className="prompt-hint">* User instruction will be inserted at {`{instruction}`}</p>
                </div>
              </>
            )}
            <div className="button-group" style={{ marginTop: '1em' }}>
              <button onClick={() => {
                setCustomEditPrompt(DEFAULT_EDIT_PROMPT);
                setCustomAskPrompt(DEFAULT_ASK_PROMPT);
              }}>Reset to Defaults</button>
              <button onClick={() => setShowSettings(false)}>Close Settings</button>
            </div>
          </div>
        ) : (
          <div className="tool-container">
            {/* Global Settings */}
            <div className="card">
                <h3>Global Settings</h3>
                <div className="form-group">
                    <label>Target Project Directory</label>
                    <div className="input-group">
                        <input type="text" value={targetDir} onChange={(e) => setTargetDir(e.target.value)} placeholder="/path/to/your/project" />
                        <button onClick={handleSelectDirectory}>Select Folder</button>
                    </div>
                </div>
                <div className="form-group">
                    <label>File Encoding (e.g., cp932)</label>
                    <input type="text" value={fileEncoding} onChange={(e) => setFileEncoding(e.target.value)} placeholder="Leave blank for default" />
                </div>
                <div className="form-group">
                    <label>Map Tokens (Optional. e.g., 1024)</label>
                    <input type="number" value={mapTokens} onChange={(e) => setMapTokens(e.target.value)} placeholder="Leave blank for Aider default" />
                </div>
            </div>

            {/* Block A: Repo Map Generation */}
            <div className="card">
                <h3>A. Generate Repo Map & Prompt</h3>
                <div className="form-group">
                    <label>Operation Mode</label>
                    <div className="mode-selector">
                        <label><input type="radio" value="edit" checked={mode === 'edit'} onChange={() => setMode('edit')}/> Edit Code</label>
                        <label><input type="radio" value="ask" checked={mode === 'ask'} onChange={() => setMode('ask')}/> Ask about Repo</label>
                    </div>
                </div>
                <div className="form-group">
                    <label>Instructions for LLM</label>
                    <textarea value={instruction} onChange={(e) => setInstruction(e.target.value)} placeholder="Fix the bug in..."/>
                </div>
                <button onClick={handleLaunchAider}>Generate Repo Map & Prompt</button>
                {repoMapData && (
                    <div className="prompt-content">
                        <div className="info-box">
                            <h4>RepoMap File</h4>
                            <div className="draggable-file-wrapper">
                                <div className="draggable-file" draggable={true} onDragStart={(e) => handleDragFile(e, repoMapData.repo_map_file_path, repoMapData.icon_file_path)}>
                                    <svg width="64" height="80" viewBox="0 0 100 120"><path d="M0 4C0 1.8 1.8 0 4 0H65L100 35V116C100 118.2 98.2 120 96 120H4C1.8 120 0 118.2 0 116V4Z" fill="#CF84E1"/><path d="M65 0V31C65 33.2 66.8 35 69 35H100L65 0Z" fill="#B463C8"/><text x="50" y="78" fill="white" fontSize="36" fontFamily="monospace" textAnchor="middle" fontWeight="bold">&lt;/&gt;</text></svg>
                                    <span className="file-name">repo_map.xml</span>
                                </div>
                            </div>
                        </div>
                        <div className="info-box">
                            <h4>Prompt</h4>
                            <div className="prompt-display">
                                <pre>{repoMapData.prompt}</pre>
                                <button onClick={handleCopyToClipboard}>Copy</button>
                            </div>
                        </div>
                    </div>
                )}
            </div>

            {/* Block B: Target Files XML Packing */}
            <div className="card">
                <h3>B. Pack Target Files to XML</h3>
                <div className="form-group">
                    <label>Target Files (Space-separated)</label>
                    <input type="text" value={files} onChange={(e) => setFiles(e.target.value)} placeholder="src/main.rs src/lib.rs"/>
                </div>
                <button onClick={handlePackFiles}>Pack Target Files to XML</button>
                {packedFilesPath && repoMapData?.icon_file_path && (
                  <div className="info-box" style={{marginTop: '1em'}}>
                      <h4>Packed Files</h4>
                      <div className="draggable-file-wrapper">
                          <div className="draggable-file" draggable={true} onDragStart={(e) => handleDragFile(e, packedFilesPath, repoMapData.icon_file_path)}>
                              <svg width="64" height="80" viewBox="0 0 100 120"><path d="M0 4C0 1.8 1.8 0 4 0H65L100 35V116C100 118.2 98.2 120 96 120H4C1.8 120 0 118.2 0 116V4Z" fill="#84A1E1"/><path d="M65 0V31C65 33.2 66.8 35 69 35H100L65 0Z" fill="#637BC8"/><text x="50" y="78" fill="white" fontSize="36" fontFamily="monospace" textAnchor="middle" fontWeight="bold">&lt;/&gt;</text></svg>
                              <span className="file-name">target_files.xml</span>
                          </div>
                      </div>
                  </div>
                )}
            </div>

            {/* Block C: Patch Application */}
            <div className="card">
                <h3>C. Apply Patch</h3>
                <div className="response-content">
                    <textarea value={aiResponse} onChange={(e) => setAiResponse(e.target.value)} placeholder="Paste AI response with SEARCH/REPLACE blocks here..."/>
                    <div className="button-group">
                        <button onClick={handleApplyPatch}>Apply Patch</button>
                    </div>
                </div>
            </div>
          </div>
        )}
      </main>

      <footer className="bottom-terminal">
        <div className="terminal-log">
          {logs.map((log, index) => (
            <div key={index}>{log}</div>
          ))}
          <div ref={messagesEndRef} />
        </div>
      </footer>
    </div>
  );
}

export default App;
