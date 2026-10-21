/**
 * Cenas de demonstração para as capturas do README (só no `yarn dev`, nunca no instalador):
 * `http://localhost:1420/#demo=sala`, `#demo=transmitir`, `#demo=config` ou `#demo=amigos`.
 * Preenche as stores com uma sala de exemplo, sem precisar do Rust.
 */
import { useFriendsStore } from "../friends/friends-store";
import { useAppearanceStore } from "../settings/appearance-store";
import { useRoomStore } from "../room/room-store";
import { type Source, sourceKey, useStreamStore } from "../stream/stream-store";

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

/** Respostas de mentira do Rust, para as telas que perguntam algo a ele. */
const FAKE_COMMANDS: Record<string, unknown> = {
  diagnostics_status: { enabled: true, files: 3 },
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
    __TAURI_INTERNALS__: { invoke: async (cmd: string) => FAKE_COMMANDS[cmd] ?? null },
  });
  if (scene === "amigos") {
    useRoomStore.setState({ members: WITH_FRIENDS });
    useFriendsStore.setState({
      status: "online",
      open: true,
      friends: FRIENDS,
      incoming: [{ id: "100000000000000006", name: "Hinata", avatar: null }],
      joinRequests: [{ id: "00000000000000aa", from: { id: "100000000000000007", name: "Naruto", avatar: null } }],
    });
  }
  // Espera a interface montar antes de abrir os modais.
  setTimeout(() => {
    // Na sala, o nome e os controles só aparecem com o mouse em cima (como no Discord).
    document
      .querySelector('[aria-label="Canal de voz"]')
      ?.dispatchEvent(new MouseEvent("mouseover", { bubbles: true, relatedTarget: document.body }));
    // A sala com alguém em foco, como no Discord.
    if (scene === "sala") click("Pôr Lobo em foco");
    if (scene === "transmitir") click("Transmitir minha tela");
    if (scene === "config") click("Configurações");
  }, 300);
}
