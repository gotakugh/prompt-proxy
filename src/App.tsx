import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

type AppState = "idle" | "pending";

interface PromptPayload {
  request_id: string;
  context_file_path: string;
  prompt: string;
}

function App() {
  const [appState, setAppState] = useState<AppState>("idle");
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

  return (
    <main className="container">
      <h1>LLM Prompt Proxy</h1>

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
