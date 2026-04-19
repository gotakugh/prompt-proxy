import { useState, useEffect, DragEvent, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import "./App.css";

const DEFAULT_EDIT_PROMPT = "Read the attached context.xml, understand the context, and modify the code according to the instructions below.\n\n" +
  "=== Instructions ===\n{instruction}\n================\n\n" +
  "🚨 [CRITICAL FORMATTING RULES] 🚨\n" +
  "1. Omit all greetings and explanations.\n" +
  "2. Output ALL your modifications within a SINGLE markdown code block (` ```text ` or ` ``` `). Do NOT split them into multiple blocks, even if modifying multiple files.\n" +
  "3. Inside the code block, you MUST write the exact 'target file path' on a single line immediately before EVERY `<<<<<<< SEARCH` marker.\n" +
  "   ⚠️ IMPORTANT: If you modify the SAME file multiple times, you MUST repeat the file path before EACH and EVERY `<<<<<<< SEARCH` block. Do NOT omit it.\n\n" +
  "Example Output Format (showing multiple edits in the same file):\n" +
  "```text\n" +
  "src/main.rs\n" +
  "<<<<<<< SEARCH\n" +
  "// first old code in main\n" +
  "=======\n" +
  "// first new code in main\n" +
  ">>>>>>> REPLACE\n\n" +
  "src/main.rs\n" +
  "<<<<<<< SEARCH\n" +
  "// second old code in main\n" +
  "=======\n" +
  "// second new code in main\n" +
  ">>>>>>> REPLACE\n\n" +
  "src/utils.rs\n" +
  "<<<<<<< SEARCH\n" +
  "// old code in utils\n" +
  "=======\n" +
  "// new code in utils\n" +
  ">>>>>>> REPLACE\n" +
  "```\n\n" +
  "4. ONLY if you determine that necessary files are missing from context.xml, DO NOT output the code modification block. Instead, output the missing file paths in a single code block and ask the user to add them.";

const DEFAULT_ASK_PROMPT = "Read the attached context.xml, understand the repository context, and answer the following question.\n\n=== Question ===\n{instruction}\n==============\n\n[IMPORTANT]\nIf you determine that necessary files are missing from context.xml to answer the question, please tell the user which files are missing. Output the missing file paths in a single markdown code block (so the user can easily copy them), and ask the user to add them to the 'Target Files' input and run again.";

interface RepoMapPayload {
  repo_map_file_path: string;
  icon_file_path: string;
  prompt: string;
}

interface AppConfig {
  target_dir: string;
  file_encoding: string;
  map_tokens: string;
  max_file_size_kb: string;
  output_extension: string;
  git_path: string;
  aider_path: string;
  chat_language: string;
  api_port: string;
  prompt_settings: string;
}

function App() {
  const [activeTab, setActiveTab] = useState("A");
  const [mode, setMode] = useState<"edit" | "ask">("edit");
  const [targetDir, setTargetDir] = useState("");
  const [files, setFiles] = useState("");
  const [instruction, setInstruction] = useState("");
  const [fileEncoding, setFileEncoding] = useState("");
  const [mapTokens, setMapTokens] = useState("");
  const [outputExtension, setOutputExtension] = useState("xml");
  const [gitPath, setGitPath] = useState("");
  const [chatLanguage, setChatLanguage] = useState("English");
  const [aiderPath, setAiderPath] = useState("aider");
  const [apiPort, setApiPort] = useState(8080);
  const [repoMapData, setRepoMapData] = useState<RepoMapPayload | null>(null);
  const [maxFileSizeKb, setMaxFileSizeKb] = useState("80");
  const [packedFilesPaths, setPackedFilesPaths] = useState<string[]>([]);
  const [aiResponse, setAiResponse] = useState("");
  const [logs, setLogs] = useState<string[]>([]);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  // Settings states
  const [showSettings, setShowSettings] = useState(false);
  const [useCustomPrompt, setUseCustomPrompt] = useState(false);
  const [customEditPrompt, setCustomEditPrompt] = useState(DEFAULT_EDIT_PROMPT);
  const [customAskPrompt, setCustomAskPrompt] = useState(DEFAULT_ASK_PROMPT);

  // Temporary states for settings modal
  const [tempAiderPath, setTempAiderPath] = useState("aider");
  const [tempApiPort, setTempApiPort] = useState(8080);
  const [tempGitPath, setTempGitPath] = useState("");
  const [tempChatLanguage, setTempChatLanguage] = useState("English");
  const [tempMapTokens, setTempMapTokens] = useState("1024");
  const [tempMaxFileSizeKb, setTempMaxFileSizeKb] = useState("80");
  const [tempOutputExtension, setTempOutputExtension] = useState("xml");
  const [tempUseCustomPrompt, setTempUseCustomPrompt] = useState(false);
  const [tempCustomEditPrompt, setTempCustomEditPrompt] = useState(DEFAULT_EDIT_PROMPT);
  const [tempCustomAskPrompt, setTempCustomAskPrompt] = useState(DEFAULT_ASK_PROMPT);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  const isInitialLoad = useRef(true);

  // Load config from file on mount
  useEffect(() => {
    invoke("reset_aider_state").catch(console.error);
    
    invoke<AppConfig>("load_config").then(config => {
      setTargetDir(config.target_dir);
      setFileEncoding(config.file_encoding);
      setMapTokens(config.map_tokens);
      setMaxFileSizeKb(config.max_file_size_kb);
      setOutputExtension(config.output_extension);
      setGitPath(config.git_path);
      setAiderPath(config.aider_path);
      setChatLanguage(config.chat_language);
      setApiPort(Number(config.api_port));

      if (config.prompt_settings) {
        try {
          const settings = JSON.parse(config.prompt_settings);
          setUseCustomPrompt(settings.useCustomPrompt ?? false);
          setCustomEditPrompt(settings.customEditPrompt ?? DEFAULT_EDIT_PROMPT);
          setCustomAskPrompt(settings.customAskPrompt ?? DEFAULT_ASK_PROMPT);
        } catch (e) {
          console.error("Failed to parse prompt_settings from config.json", e);
        }
      }
      setTimeout(() => { isInitialLoad.current = false; }, 500);
    }).catch(console.error);
  }, []);

  // Save config to file on change
  useEffect(() => {
    if (isInitialLoad.current) return;

    const config: AppConfig = {
      target_dir: targetDir,
      file_encoding: fileEncoding,
      map_tokens: mapTokens,
      max_file_size_kb: maxFileSizeKb,
      output_extension: outputExtension,
      git_path: gitPath,
      aider_path: aiderPath,
      chat_language: chatLanguage,
      api_port: String(apiPort),
      prompt_settings: JSON.stringify({ useCustomPrompt, customEditPrompt, customAskPrompt }),
    };
    invoke("save_config", { config }).catch(console.error);
  }, [targetDir, fileEncoding, mapTokens, maxFileSizeKb, outputExtension, gitPath, aiderPath, chatLanguage, apiPort, useCustomPrompt, customEditPrompt, customAskPrompt]);

  // Sync settings that are still needed by Rust backend in memory
  useEffect(() => {
    const settingsForRust = {
      use_custom: useCustomPrompt,
      custom_edit_prompt: customEditPrompt,
      custom_ask_prompt: customAskPrompt,
    };
    invoke("update_prompt_settings", { settings: settingsForRust }).catch(console.error);
    invoke("start_api_server", { port: Number(apiPort) }).catch(console.error);
  }, [useCustomPrompt, customEditPrompt, customAskPrompt, apiPort]);

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

  const handleOpenSettings = () => {
    setTempAiderPath(aiderPath);
    setTempApiPort(apiPort);
    setTempGitPath(gitPath);
    setTempChatLanguage(chatLanguage);
    setTempMapTokens(mapTokens);
    setTempMaxFileSizeKb(maxFileSizeKb);
    setTempOutputExtension(outputExtension);
    setTempUseCustomPrompt(useCustomPrompt);
    setTempCustomEditPrompt(customEditPrompt);
    setTempCustomAskPrompt(customAskPrompt);
    setShowSettings(true);
  };

  const handleSaveSettings = () => {
    setAiderPath(tempAiderPath);
    setApiPort(tempApiPort);
    setGitPath(tempGitPath);
    setChatLanguage(tempChatLanguage);
    setMapTokens(tempMapTokens);
    setMaxFileSizeKb(tempMaxFileSizeKb);
    setOutputExtension(tempOutputExtension);
    setUseCustomPrompt(tempUseCustomPrompt);
    setCustomEditPrompt(tempCustomEditPrompt);
    setCustomAskPrompt(tempCustomAskPrompt);
    setShowSettings(false);
  };

  const handleCancelSettings = () => {
    setShowSettings(false);
  };

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
    setPackedFilesPaths([]);
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
      const paths = await invoke<string[]>("pack_target_files", {
        targetDir,
        files,
        fileEncoding,
        maxFileSizeKb: Number(maxFileSizeKb) || 0,
        outputExtension,
      });
      setPackedFilesPaths(paths);
      setLogs(prev => [...prev, `--- PromptProxy: Successfully packed into ${paths.length} file(s) ---`]);
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
      outputExtension,
      apiPort: Number(apiPort),
    });
  };

  const displayExt = outputExtension.replace(/^\./, '') || 'xml';

  const handleDragFile = async (e: DragEvent<HTMLDivElement>, filePaths: string[], iconPath?: string) => {
    const isLinux = navigator.userAgent.toLowerCase().includes("linux");

    if (isLinux) {
      // Linux (X11/Wayland): Use native drag protocol for multiple files
      const uriList = filePaths.map(p => 'file://' + p).join('\r\n') + '\r\n';
      e.dataTransfer?.setData('text/uri-list', uriList);
      e.dataTransfer?.setData('text/plain', filePaths.join('\n'));
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
          item: filePaths, // 配列をそのまま渡す
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
        <h2 style={{ margin: 0, fontSize: '1.2em' }}>LLM Prompt Proxy</h2>
        <div className="header-actions">
          <button onClick={handleReset} className="abort-button">⏹ Abort</button>
          <button className="settings-button" onClick={handleOpenSettings}>⚙️ Settings</button>
        </div>
      </header>

      {showSettings && (
        <div className="modal-overlay" onClick={handleCancelSettings}>
          <div className="settings-container" onClick={(e) => e.stopPropagation()}>
            <h2>App Settings</h2>
            <div className="settings-grid" style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '1em' }}>
              <div className="form-group">
                <label>Aider Path</label>
                <input type="text" value={tempAiderPath} onChange={(e) => setTempAiderPath(e.target.value)} />
              </div>
              <div className="form-group">
                <label>API Port</label>
                <input type="number" value={tempApiPort} onChange={(e) => setTempApiPort(Number(e.target.value))} />
              </div>
              <div className="form-group">
                <label>Chat Language</label>
                <input type="text" value={tempChatLanguage} onChange={(e) => setTempChatLanguage(e.target.value)} />
              </div>
              <div className="form-group">
                <label>RepoMap Max Tokens</label>
                <input type="number" value={tempMapTokens} onChange={(e) => setTempMapTokens(e.target.value)} />
              </div>
              <div className="form-group">
                <label>Max Split Size (KB)</label>
                <input type="number" value={tempMaxFileSizeKb} onChange={(e) => setTempMaxFileSizeKb(e.target.value)} />
              </div>
              <div className="form-group">
                <label>Output File Extension</label>
                <input type="text" value={tempOutputExtension} onChange={(e) => setTempOutputExtension(e.target.value)} />
              </div>
              <div className="form-group">
                <label>Git Path (Optional)</label>
                <input type="text" value={tempGitPath} onChange={(e) => setTempGitPath(e.target.value)} />
              </div>
            </div>

            <div className="form-group" style={{ marginTop: '1.5em' }}>
              <label>Prompt Settings</label>
              <div className="mode-selector">
                <label><input type="radio" checked={!tempUseCustomPrompt} onChange={() => setTempUseCustomPrompt(false)}/> Default</label>
                <label><input type="radio" checked={tempUseCustomPrompt} onChange={() => setTempUseCustomPrompt(true)}/> Custom</label>
              </div>
            </div>

            {tempUseCustomPrompt && (
              <>
                <div className="form-group">
                  <label>Edit Mode Prompt</label>
                  <textarea value={tempCustomEditPrompt} onChange={(e) => setTempCustomEditPrompt(e.target.value)}/>
                </div>
                <div className="form-group">
                  <label>Ask Mode Prompt</label>
                  <textarea value={tempCustomAskPrompt} onChange={(e) => setTempCustomAskPrompt(e.target.value)}/>
                </div>
              </>
            )}

            <div className="button-group" style={{ marginTop: '2em', display: 'flex', gap: '1em' }}>
              <button onClick={handleSaveSettings} style={{ flex: 1, backgroundColor: '#396cd8', color: 'white' }}>Save</button>
              <button onClick={handleCancelSettings} style={{ flex: 1 }}>Cancel</button>
            </div>
            <div className="button-group" style={{ marginTop: '1em', display: 'flex', gap: '1em' }}>
              <button onClick={() => invoke("open_config_dir")} style={{ flex: 1 }}>Open Config Folder</button>
              <button onClick={() => { setTempCustomEditPrompt(DEFAULT_EDIT_PROMPT); setTempCustomAskPrompt(DEFAULT_ASK_PROMPT); }} style={{ flex: 1 }}>Reset Prompts</button>
            </div>
          </div>
        </div>
      )}

      <main className="main-content">
        <div className="card project-settings-container" style={{ display: 'flex', gap: '1em', marginBottom: '1em', paddingBottom: '0.5em' }}>
            <div className="form-group" style={{ flex: 2 }}>
                <label>Target Directory</label>
                <div className="input-group">
                    <input type="text" value={targetDir} onChange={(e) => setTargetDir(e.target.value)} placeholder="/path/to/your/project" />
                    <button onClick={handleSelectDirectory}>Select</button>
                </div>
            </div>
            <div className="form-group" style={{ flex: 1 }}>
                <label>File Encoding</label>
                <input type="text" value={fileEncoding} onChange={(e) => setFileEncoding(e.target.value)} placeholder="utf-8" />
            </div>
        </div>
        
        <div className="tabs-container">
          <div className="tabs">
            <button className={`tab-button ${activeTab === 'A' ? 'active' : ''}`} onClick={() => setActiveTab('A')}>RepoMap / Prompt</button>
            <button className={`tab-button ${activeTab === 'B' ? 'active' : ''}`} onClick={() => setActiveTab('B')}>Add Files</button>
            <button className={`tab-button ${activeTab === 'C' ? 'active' : ''}`} onClick={() => setActiveTab('C')}>Apply Patch</button>
          </div>
          
          <div className="tab-content">
            {activeTab === 'A' && (
              <div className="card">
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
                  <button onClick={handleLaunchAider}>Generate RepoMap & Prompt</button>
                  {repoMapData && (
                      <div className="prompt-content" style={{ marginTop: '1em' }}>
                          <div className="info-box">
                              <h4>RepoMap File</h4>
                              <div className="draggable-file-wrapper">
                                  <div className="draggable-file" draggable={true} onDragStart={(e) => handleDragFile(e, [repoMapData.repo_map_file_path], repoMapData.icon_file_path)}>
                                      <svg width="64" height="80" viewBox="0 0 100 120"><path d="M0 4C0 1.8 1.8 0 4 0H65L100 35V116C100 118.2 98.2 120 96 120H4C1.8 120 0 118.2 0 116V4Z" fill="#CF84E1"/><path d="M65 0V31C65 33.2 66.8 35 69 35H100L65 0Z" fill="#B463C8"/><text x="50" y="78" fill="white" fontSize="36" fontFamily="monospace" textAnchor="middle" fontWeight="bold">&lt;/&gt;</text></svg>
                                      <span className="file-name">repo_map.{displayExt}</span>
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
            )}
            
            {activeTab === 'B' && (
              <div className="card">
                  <div className="form-group">
                      <label>Target Files (Space-separated)</label>
                      <input type="text" value={files} onChange={(e) => setFiles(e.target.value)} placeholder="src/main.rs src/lib.rs"/>
                  </div>
                  <button onClick={handlePackFiles}>Pack Files</button>
                  {packedFilesPaths.length > 0 && repoMapData?.icon_file_path && (
                    <div className="info-box" style={{marginTop: '1em'}}>
                        <h4>Packed Files</h4>
                        <div className="draggable-file-wrapper" style={{ flexWrap: 'wrap' }}>
                            {packedFilesPaths.length > 1 && (
                                <div className="draggable-file" style={{ backgroundColor: 'rgba(57, 108, 216, 0.1)', borderColor: '#396cd8' }} draggable={true} onDragStart={(e) => handleDragFile(e, packedFilesPaths, repoMapData.icon_file_path!)}>
                                    <svg width="64" height="80" viewBox="0 0 100 120"><path d="M0 4C0 1.8 1.8 0 4 0H65L100 35V116C100 118.2 98.2 120 96 120H4C1.8 120 0 118.2 0 116V4Z" fill="#396CD8"/><path d="M65 0V31C65 33.2 66.8 35 69 35H100L65 0Z" fill="#2952A3"/><text x="50" y="78" fill="white" fontSize="24" fontFamily="monospace" textAnchor="middle" fontWeight="bold">ALL</text></svg>
                                    <span className="file-name" style={{ fontWeight: 'bold', color: '#396cd8' }}>Drag All ({packedFilesPaths.length})</span>
                                </div>
                            )}
                            {packedFilesPaths.map((path, index) => (
                                <div key={index} className="draggable-file" draggable={true} onDragStart={(e) => handleDragFile(e, [path], repoMapData.icon_file_path!)}>
                                    <svg width="64" height="80" viewBox="0 0 100 120"><path d="M0 4C0 1.8 1.8 0 4 0H65L100 35V116C100 118.2 98.2 120 96 120H4C1.8 120 0 118.2 0 116V4Z" fill="#84A1E1"/><path d="M65 0V31C65 33.2 66.8 35 69 35H100L65 0Z" fill="#637BC8"/><text x="50" y="78" fill="white" fontSize="36" fontFamily="monospace" textAnchor="middle" fontWeight="bold">&lt;/&gt;</text></svg>
                                    <span className="file-name">target_files_{index + 1}.{displayExt}</span>
                                </div>
                            ))}
                        </div>
                    </div>
                  )}
              </div>
            )}
            
            {activeTab === 'C' && (
              <div className="card">
                  <div className="form-group">
                      <label>AI Response (SEARCH/REPLACE Blocks)</label>
                      <textarea 
                        value={aiResponse} 
                        onChange={(e) => setAiResponse(e.target.value)} 
                        placeholder="Paste the full response from your Web LLM here..."
                      />
                  </div>
                  <button onClick={handleApplyPatch} style={{ width: '100%', marginTop: '0.5em' }}>
                      Apply Patch to Files
                  </button>
              </div>
            )}
          </div>
        </div>
      </main>

      <footer className="bottom-terminal">
        <div className="terminal-header">
          <span className="terminal-title">System Logs</span>
          <button className="clear-logs-button" onClick={() => setLogs([])}>Clear</button>
        </div>
        <div className="terminal-log">
          {logs.length === 0 && (
            <div className="terminal-placeholder">Ready. Waiting for operations...</div>
          )}
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
