import { presenceInfo } from "./presence-store";

const base = {
  show: true,
  shareInvite: false,
  code: "F5B6S9",
  joinedAt: 1000,
  members: 3,
  live: false,
  watching: null,
};

describe("status no Discord", () => {
  it("fora de sala ou desligado, limpa o status", () => {
    expect(presenceInfo({ ...base, code: null })).toBeNull();
    expect(presenceInfo({ ...base, show: false })).toBeNull();
  });

  it("mostra a sala sem o botão de entrar por padrão", () => {
    expect(presenceInfo(base)).toEqual({
      members: 3,
      live: false,
      watching: null,
      joinedAtMs: 1000,
      inviteUrl: null,
    });
  });

  it("com permissão, o botão leva ao link de convite", () => {
    expect(presenceInfo({ ...base, shareInvite: true })?.inviteUrl).toBe(
      "https://kaique292.github.io/squadview/convite/?sala=F5B6S9",
    );
  });
});
