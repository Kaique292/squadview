/** Página de convite (GitHub Pages): abre o app pelo `squadview://` ou oferece o download. */
export const INVITE_PAGE = "https://kaique292.github.io/squadview/convite/";

/** Link para mandar à galera: funciona para quem tem e para quem não tem o app. */
export function inviteUrl(code: string): string {
  return `${INVITE_PAGE}?sala=${encodeURIComponent(code)}`;
}
