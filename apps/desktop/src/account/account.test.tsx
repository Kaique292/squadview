import { act, fireEvent, render, screen } from "@testing-library/react";
import { useToastStore } from "../ui/toast-store";
import { DiscordLoginButton } from "./DiscordLoginButton";
import { type DiscordProfile, avatarUrl, displayName, useAccountStore } from "./account-store";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

const kaique: DiscordProfile = { id: "80351110224678912", username: "kaique", globalName: "K.A", avatar: "abc123" };

beforeEach(() => {
  invoke.mockReset();
  useAccountStore.setState({ profile: null, loggingIn: false });
  useToastStore.setState({ toasts: [] });
});

describe("conta do Discord", () => {
  it("entra pelo botão e guarda o perfil", async () => {
    invoke.mockResolvedValue(kaique);
    render(<DiscordLoginButton />);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Entrar com o Discord" })));
    expect(invoke).toHaveBeenCalledWith("discord_login");
    expect(useAccountStore.getState().profile).toEqual(kaique);
    expect(useToastStore.getState().toasts[0].message).toBe("Entrou como K.A.");
  });

  it("mostra que está esperando o navegador", () => {
    useAccountStore.setState({ loggingIn: true });
    render(<DiscordLoginButton />);
    expect(screen.getByRole("button", { name: "Autorize no navegador…" })).toBeDisabled();
  });

  it("avisa quando o login falha", async () => {
    invoke.mockRejectedValue("Você cancelou o login no Discord.");
    await act(() => useAccountStore.getState().login());
    expect(useAccountStore.getState().profile).toBeNull();
    expect(useAccountStore.getState().loggingIn).toBe(false);
    expect(useToastStore.getState().toasts[0]).toMatchObject({
      kind: "error",
      message: "Você cancelou o login no Discord.",
    });
  });

  it("sai da conta", async () => {
    useAccountStore.setState({ profile: kaique });
    invoke.mockResolvedValue(undefined);
    await act(() => useAccountStore.getState().logout());
    expect(invoke).toHaveBeenCalledWith("discord_logout");
    expect(useAccountStore.getState().profile).toBeNull();
  });

  it("carrega o perfil salvo", async () => {
    invoke.mockResolvedValue(kaique);
    await useAccountStore.getState().load();
    expect(invoke).toHaveBeenCalledWith("discord_profile");
    expect(useAccountStore.getState().profile).toEqual(kaique);
  });

  it("usa o apelido global e cai no nome de usuário", () => {
    expect(displayName(kaique)).toBe("K.A");
    expect(displayName({ ...kaique, globalName: null })).toBe("kaique");
  });

  it("monta a foto do CDN, animada ou padrão", () => {
    expect(avatarUrl(kaique)).toBe("https://cdn.discordapp.com/avatars/80351110224678912/abc123.png?size=128");
    expect(avatarUrl({ ...kaique, avatar: "a_xyz" }, 64)).toContain("a_xyz.gif?size=64");
    expect(avatarUrl({ ...kaique, avatar: null })).toMatch(/embed\/avatars\/[0-5]\.png$/);
  });
});
