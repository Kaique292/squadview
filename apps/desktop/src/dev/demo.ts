/**
 * Cenas de demonstração para as capturas do README (só no `yarn dev`, nunca no instalador):
 * `http://localhost:1420/#demo=sala`, `#demo=transmitir`, `#demo=config`, `#demo=amigos`, `#demo=conversa`,
 * `#demo=servidor`, `#demo=transmitindo`, `#demo=detalhes` ou `#demo=assistindo`.
 * Preenche as stores com uma sala de exemplo, sem precisar do Rust.
 */
import { useAccountStore } from "../account/account-store";
import { type DmMessage, useDmStore } from "../dm/dm-store";
import { useFriendsStore } from "../friends/friends-store";
import { useAppearanceStore } from "../settings/appearance-store";
import { useRoomStore } from "../room/room-store";
import { useServersStore } from "../servers/servers-store";
import { type Source, type StreamStats, sourceKey, useStreamStore } from "../stream/stream-store";

// Lobo e Yondaime assistem o Minato: o 👁 2 aparece ao lado do "Ao vivo".
const MEMBERS = [
  { id: "eu", name: "Kaique", live: false },
  { id: "a", name: "Minato", live: true },
  { id: "b", name: "Lobo", live: false, muted: true, watching: "a" },
  { id: "c", name: "Yondaime", live: false, watching: "a" },
];

/** Na página de amigos, Minato e Lobo estão na sala com a gente: aparecem "Na sua sala". */
const WITH_FRIENDS = MEMBERS.map((m) =>
  m.id === "a" || m.id === "b"
    ? { ...m, discord: { id: m.id === "a" ? "100000000000000001" : "100000000000000002", avatar: null } }
    : m,
);

const online = (room: string | null, live = false) => ({ online: true, inRoom: room !== null, room, live });
const FRIENDS = [
  { id: "100000000000000001", name: "Minato", avatar: null, direct: true, presence: online("Galera do LoL", true) },
  { id: "100000000000000002", name: "Lobo", avatar: null, direct: false, presence: online("Galera do LoL") },
  { id: "100000000000000003", name: "Sakura", avatar: null, direct: false, presence: online(null) },
  { id: "100000000000000004", name: "Itachi", avatar: null, direct: false, presence: online("Ranked da noite") },
  {
    id: "100000000000000005",
    name: "Kakashi",
    avatar: null,
    direct: false,
    presence: { online: false, inRoom: false, room: null, live: false },
  },
];

const SOURCES: Source[] = [
  {
    id: { kind: "window", handle: 1 },
    name: "League of Legends (TM) Client",
    processId: 1,
    processName: "League of Legends.exe",
  },
  { id: { kind: "window", handle: 2 }, name: "VALORANT", processId: 2, processName: "VALORANT.exe" },
  { id: { kind: "window", handle: 3 }, name: "Spotify Premium", processId: 3, processName: "Spotify.exe" },
  { id: { kind: "monitor", handle: 4 }, name: "Tela 1", processId: null, processName: null },
];

/** Miniatura de mentira: um degradê com cara de jogo. */
function thumbnail(from: string, to: string, label: string) {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="320" height="180">
    <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${from}"/><stop offset="1" stop-color="${to}"/></linearGradient></defs>
    <rect width="320" height="180" fill="url(#g)"/>
    <circle cx="250" cy="40" r="60" fill="#fff" opacity=".08"/>
    <text x="16" y="164" font-family="Segoe UI" font-size="14" font-weight="700" fill="#fff" opacity=".85">${label}</text>
  </svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

const THUMBS = [
  thumbnail("#0a3d62", "#c89b3c", "LEAGUE OF LEGENDS"),
  thumbnail("#ff4655", "#1f2731", "VALORANT"),
  thumbnail("#1db954", "#121212", "SPOTIFY"),
  thumbnail("#3a1c71", "#d76d77", "ÁREA DE TRABALHO"),
];

function click(label: string) {
  const button = [...document.querySelectorAll<HTMLElement>('button, [role="button"]')].find(
    (b) => b.textContent?.trim() === label || b.getAttribute("aria-label") === label,
  );
  button?.click();
}

/** Ícone de servidor de mentira: um degradê com uma letra. */
function serverIcon(from: string, to: string, letter: string) {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96">
    <defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${from}"/><stop offset="1" stop-color="${to}"/></linearGradient></defs>
    <rect width="96" height="96" fill="url(#g)"/>
    <text x="48" y="62" font-family="Segoe UI" font-size="44" font-weight="800" fill="#fff" text-anchor="middle">${letter}</text>
  </svg>`;
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

const SERVERS = [
  {
    id: "s1",
    name: "Turma do Bairro",
    icon: serverIcon("#5865f2", "#eb459e", "T"),
    rooms: [
      { id: "r1", name: "Galera do LoL", code: "F5B6S9" },
      { id: "r2", name: "Ranked da noite", code: "K7M2Q4" },
      { id: "r3", name: "Só papo", code: "P3X8W1" },
    ],
    members: [
      { id: "eu", name: "Kaique", avatar: null, role: "owner" as const },
      { id: "100000000000000004", name: "Itachi", avatar: null, role: "admin" as const },
      { id: "100000000000000003", name: "Sakura", avatar: null, role: "member" as const },
      { id: "100000000000000005", name: "Kakashi", avatar: null, role: "member" as const },
    ],
  },
  { id: "s2", name: "Valorant BR", icon: serverIcon("#ff4655", "#1f2731", "V"), rooms: [], members: [] },
  { id: "s3", name: "Faculdade", icon: null, rooms: [], members: [] },
];

/** Conversa privada com o Minato: texto, uma foto e um arquivo esperando "Baixar". */
const minute = 60 * 1000;
const now = Date.now();
const CHAT: DmMessage[] = [
  { id: "1", mine: false, at: now - 42 * minute, text: "viu o clutch que eu fiz ontem?", status: "received" },
  { id: "2", mine: true, at: now - 41 * minute, text: "não! manda aí", status: "delivered" },
  {
    id: "3",
    mine: false,
    at: now - 40 * minute,
    file: { name: "clutch.png", size: 412_000, mime: "image/png", path: "clutch.png", done: 412_000 },
    status: "received",
  },
  {
    id: "4",
    mine: false,
    at: now - 39 * minute,
    file: { name: "replay-ranked.rofl", size: 18_400_000, mime: "application/octet-stream", path: null, done: 0 },
    status: "received",
  },
  { id: "5", mine: true, at: now - 2 * minute, text: "que isso 😂 bora de novo hoje às 21h?", status: "delivered" },
  { id: "6", mine: false, at: now - minute, text: "bora, chama o Lobo", status: "received" },
];

const CONVERSATIONS = [
  { friend: "100000000000000001", last: CHAT[5], pending: 0 },
  {
    friend: "100000000000000003",
    last: { id: "s", mine: true, at: now - 3 * 3600 * 1000, text: "valeu pela call!", status: "delivered" as const },
    pending: 0,
  },
  {
    friend: "100000000000000004",
    last: {
      id: "i",
      mine: false,
      at: now - 26 * 3600 * 1000,
      file: { name: "mapa.zip", size: 1, mime: "application/zip", path: null, done: 0 },
      status: "received" as const,
    },
    pending: 0,
  },
];

/** Um minuto de medições da nossa live (a cada 500 ms), com cara de partida de verdade. */
function liveHistory(): StreamStats[] {
  return Array.from({ length: 120 }, (_, i) => ({
    rttMs: 18,
    audioActive: true,
    audioError: null,
    audioKbps: 128,
    audioLevel: 0.42 + 0.2 * Math.abs(Math.sin(i / 3)),
    audioLost: 0,
    sourceFps: 144,
    fps: 30 - (i % 37 === 0 ? 2 : 0),
    bitrateKbps: 4200 + 900 * Math.sin(i / 9) + 250 * Math.sin(i / 2.3),
    encodeMs: 1.6 + 0.5 * Math.sin(i / 4) + 0.15 * Math.sin(i * 1.7),
    latencyMs: 13 + 3 * Math.sin(i / 7) + 1.2 * Math.sin(i * 1.3),
    latencyP95Ms: 22,
    dropped: 0,
    width: 1280,
    height: 720,
    encoder: "NVIDIA H.264 Encoder MFT",
  }));
}

/** A nossa live no ar: no lugar do vídeo nativo (que não existe no navegador), uma imagem do jogo. */
function goLive() {
  const history = liveHistory();
  useStreamStore.setState({
    status: "live",
    quality: { resolution: "720", fps: 30 },
    stats: history.at(-1),
    history,
    system: { gpu3d: 64, gpuEncode: 17, gpuDecode: 0, cpu: 22 },
    current: { source: SOURCES[0].id, audioPid: 1, lastAudioPid: 1 },
  });
  useRoomStore.setState({
    members: MEMBERS.map((m) =>
      m.id === "eu" ? { ...m, live: true } : { ...m, watching: m.id === "a" ? null : "eu" },
    ),
  });
  setTimeout(() => {
    const area = document.querySelector<HTMLElement>('[data-testid="area-do-video"]');
    if (area) {
      area.style.background = `center / cover url("${thumbnail("#0a3d62", "#c89b3c", "")}")`;
      area.replaceChildren();
    }
  }, 250);
}

/** Assistindo duas lives ao mesmo tempo (Minato e Yondaime), cada uma como um quadro da sala. */
function watchTwo() {
  const history = liveHistory();
  const members = MEMBERS.map((m) =>
    m.id === "c" ? { ...m, live: true, watching: null, watchingAll: [] } : { ...m, watchingAll: ["a", "c"] },
  );
  const watch = (id: string) => ({
    member: members.find((m) => m.id === id)!,
    stats: { ...history.at(-1)!, rttMs: id === "a" ? 18 : 41 },
    history,
    listening: id === "a",
    volume: 100,
  });
  useRoomStore.setState({ members, watching: { a: watch("a"), c: watch("c") } });
  setTimeout(() => {
    for (const [id, colors] of [
      ["a", ["#0a3d62", "#c89b3c"]],
      ["c", ["#3c1053", "#ad5389"]],
    ] as const) {
      const area = document.querySelector<HTMLElement>(`[data-testid="video-de-${id}"]`);
      if (area) {
        area.style.background = `center / cover url("${thumbnail(colors[0], colors[1], "")}")`;
        area.replaceChildren();
      }
    }
  }, 250);
}

/** Respostas de mentira do Rust, para as telas que perguntam algo a ele. */
const FAKE_COMMANDS: Record<string, unknown> = {
  diagnostics_status: { enabled: true, files: 3 },
  dm_preview: thumbnail("#0a3d62", "#c89b3c", "PENTAKILL"),
};

export function runDemo(scene: string) {
  useAppearanceStore.getState().setGlass(0);
  useRoomStore.setState({
    code: "F5B6S9",
    roomName: "Galera do LoL",
    joinedAt: Date.now() - (1 * 3600 + 24 * 60 + 9) * 1000,
    members: MEMBERS,
  });
  useStreamStore.setState({
    loadSources: async () => {
      useStreamStore.setState({
        sources: SOURCES,
        loadingSources: false,
        error: null,
        thumbnails: Object.fromEntries(SOURCES.map((s, i) => [sourceKey(s.id), THUMBS[i]])),
      });
    },
  });
  // Por último (o vidro acima mexeria na janela "de verdade"): só as telas usam este invoke.
  Object.assign(window, {
    __TAURI_INTERNALS__: {
      invoke: async (cmd: string) => FAKE_COMMANDS[cmd] ?? null,
      // A prévia dos anexos já vem como data URL.
      convertFileSrc: (path: string) => path,
      // Arrastar arquivos para a conversa escuta eventos da janela: basta existir.
      metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
      transformCallback: () => 0,
    },
  });
  if (scene === "amigos" || scene === "conversa") {
    useRoomStore.setState({ members: WITH_FRIENDS });
    useDmStore.setState({ conversations: CONVERSATIONS, unread: { "100000000000000004": 1 } });
    useFriendsStore.setState({
      status: "online",
      open: true,
      friends: FRIENDS,
      incoming: [{ id: "100000000000000006", name: "Hinata", avatar: null }],
      joinRequests: [{ id: "00000000000000aa", from: { id: "100000000000000007", name: "Naruto", avatar: null } }],
    });
  }
  if (scene === "conversa") {
    // Logado com o Discord, e sem o pedido de entrar cobrindo a caixa de escrever.
    useAccountStore.setState({
      profile: { id: "100000000000000000", username: "kaique", globalName: "Kaique", avatar: null },
    });
    useFriendsStore.setState({ joinRequests: [] });
    useDmStore.setState({
      open: "100000000000000001",
      messages: { "100000000000000001": CHAT },
      online: { "100000000000000001": true },
    });
  }
  if (scene === "servidor")
    useServersStore.setState({
      servers: SERVERS,
      selected: "s1",
      // Gente nas outras salas do servidor, mesmo com a gente fora delas (ADR 0018).
      presence: {
        s1: {
          r2: [
            { id: "100000000000000004", live: true },
            { id: "100000000000000003", live: false },
          ],
          r3: [{ id: "100000000000000005", live: false }],
        },
      },
    });
  if (scene === "transmitindo" || scene === "detalhes") goLive();
  if (scene === "assistindo") watchTwo();
  // Espera a interface montar antes de abrir os modais.
  setTimeout(() => {
    // Na sala, o nome e os controles só aparecem com o mouse em cima (como no Discord).
    document
      .querySelector('[aria-label="Canal de voz"]')
      ?.dispatchEvent(new MouseEvent("mouseover", { bubbles: true, relatedTarget: document.body }));
    // A sala com alguém em foco, como no Discord.
    if (scene === "sala" || scene === "servidor") click("Pôr Lobo em foco");
    if (scene === "transmitir") click("Transmitir minha tela");
    if (scene === "config") click("Configurações");
    // Os detalhes da live abrem pelo menu dela (botão direito no quadro).
    if (scene === "detalhes") {
      document
        .querySelector("[data-stream-tile]")
        ?.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 400, clientY: 300 }));
      setTimeout(
        () =>
          [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')]
            .find((i) => i.textContent?.includes("Detalhes da transmissão"))
            ?.click(),
        200,
      );
    }
  }, 300);
}
