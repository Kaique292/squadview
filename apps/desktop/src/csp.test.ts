import tauriConf from "../src-tauri/tauri.conf.json?raw";

/** As regras de segurança (CSP) da janela, por diretiva. */
function csp(): Record<string, string[]> {
  const policy: string = JSON.parse(tauriConf).app.security.csp;
  return Object.fromEntries(
    policy
      .split(";")
      .map((d) => d.trim().split(/\s+/))
      .filter((d) => d[0])
      .map(([name, ...sources]) => [name, sources]),
  );
}

describe("segurança da janela (CSP)", () => {
  it("deixa abrir a imagem escolhida para o ícone do servidor (vem como blob:)", () => {
    // Sem `blob:`, o recorte do ícone mostrava "Não deu para abrir essa imagem." no app de verdade
    // (no navegador e nos testes não há CSP, então só aparecia instalado).
    expect(csp()["img-src"]).toContain("blob:");
  });

  it("deixa ver a prévia dos anexos das conversas (protocolo asset)", () => {
    expect(csp()["img-src"]).toEqual(expect.arrayContaining(["asset:", "http://asset.localhost"]));
    expect(csp()["media-src"]).toEqual(expect.arrayContaining(["asset:", "http://asset.localhost"]));
  });

  it("não libera scripts de fora", () => {
    expect(csp()["script-src"]).toBeUndefined();
    expect(csp()["default-src"]).toEqual(["'self'"]);
  });
});
