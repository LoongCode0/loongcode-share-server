import { useEffect, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeHighlight from "rehype-highlight";

type ThemeName = "dark" | "light" | "eyecare";
const THEME_STORAGE_KEY = "loongcode-share:theme";
const THEME_ORDER: readonly ThemeName[] = ["dark", "light", "eyecare"];
const THEME_LABELS: Record<ThemeName, string> = { dark: "深色", light: "浅色", eyecare: "护眼" };

// 按主题选择 logo / favicon 资源。映射需与客户端仓库 src/lib/logoSrc.ts 的
// LOGO_BY_THEME 保持同步（三主题各一张 256x256 PNG，未知主题一律回落 dark 版）。
const LOGO_BY_THEME: Record<string, string> = {
  dark: "/logo.png",
  light: "/logo-light.png",
  eyecare: "/logo-eyecare.png",
};
function logoSrcForTheme(theme: string): string {
  return LOGO_BY_THEME[theme] ?? "/logo.png";
}

function isThemeName(v: string | null): v is ThemeName {
  return v === "dark" || v === "light" || v === "eyecare";
}

function readStoredTheme(): ThemeName {
  try {
    const v = window.localStorage.getItem(THEME_STORAGE_KEY);
    return isThemeName(v) ? v : "dark";
  } catch {
    return "dark";
  }
}

function useTheme(): { theme: ThemeName; cycleTheme: () => void } {
  const [theme, setTheme] = useState<ThemeName>(readStoredTheme);

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    // favicon 随主题同步；index.html 的防闪烁内联脚本已在首帧前做过一次同样的事，
    // 这里是运行期切换主题时的后续同步。
    const favicon = document.querySelector<HTMLLinkElement>('link[rel="icon"]');
    if (favicon) favicon.href = logoSrcForTheme(theme);
    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, theme);
    } catch {
      // localStorage 不可用（隐私模式等）时忽略持久化，仅当次会话生效
    }
  }, [theme]);

  const cycleTheme = () => {
    setTheme((prev) => THEME_ORDER[(THEME_ORDER.indexOf(prev) + 1) % THEME_ORDER.length]);
  };

  return { theme, cycleTheme };
}

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

function ShareView({ state, theme, cycleTheme }: { state: ViewState; theme: ThemeName; cycleTheme: () => void }) {
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
        <img src={logoSrcForTheme(theme)} alt="LoongCode" className="logo" />
        <span className="brand-name">LoongCode</span>
        <span className="muted small">· 分享的对话</span>
        <button
          type="button"
          className="theme-toggle"
          onClick={cycleTheme}
          title={`当前主题：${THEME_LABELS[theme]}`}
          aria-label={`当前主题：${THEME_LABELS[theme]}，点击切换`}
        >
          ◑
        </button>
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

export function App() {
  const [state, setState] = useState<ViewState>({ kind: "loading" });
  const { theme, cycleTheme } = useTheme();

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

  return (
    <>
      <ShareView state={state} theme={theme} cycleTheme={cycleTheme} />
      <footer className="icp">
        <a href="https://beian.miit.gov.cn/" target="_blank" rel="noreferrer">湘ICP备2023030882号-2</a>
      </footer>
    </>
  );
}
