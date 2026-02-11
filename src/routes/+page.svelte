<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let inRoom = $state(true);
  let isDJ = $state(false);
  let roomParticipants: string[] = $state([]);
  let musicVolume = $state(50);
  let showSettings = $state(false);
  let livekitUrl = $state("wss://gezellig-tmbd1vyo.livekit.cloud");
  let livekitToken = $state("");
  let setupComplete = $state(false);
  let livekitConnected = $state(false);
  let notifications: string[] = $state([]);
  let djStatus: { type: string; track?: string; artist?: string } = $state({ type: "Idle" });
  let djStatusPollInterval: ReturnType<typeof setInterval> | null = $state(null);
  let djQueueUrl = $state("");
  let djQueue: string[] = $state([]);
  let showDebug = $state(false);
  let debugLogs: string[] = $state([]);
  let lastStatusLog = "";
  let participantPollInterval: ReturnType<typeof setInterval> | null = $state(null);

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

  // Check for saved setup on mount
  async function checkSavedSetup() {
    // Env vars take priority over localStorage
    try {
      const envConfig: Record<string, string> = await invoke("get_env_config");
      if (envConfig.livekitUrl && envConfig.livekitToken) {
        livekitUrl = envConfig.livekitUrl;
        livekitToken = envConfig.livekitToken;
        setupComplete = true;
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
        if (livekitUrl && livekitToken) {
          setupComplete = true;
          connectToLiveKit();
        }
      }
    } catch {
      // No saved setup
    }
  }

  checkSavedSetup();
  loadMusicVolume();

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
    }));
    setupComplete = true;
    await connectToLiveKit();
  }

  async function resetConfig() {
    stopParticipantPolling();
    try {
      await invoke("livekit_disconnect");
    } catch {
      // Running outside Tauri
    }
    localStorage.removeItem("gezellig-setup");
    livekitConnected = false;
    setupComplete = false;
    showSettings = false;
    livekitUrl = "wss://gezellig-tmbd1vyo.livekit.cloud";
    livekitToken = "";
    inRoom = false;
    isDJ = false;
    roomParticipants = [];
    musicVolume = 50;
  }

  let canConnect = $derived(livekitUrl.length > 0 && livekitToken.length > 0);

  function addNotification(message: string) {
    notifications = [...notifications, message];
    setTimeout(() => {
      notifications = notifications.slice(1);
    }, 5000);
  }

  async function joinRoom() {
    try {
      await invoke("join_room");
    } catch { /* ok */ }
    inRoom = true;
    addNotification('You joined the room');
  }

  async function leaveRoom() {
    try {
      await invoke("leave_room");
    } catch { /* ok */ }
    inRoom = false;
    isDJ = false;
    addNotification('You left the room');
  }


  async function becomeDJ() {
    isDJ = true;
    addNotification('You are now the DJ');
    debugLog('becomeDJ: calling become_dj + start_dj_audio');
    try {
      await invoke("join_room");
      await invoke("become_dj");
      debugLog('becomeDJ: become_dj OK');
      await invoke("start_dj_audio");
      debugLog('becomeDJ: start_dj_audio OK');
    } catch (e) {
      debugLog(`becomeDJ error: ${e}`);
    }
    startDjStatusPolling();
  }

  async function stopDJ() {
    debugLog('stopDJ called');
    stopDjStatusPolling();
    try {
      await invoke("stop_dj_audio");
      await invoke("stop_dj");
    } catch (e) {
      debugLog(`stopDJ error: ${e}`);
    }
    isDJ = false;
    djStatus = { type: "Idle" };
    djQueue = [];
    djQueueUrl = "";
  }

  async function addToQueue() {
    if (!djQueueUrl.trim()) return;
    const url = djQueueUrl.trim();
    djQueueUrl = "";
    debugLog(`addToQueue: ${url}`);
    try {
      await invoke("queue_track", { url });
      debugLog('queue_track OK');
      djQueue = await invoke<string[]>("get_queue");
      debugLog(`get_queue: ${djQueue.length} items`);
    } catch (e) {
      debugLog(`addToQueue error: ${e}`);
      djQueue = [...djQueue, url];
    }
  }

  async function skipTrack() {
    debugLog('skipTrack called');
    try {
      await invoke("skip_track");
    } catch (e) {
      debugLog(`skipTrack error: ${e}`);
    }
  }

  function startDjStatusPolling() {
    stopDjStatusPolling();
    djStatusPollInterval = setInterval(async () => {
      try {
        const status = await invoke<any>("get_dj_status");
        const statusStr = JSON.stringify(status);
        if (statusStr !== lastStatusLog) {
          debugLog(`dj_status: ${statusStr}`);
          lastStatusLog = statusStr;
        }
        if (typeof status === "string") {
          djStatus = { type: status };
        } else if (status && typeof status === "object") {
          if (status.Playing) {
            djStatus = { type: "Playing", track: status.Playing.track, artist: status.Playing.artist };
          } else if (status.Loading !== undefined) {
            djStatus = { type: "Loading" };
          } else {
            djStatus = { type: "Idle" };
          }
        }
      } catch {
        // Outside Tauri
      }
      try {
        djQueue = await invoke<string[]>("get_queue");
      } catch {
        // Outside Tauri
      }
      try {
        const backendLogs = await invoke<string[]>("get_backend_logs");
        for (const log of backendLogs) {
          debugLog(`[rust] ${log}`);
        }
      } catch {
        // Outside Tauri
      }
    }, 1000);
  }

  function stopDjStatusPolling() {
    if (djStatusPollInterval) {
      clearInterval(djStatusPollInterval);
      djStatusPollInterval = null;
    }
  }
</script>

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
          <div class="settings-actions">
            <button data-testid="settings-save" onclick={async () => {
              localStorage.setItem("gezellig-setup", JSON.stringify({ livekitUrl, livekitToken }));
              try {
                await invoke("save_settings", { livekitUrl });
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
            {#if isDJ}
              <div data-testid="dj-status" class="card dj-section">
                <p class="dj-label">🎵 You are the DJ</p>
                <div data-testid="now-playing" class="now-playing">
                  {#if djStatus.type === "Playing"}
                    🎵 {djStatus.track} — {djStatus.artist}
                  {:else if djStatus.type === "Loading"}
                    ⏳ Loading...
                  {:else}
                    Add a YouTube URL to get started
                  {/if}
                </div>
                <div class="queue-input">
                  <input data-testid="queue-url-input" type="text" placeholder="Paste YouTube URL..." bind:value={djQueueUrl} onkeydown={(e) => e.key === 'Enter' && addToQueue()} />
                  <button data-testid="add-to-queue-button" class="btn" onclick={addToQueue}>Add to Queue</button>
                </div>
                {#if djQueue.length > 0}
                  <div data-testid="dj-queue" class="queue-list">
                    <p class="queue-label">Queue ({djQueue.length})</p>
                    {#each djQueue as url, i}
                      <div class="queue-item">{i + 1}. {url}</div>
                    {/each}
                  </div>
                {/if}
                <div class="dj-controls">
                  <button data-testid="skip-track-button" class="btn btn-outline" onclick={skipTrack}>⏭ Skip</button>
                  <button data-testid="stop-dj-button" class="btn btn-outline" onclick={stopDJ}>Stop DJ</button>
                </div>
              </div>
            {:else}
              <button data-testid="become-dj-button" class="btn" onclick={becomeDJ}>🎧 Become DJ</button>
            {/if}
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

.now-playing {
  padding: 0.6rem 0.8rem;
  margin: 0.5rem 0;
  background: #f7f5f3;
  border-radius: 8px;
  font-size: 0.9rem;
  color: #888;
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

.dj-controls {
  display: flex;
  gap: 0.5rem;
  align-items: center;
  margin-top: 0.5rem;
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
  .now-playing { background: #353331; }
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
