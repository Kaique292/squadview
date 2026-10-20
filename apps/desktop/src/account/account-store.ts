import { invoke } from "@tauri-apps/api/core";
import { create } from "zustand";
import { toast } from "../ui/toast-store";

/** Perfil público do Discord (o que o Rust salva em `discord.json`). */
export interface DiscordProfile {
  id: string;
  username: string;
  globalName: string | null;
  avatar: string | null;
}

interface AccountState {
  profile: DiscordProfile | null;
  /** Esperando a pessoa autorizar no navegador. */
  loggingIn: boolean;
  load: () => Promise<void>;
  login: () => Promise<void>;
  logout: () => Promise<void>;
}

export const useAccountStore = create<AccountState>((set) => ({
  profile: null,
  loggingIn: false,

  async load() {
    try {
      set({ profile: await invoke<DiscordProfile | null>("discord_profile") });
    } catch {
      // Sem perfil salvo: segue sem login.
    }
  },

  async login() {
    set({ loggingIn: true });
    try {
      const profile = await invoke<DiscordProfile>("discord_login");
      set({ profile });
      toast.success(`Entrou como ${displayName(profile)}.`, "Discord conectado");
    } catch (e) {
      toast.error(String(e), "Login com o Discord");
    } finally {
      set({ loggingIn: false });
    }
  },

  async logout() {
    try {
      await invoke("discord_logout");
    } finally {
      set({ profile: null });
      toast.info("Saiu da conta do Discord.");
    }
  },
}));

/** Nome que aparece no Discord (o "apelido global"; se não houver, o nome de usuário). */
export function displayName(p: DiscordProfile): string {
  return p.globalName?.trim() || p.username;
}

/** Foto do perfil no CDN do Discord; sem foto, o avatar padrão do Discord. */
export function avatarUrl(p: Pick<DiscordProfile, "id" | "avatar">, size = 128): string {
  if (p.avatar) {
    const ext = p.avatar.startsWith("a_") ? "gif" : "png";
    return `https://cdn.discordapp.com/avatars/${p.id}/${p.avatar}.${ext}?size=${size}`;
  }
  let index = 0;
  try {
    index = Number((BigInt(p.id) >> 22n) % 6n);
  } catch {
    // ID inválido: fica no primeiro avatar padrão.
  }
  return `https://cdn.discordapp.com/embed/avatars/${index}.png`;
}
