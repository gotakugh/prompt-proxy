import { useState, useEffect, DragEvent, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import "./App.css";

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
  const [promptData, setPromptData] = useState<PromptPayload | null>(null);
  const [aiResponse, setAiResponse] = useState("");
  const [logs, setLogs] = useState<string[]>([]);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

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

    const unlistenFileAdded = listen<string>("file_added_by_ai", (event) => {
      setFiles(event.payload);
      setAppState("idle");
      setPromptData(null);
      setAiResponse("");
      setLogs(prev => [...prev, `[PromptProxy] 🔄 ファイルを追加してAiderを再起動しています...`]);
    });

    return () => {
      unlisten.then((fn) => fn());
      unlistenAiderLog.then((fn) => fn());
      unlistenFileAdded.then((fn) => fn());
    };
  }, []);

  const handleCopyToClipboard = () => {
    if (promptData?.prompt) {
      navigator.clipboard.writeText(promptData.prompt).catch((err) => {
        console.error("Failed to copy text: ", err);
      });
    }
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

  const handleDragJsonFile = async (e: DragEvent<HTMLDivElement>) => {
    if (promptData?.json_file_path && promptData?.icon_file_path) {
      const isLinux = navigator.userAgent.toLowerCase().includes("linux");

      if (isLinux) {
        // Linux (X11): WebKitGTKのネイティブドラッグプロトコルに任せ、正しいイベントコンテキストを保持する
        const uri = 'file://' + promptData.json_file_path;
        e.dataTransfer?.setData('text/uri-list', uri + '\r\n');
        e.dataTransfer?.setData('text/plain', promptData.json_file_path);
        // e.preventDefault() は呼ばない！
      } else {
        // Windows/Mac: Tauriのネイティブプラグインを使用
        e.preventDefault();
        try {
          await startDrag({
            item: [promptData.json_file_path],
            icon: promptData.icon_file_path,
          });
        } catch (error) {
          console.error("Drag failed:", error);
        }
      }
    }
  };

  return (
    <main className="app-layout">
      {/* 左側：メインの操作領域 */}
      <div className="main-pane">
        <h1>LLM Prompt Proxy</h1>

        {appState === "init" && (
          <div className="init-container">
            <h2>Aider 起動</h2>
            <div className="form-group">
              <label>対象プロジェクトのディレクトリパス</label>
              <div className="input-group">
                <input
                  type="text"
                  value={targetDir}
                  onChange={(e) => setTargetDir(e.target.value)}
                  placeholder="/path/to/your/project"
                />
                <button onClick={handleSelectDirectory}>フォルダを選択</button>
              </div>
            </div>
            <div className="form-group">
              <label>対象ファイル（複数ある場合はスペース区切り。空欄でも可）</label>
              <input
                type="text"
                value={files}
                onChange={(e) => setFiles(e.target.value)}
                placeholder="src/main.rs src/lib.rs"
              />
            </div>
            <div className="form-group">
              <label>操作モード</label>
              <div className="mode-selector">
                <label>
                  <input
                    type="radio"
                    value="edit"
                    checked={mode === 'edit'}
                    onChange={() => setMode('edit')}
                  />
                  コードを修正する (Edit)
                </label>
                <label>
                  <input
                    type="radio"
                    value="ask"
                    checked={mode === 'ask'}
                    onChange={() => setMode('ask')}
                  />
                  リポジトリについて質問する (Ask)
                </label>
              </div>
            </div>
            <div className="form-group">
              <label>Aiderへの指示</label>
              <textarea
                value={instruction}
                onChange={(e) => setInstruction(e.target.value)}
                placeholder="〇〇のバグを直して..."
              />
            </div>
            <button onClick={handleLaunchAider}>
              Aiderをバックグラウンドで実行
            </button>
          </div>
        )}

        {appState === "idle" && (
          <div className="idle-container">
            <h2>待機中 (Idle)</h2>
            <p>Aiderからのリクエストを待っています...</p>
          </div>
        )}

        {appState === "pending" && promptData && (
          <div className="pending-container">
            <h2>入力待ち (Pending)</h2>

            <div className="info-box">
              <h3>コンテキストファイル</h3>
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
              <h3>プロンプト</h3>
              <div className="prompt-display">
                <pre>{promptData.prompt}</pre>
                <button onClick={handleCopyToClipboard}>コピー</button>
              </div>
            </div>

            <div className="response-box">
              <h3>AIからの返答 (SEARCH/REPLACE)</h3>
              <textarea
                value={aiResponse}
                onChange={(e) => setAiResponse(e.target.value)}
                placeholder="ここにAIの返答を貼り付けてください..."
              />
              <div className="button-group">
                <button onClick={handleReturnToAider}>Aiderに返す</button>
                <button onClick={handleSkip}>スキップして適当に返す</button>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* 右側：ターミナルとデバッグ領域（常時表示） */}
      <div className="terminal-pane">
        <div className="terminal-log">
          {logs.map((log, index) => (
            <div key={index}>{log}</div>
          ))}
          <div ref={messagesEndRef} />
        </div>
        {promptData && (
          <div className="debug-files">
            <div
              className="draggable-file"
              draggable={true}
              onDragStart={handleDragJsonFile}
            >
              <svg width="64" height="80" viewBox="0 0 100 120" xmlns="http://www.w3.org/2000/svg">
                <path d="M0 4C0 1.8 1.8 0 4 0H65L100 35V116C100 118.2 98.2 120 96 120H4C1.8 120 0 118.2 0 116V4Z" fill="#84A8E1" />
                <path d="M65 0V31C65 33.2 66.8 35 69 35H100L65 0Z" fill="#6388C8" />
                <text x="50" y="82" fill="white" fontSize="30" fontFamily="monospace" textAnchor="middle" fontWeight="bold">JSON</text>
              </svg>
              <span className="file-name">aider_payload.json</span>
            </div>
          </div>
        )}
      </div>
    </main>
  );
}

export default App;
