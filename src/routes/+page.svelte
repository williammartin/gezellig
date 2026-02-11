<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  let inRoom = $state(true);
  let roomParticipants: string[] = $state([]);
  let musicVolume = $state(50);
  let showSettings = $state(false);
  let livekitUrl = $state("wss://gezellig-tmbd1vyo.livekit.cloud");
  let livekitToken = $state("");
  let sharedQueueRepo = $state("williammartin/gezellig-queue");
  let sharedQueueFile = $state("queue.ndjson");
  let ghPath = $state("gh");
  let setupComplete = $state(false);
  let livekitConnected = $state(false);
  let notifications: string[] = $state([]);
  let djQueueUrl = $state("");
  type SharedQueueItem = { url: string; title: string | null; id: number };
  let djQueue: SharedQueueItem[] = $state([]);
  type UpdateCheck = {
    available: boolean;
    currentVersion: string;
    latestVersion: string | null;
    dmgUrl: string | null;
  };
  type UpdateStatus = "checking" | "available" | "none";
  let updateStatus: UpdateStatus = $state("checking");
  let updateInfo: UpdateCheck | null = $state(null);
  let updateCommand = $state("");
  let startupStarted = $state(false);
  type SharedHistoryItem = { url: string; title: string | null };
  type SharedQueueState = {
    queue: SharedQueueItem[];
    nowPlaying: { title: string; url: string } | null;
    history: SharedHistoryItem[];
  };
  let nowPlaying: SharedQueueState["nowPlaying"] = $state(null);
  let history: SharedHistoryItem[] = $state([]);
  let showHistory = $state(false);
  let skipping = $state(false);
  let dragIndex: number | null = $state(null);
  let showDebug = $state(false);
  let debugLogs: string[] = $state([]);
  let participantPollInterval: ReturnType<typeof setInterval> | null = $state(null);
  let queuePollInterval: ReturnType<typeof setInterval> | null = $state(null);
  let queueWebhookUnlisten: (() => void) | null = $state(null);
  let voiceChatEnabled = $state(false);
  let micTestActive = $state(false);
  let micLevel = $state(0);
  let micPollInterval: ReturnType<typeof setInterval> | null = $state(null);
  let djBotMode = $state(false);

  function debugLog(msg: string) {
    const ts = new Date().toLocaleTimeString();
    debugLogs = [...debugLogs.slice(-99), `[${ts}] ${msg}`];
  }

  function extractIdentityFromToken(token: string): string {
    try {
      const parts = token.split('.');
      if (parts.length !== 3) return "Unknown";
      const payload = JSON.parse(atob(parts[1]));
      return payload.name || payload.sub || "Unknown";
    } catch {
      return "Unknown";
    }
  }

  let displayName = $derived(extractIdentityFromToken(livekitToken));

  async function loadMusicVolume() {
    try {
      const volume = await invoke<number>("get_music_volume");
      if (typeof volume === "number") {
        musicVolume = volume;
      }
    } catch {
      // Outside Tauri
    }
  }

  async function updateMusicVolume() {
    try {
      await invoke("set_music_volume", { volume: Math.round(Number(musicVolume)) });
    } catch {
      // Outside Tauri
    }
  }

  function startMicLevelPolling() {
    if (micPollInterval) return;
    pollMicLevel();
    micPollInterval = setInterval(pollMicLevel, 200);
  }

  function stopMicLevelPolling() {
    if (micPollInterval) {
      clearInterval(micPollInterval);
      micPollInterval = null;
    }
  }

  async function pollMicLevel() {
    try {
      const level = await invoke<number>("get_mic_level");
      micLevel = typeof level === "number" ? level : 0;
    } catch {
      // Outside Tauri
    }
  }

  // Check for saved setup on mount
  async function checkSavedSetup() {
    // Env vars take priority over localStorage
    try {
      const envConfig: Record<string, string> = await invoke("get_env_config");
      if (envConfig.livekitUrl && envConfig.livekitToken) {
        livekitUrl = envConfig.livekitUrl;
        livekitToken = envConfig.livekitToken;
        if (envConfig.sharedQueueRepo) {
          sharedQueueRepo = envConfig.sharedQueueRepo;
        }
        if (envConfig.sharedQueueFile) {
          sharedQueueFile = envConfig.sharedQueueFile;
        }
        if (envConfig.ghPath) {
          ghPath = envConfig.ghPath;
        }
        setupComplete = true;
        djBotMode = envConfig.djBot === "1";
        debugLog(`Using env var config (LIVEKIT_URL + LIVEKIT_TOKEN)`);
        await connectToLiveKit();
        return;
      }
    } catch {
      // Env config not available
    }

    try {
      const saved = localStorage.getItem("gezellig-setup");
      if (saved) {
        const data = JSON.parse(saved);
        livekitUrl = data.livekitUrl || "";
        livekitToken = data.livekitToken || "";
        sharedQueueRepo = data.sharedQueueRepo || sharedQueueRepo;
        sharedQueueFile = data.sharedQueueFile || sharedQueueFile;
        ghPath = data.ghPath || ghPath;
        if (livekitUrl && livekitToken) {
          setupComplete = true;
          connectToLiveKit();
        }
      }
    } catch {
      // No saved setup
    }
  }

  function buildUpdateCommand(info: UpdateCheck) {
    if (!info.dmgUrl || !info.latestVersion) return "";
    return `curl -sL ${info.dmgUrl} -o /tmp/Gezellig.dmg && \\\n` +
      `hdiutil attach /tmp/Gezellig.dmg -nobrowse -quiet && \\\n` +
      `cp -R "/Volumes/Gezellig/Gezellig.app" /Applications/ && \\\n` +
      `hdiutil detach "/Volumes/Gezellig" -quiet && \\\n` +
      `rm /tmp/Gezellig.dmg && \\\n` +
      `xattr -dr com.apple.quarantine /Applications/Gezellig.app && \\\n` +
      `open /Applications/Gezellig.app`;
  }

  async function startApp() {
    if (startupStarted) return;
    startupStarted = true;
    await checkSavedSetup();
    loadMusicVolume();
  }

  async function checkForUpdate() {
    try {
      const result = await invoke<UpdateCheck>("check_for_update");
      if (result.available && result.latestVersion && result.dmgUrl) {
        updateInfo = result;
        updateCommand = buildUpdateCommand(result);
        updateStatus = "available";
        return;
      }
    } catch {
      // Outside Tauri or update check failed
    }
    updateStatus = "none";
    await startApp();
  }

  onMount(async () => {
    await checkForUpdate();
  });

  async function dismissUpdate() {
    updateStatus = "none";
    await startApp();
  }

  async function copyUpdateCommand() {
    if (!updateCommand) return;
    try {
      await navigator.clipboard.writeText(updateCommand);
      addNotification("Update command copied to clipboard");
    } catch (e) {
      debugLog(`copy update command error: ${e}`);
    }
  }

  async function connectToLiveKit() {
    try {
      debugLog(`Connecting to LiveKit: ${livekitUrl}`);
      debugLog(`Token length: ${livekitToken.length}, starts with: ${livekitToken.substring(0, 20)}...`);
      await invoke("livekit_connect", { url: livekitUrl, token: livekitToken });
      livekitConnected = true;
      inRoom = true;
      addNotification('Connected to LiveKit');
      debugLog('LiveKit connected successfully');
      startParticipantPolling();
      startQueuePolling();
      await startQueueWebhookListener();
      if (djBotMode) {
        debugLog("DJ bot mode enabled");
        await startBotPlayback();
      }
    } catch (e) {
      debugLog(`LiveKit connection failed: ${e}`);
    }
  }

  function startParticipantPolling() {
    if (participantPollInterval) return;
    pollParticipants();
    participantPollInterval = setInterval(pollParticipants, 2000);
  }

  function stopParticipantPolling() {
    if (participantPollInterval) {
      clearInterval(participantPollInterval);
      participantPollInterval = null;
    }
  }

  function startQueuePolling() {
    if (queuePollInterval) return;
    refreshQueue();
    queuePollInterval = setInterval(refreshQueue, 10000);
  }

  function stopQueuePolling() {
    if (queuePollInterval) {
      clearInterval(queuePollInterval);
      queuePollInterval = null;
    }
  }

  async function startQueueWebhookListener() {
    if (queueWebhookUnlisten) return;
    try {
      queueWebhookUnlisten = await listen("shared-queue-updated", () => {
        refreshQueue();
      });
    } catch {
      // Outside Tauri
    }
  }

  function stopQueueWebhookListener() {
    if (queueWebhookUnlisten) {
      queueWebhookUnlisten();
      queueWebhookUnlisten = null;
    }
  }

  async function pollParticipants() {
    try {
      const participants: { identity: string; name: string }[] = await invoke("livekit_participants");
      roomParticipants = participants.map(p => p.name || p.identity);
    } catch {
      // Not connected
    }
  }

  async function completeSetup() {
    localStorage.setItem("gezellig-setup", JSON.stringify({
      livekitUrl,
      livekitToken,
      sharedQueueRepo,
      sharedQueueFile,
      ghPath,
    }));
    setupComplete = true;
    await connectToLiveKit();
  }

  async function resetConfig() {
    stopParticipantPolling();
    try {
      await invoke("livekit_disconnect");
      await invoke("stop_voice_chat");
      await invoke("stop_mic_test");
    } catch {
      // Running outside Tauri
    }
    localStorage.removeItem("gezellig-setup");
    livekitConnected = false;
    setupComplete = false;
    showSettings = false;
    livekitUrl = "wss://gezellig-tmbd1vyo.livekit.cloud";
    livekitToken = "";
    sharedQueueRepo = "williammartin/gezellig-queue";
    sharedQueueFile = "queue.ndjson";
    ghPath = "gh";
    inRoom = false;
    roomParticipants = [];
    musicVolume = 50;
    voiceChatEnabled = false;
    micTestActive = false;
    micLevel = 0;
    stopMicLevelPolling();
    stopQueuePolling();
    stopQueueWebhookListener();
  }

  let canConnect = $derived(livekitUrl.length > 0 && livekitToken.length > 0);

  function addNotification(message: string) {
    notifications = [...notifications, message];
    setTimeout(() => {
      notifications = notifications.slice(1);
    }, 5000);
  }

  async function addToQueue() {
    if (!djQueueUrl.trim()) return;
    const url = djQueueUrl.trim();
    djQueueUrl = "";
    debugLog(`addToQueue: ${url}`);
    try {
      await invoke("queue_track", { url });
      debugLog('queue_track OK');
      await refreshQueue();
    } catch (e) {
      debugLog(`addToQueue error: ${e}`);
      djQueue = [...djQueue, { url, title: null, id: 0 }];
    }
  }

  async function refreshQueue() {
    try {
      const state = await invoke<SharedQueueState>("get_shared_queue_state");
      djQueue = state.queue || [];
      history = state.history || [];
      const prev = nowPlaying;
      nowPlaying = state.nowPlaying ?? null;
      if (prev?.url !== nowPlaying?.url || prev?.title !== nowPlaying?.title) {
        skipping = false;
      }
    } catch {
      nowPlaying = null;
    }
  }

  async function clearQueue() {
    try {
      await invoke("clear_shared_queue");
      await refreshQueue();
    } catch {
      // Outside Tauri
      djQueue = [];
      nowPlaying = null;
    }
  }

  async function skipTrack() {
    if (skipping) return;
    skipping = true;
    try {
      await invoke("skip_track");
      debugLog("skip_track OK");
    } catch (e) {
      debugLog(`skip_track error: ${e}`);
    }
  }

  async function requeueTrack(url: string) {
    try {
      await invoke("queue_track", { url });
      debugLog(`Requeued: ${url}`);
      await refreshQueue();
    } catch (e) {
      debugLog(`requeue error: ${e}`);
    }
  }

  function handleDragStart(i: number) {
    dragIndex = i;
  }

  async function handleDrop(targetIndex: number) {
    if (dragIndex === null || dragIndex === targetIndex) {
      dragIndex = null;
      return;
    }
    const newQueue = [...djQueue];
    const [moved] = newQueue.splice(dragIndex, 1);
    newQueue.splice(targetIndex, 0, moved);
    djQueue = newQueue;
    dragIndex = null;
    const order = newQueue.map(item => item.id);
    try {
      await invoke("reorder_queue", { order });
      debugLog("reorder_queue OK");
    } catch (e) {
      debugLog(`reorder error: ${e}`);
    }
  }

  async function startBotPlayback() {
    try {
      await invoke("start_dj_audio");
      debugLog("DJ bot started playback loop");
    } catch (e) {
      debugLog(`DJ bot start error: ${e}`);
    }
  }

  async function setVoiceChat(enabled: boolean) {
    try {
      if (enabled) {
        await invoke("start_voice_chat");
        voiceChatEnabled = true;
        startMicLevelPolling();
      } else {
        await invoke("stop_voice_chat");
        voiceChatEnabled = false;
        if (!micTestActive) {
          stopMicLevelPolling();
          micLevel = 0;
        }
      }
    } catch (e) {
      debugLog(`voice chat error: ${e}`);
      voiceChatEnabled = false;
    }
  }

  async function toggleMicTest() {
    try {
      if (!micTestActive) {
        await invoke("start_mic_test");
        micTestActive = true;
        startMicLevelPolling();
      } else {
        await invoke("stop_mic_test");
        micTestActive = false;
        if (!voiceChatEnabled) {
          stopMicLevelPolling();
          micLevel = 0;
        }
      }
    } catch (e) {
      debugLog(`mic test error: ${e}`);
    }
  }

</script>

{#if updateStatus !== "none"}
  <div class="update-overlay">
    <div class="update-card">
      {#if updateStatus === "checking"}
        <h2>Checking for updates…</h2>
        <p>Please wait.</p>
      {:else if updateStatus === "available" && updateInfo}
        <h2>🔄 Gezellig v{updateInfo.latestVersion} is available</h2>
        <p>You have v{updateInfo.currentVersion}. Update required before continuing.</p>
        <pre class="update-command">{updateCommand}</pre>
        <div class="update-actions">
          <button class="btn" onclick={copyUpdateCommand}>Copy update command</button>
          <button class="btn btn-outline" onclick={dismissUpdate}>I'll update later</button>
        </div>
      {/if}
    </div>
  </div>
{/if}

{#if !setupComplete}
  <main class="setup-container">
    <div data-testid="setup-screen" class="setup-screen">
      <h1>Welcome to Gezellig</h1>
      <p>Paste the connection details your admin shared with you.</p>
      <label>
        LiveKit Server URL
        <input data-testid="setup-livekit-url" type="text" bind:value={livekitUrl} placeholder="wss://your-server.livekit.cloud" />
      </label>
      <label>
        Token
        <textarea data-testid="setup-token" bind:value={livekitToken} placeholder="Paste your token here" rows="3"></textarea>
      </label>
      <button data-testid="setup-connect" onclick={completeSetup} disabled={!canConnect}>Connect</button>
    </div>
  </main>
{:else}
  <div class="app-layout">
    <aside class="sidebar">
      <div class="sidebar-brand">
        <span class="brand-icon">🏠</span>
        <span class="brand-name">Gezellig</span>
      </div>

      <nav class="sidebar-nav">
        <button class="nav-item active" onclick={() => showSettings = false}>
          <span class="nav-icon">👥</span>
          <span>Office</span>
        </button>
        <button class="nav-item" data-testid="settings-button" onclick={() => showSettings = !showSettings}>
          <span class="nav-icon">⚙️</span>
          <span>Settings</span>
        </button>
      </nav>

      <div class="sidebar-footer">
        <div class="sidebar-user">
          <span data-testid="connection-status" class="status-dot {livekitConnected ? 'connected' : 'local'}"></span>
          <span class="sidebar-username">{displayName}</span>
        </div>
      </div>
    </aside>

    <main class="content">
      <div data-testid="notification-area" class="notification-area">
        {#each notifications as notification}
          <p class="notification">{notification}</p>
        {/each}
      </div>

        {#if showSettings}
          <div data-testid="settings-panel" class="settings-panel">
            <h2>Settings</h2>
            <label>
              LiveKit Server URL
              <input data-testid="livekit-url-input" type="text" bind:value={livekitUrl} placeholder="wss://your-server.livekit.cloud" />
            </label>
            <label>
              Token
              <textarea data-testid="settings-token" bind:value={livekitToken} rows="2"></textarea>
            </label>
            <label>
              Shared Queue Repo
              <input data-testid="settings-queue-repo" type="text" bind:value={sharedQueueRepo} />
            </label>
            <label>
              Shared Queue File
              <input data-testid="settings-queue-file" type="text" bind:value={sharedQueueFile} />
            </label>
            <label>
              GH Path
              <input data-testid="settings-gh-path" type="text" bind:value={ghPath} />
            </label>
            <div class="settings-section">
              <h3>Voice Chat</h3>
              <label class="toggle-row">
                <input type="checkbox" checked={voiceChatEnabled} oninput={(e) => setVoiceChat((e.target as HTMLInputElement).checked)} />
                <span>Enable voice chat</span>
              </label>
              <div class="mic-test">
                <button data-testid="mic-test-button" class="btn btn-outline" onclick={toggleMicTest}>
                  {micTestActive ? 'Stop Mic Test' : 'Start Mic Test'}
                </button>
                <div class="mic-meter" aria-hidden="true">
                  <div class="mic-meter-fill" style={`width: ${micLevel}%`}></div>
                </div>
                <div class="mic-meter-label">{micLevel}%</div>
              </div>
            </div>
            <div class="settings-actions">
              <button data-testid="settings-save" onclick={async () => {
                localStorage.setItem("gezellig-setup", JSON.stringify({
                  livekitUrl,
                  livekitToken,
                  sharedQueueRepo,
                  sharedQueueFile,
                  ghPath,
                }));
                try {
                  await invoke("save_settings", {
                    livekitUrl,
                    sharedQueueRepo,
                    sharedQueueFile,
                    ghPath,
                  });
              } catch { /* outside Tauri */ }
              addNotification('Settings saved');
              showSettings = false;
            }}>Save</button>
            <button data-testid="settings-close" onclick={() => showSettings = false}>Close</button>
          </div>
          <button data-testid="settings-reset" class="danger" onclick={resetConfig}>Reset & Sign Out</button>
        </div>
      {:else}
        <section data-testid="room" class="card">
          <h2>Room</h2>
          {#if roomParticipants.length > 0}
            <ul class="user-list">
              {#each roomParticipants as participant, i}
                <li>
                  <span class="user-bar" style="background: {['#e8a87c', '#85cdca', '#d0e1f9', '#c9b1ff', '#f7dc6f', '#f0b27a', '#82e0aa'][i % 7]};"></span>
                  <span class="user-avatar">👤</span>
                  <span class="user-name">{participant}</span>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="empty-state">No one is in the room</p>
          {/if}
        </section>

        <div class="actions">
            <div class="card volume-card">
              <label class="volume-control">
                Music Volume
                <input data-testid="music-volume" type="range" min="0" max="100" bind:value={musicVolume} oninput={updateMusicVolume} />
              </label>
            </div>
            <div data-testid="queue-panel" class="card dj-section">
              <p class="dj-label">🎵 Shared Queue</p>
              <div class="queue-input">
                <input data-testid="queue-url-input" type="text" placeholder="Paste YouTube URL..." bind:value={djQueueUrl} onkeydown={(e) => e.key === 'Enter' && addToQueue()} />
                <button data-testid="add-to-queue-button" class="btn" onclick={addToQueue}>Add to Queue</button>
              </div>
              <div data-testid="now-playing" class="queue-list">
                <p class="queue-label">Now Playing</p>
                {#if nowPlaying}
                  <div class="queue-item">{nowPlaying.title}</div>
                  <div class="queue-item">
                    <a href={nowPlaying.url} target="_blank" rel="noreferrer">{nowPlaying.url}</a>
                  </div>
                {:else}
                  <p class="empty-state">Nothing playing</p>
                {/if}
              </div>
              <div class="queue-actions">
                <button data-testid="skip-track-button" class="btn btn-outline" onclick={skipTrack} disabled={skipping || !nowPlaying}>{skipping ? 'Skipping…' : 'Skip'}</button>
                <button data-testid="clear-queue-button" class="btn btn-outline" onclick={clearQueue}>Clear Queue</button>
              </div>
              {#if djQueue.length > 0}
                <div data-testid="dj-queue" class="queue-list">
                  <p class="queue-label">Queue ({djQueue.length})</p>
                  {#each djQueue as item, i}
                    <div
                      class="queue-item"
                      draggable="true"
                      ondragstart={() => handleDragStart(i)}
                      ondragover={(e) => e.preventDefault()}
                      ondrop={() => handleDrop(i)}
                      style={dragIndex === i ? 'opacity: 0.5' : ''}
                    >⠿ {i + 1}. {item.title || item.url}</div>
                  {/each}
                </div>
              {:else}
                <p class="empty-state">No tracks queued yet</p>
              {/if}
              {#if history.length > 0}
                <div class="queue-list">
                  <button class="btn btn-outline" onclick={() => showHistory = !showHistory} data-testid="toggle-history-button">
                    {showHistory ? 'Hide' : 'Show'} History ({history.length})
                  </button>
                  {#if showHistory}
                    <div data-testid="history-panel">
                      {#each history as item}
                        <div class="queue-item history-item">
                          <span>{item.title || item.url}</span>
                          <button class="btn btn-outline btn-small" onclick={() => requeueTrack(item.url)} data-testid="requeue-button">Requeue</button>
                        </div>
                      {/each}
                    </div>
                  {/if}
                </div>
              {/if}
            </div>
        </div>
      {/if}
    </main>
  </div>
{/if}

<!-- Debug Panel -->
<div data-testid="debug-panel-toggle" style="position: fixed; bottom: 8px; right: 8px; z-index: 1000;">
  <button onclick={() => showDebug = !showDebug} style="background: #333; color: #0f0; border: none; border-radius: 4px; padding: 4px 8px; font-size: 12px; cursor: pointer;">🐛</button>
</div>
{#if showDebug}
<div data-testid="debug-panel" style="position: fixed; bottom: 36px; right: 8px; width: 420px; max-height: 250px; background: #1a1a1a; color: #0f0; font-family: monospace; font-size: 11px; border-radius: 6px; overflow-y: auto; padding: 8px; z-index: 1000; border: 1px solid #333;">
  {#each debugLogs as log}
    <div style="white-space: pre-wrap; margin-bottom: 2px;">{log}</div>
  {/each}
  {#if debugLogs.length === 0}
    <div style="color: #666;">No debug logs yet</div>
  {/if}
</div>
{/if}

<style>
:root {
  font-family: Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  font-size: 15px;
  line-height: 1.6;
  color: #1a1a1a;
  background-color: #f0edea;
}

:global(html), :global(body) {
  height: 100%;
  margin: 0;
  overflow: hidden;
}

/* ---- App layout ---- */
.app-layout {
  display: flex;
  min-height: 100vh;
}

.sidebar {
  width: 220px;
  background: #f7f4f1;
  border-right: 1px solid #e5e2df;
  display: flex;
  flex-direction: column;
  padding: 1.25rem 0;
  flex-shrink: 0;
}

.sidebar-brand {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  padding: 0 1.25rem 1.25rem;
  border-bottom: 1px solid #e5e2df;
  margin-bottom: 0.75rem;
}

.brand-icon {
  font-size: 1.2em;
}

.brand-name {
  font-size: 1.1rem;
  font-weight: 700;
  letter-spacing: -0.02em;
}

.sidebar-nav {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 0 0.5rem;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 0.6rem;
  padding: 0.5rem 0.75rem;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: #666;
  font-size: 0.9rem;
  font-weight: 500;
  cursor: pointer;
  box-shadow: none;
  text-align: left;
  width: 100%;
  transition: background 0.15s;
}

.nav-item:hover {
  background: #ece8e4;
  color: #1a1a1a;
  border: none;
  box-shadow: none;
}

.nav-item.active {
  background: #ece8e4;
  color: #1a1a1a;
  font-weight: 600;
}

.nav-icon {
  font-size: 1em;
  width: 1.25em;
  text-align: center;
}

.sidebar-footer {
  padding: 0.75rem 1.25rem 0;
  border-top: 1px solid #e5e2df;
  margin-top: auto;
}

.sidebar-user {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.85rem;
  color: #555;
}

.status-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  flex-shrink: 0;
}

.status-dot.connected {
  background: #8cb87a;
}

.status-dot.local {
  background: #d9534f;
}

.sidebar-username {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* ---- Content area ---- */
.content {
  flex: 1;
  padding: 2rem 2.5rem;
  max-width: 640px;
}

h2 {
  font-size: 0.8rem;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: #999;
  margin-bottom: 0.75rem;
}

.card {
  background: #ffffff;
  border: 1px solid #e5e2df;
  border-radius: 12px;
  padding: 1rem 1.25rem;
  margin-bottom: 1rem;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.04);
}

/* ---- User list with colored bars ---- */
.user-list {
  list-style: none;
  padding: 0;
  margin: 0;
}

.user-list li {
  display: flex;
  align-items: center;
  padding: 0.5rem 0;
  gap: 0.6rem;
  font-size: 0.95rem;
  color: #3a3a3a;
  position: relative;
}

.user-list li + li {
  border-top: 1px solid #f0edea;
}

.user-bar {
  width: 4px;
  height: 28px;
  border-radius: 2px;
  flex-shrink: 0;
}

.user-avatar {
  font-size: 0.9em;
  opacity: 0.5;
}

.user-name {
  font-weight: 500;
}

.empty-state {
  color: #bbb;
  font-size: 0.9rem;
  margin: 0;
}

/* ---- Actions & controls ---- */
.actions {
  margin-top: 0.5rem;
}

.btn {
  border-radius: 8px;
  border: 1px solid #e5e2df;
  padding: 0.55em 1.1em;
  font-size: 0.9rem;
  font-weight: 500;
  cursor: pointer;
  background: #ffffff;
  color: #1a1a1a;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
  transition: all 0.15s ease;
}

.btn:hover {
  border-color: #ccc;
  background: #f7f5f3;
}

.btn-outline {
  background: transparent;
  border-color: #d5d2cf;
}

.btn-outline:hover {
  background: #f7f5f3;
}

/* ---- DJ section ---- */
.dj-section {
  margin-top: 1rem;
}

.dj-label {
  font-weight: 600;
  margin: 0 0 0.5rem;
}

.volume-control {
  display: block;
  margin: 0.75rem 0;
  font-size: 0.85rem;
  color: #888;
  font-weight: 500;
}

.volume-control input[type="range"] {
  display: block;
  width: 100%;
  margin-top: 0.35rem;
}

.queue-input {
  display: flex;
  gap: 0.5rem;
  margin: 0.5rem 0;
}

.queue-actions {
  display: flex;
  justify-content: flex-end;
  margin-bottom: 0.5rem;
}

.queue-input input[type="text"] {
  flex: 1;
  padding: 0.5rem 0.7rem;
  border: 1px solid #e5e2df;
  border-radius: 8px;
  font-size: 0.85rem;
  background: #faf9f7;
}

.queue-list {
  margin: 0.5rem 0;
  padding: 0.5rem 0.8rem;
  background: #f7f5f3;
  border-radius: 8px;
  font-size: 0.85rem;
}

.queue-label {
  font-weight: 600;
  font-size: 0.8rem;
  color: #888;
  margin: 0 0 0.3rem;
}

.queue-item {
  padding: 0.2rem 0;
  color: #666;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.queue-item[draggable="true"] {
  cursor: grab;
}
.queue-item[draggable="true"]:active {
  cursor: grabbing;
}
.history-item {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
.history-item span {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
}
.btn-small {
  padding: 0.1rem 0.4rem;
  font-size: 0.75rem;
}
.update-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}
.update-card {
  background: #ffffff;
  border-radius: 12px;
  padding: 2rem;
  width: min(720px, 90vw);
  box-shadow: 0 20px 40px rgba(0, 0, 0, 0.2);
}
.update-command {
  margin: 1rem 0;
  padding: 0.75rem;
  background: #f5f5f5;
  border-radius: 6px;
  font-size: 0.85rem;
  white-space: pre-wrap;
}
.update-actions {
  display: flex;
  gap: 0.75rem;
  flex-wrap: wrap;
}

/* ---- Settings panel ---- */
.settings-panel {
  background: #ffffff;
  border: 1px solid #e5e2df;
  border-radius: 12px;
  padding: 1.25rem;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.04);
}

.settings-panel h2 {
  text-transform: none;
  letter-spacing: normal;
  font-size: 1.1rem;
  color: #1a1a1a;
  margin-bottom: 0.5rem;
}

.settings-panel label {
  display: block;
  margin: 0.75rem 0 0.25rem;
  font-size: 0.85rem;
  font-weight: 500;
  color: #888;
}

.settings-section {
  margin-top: 1rem;
  padding-top: 0.75rem;
  border-top: 1px solid #f0edea;
}

.settings-section h3 {
  margin: 0 0 0.5rem;
  font-size: 0.9rem;
  font-weight: 600;
  color: #666;
}

.toggle-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  margin: 0.5rem 0 0.75rem;
  font-size: 0.85rem;
  color: #666;
}

.toggle-row input {
  margin: 0;
}

.mic-test {
  display: grid;
  grid-template-columns: auto 1fr auto;
  align-items: center;
  gap: 0.75rem;
}

.mic-meter {
  height: 10px;
  background: #f0edea;
  border-radius: 999px;
  overflow: hidden;
  border: 1px solid #e5e2df;
}

.mic-meter-fill {
  height: 100%;
  background: linear-gradient(90deg, #8cb87a, #f0b27a);
  transition: width 0.1s ease;
}

.mic-meter-label {
  font-size: 0.8rem;
  color: #777;
  min-width: 32px;
  text-align: right;
}

.settings-panel input[type="text"],
.settings-panel textarea {
  display: block;
  width: 100%;
  margin-top: 0.25rem;
  padding: 0.5em 0.6em;
  border: 1px solid #e5e2df;
  border-radius: 8px;
  font-family: inherit;
  font-size: 0.95rem;
  background: #fafaf9;
  color: #1a1a1a;
}

.settings-panel input[type="text"]:focus,
.settings-panel textarea:focus {
  outline: none;
  border-color: #8cb87a;
  box-shadow: 0 0 0 2px rgba(140, 184, 122, 0.15);
}

.settings-actions {
  display: flex;
  gap: 0.5rem;
  margin-top: 1rem;
}

/* ---- Notifications ---- */
.notification-area {
  min-height: 1px;
  margin-bottom: 0.5rem;
}

.notification {
  padding: 0.4em 0.75em;
  margin: 0.25rem 0;
  background: #f2f7ef;
  border: 1px solid #dde9d6;
  border-radius: 8px;
  font-size: 0.85rem;
  color: #4a6741;
}

/* ---- Setup screen ---- */
.setup-container {
  display: flex;
  justify-content: center;
  align-items: flex-start;
  min-height: 100vh;
  padding: 2rem;
}

.setup-screen {
  max-width: 380px;
  margin: 3rem auto;
  background: #ffffff;
  border: 1px solid #e5e2df;
  border-radius: 16px;
  padding: 2rem;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.06);
}

.setup-screen h1 {
  font-size: 1.5rem;
  font-weight: 700;
  margin-bottom: 0.25rem;
}

.setup-screen p {
  color: #888;
  font-size: 0.9rem;
  margin-bottom: 1rem;
}

.setup-screen label {
  display: block;
  margin: 0.75rem 0 0.25rem;
  font-size: 0.85rem;
  font-weight: 500;
  color: #888;
}

.setup-screen input[type="text"],
.setup-screen textarea {
  display: block;
  width: 100%;
  padding: 0.5em 0.6em;
  border: 1px solid #e5e2df;
  border-radius: 8px;
  font-family: inherit;
  font-size: 0.95rem;
  background: #fafaf9;
  color: #1a1a1a;
}

.setup-screen input[type="text"]:focus,
.setup-screen textarea:focus {
  outline: none;
  border-color: #8cb87a;
  box-shadow: 0 0 0 2px rgba(140, 184, 122, 0.15);
}

.setup-screen button {
  margin-top: 1.5rem;
  width: 100%;
  padding: 0.65em 1.1em;
  border-radius: 8px;
  border: 1px solid #1a1a1a;
  background: #1a1a1a;
  color: #fff;
  font-size: 0.95rem;
  font-weight: 500;
  cursor: pointer;
}

.setup-screen button:hover {
  background: #333;
}

.setup-screen button:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

/* ---- Global button base ---- */
button {
  border-radius: 8px;
  border: 1px solid #e5e2df;
  padding: 0.55em 1.1em;
  font-size: 0.9rem;
  font-weight: 500;
  cursor: pointer;
  background-color: #ffffff;
  color: #1a1a1a;
  transition: all 0.15s ease;
}

button:hover {
  border-color: #ccc;
  background-color: #f7f5f3;
}

button.danger {
  background: transparent;
  color: #c0392b;
  border-color: #e5e2df;
  margin-top: 1rem;
  width: 100%;
}

button.danger:hover {
  background: #fdf2f2;
  border-color: #e5b5b5;
}

/* ---- Dark mode ---- */
@media (prefers-color-scheme: dark) {
  :root {
    color: #e8e4e0;
    background-color: #1e1d1b;
  }
  .sidebar {
    background: #252422;
    border-color: #3a3836;
  }
  .sidebar-brand { border-color: #3a3836; }
  .sidebar-footer { border-color: #3a3836; }
  .nav-item { color: #999; }
  .nav-item:hover, .nav-item.active {
    background: #2e2d2a;
    color: #e8e4e0;
  }
  .sidebar-user { color: #999; }
  .card, .dj-section, .settings-panel, .setup-screen {
    background: #2a2927;
    border-color: #3a3836;
  }
  h2 { color: #777; }
  .user-list li { color: #c8c4c0; }
  .user-list li + li { border-color: #3a3836; }
  .user-avatar { opacity: 0.4; }
  .empty-state { color: #666; }
  button, .btn {
    color: #e8e4e0;
    background: #2a2927;
    border-color: #3a3836;
  }
  button:hover, .btn:hover {
    background: #353331;
    border-color: #4a4846;
  }
  button.danger {
    color: #e57373;
    background: transparent;
  }
  button.danger:hover {
    background: #352525;
  }
  .settings-panel h2 { color: #e8e4e0; }
  .settings-panel label { color: #888; }
  .settings-panel input[type="text"],
  .settings-panel textarea {
    background: #353331;
    color: #e8e4e0;
    border-color: #4a4846;
  }
  .settings-section { border-top-color: #3a3836; }
  .settings-section h3 { color: #aaa; }
  .toggle-row { color: #aaa; }
  .mic-meter {
    background: #2f2d2b;
    border-color: #4a4846;
  }
  .mic-meter-label { color: #999; }
  .setup-screen input[type="text"],
  .setup-screen textarea {
    background: #353331;
    color: #e8e4e0;
    border-color: #4a4846;
  }
  .setup-screen button {
    background: #e8e4e0;
    color: #1a1a1a;
    border-color: #e8e4e0;
  }
  .setup-screen button:hover { background: #d5d1cc; }
  .setup-screen p { color: #888; }
  .setup-screen label { color: #888; }
  .notification {
    background: #2d3329;
    border-color: #3a4336;
    color: #a8c89a;
  }
}
</style>
