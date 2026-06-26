(async function bootEutherPal() {
  const view = document.body.dataset.view;
  const state = await fetchState();

  if (view === "tv") renderTv(state);
  if (view === "mobile") renderMobile(state);
  if (view === "admin") {
    renderAdmin(state);
    await renderSettings();
  }

  if (view === "tv" || view === "admin") {
    window.setInterval(async () => {
      const freshState = await fetchState();
      if (view === "tv") renderTv(freshState);
      if (view === "admin") renderAdmin(freshState);
    }, 1200);
  }
})();

async function fetchState() {
  const response = await fetch("/api/game");
  if (!response.ok) throw new Error("Kunde inte hämta spelstatus");
  return response.json();
}

async function postAction(path, body) {
  const response = await fetch(path, {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body,
  });
  if (!response.ok) throw new Error("Kunde inte uppdatera spelet");
  return response.json();
}

async function fetchSettings() {
  const response = await fetch("/api/settings");
  if (!response.ok) throw new Error("Kunde inte hämta settings");
  return response.json();
}

function renderTv(state) {
  document.getElementById("room-code").textContent = state.roomCode;
  document.getElementById("current-player").textContent = state.currentPlayer;
  document.getElementById("dice").textContent =
    state.phase === "token_selection" ? "Pjäsval" : `Tärning: ${state.dice.join(" + ")}`;
  document.getElementById("bank-message").textContent = state.bankMessage;

  const board = document.getElementById("board");
  board.innerHTML = "";
  state.spaces.forEach((name, index) => {
    const tile = document.createElement("div");
    tile.className = `tile ${tileClass(index)}`;
    tile.style.gridArea = gridAreaForBoardIndex(index);
    tile.tabIndex = 0;
    tile.innerHTML = `<span class="tile-index">${index}</span><strong>${name}</strong><div class="tokens">${tokensAt(state.players, index)}</div>`;
    board.appendChild(tile);
  });

  const players = document.getElementById("players");
  players.innerHTML = state.players
    .map((player) => `<div class="player-row"><strong>${player.name}</strong><span>${player.cash} kr</span><em>${tokenLabel(player.token) || "Väljer pjäs"}</em></div>`)
    .join("");

  const tokenPanel = document.getElementById("token-status");
  if (tokenPanel) {
    tokenPanel.innerHTML = state.tokenChoices
      .map((token) => `<span class="${token.available ? "available" : "taken"}">${token.label}</span>`)
      .join("");
  }
}

function renderMobile(state) {
  document.getElementById("mobile-status").textContent =
    state.phase === "token_selection"
      ? `${state.currentPlayer} väljer pjäs.`
      : `${state.currentPlayer} har tur.`;
  document.getElementById("mobile-bank-message").textContent = state.bankMessage;
  document.getElementById("join-button").addEventListener("click", () => {
    const room = document.getElementById("room-input").value || state.roomCode;
    document.getElementById("mobile-status").textContent = `Ansluten till ${room}. Riktiga sessioner kommer i nästa steg.`;
  });

  renderTokenButtons(state);
  document.getElementById("roll-button").onclick = async () => {
    const updated = await postAction("/api/game/roll", "");
    renderMobile(updated);
  };
  document.getElementById("new-game-button").onclick = async () => {
    const updated = await postAction("/api/game/new", "");
    renderMobile(updated);
  };
}

function renderAdmin(state) {
  fetch("/health")
    .then((response) => response.json())
    .then((health) => {
      document.getElementById("admin-server").textContent = health.status;
      document.getElementById("admin-ai").textContent = health.ai;
    });
  document.getElementById("admin-room").textContent = state.roomCode;
}

async function renderSettings() {
  const settings = await fetchSettings();
  const modelSelect = document.getElementById("model-select");
  const prepromptInput = document.getElementById("preprompt-input");
  const status = document.getElementById("settings-status");
  const path = document.getElementById("settings-path");

  modelSelect.value = [...modelSelect.options].some((option) => option.value === settings.model)
    ? settings.model
    : "custom";
  prepromptInput.value = settings.preprompt;
  status.textContent = `Laddat från ${settings.path}`;
  path.textContent = settings.path;

  document.getElementById("settings-form").onsubmit = async (event) => {
    event.preventDefault();
    status.textContent = "Sparar...";
    const body = new URLSearchParams({
      model: modelSelect.value,
      preprompt: prepromptInput.value,
    });
    const updated = await postAction("/api/settings", body.toString());
    status.textContent = `Sparat till ${updated.path}`;
  };
}

function tokensAt(players, position) {
  return players
    .filter((player) => player.position === position && player.token)
    .map((player) => `<span title="${player.name}">${tokenIcon(player.token)}</span>`)
    .join("");
}

function renderTokenButtons(state) {
  const panel = document.getElementById("token-buttons");
  panel.innerHTML = state.tokenChoices
    .map((token) => `<button type="button" data-token="${token.id}" ${token.available ? "" : "disabled"}>${token.label}</button>`)
    .join("");

  panel.querySelectorAll("button[data-token]").forEach((button) => {
    button.addEventListener("click", async () => {
      const updated = await postAction("/api/game/select-token", `token=${encodeURIComponent(button.dataset.token)}`);
      renderMobile(updated);
    });
  });
}

function tokenLabel(token) {
  return {
    bil: "Bil",
    hatt: "Hatt",
    skepp: "Skepp",
    hund: "Hund",
    sko: "Sko",
  }[token] || "";
}

function tokenIcon(token) {
  return {
    bil: "Bil",
    hatt: "Hatt",
    skepp: "Skepp",
    hund: "Hund",
    sko: "Sko",
  }[token] || "?";
}

function tileClass(index) {
  if ([0, 10, 20, 30].includes(index)) return "corner";
  if ([2, 17, 33].includes(index)) return "community";
  if ([7, 22, 36].includes(index)) return "chance";
  if ([5, 15, 25, 35].includes(index)) return "station";
  if ([12, 28].includes(index)) return "utility";
  if ([4, 38].includes(index)) return "tax";
  return `property color-${Math.floor(index / 5) % 8}`;
}

function gridAreaForBoardIndex(index) {
  if (index <= 10) return `11 / ${11 - index} / 12 / ${12 - index}`;
  if (index <= 20) return `${21 - index} / 1 / ${22 - index} / 2`;
  if (index <= 30) return `1 / ${index - 19} / 2 / ${index - 18}`;
  return `${index - 29} / 11 / ${index - 28} / 12`;
}
