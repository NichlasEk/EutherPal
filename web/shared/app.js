const POLL_INTERVAL_MS = {
  tv: 1800,
  mobile: 3000,
  admin: 5000,
};
const ADMIN_HEALTH_TTL_MS = 15000;
let latestMobileState = null;
let cachedAdminHealth = null;
let lastAdminHealthFetchAt = 0;
let adminHealthRequest = null;

(async function bootEutherPal() {
  const view = document.body.dataset.view;
  const state = await fetchState();

  if (view === "tv") renderTv(state);
  if (view === "mobile") {
    bindMobileSwipe();
    renderMobile(state);
    renderMobileAdminIfVisible(state, true);
    await renderSettings();
  }
  if (view === "admin") {
    renderAdmin(state, { forceHealth: true });
    await renderSettings();
  }

  startPolling(view);
})();

const TV_TOKEN_STEP_MS = 170;
let latestTvState = null;
let actionInFlight = false;
const tvTokenPositions = new Map();
const tvTokenTimers = new Map();

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

function startPolling(view) {
  if (!POLL_INTERVAL_MS[view]) return;
  const interval = POLL_INTERVAL_MS[view];

  const tick = async () => {
    const startedAt = Date.now();
    try {
      if (view === "mobile" && actionInFlight) return;
      const freshState = await fetchState();
      if (view === "tv") renderTv(freshState);
      if (view === "admin") renderAdmin(freshState);
      if (view === "mobile") {
        renderMobile(freshState);
        renderMobileAdminIfVisible(freshState);
      }
    } catch (error) {
      console.warn("Kunde inte uppdatera spelstatus", error);
    } finally {
      const elapsed = Date.now() - startedAt;
      window.setTimeout(tick, Math.max(750, interval - elapsed));
    }
  };

  window.setTimeout(tick, interval);
}

function renderTv(state) {
  latestTvState = state;
  const visiblePlayers = playersForTvBoard(state);
  document.getElementById("room-code").textContent = state.roomCode;
  document.getElementById("current-player").textContent = state.currentPlayer;
  document.getElementById("dice").textContent =
    state.phase === "token_selection" ? "Pjäsval" : `Tärning: ${state.dice.join(" + ")}`;
  document.getElementById("bank-message").textContent = state.bankMessage;
  renderTvAuction(state);
  renderFreeParkingPot(state.freeParkingPot || 0);

  const board = document.getElementById("board");
  board.innerHTML = "";
  state.spaces.forEach((space, index) => {
    const tile = document.createElement("div");
    tile.className = `tile ${tileClass(space, index)}`;
    tile.style.gridArea = gridAreaForBoardIndex(index);
    tile.tabIndex = 0;
    tile.innerHTML = tileContent(space, visiblePlayers, index);
    board.appendChild(tile);
  });

  const players = document.getElementById("players");
  players.innerHTML = state.players
    .map((player) => `<div class="player-row"><strong class="player-name">${escapeHtml(player.name)}</strong><em class="player-token">${tokenAvatar(player, "mini")} ${tokenLabel(player.token) || "Väljer pjäs"}${player.jailed ? " · Fängslad" : ""}</em><span class="player-cash">${player.cash} kr</span></div>`)
    .join("");

  const tokenPanel = document.getElementById("token-status");
  if (tokenPanel) {
    tokenPanel.innerHTML = state.tokenChoices
      .map((token) => `<span class="${token.available ? "available" : "taken"}">${token.label}</span>`)
      .join("");
  }
  const tokenPanelShell = document.getElementById("tv-token-panel");
  if (tokenPanelShell) tokenPanelShell.hidden = state.phase !== "token_selection";

  renderPropertyCard(document.getElementById("tv-card"), selectedSpace(state));
  renderEvents(document.getElementById("tv-events"), state.events);
}

function tileContent(space, players, position) {
  const meta = tvTileMeta(space);
  return `<span class="tile-index">${position}</span><div class="tile-copy"><strong class="tile-name ${tileNameSizeClass(space.name)}">${escapeHtml(space.name)}</strong>${meta ? `<small class="tile-meta">${escapeHtml(meta)}</small>` : ""}</div>${buildingMarkers(space)}${tileTokenLayer(players, position)}`;
}

function tileNameSizeClass(name) {
  const length = String(name || "").replace(/\s+/g, "").length;
  if (length >= 15) return "tile-name-xlong";
  if (length >= 11) return "tile-name-long";
  return "";
}

function tvTileMeta(space) {
  const rent = space.currentRent || space.rent;
  if (space.mortgaged) return rent ? `Intecknad · Hyra ${rent} kr` : "Intecknad";
  if (space.owner) return rent ? `Ägs ${space.owner} · Hyra ${rent} kr` : `Ägs ${space.owner}`;
  if (space.price) return rent ? `${space.price} kr · Hyra ${rent} kr` : `${space.price} kr`;
  return "";
}

function tileTokenLayer(players, position) {
  const occupants = players.filter((player) => player.position === position && player.token && !player.bankrupt);
  const count = Math.min(occupants.length, 5);
  return `<div class="tokens tokens-count-${count}">${occupants.map((player) => tokenAvatar(player, "mini")).join("")}</div>`;
}

function renderTvAuction(state) {
  const panel = document.getElementById("tv-auction");
  if (!panel) return;
  if (!state.auction) {
    panel.innerHTML = "";
    panel.hidden = true;
    return;
  }
  const auction = state.auction;
  panel.hidden = false;
  panel.innerHTML = `<strong>Auktion: ${escapeHtml(auction.spaceName)}</strong><span>Högsta bud ${auction.highestBid} kr${auction.highestBidder ? ` · ${escapeHtml(auction.highestBidder)}` : ""}</span><em>${auction.canFinish ? "Kan slutföras nu" : `${auction.secondsLeft}s kvar att bjuda`}</em>`;
}

function renderFreeParkingPot(amount) {
  const pot = document.getElementById("free-parking-pot");
  if (!pot) return;
  pot.innerHTML = `<span class="coin-stack" aria-hidden="true"><i></i><i></i><i></i></span><strong>Fri parkering</strong><em>${amount} kr</em>`;
}

function playersForTvBoard(state) {
  const boardSize = state.spaces.length || 40;
  state.players.forEach((player, index) => {
    const key = tvPlayerKey(player, index);
    const target = Number(player.position) || 0;
    const visible = tvTokenPositions.get(key);
    if (visible === undefined) {
      tvTokenPositions.set(key, target);
      return;
    }
    if (visible !== target && !tvTokenTimers.has(key)) {
      startTvTokenMove(key, visible, target, boardSize);
    }
  });

  return state.players.map((player, index) => {
    const key = tvPlayerKey(player, index);
    const visible = tvTokenPositions.get(key);
    return {
      ...player,
      position: visible === undefined ? player.position : visible,
      moving: tvTokenTimers.has(key),
    };
  });
}

function startTvTokenMove(key, from, to, boardSize) {
  let current = from;
  let remaining = (to - from + boardSize) % boardSize;
  if (remaining === 0) {
    tvTokenPositions.set(key, to);
    return;
  }

  const timer = window.setInterval(() => {
    current = (current + 1) % boardSize;
    remaining -= 1;
    tvTokenPositions.set(key, current);
    if (latestTvState) renderTv(latestTvState);
    if (remaining <= 0) {
      window.clearInterval(timer);
      tvTokenTimers.delete(key);
      tvTokenPositions.set(key, to);
      if (latestTvState) renderTv(latestTvState);
    }
  }, TV_TOKEN_STEP_MS);
  tvTokenTimers.set(key, timer);
}

function tvPlayerKey(player, index) {
  return `${player.name || "spelare"}:${player.token || index}`;
}

function renderMobile(state) {
  latestMobileState = state;
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
  document.getElementById("roll-button").onclick = (event) =>
    runMobileAction(event.currentTarget, "Kastar...", "/api/game/roll", playerBody());
  document.getElementById("buy-button").onclick = (event) =>
    runMobileAction(event.currentTarget, "Köper...", "/api/game/buy", playerBody());
  document.getElementById("decline-button").onclick = (event) =>
    runMobileAction(event.currentTarget, "Startar auktion...", "/api/game/decline", playerBody());
}

function renderAdmin(state, options = {}) {
  renderCachedAdminHealth();
  maybeRefreshAdminHealth(Boolean(options.forceHealth));
  document.getElementById("admin-room").textContent = state.roomCode;
  renderAdminTools(state);
  renderBankChat(document.getElementById("admin-bank-chat"), state.bankChat);
  bindAdminBankChat();
}

function renderCachedAdminHealth() {
  if (!cachedAdminHealth) return;
  const server = document.getElementById("admin-server");
  const ai = document.getElementById("admin-ai");
  if (server) server.textContent = cachedAdminHealth.status || "okänd";
  if (ai) ai.textContent = cachedAdminHealth.ai || "-";
}

function maybeRefreshAdminHealth(force = false) {
  const server = document.getElementById("admin-server");
  const ai = document.getElementById("admin-ai");
  if (!server && !ai) return;
  const now = Date.now();
  if (!force && cachedAdminHealth && now - lastAdminHealthFetchAt < ADMIN_HEALTH_TTL_MS) return;
  if (adminHealthRequest) return;

  lastAdminHealthFetchAt = now;
  adminHealthRequest = fetch("/health")
    .then((response) => response.json())
    .then((health) => {
      cachedAdminHealth = health;
      renderCachedAdminHealth();
    })
    .catch(() => {
      if (server) server.textContent = "okänd";
      if (ai) ai.textContent = "offline";
    })
    .finally(() => {
      adminHealthRequest = null;
    });
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
  if (document.body.dataset.view === "tv") {
    if (space.owner) return `<small>Ägs ${shortName(space.owner)}${space.mortgaged ? " · Pant" : ""}</small>`;
    if (space.price) return `<small>${space.price} kr${rent ? ` · H ${rent}` : ""}</small>`;
    return "";
  }
  if (space.owner) return `<small>Ägs: ${space.owner}${rent ? ` · Hyra ${rent}` : ""}${mortgage}</small>`;
  if (space.price) return `<small>${space.price} kr${rent ? ` · Hyra ${rent}` : ""}</small>`;
  return "";
}

function shortName(name) {
  const parts = cleanName(name).split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "";
  if (parts.length === 1) return parts[0].slice(0, 8);
  return parts.map((part) => part[0].toUpperCase()).join("");
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

function renderBankThinking(container, messages, playerName, message) {
  if (!container) return;
  const pending = [
    ...(messages || []),
    { speaker: playerName || "Spelare", text: message, fromBank: false },
    { speaker: "Banken", text: "Tänker", fromBank: true, thinking: true },
  ];
  container.innerHTML = pending
    .map((item) => item.thinking
      ? `<div class="bank-chat-message from-bank thinking"><strong>${escapeHtml(item.speaker)}</strong><p><span class="thinking-spinner" aria-hidden="true"></span>${escapeHtml(item.text)}...</p></div>`
      : `<div class="bank-chat-message ${item.fromBank ? "from-bank" : "from-player"}"><strong>${escapeHtml(item.speaker)}</strong><p>${escapeHtml(item.text)}</p></div>`)
    .join("");
  container.scrollTop = container.scrollHeight;
}

function bindMobileBankChat(state) {
  const askButton = document.getElementById("ask-bank-button");
  const form = document.getElementById("bank-chat-form");
  const input = document.getElementById("bank-chat-input");
  const submit = form?.querySelector('button[type="submit"]');
  if (!form || !input) return;
  if (askButton) askButton.onclick = () => input.focus();
  form.onsubmit = async (event) => {
    event.preventDefault();
    const message = input.value.trim();
    if (!message) return;
    const player = localPlayerName();
    const body = new URLSearchParams({ player, message });
    input.value = "";
    input.disabled = true;
    if (submit) submit.disabled = true;
    renderBankThinking(document.getElementById("bank-chat"), state.bankChat, player, message);
    try {
      const updated = await postAction("/api/bank/chat", body.toString());
      renderMobile(updated);
    } catch (error) {
      renderBankChat(document.getElementById("bank-chat"), [
        ...(state.bankChat || []),
        { speaker: player || "Spelare", text: message, fromBank: false },
        { speaker: "Banken", text: "Jag fick inget svar från LLM just nu. Försök igen.", fromBank: true },
      ]);
    } finally {
      input.disabled = false;
      if (submit) submit.disabled = false;
      input.focus();
    }
  };
}

function bindMobileSwipe() {
  const swipe = document.getElementById("mobile-swipe");
  const pages = swipe?.querySelector(".mobile-pages");
  const tabs = Array.from(document.querySelectorAll("[data-mobile-tab]"));
  const title = document.getElementById("mobile-view-title");
  if (!swipe || !pages || tabs.length === 0 || swipe.dataset.bound === "true") return;
  swipe.dataset.bound = "true";
  const panes = ["player", "assets", "admin"];
  const paneTitles = {
    player: "Spelarpanel",
    assets: "Fastigheter",
    admin: "Admin",
  };
  let active = panes.includes(localStorage.getItem("eutherpal.mobilePane"))
    ? localStorage.getItem("eutherpal.mobilePane")
    : "player";
  let startX = 0;
  let startY = 0;

  const apply = (next) => {
    active = panes.includes(next) ? next : "player";
    const index = panes.indexOf(active);
    localStorage.setItem("eutherpal.mobilePane", active);
    pages.style.transform = `translateX(-${index * (100 / panes.length)}%)`;
    if (title) title.textContent = paneTitles[active] || "Spelarpanel";
    tabs.forEach((tab) => {
      const selected = tab.dataset.mobileTab === active;
      tab.classList.toggle("active", selected);
      tab.setAttribute("aria-selected", selected ? "true" : "false");
    });
    renderMobileAdminIfVisible(latestMobileState, true);
  };

  tabs.forEach((tab) => {
    tab.addEventListener("click", () => apply(tab.dataset.mobileTab));
  });
  swipe.addEventListener("touchstart", (event) => {
    if (event.touches.length !== 1) return;
    startX = event.touches[0].clientX;
    startY = event.touches[0].clientY;
  }, { passive: true });
  swipe.addEventListener("touchend", (event) => {
    const touch = event.changedTouches[0];
    if (!touch) return;
    const dx = touch.clientX - startX;
    const dy = touch.clientY - startY;
    if (Math.abs(dx) < 70 || Math.abs(dx) < Math.abs(dy) * 1.35) return;
    const currentIndex = panes.indexOf(active);
    const nextIndex = dx < 0
      ? Math.min(panes.length - 1, currentIndex + 1)
      : Math.max(0, currentIndex - 1);
    apply(panes[nextIndex]);
  }, { passive: true });

  apply(active);
}

function currentMobilePane() {
  const pane = localStorage.getItem("eutherpal.mobilePane");
  return ["player", "assets", "admin"].includes(pane) ? pane : "player";
}

function renderMobileAdminIfVisible(state, forceHealth = false) {
  if (!state || currentMobilePane() !== "admin") return;
  renderAdmin(state, { forceHealth });
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

function renderAdminTools(state) {
  const status = document.getElementById("admin-tools-status");
  const playerSelect = document.getElementById("admin-adjust-player");
  if (!playerSelect) return;
  playerSelect.innerHTML = state.players
    .map((player) => `<option value="${escapeHtml(player.name)}">${escapeHtml(player.name)}</option>`)
    .join("");

  bindAdminToolButton("admin-save-game", "/api/game/save", status, "Sparat");
  bindAdminToolButton("admin-load-game", "/api/game/load", status, "Laddat");
  bindAdminToolButton("admin-demo-game", "/api/game/demo", status, "Demo laddad");
  bindAdminNewGameButton(status);

  const form = document.getElementById("admin-adjust-form");
  form.onsubmit = async (event) => {
    event.preventDefault();
    const body = new URLSearchParams({
      player: playerSelect.value,
      cashDelta: document.getElementById("admin-cash-delta").value || "0",
      position: document.getElementById("admin-position").value || "",
    });
    const updated = await postAction("/api/game/admin-adjust", body.toString());
    if (status) status.textContent = "Justerat";
    renderAdmin(updated);
  };
}

function bindAdminNewGameButton(status) {
  const button = document.getElementById("admin-new-game");
  const select = document.getElementById("admin-new-game-players");
  if (!button || !select) return;
  button.onclick = async () => {
    const players = select.value || "4";
    const updated = await postAction("/api/game/new", new URLSearchParams({ players }).toString());
    if (status) status.textContent = `Nytt spel: ${players} spelare`;
    renderAdmin(updated);
  };
}

function bindAdminToolButton(id, path, status, label) {
  const button = document.getElementById(id);
  if (!button) return;
  button.onclick = async () => {
    const updated = await postAction(path, "");
    if (status) status.textContent = label;
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
  if (actionInFlight) {
    buyButton.disabled = true;
    declineButton.disabled = true;
    rollButton.disabled = true;
  }
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
    ? `<div class="auction-row"><strong>${escapeHtml(localPlayer.name)}</strong><button type="button" data-bidder="${escapeHtml(localPlayer.name)}" data-amount="${bid100}" ${actionInFlight || localPlayer.cash < bid100 ? "disabled" : ""}>${bid100} kr</button><button type="button" data-bidder="${escapeHtml(localPlayer.name)}" data-amount="${bid500}" ${actionInFlight || localPlayer.cash < bid500 ? "disabled" : ""}>${bid500} kr</button></div>`
    : `<p>Skriv ditt namn och välj pjäs för att lägga bud.</p>`;
  const finishText = auction.canFinish ? "Slutför auktion" : `Vänta ${auction.secondsLeft}s`;

  panel.innerHTML = `<h3>Auktion: ${auction.spaceName}</h3><p>Högsta bud: ${auction.highestBid} kr${auction.highestBidder ? `, ${auction.highestBidder}` : ""}. ${auction.canFinish ? "Auktionen kan slutföras." : `${auction.secondsLeft}s kvar att bjuda.`}</p>${bidderButtons}<button id="finish-auction-button" type="button" ${actionInFlight || !auction.canFinish ? "disabled" : ""}>${finishText}</button>`;

  panel.querySelectorAll("button[data-bidder]").forEach((button) => {
    button.addEventListener("click", () => {
      const body = new URLSearchParams({
        player: button.dataset.bidder,
        amount: button.dataset.amount,
      });
      runMobileAction(button, "Budar...", "/api/game/auction/bid", body.toString());
    });
  });

  document.getElementById("finish-auction-button").onclick = (event) =>
    runMobileAction(event.currentTarget, "Slutför...", "/api/game/auction/finish", "");
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
    button.disabled = button.disabled || actionInFlight;
    button.onclick = (event) =>
      runMobileAction(event.currentTarget, "Betalar...", "/api/game/pay-jail", playerBody());
  }
}

function renderBuildControls(state, localPlayer) {
  const panel = document.getElementById("build-panel");
  const buildButton = document.getElementById("build-button");
  if (!panel || !buildButton) return;

  const isLocalTurn = localPlayer?.name === state.currentPlayer;
  const options = state.buildableProperties || [];
  buildButton.disabled = !isLocalTurn || options.length === 0 || !options.some((option) => option.canBuild);
  if (actionInFlight) buildButton.disabled = true;

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
    if (actionInFlight) button.disabled = true;
    button.addEventListener("click", () => {
      const body = new URLSearchParams({ player: localPlayerName(), spaceIndex: button.dataset.build });
      runMobileAction(button, "Bygger...", "/api/game/build", body.toString());
    });
  });

  const first = options.find((option) => option.canBuild);
  buildButton.textContent = first && isLocalTurn ? `Bygg ${first.spaceName}` : "Bygg";
  buildButton.onclick = first && isLocalTurn
    ? (event) => {
        const body = new URLSearchParams({ player: localPlayerName(), spaceIndex: first.spaceIndex });
        runMobileAction(event.currentTarget, "Bygger...", "/api/game/build", body.toString());
      }
    : null;
}

function renderAssetControls(state, localPlayer) {
  const panel = document.getElementById("asset-panel");
  const summary = document.getElementById("asset-summary");
  if (!panel) return;
  const isLocalTurn = localPlayer?.name === state.currentPlayer;
  const actionMap = new Map((state.assetActions || []).map((action) => [Number(action.spaceIndex), action]));
  const owned = localPlayer
    ? state.spaces
        .filter((space) => space.owner === localPlayer.name)
        .map((space) => {
          const action = isLocalTurn ? actionMap.get(Number(space.index)) : null;
          return {
            spaceIndex: space.index,
            spaceName: space.name,
            kind: space.kind,
            buildings: space.buildings || 0,
            mortgaged: Boolean(space.mortgaged),
            mortgageValue: space.mortgageValue || 0,
            unmortgageCost: space.unmortgageCost || 0,
            sellValue: Math.floor((space.buildCost || 0) / 2),
            canMortgage: Boolean(action?.canMortgage),
            canUnmortgage: Boolean(action?.canUnmortgage),
            canSellBuilding: Boolean(action?.canSellBuilding),
            color: space.color || space.kind,
          };
        })
    : [];
  if (!localPlayer || owned.length === 0) {
    if (summary) summary.textContent = localPlayer ? "Du äger inga fastigheter ännu." : "Skriv ditt namn och välj pjäs först.";
    panel.innerHTML = "";
    return;
  }
  const totalValue = owned.reduce((sum, action) => sum + Number(action.mortgageValue || 0), 0);
  if (summary) {
    summary.textContent = `${owned.length} fastigheter · pantvärde ${totalValue} kr${localPlayer.cash < 0 ? ` · skuld ${Math.abs(localPlayer.cash)} kr` : ""}`;
  }
  panel.innerHTML = `${localPlayer.cash < 0 ? `<p class="debt">Du ligger ${Math.abs(localPlayer.cash)} kr back. Sälj byggnad eller inteckna.</p>` : ""}${owned
    .map((action) => `<div class="asset-row asset-color-${escapeHtml(action.color)}"><div class="asset-title"><strong>${escapeHtml(action.spaceName)}</strong><span>${assetStateLabel(action)}</span></div><div class="asset-buttons"><button type="button" data-action="mortgage" data-space="${action.spaceIndex}" ${isLocalTurn && action.canMortgage && !actionInFlight ? "" : "disabled"}>Inteckna +${action.mortgageValue}</button><button type="button" data-action="unmortgage" data-space="${action.spaceIndex}" ${isLocalTurn && action.canUnmortgage && !actionInFlight ? "" : "disabled"}>Lös ${action.unmortgageCost}</button><button type="button" data-action="sell-building" data-space="${action.spaceIndex}" ${isLocalTurn && action.canSellBuilding && !actionInFlight ? "" : "disabled"}>Sälj byggnad +${action.sellValue}</button></div></div>`)
    .join("")}`;
  panel.querySelectorAll("button[data-action]").forEach((button) => {
    button.addEventListener("click", () => {
      const body = new URLSearchParams({ player: localPlayerName(), spaceIndex: button.dataset.space });
      runMobileAction(button, "Skickar...", `/api/game/${button.dataset.action}`, body.toString());
    });
  });
}

function assetStateLabel(action) {
  if (action.mortgaged) return "Intecknad";
  if (action.buildings) return buildingLabel(action.buildings);
  return action.kind === "property" ? "Tomt" : "Ägd";
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
    if (actionInFlight) button.disabled = true;
    button.addEventListener("click", () => {
      saveLocalPlayer(localPlayerName());
      const body = new URLSearchParams({ player: localPlayerName(), token: button.dataset.token });
      runMobileAction(button, "Väljer...", "/api/game/select-token", body.toString());
    });
  });
}

async function runMobileAction(button, loadingText, path, body) {
  if (actionInFlight) return;
  const oldText = button?.textContent || "";
  actionInFlight = true;
  disableMobileActionButtons(true);
  if (button) {
    button.disabled = true;
    button.textContent = loadingText;
  }
  try {
    const updated = await postAction(path, body);
    actionInFlight = false;
    renderMobile(updated);
  } catch (error) {
    actionInFlight = false;
    if (button) {
      button.textContent = "Försök igen";
      window.setTimeout(() => {
        button.textContent = oldText;
      }, 1200);
    }
    const status = document.getElementById("mobile-status");
    if (status) status.textContent = "Requesten gick inte fram. Försök igen.";
    try {
      renderMobile(await fetchState());
    } catch (_) {
      disableMobileActionButtons(false);
    }
  }
}

function disableMobileActionButtons(disabled) {
  document
    .querySelectorAll(".action-grid button, .auction-panel button, .asset-panel button, #token-buttons button, #jail-panel button")
    .forEach((button) => {
      button.disabled = disabled;
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
  const icon = tokenIcon(token);
  const glyph = {
    bil: "BI",
    hatt: "HA",
    skepp: "SK",
    hund: "HU",
    sko: "SO",
  }[token] || initials(name);
  const title = `${escapeHtml(name)} ${tokenLabel(token)}`.trim();
  const iconHtml = icon
    ? `<img src="${icon}" alt="" loading="lazy"><i>${escapeHtml(glyph)}</i>`
    : `<i>${escapeHtml(glyph)}</i>`;
  const movingClass = player?.moving ? " token-moving" : "";
  return `<span class="token-avatar token-${token} token-${size}${movingClass}" title="${title}">${iconHtml}</span>`;
}

function tokenIcon(token) {
  return {
    bil: "/assets/tokens/bil.png",
    hatt: "/assets/tokens/hatt.png",
    skepp: "/assets/tokens/skepp.png",
    hund: "/assets/tokens/hund.png",
    sko: "/assets/tokens/sko.png",
  }[token] || "";
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
