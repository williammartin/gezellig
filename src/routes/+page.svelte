<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let inRoom = $state(false);
  let isMuted = $state(false);
  let isDJ = $state(false);
  let roomParticipants: string[] = $state([]);
  let musicVolume = $state(50);
  let showSettings = $state(false);
  let displayName = $state("");
  let livekitUrl = $state("wss://gezellig-tmbd1vyo.livekit.cloud");
  let livekitToken = $state("");
  let setupComplete = $state(false);
  let livekitConnected = $state(false);
  let notifications: string[] = $state([]);

  // Check for saved setup on mount
  function checkSavedSetup() {
    try {
      const saved = localStorage.getItem("gezellig-setup");
      if (saved) {
        const data = JSON.parse(saved);
        displayName = data.displayName || "";
        livekitUrl = data.livekitUrl || "";
        livekitToken = data.livekitToken || "";
        if (displayName && livekitUrl && livekitToken) {
          setupComplete = true;
        }
      }
    } catch {
      // No saved setup
    }
  }

  checkSavedSetup();

  async function connectToLiveKit() {
    try {
      await invoke("livekit_connect", { url: livekitUrl, token: livekitToken });
      livekitConnected = true;
      addNotification('Connected to LiveKit');
    } catch {
      // Running outside Tauri or connection failed — continue in local mode
    }
  }

  async function completeSetup() {
    localStorage.setItem("gezellig-setup", JSON.stringify({
      displayName,
      livekitUrl,
      livekitToken,
    }));
    setupComplete = true;
    await connectToLiveKit();
  }

  async function resetConfig() {
    try {
      await invoke("livekit_disconnect");
    } catch {
      // Running outside Tauri
    }
    localStorage.removeItem("gezellig-setup");
    livekitConnected = false;
    setupComplete = false;
    showSettings = false;
    displayName = "";
    livekitUrl = "wss://gezellig-tmbd1vyo.livekit.cloud";
    livekitToken = "";
    inRoom = false;
    isMuted = false;
    isDJ = false;
    roomParticipants = [];
  }

  let canConnect = $derived(displayName.length > 0 && livekitUrl.length > 0 && livekitToken.length > 0);

  function addNotification(message: string) {
    notifications = [...notifications, message];
    setTimeout(() => {
      notifications = notifications.slice(1);
    }, 5000);
  }

  async function joinRoom() {
    try {
      roomParticipants = await invoke("join_room");
    } catch {
      roomParticipants = [displayName];
    }
    inRoom = true;
    addNotification('You joined the room');
  }

  async function leaveRoom() {
    try {
      roomParticipants = await invoke("leave_room");
    } catch {
      roomParticipants = [];
    }
    inRoom = false;
    isMuted = false;
    isDJ = false;
    addNotification('You left the room');
  }

  function toggleMute() {
    isMuted = !isMuted;
  }

  function becomeDJ() {
    isDJ = true;
    addNotification('You are now the DJ');
  }

  function stopDJ() {
    isDJ = false;
  }
</script>

{#if !setupComplete}
  <main class="setup-container">
    <div data-testid="setup-screen" class="setup-screen">
      <h1>Welcome to Gezellig</h1>
      <p>Paste the connection details your admin shared with you.</p>
      <label>
        Display Name
        <input data-testid="setup-display-name" type="text" bind:value={displayName} placeholder="Your name" />
      </label>
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
            Display Name
            <input data-testid="display-name-input" type="text" bind:value={displayName} />
          </label>
          <label>
            LiveKit Server URL
            <input data-testid="livekit-url-input" type="text" bind:value={livekitUrl} placeholder="wss://your-server.livekit.cloud" />
          </label>
          <label>
            Token
            <textarea data-testid="settings-token" bind:value={livekitToken} rows="2"></textarea>
          </label>
          <div class="settings-actions">
            <button data-testid="settings-save" onclick={() => showSettings = false}>Save</button>
            <button data-testid="settings-close" onclick={() => showSettings = false}>Close</button>
          </div>
          <button data-testid="settings-reset" class="danger" onclick={resetConfig}>Reset & Sign Out</button>
        </div>
      {:else}
        <section data-testid="online-users" class="card">
          <h2>Online</h2>
          <ul class="user-list">
            <li>
              <span class="user-bar" style="background: #e8a87c;"></span>
              <span class="user-avatar">👤</span>
              <span class="user-name">{displayName}</span>
            </li>
          </ul>
        </section>

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
          {#if inRoom}
            <div class="controls">
              <button data-testid="mute-button" class="btn" onclick={toggleMute}>
                {isMuted ? '🔇 Unmute' : '🎤 Mute'}
              </button>
              <button data-testid="leave-room-button" class="btn btn-outline" onclick={leaveRoom}>Leave Room</button>
            </div>

            {#if isDJ}
              <div data-testid="dj-status" class="card dj-section">
                <p class="dj-label">🎵 You are the DJ</p>
                <div data-testid="now-playing" class="now-playing">
                  Waiting for Spotify — select "Gezellig" as your device
                </div>
                <label class="volume-control">
                  Music Volume
                  <input data-testid="music-volume" type="range" min="0" max="100" bind:value={musicVolume} />
                </label>
                <button data-testid="stop-dj-button" class="btn btn-outline" onclick={stopDJ}>Stop DJ</button>
              </div>
            {:else}
              <button data-testid="become-dj-button" class="btn" onclick={becomeDJ}>🎧 Become DJ</button>
            {/if}
          {:else}
            <button data-testid="join-room-button" class="btn btn-primary" onclick={joinRoom}>Join Room</button>
          {/if}
        </div>
      {/if}
    </main>
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

.controls {
  display: flex;
  gap: 0.5rem;
  margin-bottom: 1rem;
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

.btn-primary {
  background: #1a1a1a;
  color: #fff;
  border-color: #1a1a1a;
}

.btn-primary:hover {
  background: #333;
  border-color: #333;
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
  .btn-primary {
    background: #e8e4e0;
    color: #1a1a1a;
    border-color: #e8e4e0;
  }
  .btn-primary:hover {
    background: #d5d1cc;
    border-color: #d5d1cc;
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
