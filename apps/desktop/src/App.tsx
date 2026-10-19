import { MonitorUp, Plus, Settings, Users } from "lucide-react";
import { UpdateBanner } from "./update/UpdateBanner";

/**
 * Esqueleto do layout principal (Fase 0). As salas, os membros e os streams
 * de verdade chegam nas Fases 3 e 4 (ver docs/roadmap.md).
 */
export function App() {
  return (
    <div className="flex h-full">
      <nav aria-label="Salas" className="flex w-[72px] flex-col items-center gap-2 bg-surface-0 py-3">
        <div className="grid size-12 place-items-center rounded-2xl bg-brand text-lg font-bold">SV</div>
        <div className="h-0.5 w-8 rounded bg-surface-3" />
        <button
          type="button"
          aria-label="Criar sala"
          className="grid size-12 place-items-center rounded-3xl bg-surface-3 text-online transition-all hover:rounded-2xl hover:bg-online hover:text-white"
        >
          <Plus size={22} />
        </button>
      </nav>

      <aside className="flex w-60 flex-col bg-surface-1">
        <header className="flex h-12 items-center px-4 font-semibold shadow-sm">SquadView</header>
        <div className="flex-1 px-2 py-3 text-sm text-text-faint">
          <p className="flex items-center gap-2 px-2">
            <Users size={16} /> Nenhuma sala ainda
          </p>
        </div>
        <footer className="flex h-14 items-center justify-between bg-surface-0/60 px-2">
          <div className="flex items-center gap-2">
            <div className="size-8 rounded-full bg-surface-3" />
            <span className="text-sm font-medium">Você</span>
          </div>
          <button
            type="button"
            aria-label="Configurações"
            className="rounded p-1.5 text-text-muted hover:bg-surface-hover"
          >
            <Settings size={18} />
          </button>
        </footer>
      </aside>

      <div className="flex flex-1 flex-col">
        <UpdateBanner />
        <main className="flex flex-1 flex-col items-center justify-center gap-4 bg-surface-2 text-center">
          <MonitorUp size={56} className="text-text-faint" />
          <h1 className="text-2xl font-semibold">Ninguém está transmitindo ainda</h1>
          <p className="max-w-sm text-text-muted">
            Crie uma sala, chame a galera e compartilhe a tela. A própria sala distribui o vídeo entre vocês.
          </p>
          <button
            type="button"
            disabled
            className="rounded-md bg-brand px-4 py-2 font-medium transition-colors hover:bg-brand-hover disabled:opacity-50"
          >
            Transmitir minha tela
          </button>
        </main>
      </div>
    </div>
  );
}
