import { Loader2, LogIn } from "lucide-react";
import { useAccountStore } from "./account-store";

/** Botão "Entrar com o Discord", na cor do Discord. Abre o navegador para autorizar. */
export function DiscordLoginButton(props: { className?: string }) {
  const loggingIn = useAccountStore((s) => s.loggingIn);
  const login = useAccountStore((s) => s.login);
  return (
    <button
      type="button"
      onClick={() => void login()}
      disabled={loggingIn}
      className={`flex items-center justify-center gap-2 rounded-md bg-[#5865f2] px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-[#4752c4] disabled:opacity-70 ${props.className ?? ""}`}
    >
      {loggingIn ? <Loader2 size={16} className="animate-spin" /> : <LogIn size={16} />}
      {loggingIn ? "Autorize no navegador…" : "Entrar com o Discord"}
    </button>
  );
}
