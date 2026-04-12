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

type AppState = "init" | "idle" | "pending";

interface PromptPayload {
  request_id: string;
  context_file_path: string;
  icon_file_path: string;
  json_file_path: string;
  prompt: string;
}

function App() {
  const [appState, setAppState] = useState<AppState>("init");
  const [mode, setMode] = useState<"edit" | "ask">("edit");
  const [targetDir, setTargetDir] = useState("");
  const [files, setFiles] = useState("");
  const [instruction, setInstruction] = useState("");
  const [fileEncoding, setFileEncoding] = useState(() => localStorage.getItem("fileEncoding") || "");
  const [chatLanguage, setChatLanguage] = useState(() => localStorage.getItem("chatLanguage") || "English");
  const [aiderPath, setAiderPath] = useState(() => localStorage.getItem("aiderPath") || "aider");
  const [apiPort, setApiPort] = useState(() => Number(localStorage.getItem("apiPort") || 8080));
  const [promptData, setPromptData] = useState<PromptPayload | null>(null);
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
    localStorage.setItem("apiPort", String(apiPort));

    const settingsForRust = {
      use_custom: useCustomPrompt,
      custom_edit_prompt: customEditPrompt,
      custom_ask_prompt: customAskPrompt,
    };
    invoke("update_prompt_settings", { settings: settingsForRust }).catch(
      console.error
    );
  }, [useCustomPrompt, customEditPrompt, customAskPrompt, aiderPath, apiPort]);

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
    const unlisten = listen<PromptPayload>("prompt_received", (event) => {
      setLogs(prev => [...prev, `--- PromptProxy: Received request from Aider [${new Date().toLocaleTimeString()}] ---`]);
      setPromptData(event.payload);
      setAiResponse(""); // Clear previous response
      setAppState("pending");
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
    if (promptData?.prompt) {
      navigator.clipboard.writeText(promptData.prompt).catch((err) => {
        console.error("Failed to copy text: ", err);
      });
    }
  };

  const handleReset = async () => {
    await invoke("reset_aider_state");
    setAppState("init");
    setPromptData(null);
    setAiResponse("");
  };

  const sendResponseToAider = async (response: string) => {
    if (promptData?.request_id) {
      setLogs(prev => [...prev, `--- PromptProxy: Sending response to Aider [${new Date().toLocaleTimeString()}] ---`]);
      await invoke("respond_to_llm_request", {
        requestId: promptData.request_id,
        response,
      });

      setAppState("idle");
      setPromptData(null);
      setAiResponse("");
    }
  };

  const handleReturnToAider = () => {
    sendResponseToAider(aiResponse);
  };

  const handleSkip = () => {
    sendResponseToAider("Understood. No further changes needed.");
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
    const finalMessage = mode === "ask" ? `/ask ${instruction}` : instruction;
    await invoke("launch_aider_batch", {
      targetDir,
      files,
      message: finalMessage,
      chatLanguage,
      aiderPath,
      fileEncoding,
      apiPort: Number(apiPort),
    });
    setAppState("idle");
  };

  const handleDragFile = async (e: DragEvent<HTMLDivElement>) => {
    if (promptData?.context_file_path && promptData?.icon_file_path) {
      const isLinux = navigator.userAgent.toLowerCase().includes("linux");

      if (isLinux) {
        // Linux (X11): WebKitGTKのネイティブドラッグプロトコルに任せ、正しいイベントコンテキストを保持する
        const uri = 'file://' + promptData.context_file_path;
        e.dataTransfer?.setData('text/uri-list', uri + '\r\n');
        e.dataTransfer?.setData('text/plain', promptData.context_file_path);
        // e.preventDefault() は呼ばない！
      } else {
        // Windows/Mac: Tauriのネイティブプラグインを使用
        e.preventDefault();
        try {
          await startDrag({
            item: [promptData.context_file_path],
            icon: promptData.icon_file_path,
          });
        } catch (error) {
          console.error("Drag failed:", error);
        }
      }
    }
  };

  return (
    <div className="app-container">
      <header className="status-header">
        <div className="stepper">
          <span className={appState === "init" ? "active" : ""}>1. User Input</span> ＞ 
          <span className={appState === "idle" ? "active" : ""}>2. Aider Running</span> ＞ 
          <span className={appState === "pending" ? "active" : ""}>3. LLM Proxy Response</span>
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
          <>
            {/* 1. ユーザー指示領域 */}
            <div className={`card ${appState !== "init" ? "inactive" : ""}`}>
              <h3>1. User Input</h3>
              <div className="form-group">
                <label>Target Project Directory</label>
                <div className="input-group">
                  <input
                    type="text"
                    value={targetDir}
                    onChange={(e) => setTargetDir(e.target.value)}
                    placeholder="/path/to/your/project"
                  />
                  <button onClick={handleSelectDirectory}>Select Folder</button>
                </div>
              </div>
              <div className="form-group">
                <label>File Encoding (e.g. cp932. Leave blank for default)</label>
                <input
                  type="text"
                  value={fileEncoding}
                  onChange={(e) => setFileEncoding(e.target.value)}
                  placeholder="Leave blank for default"
                />
              </div>
              <div className="form-group">
                <label>Target Files (Space-separated for multiple. Can be blank)</label>
                <input
                  type="text"
                  value={files}
                  onChange={(e) => setFiles(e.target.value)}
                  placeholder="src/main.rs src/lib.rs"
                />
              </div>
              <div className="form-group">
                <label>Operation Mode</label>
                <div className="mode-selector">
                  <label>
                    <input
                      type="radio"
                      value="edit"
                      checked={mode === 'edit'}
                      onChange={() => setMode('edit')}
                    />
                    Edit Code (Edit)
                  </label>
                  <label>
                    <input
                      type="radio"
                      value="ask"
                      checked={mode === 'ask'}
                      onChange={() => setMode('ask')}
                    />
                    Ask about Repository (Ask)
                  </label>
                </div>
              </div>
              <div className="form-group">
                <label>Instructions for Aider</label>
                <textarea
                  value={instruction}
                  onChange={(e) => setInstruction(e.target.value)}
                  placeholder="Fix the bug in..."
                />
              </div>
              <button onClick={handleLaunchAider}>
                Run Aider in Background
              </button>
            </div>

            {/* 2. LLMへの指示領域 */}
            <div className={`card ${appState !== "pending" ? "inactive" : ""}`}>
              <h3>2. Instructions for LLM (Aider -&gt; LLM)</h3>
              {promptData ? (
                <div className="prompt-content">
                  <div className="info-box">
                    <h4>Context File</h4>
                    <div className="draggable-file-wrapper">
                      <div
                        className="draggable-file"
                        draggable={true}
                        onDragStart={handleDragFile}
                      >
                        <svg width="64" height="80" viewBox="0 0 100 120" xmlns="http://www.w3.org/2000/svg">
                          <path d="M0 4C0 1.8 1.8 0 4 0H65L100 35V116C100 118.2 98.2 120 96 120H4C1.8 120 0 118.2 0 116V4Z" fill="#CF84E1" />
                          <path d="M65 0V31C65 33.2 66.8 35 69 35H100L65 0Z" fill="#B463C8" />
                          <text x="50" y="78" fill="white" fontSize="36" fontFamily="monospace" textAnchor="middle" fontWeight="bold">&lt;/&gt;</text>
                        </svg>
                        <span className="file-name">context.xml</span>
                      </div>
                    </div>
                  </div>
                  <div className="info-box">
                    <h4>Prompt</h4>
                    <div className="prompt-display">
                      <pre>{promptData.prompt}</pre>
                      <button onClick={handleCopyToClipboard}>Copy</button>
                    </div>
                  </div>
                </div>
              ) : appState === "idle" ? (
                <p className="placeholder-text">⏳ Aider is generating context...</p>
              ) : (
                <p className="placeholder-text">Prompt will be generated here when Aider runs.</p>
              )}
            </div>

            {/* 3. LLMからの指示領域 */}
            <div className={`card ${appState !== "pending" ? "inactive" : ""}`}>
              <h3>3. Response from LLM (LLM -&gt; Aider)</h3>
              <div className="response-content">
                <textarea
                  value={aiResponse}
                  onChange={(e) => setAiResponse(e.target.value)}
                  placeholder="Paste AI response here..."
                  disabled={appState !== "pending"}
                />
                <div className="button-group">
                  <button onClick={handleReturnToAider} disabled={appState !== "pending"}>Return to Aider</button>
                  <button onClick={handleSkip} disabled={appState !== "pending"}>Skip and return dummy response</button>
                </div>
              </div>
            </div>
          </>
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
