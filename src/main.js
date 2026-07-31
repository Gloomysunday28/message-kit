const { invoke } = window.__TAURI__.core;

const $ = (id) => document.getElementById(id);
const SELF_BUNDLE_ID = "com.weiguang.lingdongdao";
const isDetailsMode = new URLSearchParams(location.search).get("mode") === "details";
const PET_IDLE_SECONDS = 10;
const PET_SLEEPY_SECONDS = 45 * 60;
const PET_REACTIONS = [
  { mood: "happy", message: "喵，继续加油" },
  { mood: "love", message: "收到摸摸啦" },
  { mood: "surprised", message: "呀！被发现了" },
  { mood: "happy", message: "陪你一会儿" },
];

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
  idleSeconds: 0,
  petMood: "peek",
  petOverrideUntil: 0,
  petReactionIndex: 0,
  petMessageTimer: null,
  petPressTimer: null,
  petYawning: false,
  petFallbackStartedAt: Date.now(),
};

function renderApp(app) {
  if (!app || app.bundleId === SELF_BUNDLE_ID) return;
  const changed = state.app?.bundleId !== app.bundleId;
  if (changed) {
    state.appStartedAt = Date.now();
    state.petFallbackStartedAt = Date.now();
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
  if (activity?.active) state.petFallbackStartedAt = Date.now();
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

function isLateNight(date = new Date()) {
  const hour = date.getHours();
  return hour >= 23 || hour < 6;
}

function focusedSeconds() {
  return Math.max(0, Math.floor((Date.now() - state.appStartedAt) / 1000));
}

function setPetMessage(message, duration = 2600) {
  const bubble = $("pet-bubble");
  bubble.textContent = message;
  bubble.classList.toggle("visible", Boolean(message));
  clearTimeout(state.petMessageTimer);
  if (message) {
    state.petMessageTimer = setTimeout(() => {
      bubble.classList.remove("visible");
    }, duration);
  }
}

function renderPet() {
  if (isDetailsMode) return;
  const now = Date.now();
  const isWorking = focusedSeconds() >= PET_SLEEPY_SECONDS;
  const isIdle = state.idleSeconds >= PET_IDLE_SECONDS;
  const isInteracting = now < state.petOverrideUntil;
  const visible = isIdle || isWorking || isInteracting || state.petYawning;

  let mood = state.petMood;
  if (!isInteracting) {
    if (state.petYawning) mood = "yawn";
    else if (focusedSeconds() >= PET_SLEEPY_SECONDS) mood = "sleepy";
    else if (isIdle) mood = "peek";
    else mood = "hidden";
  }

  state.petMood = mood;
  document.documentElement.classList.toggle("pet-active", visible);
  const pet = $("island-pet");
  const previousMood = pet.dataset.mood;
  pet.dataset.mood = mood;
  pet.setAttribute("aria-hidden", String(!visible));
  pet.tabIndex = visible ? 0 : -1;
  if (mood === "sleepy" && previousMood !== "sleepy") {
    setPetMessage("工作很久啦，眯一下", 3200);
  }
}

async function pollSystemIdle() {
  if (isDetailsMode) return;
  try {
    state.idleSeconds = await invoke("system_idle_seconds");
  } catch {
    state.idleSeconds = Math.floor((Date.now() - state.petFallbackStartedAt) / 1000);
  }
  renderPet();
}

function reactPet(mood, message, duration = 3600) {
  state.petMood = mood;
  state.petOverrideUntil = Date.now() + duration;
  setPetMessage(message);
  renderPet();
}

function triggerPetClick() {
  const reaction = PET_REACTIONS[state.petReactionIndex % PET_REACTIONS.length];
  state.petReactionIndex += 1;
  reactPet(reaction.mood, reaction.message);
}

function scheduleLateNightYawn() {
  if (!isLateNight() || isDetailsMode) return;
  state.petYawning = true;
  state.petOverrideUntil = Date.now() + 5200;
  setPetMessage("哈——该休息啦", 3600);
  renderPet();
  setTimeout(() => {
    state.petYawning = false;
    renderPet();
  }, 4800);
}

function updatePetGaze(event) {
  const pet = $("island-pet");
  const bounds = pet.getBoundingClientRect();
  const x = Math.max(-1, Math.min(1, (event.clientX - bounds.left) / bounds.width * 2 - 1));
  const y = Math.max(-1, Math.min(1, (event.clientY - bounds.top) / bounds.height * 2 - 1));
  pet.style.setProperty("--pet-look-x", `${(x * 1.7).toFixed(2)}px`);
  pet.style.setProperty("--pet-look-y", `${(y * 1.3).toFixed(2)}px`);
}

async function setExpanded(expanded) {
  if (isDetailsMode) return;
  await invoke("set_details_visible", { visible: expanded }).catch(() => {});
  state.expanded = expanded;
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

$("island-pet").addEventListener("pointerenter", () => {
  state.petOverrideUntil = Date.now() + 8000;
  renderPet();
});

$("island-pet").addEventListener("pointermove", updatePetGaze);

$("island-pet").addEventListener("pointerleave", () => {
  $("island-pet").style.setProperty("--pet-look-x", "0px");
  $("island-pet").style.setProperty("--pet-look-y", "0px");
});

$("island-pet").addEventListener("pointerdown", (event) => {
  event.stopPropagation();
  state.petFallbackStartedAt = Date.now();
  state.idleSeconds = 0;
  $("island-pet").setPointerCapture?.(event.pointerId);
  state.petPressTimer = setTimeout(() => {
    state.petPressTimer = null;
    reactPet("purring", "呼噜呼噜…", 5200);
  }, 620);
});

$("island-pet").addEventListener("pointerup", (event) => {
  event.stopPropagation();
  if (state.petPressTimer) {
    clearTimeout(state.petPressTimer);
    state.petPressTimer = null;
    triggerPetClick();
  }
});

$("island-pet").addEventListener("pointercancel", () => {
  clearTimeout(state.petPressTimer);
  state.petPressTimer = null;
});

$("island-pet").addEventListener("dblclick", (event) => {
  event.stopPropagation();
  reactPet("love", "今天也陪着你", 4400);
});

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
connectCodexActivity();
pollFrontmostApp();
setInterval(pollFrontmostApp, 700);
setInterval(renderFocusTime, 1000);
pollSystemIdle();
setInterval(pollSystemIdle, 2500);
setInterval(renderPet, 1000);
setTimeout(scheduleLateNightYawn, 12_000);
setInterval(scheduleLateNightYawn, 90_000);
setTimeout(() => checkWebuiUpdate({ quiet: true }), 2600);
setInterval(() => checkWebuiUpdate({ quiet: true }), 15_000);
