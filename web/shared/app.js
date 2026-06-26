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
  state.spaces.forEach((space, index) => {
    const tile = document.createElement("div");
    tile.className = `tile ${tileClass(space, index)}`;
    tile.style.gridArea = gridAreaForBoardIndex(index);
    tile.tabIndex = 0;
    tile.innerHTML = `<span class="tile-index">${index}</span><strong>${space.name}</strong>${spaceDetails(space)}<div class="tokens">${tokensAt(state.players, index)}</div>`;
    board.appendChild(tile);
  });

  const players = document.getElementById("players");
  players.innerHTML = state.players
    .map((player) => `<div class="player-row"><strong>${player.name}</strong><span>${player.cash} kr</span><em>${tokenLabel(player.token) || "Väljer pjäs"}${player.jailed ? " · Fängslad" : ""}</em></div>`)
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
  document.getElementById("mobile-status").textContent =
    state.phase === "token_selection"
      ? `${state.currentPlayer} väljer pjäs.`
      : state.phase === "auction"
        ? `Auktion pågår.`
      : `${state.currentPlayer} har tur.`;
  document.getElementById("mobile-bank-message").textContent = state.bankMessage;
  document.getElementById("join-button").addEventListener("click", () => {
    const room = document.getElementById("room-input").value || state.roomCode;
    document.getElementById("mobile-status").textContent = `Ansluten till ${room}. Riktiga sessioner kommer i nästa steg.`;
  });

  renderTokenButtons(state);
  renderPropertyCard(document.getElementById("mobile-card"), selectedSpace(state));
  renderEvents(document.getElementById("mobile-events"), state.events);
  renderOfferControls(state);
  renderAuctionControls(state);
  document.getElementById("roll-button").onclick = async () => {
    const updated = await postAction("/api/game/roll", "");
    renderMobile(updated);
  };
  document.getElementById("buy-button").onclick = async () => {
    const updated = await postAction("/api/game/buy", "");
    renderMobile(updated);
  };
  document.getElementById("decline-button").onclick = async () => {
    const updated = await postAction("/api/game/decline", "");
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

function spaceDetails(space) {
  if (space.owner) return `<small>Ägs: ${space.owner}</small>`;
  if (space.price) return `<small>${space.price} kr</small>`;
  return "";
}

function selectedSpace(state) {
  if (state.drawnCard) return drawnCardAsSpace(state.drawnCard);
  if (state.pendingOffer) return state.spaces[state.pendingOffer.spaceIndex];
  if (state.auction) return state.spaces[state.auction.spaceIndex];
  const current = state.players.find((player) => player.name === state.currentPlayer) || state.players[0];
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
    space.rent ? `Hyra ${space.rent} kr` : "",
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
    .filter((player) => player.position === position && player.token)
    .map((player) => `<span title="${player.name}">${tokenIcon(player.token)}</span>`)
    .join("");
}

function renderOfferControls(state) {
  const buyButton = document.getElementById("buy-button");
  const declineButton = document.getElementById("decline-button");
  const rollButton = document.getElementById("roll-button");
  const offer = state.pendingOffer;

  buyButton.disabled = !offer;
  declineButton.disabled = !offer;
  rollButton.disabled = Boolean(offer) || Boolean(state.auction) || state.phase === "token_selection";

  if (offer) {
    buyButton.textContent = `Köp ${offer.spaceName}`;
    declineButton.textContent = "Avstå köp";
  } else {
    buyButton.textContent = "Köp fastighet";
    declineButton.textContent = "Avstå";
  }
}

function renderAuctionControls(state) {
  const panel = document.getElementById("auction-panel");
  if (!state.auction) {
    panel.innerHTML = "";
    return;
  }

  const auction = state.auction;
  const bidderButtons = state.players
    .map((player) => {
      const bid100 = auction.nextBid;
      const bid500 = auction.highestBid + 500;
      const disabled100 = player.cash < bid100 ? "disabled" : "";
      const disabled500 = player.cash < bid500 ? "disabled" : "";
      return `<div class="auction-row"><strong>${player.name}</strong><button type="button" data-bidder="${player.name}" data-amount="${bid100}" ${disabled100}>${bid100} kr</button><button type="button" data-bidder="${player.name}" data-amount="${bid500}" ${disabled500}>${bid500} kr</button></div>`;
    })
    .join("");

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
