import {
  centeredCrop,
  clampCrop,
  coverScale,
  encodeIcon,
  MAX_ICON_LENGTH,
  MAX_ZOOM,
  sourceRect,
  zoomAround,
} from "./icon-crop";

const VIEW = 256;
// Uma foto deitada: 800×500.
const W = 800;
const H = 500;

describe("recorte do ícone", () => {
  it("a imagem cobre a janela pelo lado menor e começa centralizada", () => {
    expect(coverScale(W, H, VIEW)).toBeCloseTo(256 / 500);
    const c = centeredCrop(W, H, VIEW);
    expect(c.zoom).toBe(1);
    expect(c.y).toBe(0);
    // Sobra o mesmo tanto dos dois lados.
    expect(c.x).toBeCloseTo((256 - 800 * (256 / 500)) / 2);
  });

  it("nunca deixa sobrar fundo: arrastar para fora volta ao limite", () => {
    const scaled = W * coverScale(W, H, VIEW);
    expect(clampCrop({ x: 50, y: 50, zoom: 1 }, W, H, VIEW)).toEqual({ x: 0, y: 0, zoom: 1 });
    expect(clampCrop({ x: -9999, y: -9999, zoom: 1 }, W, H, VIEW)).toEqual({ x: VIEW - scaled, y: 0, zoom: 1 });
    // Zoom fora da faixa também volta.
    expect(clampCrop({ x: 0, y: 0, zoom: 0.2 }, W, H, VIEW).zoom).toBe(1);
    expect(clampCrop({ x: 0, y: 0, zoom: 99 }, W, H, VIEW).zoom).toBe(MAX_ZOOM);
  });

  it("o zoom mantém parado o ponto sob o cursor", () => {
    const start = centeredCrop(W, H, VIEW);
    const scale = coverScale(W, H, VIEW);
    const at = { x: 100, y: 80 };
    // Que ponto da foto está sob o cursor antes e depois do zoom.
    const pointOf = (c: typeof start) => ({ x: (at.x - c.x) / (scale * c.zoom), y: (at.y - c.y) / (scale * c.zoom) });
    const zoomed = zoomAround(start, 2, W, H, VIEW, at.x, at.y);
    expect(zoomed.zoom).toBe(2);
    expect(pointOf(zoomed).x).toBeCloseTo(pointOf(start).x);
    expect(pointOf(zoomed).y).toBeCloseTo(pointOf(start).y);
    // Sem ponto, o zoom é em volta do centro.
    expect(zoomAround(start, 1, W, H, VIEW)).toEqual(start);
  });

  it("o pedaço recortado da foto é quadrado e acompanha o zoom", () => {
    const c = centeredCrop(W, H, VIEW);
    expect(sourceRect(c, W, H, VIEW)).toEqual({
      sx: expect.closeTo(150),
      sy: expect.closeTo(0),
      side: expect.closeTo(500),
    });
    const z = zoomAround(c, 2, W, H, VIEW);
    expect(sourceRect(z, W, H, VIEW).side).toBeCloseTo(250);
  });

  it("comprime até caber: WebP com qualidade caindo, JPEG sem WebP, e desiste se nada couber", () => {
    const canvas = (answer: (type: string, quality: number) => string) =>
      ({ toDataURL: vi.fn(answer) }) as unknown as HTMLCanvasElement;
    const big = "x".repeat(MAX_ICON_LENGTH);

    const webp = canvas((type, q) => (q > 0.6 ? `data:${type};${big}` : `data:${type};base64,ok`));
    expect(encodeIcon(webp)).toBe("data:image/webp;base64,ok");

    // Sem WebP, o navegador devolve PNG: vai para o JPEG.
    const semWebp = canvas((type) => (type === "image/webp" ? "data:image/png;base64,x" : "data:image/jpeg;base64,j"));
    expect(encodeIcon(semWebp)).toBe("data:image/jpeg;base64,j");

    expect(encodeIcon(canvas((type) => `data:${type};${big}`))).toBeNull();
  });
});
