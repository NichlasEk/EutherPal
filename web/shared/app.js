(async function bootEutherPal() {
  const view = document.body.dataset.view;
  const state = await fetchState();

  if (view === "tv") renderTv(state);
  if (view === "mobile") renderMobile(state);
  if (view === "admin") {
    renderAdmin(state);
    await renderSettings();
  }

  if (view === "tv" || view === "admin" || view === "mobile") {
    window.setInterval(async () => {
      const freshState = await fetchState();
      if (view === "tv") renderTv(freshState);
      if (view === "admin") renderAdmin(freshState);
      if (view === "mobile") renderMobile(freshState);
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
  state.spaces.forEach((space, index) => {
    const tile = document.createElement("div");
    tile.className = `tile ${tileClass(space, index)}`;
    tile.style.gridArea = gridAreaForBoardIndex(index);
    tile.tabIndex = 0;
    tile.innerHTML = `<span class="tile-index">${index}</span><strong>${space.name}</strong>${spaceDetails(space)}${buildingMarkers(space)}<div class="tokens">${tokensAt(state.players, index)}</div>`;
    board.appendChild(tile);
  });

  const players = document.getElementById("players");
  players.innerHTML = state.players
    .map((player) => `<div class="player-row"><strong>${escapeHtml(player.name)}</strong><span>${player.cash} kr</span><em>${tokenAvatar(player, "mini")} ${tokenLabel(player.token) || "Väljer pjäs"}${player.jailed ? " · Fängslad" : ""}</em></div>`)
    .join("");

  const tokenPanel = document.getElementById("token-status");
  if (tokenPanel) {
    tokenPanel.innerHTML = state.tokenChoices
      .map((token) => `<span class="${token.available ? "available" : "taken"}">${token.label}</span>`)
      .join("");
  }

  renderPropertyCard(document.getElementById("tv-card"), selectedSpace(state));
  renderEvents(document.getElementById("tv-events"), state.events);
}

function renderMobile(state) {
  syncNameInput();
  const localPlayerName = localPlayer();
  const local = state.players.find((player) => player.name === localPlayerName);
  document.getElementById("mobile-status").textContent =
    state.phase === "token_selection"
      ? `${state.currentPlayer} väljer pjäs.`
      : state.phase === "auction"
        ? `Auktion pågår.`
        : local?.name === state.currentPlayer
          ? "Det är din tur."
          : `${state.currentPlayer} har tur.`;
  document.getElementById("mobile-bank-message").textContent = state.bankMessage;
  document.getElementById("join-button").onclick = () => {
    const room = document.getElementById("room-input").value || state.roomCode;
    document.getElementById("mobile-status").textContent = `Ansluten till ${room}.`;
  };
  document.getElementById("save-name-button").onclick = () => {
    saveLocalPlayer(document.getElementById("player-name-input").value);
    renderMobile(state);
  };

  renderPlayerSummary(state, local);
  renderTokenButtons(state);
  renderPropertyCard(document.getElementById("mobile-card"), selectedSpace(state, local));
  renderEvents(document.getElementById("mobile-events"), state.events);
  renderOfferControls(state, local);
  renderAuctionControls(state, local);
  renderJailControls(state, local);
  renderBuildControls(state, local);
  renderAssetControls(state, local);
  renderBankChat(document.getElementById("bank-chat"), state.bankChat);
  bindMobileBankChat(state);
  document.getElementById("roll-button").onclick = async () => {
    const updated = await postAction("/api/game/roll", playerBody());
    renderMobile(updated);
  };
  document.getElementById("buy-button").onclick = async () => {
    const updated = await postAction("/api/game/buy", playerBody());
    renderMobile(updated);
  };
  document.getElementById("decline-button").onclick = async () => {
    const updated = await postAction("/api/game/decline", playerBody());
    renderMobile(updated);
  };
  document.getElementById("new-game-button").onclick = async () => {
    const players = document.getElementById("new-game-players")?.value || "4";
    const updated = await postAction("/api/game/new", new URLSearchParams({ players }).toString());
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
  renderBankChat(document.getElementById("admin-bank-chat"), state.bankChat);
  bindAdminBankChat();
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

function spaceDetails(space) {
  const rent = space.currentRent || space.rent;
  const mortgage = space.mortgaged ? " · Intecknad" : "";
  if (space.owner) return `<small>Ägs: ${space.owner}${rent ? ` · Hyra ${rent}` : ""}${mortgage}</small>`;
  if (space.price) return `<small>${space.price} kr${rent ? ` · Hyra ${rent}` : ""}</small>`;
  return "";
}

function selectedSpace(state, preferredPlayer) {
  if (state.drawnCard) return drawnCardAsSpace(state.drawnCard);
  if (state.pendingOffer) return state.spaces[state.pendingOffer.spaceIndex];
  if (state.auction) return state.spaces[state.auction.spaceIndex];
  const current = preferredPlayer || state.players.find((player) => player.name === state.currentPlayer) || state.players[0];
  return state.spaces[current?.position || 0];
}

function drawnCardAsSpace(card) {
  return {
    kind: card.deck,
    name: card.title,
    cardTitle: card.title,
    cardText: card.text,
    cardIcon: card.icon,
  };
}

function renderPropertyCard(container, space) {
  if (!container || !space) return;
  const title = space.cardTitle || space.name;
  const icon = space.cardIcon || space.kind;
  const meta = [
    space.price ? `Pris ${space.price} kr` : "",
    space.currentRent ? `Hyra ${space.currentRent} kr` : space.rent ? `Hyra ${space.rent} kr` : "",
    space.mortgaged ? "Intecknad ja" : "",
    space.buildCost ? `Bygg ${space.buildCost} kr` : "",
    space.buildings ? `Nivå ${buildingLabel(space.buildings)}` : "",
    space.amount ? `Belopp ${space.amount} kr` : "",
    space.owner ? `Ägare ${space.owner}` : "",
  ].filter(Boolean);
  container.className = `property-card card-${space.color || space.kind}`;
  container.innerHTML = `<div class="card-band"><span>${cardIcon(icon)}</span></div><strong>${title}</strong><p>${space.cardText || "Specialruta."}</p><dl>${meta.map((item) => `<div><dt>${item.split(" ")[0]}</dt><dd>${item.split(" ").slice(1).join(" ")}</dd></div>`).join("")}</dl>`;
}

function renderEvents(container, events) {
  if (!container) return;
  container.innerHTML = (events || [])
    .slice()
    .reverse()
    .map((event) => `<li>${event}</li>`)
    .join("");
}

function renderBankChat(container, messages) {
  if (!container) return;
  container.innerHTML = (messages || [])
    .map((message) => `<div class="bank-chat-message ${message.fromBank ? "from-bank" : "from-player"}"><strong>${escapeHtml(message.speaker)}</strong><p>${escapeHtml(message.text)}</p></div>`)
    .join("");
  container.scrollTop = container.scrollHeight;
}

function bindMobileBankChat(state) {
  const askButton = document.getElementById("ask-bank-button");
  const form = document.getElementById("bank-chat-form");
  const input = document.getElementById("bank-chat-input");
  if (!form || !input) return;
  if (askButton) askButton.onclick = () => input.focus();
  form.onsubmit = async (event) => {
    event.preventDefault();
    const message = input.value.trim();
    if (!message) return;
    const body = new URLSearchParams({ player: localPlayerName(), message });
    input.value = "";
    const updated = await postAction("/api/bank/chat", body.toString());
    renderMobile(updated);
  };
}

function bindAdminBankChat() {
  const form = document.getElementById("admin-bank-form");
  const input = document.getElementById("admin-bank-input");
  if (!form || !input) return;
  form.onsubmit = async (event) => {
    event.preventDefault();
    const message = input.value.trim();
    if (!message) return;
    input.value = "";
    const updated = await postAction("/api/bank/admin-message", new URLSearchParams({ message }).toString());
    renderAdmin(updated);
  };
}

function cardIcon(icon) {
  return {
    brown: "BR",
    light_blue: "LB",
    pink: "PK",
    orange: "OR",
    red: "RD",
    yellow: "YL",
    green: "GR",
    blue: "BL",
    station: "ST",
    bolt: "EL",
    water: "VA",
    utility: "VE",
    chance: "CH",
    community: "AL",
    jail: "FÄ",
    tax: "SK",
  }[icon] || "EP";
}

function tokensAt(players, position) {
  return players
    .filter((player) => player.position === position && player.token && !player.bankrupt)
    .map((player) => tokenAvatar(player, "mini"))
    .join("");
}

function renderOfferControls(state, localPlayer) {
  const buyButton = document.getElementById("buy-button");
  const declineButton = document.getElementById("decline-button");
  const rollButton = document.getElementById("roll-button");
  const offer = state.pendingOffer;
  const isLocalTurn = localPlayer?.name === state.currentPlayer;
  const offerForLocal = Boolean(offer && localPlayer?.name === offer.player);

  buyButton.disabled = !offerForLocal;
  declineButton.disabled = !offerForLocal;
  rollButton.disabled = !isLocalTurn || Boolean(offer) || Boolean(state.auction) || state.phase === "token_selection";
  if (localPlayer?.jailed) rollButton.textContent = "Slå för dubbel";
  else rollButton.textContent = "Kasta tärning";

  if (offer) {
    buyButton.textContent = `Köp ${offer.spaceName}`;
    declineButton.textContent = "Avstå köp";
  } else {
    buyButton.textContent = "Köp fastighet";
    declineButton.textContent = "Avstå";
  }
}

function renderAuctionControls(state, localPlayer) {
  const panel = document.getElementById("auction-panel");
  if (!state.auction) {
    panel.innerHTML = "";
    return;
  }

  const auction = state.auction;
  const bid100 = auction.nextBid;
  const bid500 = auction.highestBid + 500;
  const bidderButtons = localPlayer
    ? `<div class="auction-row"><strong>${escapeHtml(localPlayer.name)}</strong><button type="button" data-bidder="${escapeHtml(localPlayer.name)}" data-amount="${bid100}" ${localPlayer.cash < bid100 ? "disabled" : ""}>${bid100} kr</button><button type="button" data-bidder="${escapeHtml(localPlayer.name)}" data-amount="${bid500}" ${localPlayer.cash < bid500 ? "disabled" : ""}>${bid500} kr</button></div>`
    : `<p>Skriv ditt namn och välj pjäs för att lägga bud.</p>`;

  panel.innerHTML = `<h3>Auktion: ${auction.spaceName}</h3><p>Högsta bud: ${auction.highestBid} kr${auction.highestBidder ? `, ${auction.highestBidder}` : ""}</p>${bidderButtons}<button id="finish-auction-button" type="button">Slutför auktion</button>`;

  panel.querySelectorAll("button[data-bidder]").forEach((button) => {
    button.addEventListener("click", async () => {
      const body = new URLSearchParams({
        player: button.dataset.bidder,
        amount: button.dataset.amount,
      });
      const updated = await postAction("/api/game/auction/bid", body.toString());
      renderMobile(updated);
    });
  });

  document.getElementById("finish-auction-button").onclick = async () => {
    const updated = await postAction("/api/game/auction/finish", "");
    renderMobile(updated);
  };
}

function renderJailControls(state, localPlayer) {
  const panel = document.getElementById("jail-panel");
  if (!panel) return;
  const isLocalTurn = localPlayer?.name === state.currentPlayer;
  if (!localPlayer?.jailed) {
    panel.innerHTML = "";
    return;
  }
  panel.innerHTML = `<h3>Fängelse</h3><p>Försök ${localPlayer.jailTurns || 0}/3. Slå dubbel eller betala 500 kr.</p><button id="pay-jail-button" type="button" ${isLocalTurn && localPlayer.cash >= 500 ? "" : "disabled"}>Betala 500 kr</button>`;
  const button = document.getElementById("pay-jail-button");
  if (button) {
    button.onclick = async () => {
      const updated = await postAction("/api/game/pay-jail", playerBody());
      renderMobile(updated);
    };
  }
}

function renderBuildControls(state, localPlayer) {
  const panel = document.getElementById("build-panel");
  const buildButton = document.getElementById("build-button");
  if (!panel || !buildButton) return;

  const isLocalTurn = localPlayer?.name === state.currentPlayer;
  const options = state.buildableProperties || [];
  buildButton.disabled = !isLocalTurn || options.length === 0 || !options.some((option) => option.canBuild);

  if (options.length === 0) {
    panel.innerHTML = "";
    buildButton.textContent = "Bygg";
    buildButton.onclick = null;
    return;
  }

  panel.innerHTML = `<h3>Byggnader</h3>${options
    .map((option) => `<button type="button" data-build="${option.spaceIndex}" ${isLocalTurn && option.canBuild ? "" : "disabled"}><strong>${option.spaceName}</strong><span>${option.label} → ${option.nextLabel}</span><em>${option.buildCost} kr · ny hyra ${option.rentAfter} kr</em></button>`)
    .join("")}`;

  panel.querySelectorAll("button[data-build]").forEach((button) => {
    button.addEventListener("click", async () => {
      const body = new URLSearchParams({ player: localPlayerName(), spaceIndex: button.dataset.build });
      const updated = await postAction("/api/game/build", body.toString());
      renderMobile(updated);
    });
  });

  const first = options.find((option) => option.canBuild);
  buildButton.textContent = first && isLocalTurn ? `Bygg ${first.spaceName}` : "Bygg";
  buildButton.onclick = first && isLocalTurn
    ? async () => {
        const body = new URLSearchParams({ player: localPlayerName(), spaceIndex: first.spaceIndex });
        const updated = await postAction("/api/game/build", body.toString());
        renderMobile(updated);
      }
    : null;
}

function renderAssetControls(state, localPlayer) {
  const panel = document.getElementById("asset-panel");
  if (!panel) return;
  const isLocalTurn = localPlayer?.name === state.currentPlayer;
  const actions = state.assetActions || [];
  if (!localPlayer || !isLocalTurn || actions.length === 0) {
    panel.innerHTML = "";
    return;
  }
  panel.innerHTML = `<h3>Ekonomi</h3>${localPlayer.cash < 0 ? `<p class="debt">Du ligger ${Math.abs(localPlayer.cash)} kr back. Sälj eller inteckna.</p>` : ""}${actions
    .map((action) => `<div class="asset-row"><strong>${escapeHtml(action.spaceName)}</strong><span>${action.mortgaged ? "Intecknad" : action.buildings ? buildingLabel(action.buildings) : "Fri"}</span><button type="button" data-action="mortgage" data-space="${action.spaceIndex}" ${isLocalTurn && action.canMortgage ? "" : "disabled"}>Inteckna +${action.mortgageValue}</button><button type="button" data-action="unmortgage" data-space="${action.spaceIndex}" ${isLocalTurn && action.canUnmortgage ? "" : "disabled"}>Lös ${action.unmortgageCost}</button><button type="button" data-action="sell-building" data-space="${action.spaceIndex}" ${isLocalTurn && action.canSellBuilding ? "" : "disabled"}>Sälj byggnad +${action.sellValue}</button></div>`)
    .join("")}`;
  panel.querySelectorAll("button[data-action]").forEach((button) => {
    button.addEventListener("click", async () => {
      const body = new URLSearchParams({ player: localPlayerName(), spaceIndex: button.dataset.space });
      const updated = await postAction(`/api/game/${button.dataset.action}`, body.toString());
      renderMobile(updated);
    });
  });
}

function renderTokenButtons(state) {
  const panel = document.getElementById("token-buttons");
  const name = localPlayerName();
  const alreadyPicked = state.players.some((player) => player.name === name && player.token);
  const canPickToken = state.phase === "token_selection" && state.currentPlayer && name.length > 0 && !alreadyPicked;
  panel.innerHTML = state.tokenChoices
    .map((token) => `<button type="button" data-token="${token.id}" ${token.available && canPickToken ? "" : "disabled"}>${tokenAvatar({ name, token: token.id }, "choice")}<span>${token.label}</span></button>`)
    .join("");

  panel.querySelectorAll("button[data-token]").forEach((button) => {
    button.addEventListener("click", async () => {
      saveLocalPlayer(localPlayerName());
      const body = new URLSearchParams({ player: localPlayerName(), token: button.dataset.token });
      const updated = await postAction("/api/game/select-token", body.toString());
      renderMobile(updated);
    });
  });
}

function renderPlayerSummary(state, localPlayer) {
  const summary = document.getElementById("player-summary");
  if (!summary) return;
  const name = localPlayerName();
  if (!name) {
    summary.innerHTML = `<strong>Ingen spelare vald</strong><span>Skriv ditt namn innan du tar pjäs.</span>`;
    return;
  }
  if (!localPlayer) {
    summary.innerHTML = `<strong>${escapeHtml(name)}</strong><span>Väntar på att du väljer pjäs.</span>`;
    return;
  }
  const space = state.spaces[localPlayer.position];
  summary.innerHTML = `${tokenAvatar(localPlayer, "large")}<div><strong>${escapeHtml(localPlayer.name)}</strong><span>${tokenLabel(localPlayer.token) || "Ingen pjäs"} · ${space?.name || "Okänd ruta"} · ${localPlayer.cash} kr${localPlayer.jailed ? " · Fängslad" : ""}${localPlayer.bankrupt ? " · Konkurs" : ""}</span></div>`;
}

function buildingMarkers(space) {
  if (!space.buildings) return "";
  if (space.buildings >= 5) return `<div class="buildings hotel"><span>H</span></div>`;
  return `<div class="buildings">${Array.from({ length: space.buildings }, () => "<span></span>").join("")}</div>`;
}

function buildingLabel(level) {
  return {
    0: "ingen byggnad",
    1: "1 hus",
    2: "2 hus",
    3: "3 hus",
    4: "4 hus",
    5: "hotell",
  }[level] || "hotell";
}

function syncNameInput() {
  const input = document.getElementById("player-name-input");
  if (input && !input.value) input.value = localPlayerName();
}

function localPlayerName() {
  const input = document.getElementById("player-name-input");
  const typed = cleanName(input?.value || "");
  return typed || localStorage.getItem("eutherpal.playerName") || "";
}

function localPlayer() {
  return localPlayerName();
}

function saveLocalPlayer(name) {
  const cleaned = cleanName(name);
  if (cleaned) localStorage.setItem("eutherpal.playerName", cleaned);
}

function playerBody() {
  return new URLSearchParams({ player: localPlayerName() }).toString();
}

function cleanName(name) {
  return (name || "").trim().replace(/[\u0000-\u001f]/g, "").slice(0, 24);
}

function escapeHtml(value) {
  return String(value || "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
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

function tokenAvatar(player, size = "mini") {
  const token = player?.token || "none";
  const name = player?.name || "";
  const glyph = {
    bil: "BI",
    hatt: "HA",
    skepp: "SK",
    hund: "HU",
    sko: "SO",
  }[token] || initials(name);
  const title = `${escapeHtml(name)} ${tokenLabel(token)}`.trim();
  return `<span class="token-avatar token-${token} token-${size}" title="${title}"><i>${escapeHtml(glyph)}</i></span>`;
}

function initials(name) {
  const parts = cleanName(name).split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "EP";
  return parts.slice(0, 2).map((part) => part[0].toUpperCase()).join("");
}

function tileClass(space, index) {
  if (["go", "jail", "free_parking", "go_to_jail"].includes(space.kind)) return "corner";
  if (space.kind === "community") return "community";
  if (space.kind === "chance") return "chance";
  if (space.kind === "station") return "station";
  if (space.kind === "utility") return "utility";
  if (space.kind === "tax") return "tax";
  return `property color-${space.color || Math.floor(index / 5) % 8}`;
}

function gridAreaForBoardIndex(index) {
  if (index <= 10) return `11 / ${11 - index} / 12 / ${12 - index}`;
  if (index <= 20) return `${21 - index} / 1 / ${22 - index} / 2`;
  if (index <= 30) return `1 / ${index - 19} / 2 / ${index - 18}`;
  return `${index - 29} / 11 / ${index - 28} / 12`;
}
