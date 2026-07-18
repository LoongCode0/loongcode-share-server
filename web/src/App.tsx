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

interface ShareToolSummaryPart { text: string; tone: "ok" | "err" | "muted" }
interface ShareToolPayload {
  name: string;
  target?: string;
  summary?: ShareToolSummaryPart[];
  status: "ok" | "err" | "running";
}
interface ShareMessage { role: "user" | "assistant" | "tool"; text: string; tool?: ShareToolPayload }
interface ShareData {
  workspaceName: string;
  taskTitle: string;
  createdAt: number;
  expiresAt: number;
  messages: ShareMessage[];
}
type ViewState =
  | { kind: "loading" }
  | { kind: "notFound" }
  | { kind: "passwordPrompt"; wrongAttempt: boolean }
  | { kind: "ok"; data: ShareData };

function parsePath(pathname: string): { device: string; share: string } | null {
  const m = pathname.match(/^\/s\/([0-9a-f]{16})\/([0-9A-Za-z]{12})$/);
  return m ? { device: m[1], share: m[2] } : null;
}

function passwordStorageKey(device: string, share: string): string {
  return `loongcode-share:pwd:${device}/${share}`;
}

function readStoredPassword(device: string, share: string): string | null {
  try {
    return window.localStorage.getItem(passwordStorageKey(device, share));
  } catch {
    return null;
  }
}

function storePassword(device: string, share: string, password: string): void {
  try {
    window.localStorage.setItem(passwordStorageKey(device, share), password);
  } catch {
    // localStorage 不可用（隐私模式等）时忽略持久化，仅当次会话生效
  }
}

function clearStoredPassword(device: string, share: string): void {
  try {
    window.localStorage.removeItem(passwordStorageKey(device, share));
  } catch {
    // 同上
  }
}

function fmt(tsSecs: number): string {
  const d = new Date(tsSecs * 1000);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function PasswordPrompt({ wrongAttempt, onSubmit }: { wrongAttempt: boolean; onSubmit: (password: string) => void }) {
  const [value, setValue] = useState("");
  return (
    <div className="center">
      <div className="nf-icon">🔒</div>
      <h1 className="nf-title">此分享需要访问密码</h1>
      <p className="muted">向分享者索取密码后在下方输入</p>
      <form
        className="pwd-form"
        onSubmit={(e) => {
          e.preventDefault();
          const v = value.trim();
          if (v !== "") onSubmit(v);
        }}
      >
        <input
          className="pwd-input"
          type="password"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder="请输入访问密码"
          autoFocus
        />
        <button type="submit" className="pwd-submit">解锁</button>
      </form>
      {wrongAttempt && <p className="pwd-error">密码错误，请重试</p>}
    </div>
  );
}

/** 连续 tool 消息聚为一组（左侧竖线分组，同一回合的工具行视觉成组）。 */
type MsgBlock = { kind: "single"; m: ShareMessage } | { kind: "tools"; items: ShareMessage[] };
function groupMessages(messages: ShareMessage[]): MsgBlock[] {
  const blocks: MsgBlock[] = [];
  for (const m of messages) {
    if (m.role === "tool") {
      const last = blocks[blocks.length - 1];
      if (last && last.kind === "tools") last.items.push(m);
      else blocks.push({ kind: "tools", items: [m] });
    } else {
      blocks.push({ kind: "single", m });
    }
  }
  return blocks;
}

function toolStatusLabel(status: ShareToolPayload["status"]): string {
  return status === "ok" ? "成功" : status === "err" ? "失败" : "运行中";
}

/** 只读工具行：状态符 + 名称 + 目标（mono 截断）+ 摘要分段。不可点、不可展开。 */
function ToolGroup({ items }: { items: ShareMessage[] }) {
  // 防御：role=tool 但缺 tool 字段的畸形行跳过（正常 payload 服务器已拦，不会走到）
  const rows = items.filter((m) => m.tool);
  if (rows.length === 0) return null;
  return (
    <div className="tool-group">
      {rows.map((m, i) => {
        const tl = m.tool!;
        return (
          <div key={i} className="tool-row">
            <span className="sr-only">{toolStatusLabel(tl.status)}</span>
            <span className={`tool-sym tool-sym-${tl.status}`} aria-hidden>
              {tl.status === "ok" ? "✓" : tl.status === "err" ? "✗" : "…"}
            </span>
            <span className="tool-name">{tl.name}</span>
            {tl.target && (
              <span className="tool-target" title={tl.target}>{tl.target}</span>
            )}
            {tl.summary?.map((p, j) => (
              <span key={j} className={`tool-part tool-part-${p.tone}`}>{p.text}</span>
            ))}
          </div>
        );
      })}
    </div>
  );
}

function ShareView({
  state, theme, cycleTheme, onSubmitPassword,
}: {
  state: ViewState;
  theme: ThemeName;
  cycleTheme: () => void;
  onSubmitPassword: (password: string) => void;
}) {
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
  if (state.kind === "passwordPrompt") {
    return <PasswordPrompt wrongAttempt={state.wrongAttempt} onSubmit={onSubmitPassword} />;
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
        {groupMessages(data.messages).map((b, i) =>
          b.kind === "tools" ? (
            <ToolGroup key={i} items={b.items} />
          ) : b.m.role === "user" ? (
            <div key={i} className="row user"><div className="bubble">{b.m.text}</div></div>
          ) : (
            <div key={i} className="row assistant">
              <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeHighlight]}>
                {b.m.text}
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
  const target = parsePath(window.location.pathname);

  async function load(device: string, share: string, password: string | null) {
    try {
      const headers: Record<string, string> = {};
      if (password) headers["X-Share-Password"] = password;
      const r = await fetch(`/api/shares/${device}/${share}`, { headers });
      if (r.status === 404) { clearStoredPassword(device, share); setState({ kind: "notFound" }); return; }
      if (r.status === 401) {
        if (password) clearStoredPassword(device, share);
        setState({ kind: "passwordPrompt", wrongAttempt: !!password });
        return;
      }
      if (!r.ok) { setState({ kind: "notFound" }); return; }
      const data = (await r.json()) as ShareData;
      if (password) storePassword(device, share, password);
      document.title = `${data.taskTitle} · LoongCode 分享`;
      setState({ kind: "ok", data });
    } catch {
      setState({ kind: "notFound" });
    }
  }

  useEffect(() => {
    if (!target) { setState({ kind: "notFound" }); return; }
    const hashParams = new URLSearchParams(window.location.hash.replace(/^#/, ""));
    const hashPassword = hashParams.get("pwd");
    if (hashPassword) {
      // 不等验证结果——无论接下来验证成不成功，地址栏都不应该残留密码片段。
      window.history.replaceState(null, "", window.location.pathname);
    }
    const password = hashPassword ?? readStoredPassword(target.device, target.share);
    void load(target.device, target.share, password);
  }, []);

  return (
    <>
      <ShareView
        state={state}
        theme={theme}
        cycleTheme={cycleTheme}
        onSubmitPassword={(password) => {
          if (!target) return;
          setState({ kind: "loading" });
          void load(target.device, target.share, password);
        }}
      />
      <footer className="icp">
        <a href="https://beian.miit.gov.cn/" target="_blank" rel="noreferrer">湘ICP备2023030882号-2</a>
      </footer>
    </>
  );
}
