/** Shared Nocturne tokens and overlay styles for the shell and every tab. */
export const ERNO_DEVTOOLS_STYLES = `
  :host {
    --dt-bg: #161826;
    --dt-surface: #232532;
    --dt-text: #e9e9ed;
    --dt-accent: #9184d9;
    --dt-accent-200: #e7e5fe;
    --dt-accent-800: #423a6a;
    --dt-accent-100: #f5f4ff;
    --dt-n400: #b2b6ca;
    --dt-n500: #9397ab;
    --dt-n600: #75798c;
    --dt-n700: #595d6c;
    --dt-n800: #3f424d;
    --dt-n900: #292b31;
    --dt-div: color-mix(in srgb, #e9e9ed 16%, transparent);
    --dt-font: Inter, system-ui, sans-serif;
    --dt-mono: "JetBrains Mono", ui-monospace, SFMono-Regular, Menlo, monospace;
    --dt-ok: oklch(0.755 0.085 168);
    --dt-warn: oklch(0.80 0.095 82);
    --dt-err: oklch(0.695 0.125 22);
    --dt-radius: 14px;
  }

  .panel, .pill {
    position: fixed;
    right: 28px;
    bottom: 28px;
    z-index: 9999;
    color: var(--dt-text);
    font-family: var(--dt-font);
    animation: dt-rise 0.22s ease-out;
  }

  .panel {
    width: min(400px, calc(100vw - 32px));
    display: flex;
    flex-direction: column;
    border-radius: var(--dt-radius);
    background: var(--dt-bg);
    box-shadow: 0 0 0 1px var(--dt-n800), 0 16px 40px rgba(0, 0, 0, 0.65);
    overflow: hidden;
  }

  .head, .foot {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 11px 12px 11px 14px;
    background: var(--dt-surface);
  }
  .head { border-bottom: 1px solid var(--dt-div); }
  .foot { border-top: 1px solid var(--dt-div); padding: 8px 12px 8px 14px; }

  .health {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex: none;
    background: var(--health, var(--dt-ok));
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--health, var(--dt-ok)) 20%, transparent);
  }

  .title {
    font-size: 13px;
    font-weight: 500;
    letter-spacing: 0.01em;
  }
  .ver {
    font-family: var(--dt-mono);
    font-size: 10px;
    color: var(--dt-n600);
  }
  .head-acts { margin-left: auto; display: flex; align-items: center; gap: 2px; }

  .tabs {
    display: flex;
    gap: 2px;
    padding: 9px 12px 0;
    overflow-x: auto;
    flex-wrap: nowrap;
    scrollbar-width: thin;
    scrollbar-color: var(--dt-n800) transparent;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 11px;
    cursor: pointer;
    font: 12px/1.2 var(--dt-font);
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--dt-n500);
    flex: none;
  }
  .tab.on {
    background: color-mix(in srgb, var(--dt-accent) 16%, transparent);
    color: var(--dt-accent-200);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--dt-accent) 50%, transparent);
  }
  .count {
    font-family: var(--dt-mono);
    font-size: 10px;
    padding: 1px 5px;
    border-radius: 999px;
    background: var(--dt-n900);
    color: var(--dt-n500);
  }
  .count.accent { background: var(--dt-accent-800); color: var(--dt-accent-100); }
  .count.err { background: color-mix(in srgb, var(--dt-err) 22%, transparent); color: var(--dt-err); }

  .rule { height: 1px; background: var(--dt-div); margin-top: 9px; }

  .body {
    max-height: 300px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    scrollbar-width: thin;
    scrollbar-color: var(--dt-n800) transparent;
  }
  .panel.tall .body { max-height: 460px; }

  .srow {
    display: grid;
    grid-template-columns: 76px minmax(0, 1fr) auto;
    gap: 0 10px;
    align-items: baseline;
    padding: 8px 14px;
    border-bottom: 1px solid color-mix(in srgb, var(--dt-div) 55%, transparent);
  }
  .srow:hover { background: color-mix(in srgb, var(--dt-text) 5%, transparent); }
  .skey { font-family: var(--dt-mono); font-size: 11px; color: var(--dt-n600); }
  .sval { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .smain { font-size: 13px; font-family: var(--dt-mono); }
  .sdetail { font-size: 11px; color: var(--dt-n600); text-wrap: pretty; }
  .smeta { font-family: var(--dt-mono); font-size: 11px; color: var(--dt-n700); white-space: nowrap; }

  .sync-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 12px 14px 10px;
  }
  .sync-note { font-size: 11px; color: var(--dt-n600); }

  .erow {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 10px 14px;
    border-bottom: 1px solid color-mix(in srgb, var(--dt-div) 55%, transparent);
    cursor: pointer;
  }
  .erow:hover { background: color-mix(in srgb, var(--dt-text) 5%, transparent); }
  .erow:hover .eact { opacity: 1; }
  .eline { display: flex; align-items: baseline; gap: 8px; }
  .esubj {
    font-size: 13px;
    font-weight: 500;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .esubj.read { color: var(--dt-n400); }
  .udot { width: 5px; height: 5px; border-radius: 50%; background: var(--dt-accent); flex: none; }
  .etime { margin-left: auto; font-family: var(--dt-mono); font-size: 11px; color: var(--dt-n700); flex: none; }
  .eto {
    font-family: var(--dt-mono);
    font-size: 11px;
    color: var(--dt-n500);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .eact {
    margin-left: auto;
    display: flex;
    gap: 8px;
    opacity: 0.35;
    transition: opacity 0.15s;
    flex: none;
  }

  .jbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 14px;
    border-bottom: 1px solid var(--dt-div);
  }
  .filter {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 8px;
    font: 12px var(--dt-mono);
    color: var(--dt-text);
    background: var(--dt-bg);
    border: 1px solid var(--dt-div);
    border-radius: 8px;
  }
  .filter:focus-visible { outline: 2px solid var(--dt-accent); outline-offset: 0; border-color: var(--dt-accent); }
  .chip {
    padding: 4px 8px;
    font: 11px var(--dt-mono);
    cursor: pointer;
    border-radius: 4px;
    border: 1px solid var(--dt-div);
    background: transparent;
    color: var(--dt-n600);
    white-space: nowrap;
    flex: none;
  }
  .chip.on { border-color: var(--dt-accent); color: var(--dt-accent); }

  .jkind { border-bottom: 1px solid color-mix(in srgb, var(--dt-div) 55%, transparent); }
  .jrow {
    display: grid;
    grid-template-columns: 12px minmax(0, 1fr) auto auto;
    gap: 0 9px;
    align-items: center;
    padding: 7px 14px;
    cursor: pointer;
  }
  .jrow:hover { background: color-mix(in srgb, var(--dt-text) 5%, transparent); }
  .caret {
    font-size: 9px;
    color: var(--dt-n600);
    transition: transform 0.15s;
  }
  .caret.open { transform: rotate(90deg); }
  .jname {
    display: flex;
    align-items: baseline;
    gap: 7px;
    min-width: 0;
    font-family: var(--dt-mono);
    font-size: 12px;
    font-weight: 500;
  }
  .jname > span:first-child {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .xcount {
    font-family: var(--dt-mono);
    font-size: 10px;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--dt-n900);
    color: var(--dt-n500);
    flex: none;
  }
  .jstat { font-size: 11px; white-space: nowrap; }
  .jstat.pulse { animation: dt-pulse 1.4s infinite; }
  .jtime { font-family: var(--dt-mono); font-size: 11px; color: var(--dt-n700); white-space: nowrap; }

  .jexp {
    display: flex;
    flex-direction: column;
    padding: 2px 14px 9px 35px;
    animation: dt-rise 0.18s ease-out;
  }
  .run {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 54px 62px;
    gap: 0 8px;
    align-items: baseline;
    padding: 3px 0;
    font-family: var(--dt-mono);
    font-size: 11px;
  }
  .run-id { color: var(--dt-n600); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .run-ms { color: var(--dt-n600); text-align: right; }
  .run-ms.warn { color: var(--dt-warn); }
  .run-st { text-align: right; }

  .errbox {
    margin-top: 7px;
    padding: 8px 10px;
    border-radius: 8px;
    border: 1px solid color-mix(in srgb, var(--dt-err) 45%, transparent);
    background: color-mix(in srgb, var(--dt-err) 9%, transparent);
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .err-msg { font-family: var(--dt-mono); font-size: 11px; color: #d2cefd; text-wrap: pretty; }
  .err-acts { display: flex; gap: 6px; padding-top: 2px; }

  .empty {
    padding: 28px 14px;
    display: flex;
    flex-direction: column;
    gap: 5px;
    align-items: center;
    text-align: center;
  }
  .empty-title { font-size: 13px; color: var(--dt-n500); }
  .empty-sub { font-size: 11px; color: var(--dt-n700); }

  .fnote {
    font-family: var(--dt-mono);
    font-size: 11px;
    color: var(--dt-n600);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pill {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 8px 13px;
    border-radius: 999px;
    border: 1px solid var(--dt-n800);
    background: var(--dt-surface);
    box-shadow: 0 0 0 1px #595d6c, 0 6px 18px rgba(0, 0, 0, 0.55);
    color: var(--dt-text);
    font: 12px var(--dt-font);
    cursor: pointer;
  }
  .pill:hover { border-color: var(--dt-accent); }
  .pcnt { font-family: var(--dt-mono); font-size: 11px; color: var(--dt-n500); }

  button { font-family: inherit; }
  .ghost, .primary, .secondary {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 8px;
  }
  .ghost {
    height: 26px;
    padding: 0 8px;
    font-size: 11px;
    color: var(--dt-n500);
  }
  .ghost.icon { width: 26px; padding: 0; font-size: 14px; }
  .ghost.sm { height: 22px; padding: 0 6px; font-size: 11px; }
  .ghost.mute { color: var(--dt-n500); }
  .ghost:hover { background: color-mix(in srgb, var(--dt-accent) 10%, transparent); }
  .primary {
    height: 32px;
    padding: 0 12px;
    font-size: 12px;
    color: var(--dt-accent);
    border-color: var(--dt-accent);
  }
  .primary:hover { background: color-mix(in srgb, var(--dt-accent) 12%, transparent); }
  .primary:disabled { opacity: 0.45; cursor: not-allowed; }
  .secondary {
    margin-left: auto;
    height: 26px;
    padding: 0 10px;
    font-size: 11px;
    flex: none;
    border-color: var(--dt-div);
    color: var(--dt-text);
  }
  .secondary:hover { background: color-mix(in srgb, var(--dt-text) 7%, transparent); }

  .spin {
    display: inline-block;
    width: 10px;
    height: 10px;
    margin-right: 7px;
    border: 1.5px solid var(--dt-accent);
    border-top-color: transparent;
    border-radius: 50%;
    animation: dt-spin 0.7s linear infinite;
  }

  .acts {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    padding: 12px 14px 10px;
  }

  :host :focus-visible { outline: 2px solid var(--dt-accent); outline-offset: 2px; }

  @keyframes dt-rise {
    from { opacity: 0; transform: translateY(6px); }
    to { opacity: 1; transform: none; }
  }
  @keyframes dt-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.35; }
  }
  @keyframes dt-spin { to { transform: rotate(360deg); } }
`;
