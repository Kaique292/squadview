import { ZoomIn, ZoomOut } from "lucide-react";
import { type CSSProperties, type KeyboardEvent, type PointerEvent, useRef, useState } from "react";
import { Modal, ModalClose } from "../ui/Modal";
import {
  type Crop,
  centeredCrop,
  clampCrop,
  coverScale,
  encodeIcon,
  ICON_SIZE,
  MAX_ZOOM,
  sourceRect,
  zoomAround,
} from "./icon-crop";

/** Lado da janela de recorte (px da tela). */
const VIEW = 256;

/**
 * Ajustar o ícone antes de salvar: arrastar a imagem posiciona, o controle (ou a roda do mouse)
 * dá zoom, e as prévias mostram como fica na barra de salas. "Aplicar" recorta em 96 px e
 * comprime para caber no servidor.
 */
export function IconCropDialog(props: {
  /** A imagem escolhida (`null`: fechado). */
  image: HTMLImageElement | null;
  onClose: () => void;
  onApply: (icon: string) => void;
}) {
  return (
    <Modal
      open={props.image !== null}
      onOpenChange={(open) => !open && props.onClose()}
      width={520}
      title="Ajustar o ícone"
      description="Arraste para posicionar e use o zoom para escolher o pedaço."
    >
      {props.image && <Cropper key={props.image.src} {...props} image={props.image} />}
    </Modal>
  );
}

function Cropper(props: { image: HTMLImageElement; onClose: () => void; onApply: (icon: string) => void }) {
  const { naturalWidth: w, naturalHeight: h } = props.image;
  const [crop, setCrop] = useState<Crop>(() => centeredCrop(w, h, VIEW));
  const [error, setError] = useState<string | null>(null);
  const drag = useRef<{ x: number; y: number; crop: Crop } | null>(null);
  const scale = coverScale(w, h, VIEW) * crop.zoom;

  function onPointerDown(e: PointerEvent<HTMLDivElement>) {
    e.currentTarget.setPointerCapture?.(e.pointerId);
    drag.current = { x: e.clientX, y: e.clientY, crop };
  }

  function onPointerMove(e: PointerEvent<HTMLDivElement>) {
    const start = drag.current;
    if (!start) return;
    setCrop(
      clampCrop(
        { ...start.crop, x: start.crop.x + e.clientX - start.x, y: start.crop.y + e.clientY - start.y },
        w,
        h,
        VIEW,
      ),
    );
  }

  /** Setas movem a imagem (10 px com Shift); + e - dão zoom. */
  function onKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    const step = e.shiftKey ? 10 : 2;
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [step, 0],
      ArrowRight: [-step, 0],
      ArrowUp: [0, step],
      ArrowDown: [0, -step],
    };
    if (e.key === "+" || e.key === "=") setCrop((c) => zoomAround(c, c.zoom + 0.1, w, h, VIEW));
    else if (e.key === "-") setCrop((c) => zoomAround(c, c.zoom - 0.1, w, h, VIEW));
    else if (moves[e.key])
      setCrop((c) => clampCrop({ ...c, x: c.x + moves[e.key][0], y: c.y + moves[e.key][1] }, w, h, VIEW));
    else return;
    e.preventDefault();
  }

  function apply() {
    const canvas = document.createElement("canvas");
    canvas.width = ICON_SIZE;
    canvas.height = ICON_SIZE;
    const ctx = canvas.getContext("2d");
    const { sx, sy, side } = sourceRect(crop, w, h, VIEW);
    if (ctx) {
      ctx.imageSmoothingQuality = "high";
      ctx.drawImage(props.image, sx, sy, side, side, 0, 0, ICON_SIZE, ICON_SIZE);
    }
    const icon = ctx ? encodeIcon(canvas) : null;
    if (!icon) return setError("Não deu para preparar essa imagem. Tente outra.");
    props.onApply(icon);
    props.onClose();
  }

  // O mesmo recorte desenhado pequeno: só muda a escala da imagem dentro de cada prévia.
  const preview = (size: number, round: string) => {
    const k = size / VIEW;
    return (
      <span className={`relative block overflow-hidden ${round}`} style={{ width: size, height: size }}>
        <img
          src={props.image.src}
          alt=""
          draggable={false}
          className="pointer-events-none absolute max-w-none"
          style={{ left: crop.x * k, top: crop.y * k, width: w * scale * k, height: h * scale * k }}
        />
      </span>
    );
  };

  return (
    <div className="flex flex-col items-center gap-4">
      <div className="flex items-center gap-6">
        <div
          role="slider"
          tabIndex={0}
          aria-label="Posição do ícone"
          aria-valuetext={`zoom ${crop.zoom.toFixed(1)}×`}
          aria-valuenow={Math.round(crop.zoom * 10)}
          aria-valuemin={10}
          aria-valuemax={MAX_ZOOM * 10}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={() => (drag.current = null)}
          onPointerCancel={() => (drag.current = null)}
          onWheel={(e) => {
            const r = e.currentTarget.getBoundingClientRect();
            // A partir do zoom mais recente: giros rápidos seguidos somam, não contam como um só.
            setCrop((c) =>
              zoomAround(c, c.zoom * (e.deltaY < 0 ? 1.1 : 1 / 1.1), w, h, VIEW, e.clientX - r.left, e.clientY - r.top),
            );
          }}
          onKeyDown={onKeyDown}
          className="relative cursor-grab touch-none overflow-hidden rounded-lg bg-black outline-none select-none focus-visible:ring-2 focus-visible:ring-brand active:cursor-grabbing"
          style={{ width: VIEW, height: VIEW }}
        >
          <img
            src={props.image.src}
            alt=""
            draggable={false}
            className="pointer-events-none absolute max-w-none"
            style={{ left: crop.x, top: crop.y, width: w * scale, height: h * scale }}
          />
          {/* O que fica de fora do ícone escurece; o quadrado arredondado é o recorte. */}
          <span
            aria-hidden
            className="pointer-events-none absolute inset-0 rounded-[28%] shadow-[0_0_0_999px_rgba(0,0,0,0.55)] ring-2 ring-white/80"
          />
        </div>
        <div className="flex flex-col items-center gap-3" aria-label="Prévias">
          {preview(96, "rounded-3xl")}
          <div className="flex items-end gap-3">
            {preview(48, "rounded-2xl")}
            {preview(48, "rounded-full")}
          </div>
          <span className="text-xs text-text-faint">Na barra de salas</span>
        </div>
      </div>

      <label className="flex w-full max-w-xs items-center gap-3 text-text-muted">
        <ZoomOut size={16} aria-hidden />
        <input
          type="range"
          aria-label="Zoom"
          min={1}
          max={MAX_ZOOM}
          step={0.01}
          value={crop.zoom}
          onChange={(e) => setCrop((c) => zoomAround(c, Number(e.target.value), w, h, VIEW))}
          style={{ "--fill": `${((crop.zoom - 1) / (MAX_ZOOM - 1)) * 100}%` } as CSSProperties}
          className="glass-slider flex-1"
        />
        <ZoomIn size={16} aria-hidden />
      </label>

      {error && (
        <p role="alert" className="text-sm text-live">
          {error}
        </p>
      )}

      <div className="flex w-full justify-end gap-2">
        <button
          type="button"
          onClick={() => setCrop(centeredCrop(w, h, VIEW))}
          className="mr-auto rounded-md px-3 py-2 text-sm text-text-muted hover:text-text hover:underline"
        >
          Recomeçar
        </button>
        <ModalClose className="rounded-md px-4 py-2 text-sm hover:underline">Cancelar</ModalClose>
        <button
          type="button"
          onClick={apply}
          className="rounded-md bg-brand px-5 py-2 text-sm font-semibold transition-colors hover:bg-brand-hover"
        >
          Aplicar
        </button>
      </div>
    </div>
  );
}
