import { useEffect, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";

interface ShareMessage { role: "user" | "assistant"; text: string }
interface ShareData {
  workspaceName: string;
  taskTitle: string;
  createdAt: number;
  expiresAt: number;
  messages: ShareMessage[];
}
type ViewState = { kind: "loading" } | { kind: "notFound" } | { kind: "ok"; data: ShareData };

function parsePath(pathname: string): { device: string; share: string } | null {
  const m = pathname.match(/^\/s\/([0-9a-f]{16})\/([0-9A-Za-z]{12})$/);
  return m ? { device: m[1], share: m[2] } : null;
}

function fmt(tsSecs: number): string {
  const d = new Date(tsSecs * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function App() {
  const [state, setState] = useState<ViewState>({ kind: "loading" });

  useEffect(() => {
    const target = parsePath(window.location.pathname);
    if (!target) { setState({ kind: "notFound" }); return; }
    fetch(`/api/shares/${target.device}/${target.share}`)
      .then(async (r) => {
        if (!r.ok) { setState({ kind: "notFound" }); return; }
        const data = (await r.json()) as ShareData;
        document.title = `${data.taskTitle} · LoongCode 分享`;
        setState({ kind: "ok", data });
      })
      .catch(() => setState({ kind: "notFound" }));
  }, []);

  if (state.kind === "loading") return <div className="center muted">加载中…</div>;
  if (state.kind === "notFound") {
    return (
      <div className="center">
        <div className="nf-icon">⏳</div>
        <h1 className="nf-title">分享不存在或已过期</h1>
        <p className="muted">链接可能已失效、被撤销，或从未存在。</p>
        <a className="cta" href="https://loongcode.cc" rel="noreferrer">了解 LoongCode</a>
      </div>
    );
  }

  const { data } = state;
  return (
    <div className="page">
      <header className="brand">
        <span className="logo">龙</span>
        <span className="brand-name">LoongCode</span>
        <span className="muted small">· 分享的对话</span>
      </header>
      <section className="head">
        <div className="muted small">{data.workspaceName}</div>
        <h1 className="title">{data.taskTitle}</h1>
        <div className="muted small">
          分享于 {fmt(data.createdAt)} · {data.messages.length} 条消息 · 链接将于 {fmt(data.expiresAt)} 失效
        </div>
      </section>
      <main>
        {data.messages.map((m, i) =>
          m.role === "user" ? (
            <div key={i} className="row user"><div className="bubble">{m.text}</div></div>
          ) : (
            <div key={i} className="row assistant">
              <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
                {m.text}
              </ReactMarkdown>
            </div>
          ),
        )}
      </main>
      <footer className="foot muted small">
        <span>内容由用户主动分享，到期自动删除</span>
        <span>由 <a href="https://loongcode.cc" rel="noreferrer">LoongCode</a> 生成</span>
      </footer>
    </div>
  );
}
