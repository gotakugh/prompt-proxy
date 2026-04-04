import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

type AppState = "init" | "idle" | "pending";

interface PromptPayload {
  request_id: string;
  context_file_path: string;
  prompt: string;
}

function App() {
  const [appState, setAppState] = useState<AppState>("init");
  const [targetDir, setTargetDir] = useState("");
  const [files, setFiles] = useState("");
  const [instruction, setInstruction] = useState("");
  const [promptData, setPromptData] = useState<PromptPayload | null>(null);
  const [aiResponse, setAiResponse] = useState("");

  useEffect(() => {
    const unlisten = listen<PromptPayload>("prompt_received", (event) => {
      setPromptData(event.payload);
      setAiResponse(""); // Clear previous response
      setAppState("pending");
    });
    return () => {
      unlisten.then((fn) => fn());
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
    await invoke("launch_aider_batch", {
      targetDir,
      files,
      message: instruction,
    });
    setAppState("idle");
  };

  return (
    <main className="container">
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
            <p>このファイルをブラウザに添付してください:</p>
            <code>{promptData.context_file_path}</code>
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
    </main>
  );
}

export default App;
