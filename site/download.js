// Troca os links `a[data-download]` pelo instalador da última versão: um clique e o download
// começa, sem passar pela página da release (onde ficam também .sig, latest.json...).
// Se a consulta ao GitHub falhar, o link continua indo para a página da última versão.
// `window.downloadReady` resolve quando terminar (a página de convite espera por ele antes de
// tentar abrir o app: a navegação para `squadview://` cancelaria a consulta no meio).
window.downloadReady = (async () => {
  try {
    const res = await fetch("https://api.github.com/repos/Kaique292/squadview/releases/latest", {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!res.ok) return;
    const release = await res.json();
    const exe = (release.assets || []).find((a) => /-setup\.exe$/.test(a.name));
    if (!exe) return;
    const size = `${(exe.size / 1024 / 1024).toFixed(1).replace(".", ",")} MB`;
    for (const link of document.querySelectorAll("a[data-download]")) {
      link.href = exe.browser_download_url;
      link.setAttribute("download", exe.name);
    }
    for (const el of document.querySelectorAll("[data-download-info]")) {
      el.textContent = `${release.tag_name} · ${size} · Windows 10 ou 11`;
    }
  } catch {
    // Sem internet ou limite da API do GitHub: fica o link para a página da release.
  }
})();
