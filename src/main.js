const { invoke } = window.__TAURI__.core;

const $ = (id) => document.getElementById(id);
const SELF_BUNDLE_ID = "com.weiguang.lingdongdao";
const isDetailsMode = new URLSearchParams(location.search).get("mode") === "details";

if (isDetailsMode) {
  document.documentElement.classList.add("details-mode");
  $("details").setAttribute("aria-hidden", "false");
}

const state = {
  app: null,
  codex: null,
  codexIconDataUrl: null,
  expanded: false,
  appStartedAt: Date.now(),
  polling: false,
  checkingWebui: false,
};

function renderApp(app) {
  if (!app || app.bundleId === SELF_BUNDLE_ID) return;
  const changed = state.app?.bundleId !== app.bundleId;
  if (changed) {
    state.appStartedAt = Date.now();
  }
  const retainedIcon = app.iconDataUrl
    || (!changed ? state.app?.iconDataUrl : null);
  state.app = {
    ...state.app,
    ...app,
    iconDataUrl: retainedIcon,
  };
  renderPrimary();
}

function renderFocusedApp() {
  const app = state.app;
  if (!app) return;
  $("app-name").textContent = app.name;
  $("detail-name").textContent = app.name;
  $("bundle-id").textContent = app.bundleId || "无 Bundle ID";
  $("app-state").textContent = "正在前台运行";

  if (state.app.iconDataUrl) {
    $("app-icon").src = state.app.iconDataUrl;
    $("app-icon").hidden = false;
    $("app-fallback").hidden = true;
  } else {
    $("app-icon").hidden = true;
    $("app-fallback").hidden = true;
  }
}

function renderPrimary() {
  const codex = state.codex;
  const showingCodex = Boolean(codex?.visible);
  document.documentElement.classList.toggle("codex-active", showingCodex);
  document.documentElement.classList.toggle("codex-working", showingCodex && codex.active);
  document.documentElement.classList.toggle("codex-done", showingCodex && !codex.active);
  $("codex-details").hidden = !showingCodex;

  if (!showingCodex) {
    renderFocusedApp();
    return;
  }

  if (state.codexIconDataUrl) {
    $("app-icon").src = state.codexIconDataUrl;
    $("app-icon").hidden = false;
  } else {
    $("app-icon").hidden = true;
  }
  $("app-fallback").hidden = true;
  $("app-name").textContent = "Codex";
  const message = codex.message || "";
  const characters = Array.from(message);
  const compactMessage = codex.phase === "stream" && characters.length > 42
    ? `…${characters.slice(-42).join("")}`
    : message;
  const detailMessage = codex.phase === "stream" && characters.length > 360
    ? `…${characters.slice(-360).join("")}`
    : message;
  $("app-state").textContent = compactMessage;
  $("codex-status").textContent = "实时消息";
  $("codex-message").textContent = detailMessage;
  $("codex-phase").textContent = codex.active ? "实时" : "完成";
}

function updateCodexActivity(activity) {
  state.codex = activity;
  renderPrimary();
}

async function connectCodexActivity() {
  try {
    state.codexIconDataUrl = await invoke("codex_host_icon");
  } catch {}

  try {
    updateCodexActivity(await invoke("codex_current"));
  } catch {}

  try {
    const { listen } = window.__TAURI__.event;
    await listen("lingdongdao:codex-activity", ({ payload }) => {
      updateCodexActivity(payload);
    });
  } catch {
    setInterval(async () => {
      try {
        updateCodexActivity(await invoke("codex_current"));
      } catch {}
    }, 500);
  }
}

async function pollFrontmostApp() {
  if (state.polling) return;
  state.polling = true;
  try {
    const app = await invoke("frontmost_app", {
      previousBundleId: state.app?.iconDataUrl
        ? state.app.bundleId
        : null,
    });
    renderApp(app);
  } catch {
    if (!state.codex?.visible) $("app-state").textContent = "等待系统焦点";
  } finally {
    state.polling = false;
  }
}

function renderFocusTime() {
  const totalSeconds = Math.max(0, Math.floor((Date.now() - state.appStartedAt) / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  $("focus-time").textContent = hours > 0
    ? `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}`
    : `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}

async function setExpanded(expanded) {
  if (isDetailsMode) return;
  state.expanded = expanded;
  await invoke("set_details_visible", { visible: expanded }).catch(() => {});
  $("island").classList.toggle("expanded", expanded);
  $("toggle-expand").title = expanded ? "收起" : "展开";
  $("toggle-expand").setAttribute("aria-label", expanded ? "收起灵动岛" : "展开灵动岛");
}

async function applyNotchMetrics() {
  try {
    const metrics = await invoke("notch_metrics");
    document.documentElement.style.setProperty("--notch-width", `${metrics.width}px`);
    document.documentElement.style.setProperty("--notch-height", `${metrics.height}px`);
    document.documentElement.classList.toggle("has-notch", metrics.hasNotch);
  } catch {}
}

function setUpdateMessage(message, type = "") {
  const element = $("update-message");
  element.textContent = message;
  element.className = `update-message ${type}`.trim();
}

async function checkAppUpdate() {
  const button = $("check-app-update");
  button.disabled = true;
  setUpdateMessage("正在检查 App 更新…");
  try {
    const update = await invoke("check_homebrew_update");
    if (!update) {
      setUpdateMessage("已经是最新 App 版本", "success");
      return;
    }
    setUpdateMessage(`发现 ${update.latestVersion}，再次点击立即升级`, "success");
    button.dataset.install = "true";
  } catch (error) {
    setUpdateMessage(String(error), "error");
  } finally {
    button.disabled = false;
  }
}

async function installAppUpdate() {
  const button = $("check-app-update");
  button.disabled = true;
  setUpdateMessage("正在通过 Homebrew 安装，完成后会自动重启…");
  try {
    await invoke("install_homebrew_update");
  } catch (error) {
    setUpdateMessage(String(error), "error");
    button.disabled = false;
    button.dataset.install = "";
  }
}

async function checkWebuiUpdate({ quiet = false } = {}) {
  if (state.checkingWebui) return;
  state.checkingWebui = true;
  const button = $("check-ui-update");
  button.disabled = true;
  if (!quiet) setUpdateMessage("正在检查界面更新…");
  try {
    const version = await invoke("check_webui_update");
    if (version) {
      setUpdateMessage("新界面已就绪，正在应用…", "success");
      setTimeout(() => location.reload(), 350);
    } else if (!quiet) {
      setUpdateMessage("界面已经是最新版本", "success");
    }
  } catch (error) {
    if (!quiet) setUpdateMessage(String(error), "error");
  } finally {
    state.checkingWebui = false;
    button.disabled = false;
  }
}

$("toggle-expand").addEventListener("click", (event) => {
  event.stopPropagation();
  setExpanded(!state.expanded);
});

$("compact").addEventListener("dblclick", () => setExpanded(!state.expanded));

$("check-app-update").addEventListener("click", () => {
  if ($("check-app-update").dataset.install === "true") {
    installAppUpdate();
  } else {
    checkAppUpdate();
  }
});

$("check-ui-update").addEventListener("click", () => checkWebuiUpdate());
$("hide-island").addEventListener("click", () => invoke("hide_island"));

window.addEventListener("focus", pollFrontmostApp);
document.addEventListener("visibilitychange", () => {
  if (!document.hidden) {
    pollFrontmostApp();
    checkWebuiUpdate({ quiet: true });
  }
});

applyNotchMetrics();
invoke("set_pet_visible", { visible: false }).catch(() => {});
connectCodexActivity();
pollFrontmostApp();
setInterval(pollFrontmostApp, 700);
setInterval(renderFocusTime, 1000);
setTimeout(() => checkWebuiUpdate({ quiet: true }), 2600);
setInterval(() => checkWebuiUpdate({ quiet: true }), 15_000);
