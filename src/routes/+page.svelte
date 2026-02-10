<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let inRoom = $state(false);
  let isMuted = $state(false);
  let isDJ = $state(false);
  let roomParticipants: string[] = $state([]);
  let musicVolume = $state(50);

  async function joinRoom() {
    try {
      roomParticipants = await invoke("join_room");
    } catch {
      roomParticipants = ["You"];
    }
    inRoom = true;
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
  }

  function toggleMute() {
    isMuted = !isMuted;
  }

  function becomeDJ() {
    isDJ = true;
  }

  function stopDJ() {
    isDJ = false;
  }
</script>

<main class="container">
  <h1>Gezellig</h1>

  <section data-testid="online-users">
    <h2>Online</h2>
    <ul>
      <li>You</li>
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

<style>
:root {
  font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
  font-size: 16px;
  line-height: 24px;
  color: #0f0f0f;
  background-color: #f6f6f6;
}

.container {
  margin: 0 auto;
  max-width: 600px;
  padding: 2rem;
}

.controls {
  display: flex;
  gap: 0.5rem;
  margin: 1rem 0;
}

.dj-section {
  margin: 1rem 0;
  padding: 1rem;
  border: 1px solid #ccc;
  border-radius: 8px;
}

.now-playing {
  padding: 0.5rem;
  margin: 0.5rem 0;
  background: #eee;
  border-radius: 4px;
}

.volume-control {
  display: block;
  margin: 0.5rem 0;
}

button {
  border-radius: 8px;
  border: 1px solid transparent;
  padding: 0.6em 1.2em;
  font-size: 1em;
  font-weight: 500;
  cursor: pointer;
  background-color: #ffffff;
  box-shadow: 0 2px 2px rgba(0, 0, 0, 0.2);
}

button:hover {
  border-color: #396cd8;
}

@media (prefers-color-scheme: dark) {
  :root {
    color: #f6f6f6;
    background-color: #2f2f2f;
  }
  .now-playing {
    background: #444;
  }
  .dj-section {
    border-color: #555;
  }
  button {
    color: #ffffff;
    background-color: #0f0f0f98;
  }
}
</style>
