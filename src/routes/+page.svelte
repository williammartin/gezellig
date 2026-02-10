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
  <main class="container">
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
  <main class="container">
    <header>
      <h1>Gezellig</h1>
      <div>
        <span data-testid="connection-status" class="connection-status {livekitConnected ? 'connected' : 'local'}">
          {livekitConnected ? '🟢' : '🔴'}
        </span>
        <button data-testid="settings-button" onclick={() => showSettings = true}>⚙️</button>
      </div>
    </header>

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
        <button data-testid="settings-save" onclick={() => showSettings = false}>Save</button>
        <button data-testid="settings-close" onclick={() => showSettings = false}>Close</button>
        <button data-testid="settings-reset" class="danger" onclick={resetConfig}>Reset & Sign Out</button>
      </div>
    {/if}

    <section data-testid="online-users">
      <h2>Online</h2>
      <ul>
        <li>{displayName}</li>
      </ul>
    </section>

    <section data-testid="room">
      <h2>Room</h2>
      {#if roomParticipants.length > 0}
        <ul>
          {#each roomParticipants as participant}
            <li>{participant}</li>
          {/each}
        </ul>
      {:else}
        <p class="empty-state">No one is in the room</p>
      {/if}
    </section>

    {#if inRoom}
      <div class="controls">
        <button data-testid="mute-button" onclick={toggleMute}>
          {isMuted ? 'Unmute' : 'Mute'}
        </button>
        <button data-testid="leave-room-button" onclick={leaveRoom}>Leave Room</button>
      </div>

      {#if isDJ}
        <div data-testid="dj-status" class="dj-section">
          <p>🎵 You are the DJ</p>
          <div data-testid="now-playing" class="now-playing">
            Waiting for Spotify — select "Gezellig" as your device
          </div>
          <label class="volume-control">
            Music Volume
            <input data-testid="music-volume" type="range" min="0" max="100" bind:value={musicVolume} />
          </label>
          <button data-testid="stop-dj-button" onclick={stopDJ}>Stop DJ</button>
        </div>
      {:else}
        <button data-testid="become-dj-button" onclick={becomeDJ}>Become DJ</button>
      {/if}
    {:else}
      <button data-testid="join-room-button" onclick={joinRoom}>Join Room</button>
    {/if}
  </main>
{/if}

<style>
:root {
  font-family: Inter, -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
  font-size: 15px;
  line-height: 1.6;
  color: #1a1a1a;
  background-color: #f0edea;
}

.container {
  margin: 0 auto;
  max-width: 560px;
  padding: 2rem;
}

header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1.5rem;
}

header h1 {
  font-size: 1.4rem;
  font-weight: 700;
  letter-spacing: -0.02em;
}

h2 {
  font-size: 1rem;
  font-weight: 600;
  color: #1a1a1a;
  margin-bottom: 0.5rem;
}

section {
  background: #ffffff;
  border: 1px solid #e5e2df;
  border-radius: 12px;
  padding: 1rem 1.25rem;
  margin-bottom: 1rem;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.04);
}

section ul {
  list-style: none;
  padding: 0;
  margin: 0;
}

section li {
  padding: 0.35rem 0;
  color: #3a3a3a;
  font-size: 0.95rem;
}

section li::before {
  content: '●';
  color: #8cb87a;
  font-size: 0.6em;
  margin-right: 0.5rem;
  vertical-align: middle;
}

.empty-state {
  color: #999;
  font-size: 0.9rem;
  margin: 0;
}

.controls {
  display: flex;
  gap: 0.5rem;
  margin: 1rem 0;
}

.dj-section {
  margin: 1rem 0;
  padding: 1rem 1.25rem;
  border: 1px solid #e5e2df;
  border-radius: 12px;
  background: #ffffff;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.04);
}

.now-playing {
  padding: 0.6rem 0.8rem;
  margin: 0.5rem 0;
  background: #f7f5f3;
  border-radius: 8px;
  font-size: 0.9rem;
  color: #666;
}

.volume-control {
  display: block;
  margin: 0.75rem 0;
  font-size: 0.9rem;
  color: #555;
}

.settings-panel {
  padding: 1.25rem;
  margin: 0 0 1rem;
  border: 1px solid #e5e2df;
  border-radius: 12px;
  background: #ffffff;
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.04);
}

.settings-panel label {
  display: block;
  margin: 0.75rem 0 0.25rem;
  font-size: 0.85rem;
  font-weight: 500;
  color: #666;
}

.settings-panel input[type="text"],
.settings-panel textarea {
  display: block;
  width: 100%;
  margin-top: 0.25rem;
  padding: 0.5em 0.6em;
  border: 1px solid #ddd;
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

.notification-area {
  min-height: 1.5rem;
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
  color: #666;
}

.setup-screen input[type="text"],
.setup-screen textarea {
  display: block;
  width: 100%;
  padding: 0.5em 0.6em;
  border: 1px solid #ddd;
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
}

.setup-screen button:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.connection-status {
  font-size: 0.7em;
  margin-right: 0.5rem;
}

button {
  border-radius: 8px;
  border: 1px solid #e5e2df;
  padding: 0.55em 1.1em;
  font-size: 0.95rem;
  font-weight: 500;
  cursor: pointer;
  background-color: #ffffff;
  color: #1a1a1a;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.06);
  transition: all 0.15s ease;
}

button:hover {
  border-color: #ccc;
  background-color: #f7f5f3;
}

button:active {
  background-color: #f0edea;
}

button.danger {
  background-color: #fff;
  color: #c0392b;
  border-color: #e5e2df;
  margin-top: 1rem;
}

button.danger:hover {
  background-color: #fdf2f2;
  border-color: #e5b5b5;
}

@media (prefers-color-scheme: dark) {
  :root {
    color: #e8e4e0;
    background-color: #1e1d1b;
  }
  section, .dj-section, .settings-panel, .setup-screen {
    background: #2a2927;
    border-color: #3a3836;
  }
  .now-playing {
    background: #353331;
  }
  h2 { color: #e8e4e0; }
  section li { color: #c8c4c0; }
  .empty-state { color: #777; }
  button {
    color: #e8e4e0;
    background-color: #2a2927;
    border-color: #3a3836;
  }
  button:hover {
    background-color: #353331;
    border-color: #4a4846;
  }
  button.danger {
    color: #e57373;
    background-color: #2a2927;
  }
  button.danger:hover {
    background-color: #352525;
  }
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
  .setup-screen p { color: #888; }
  .notification {
    background: #2d3329;
    border-color: #3a4336;
    color: #a8c89a;
  }
}
</style>
