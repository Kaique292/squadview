//! Cálculos de tamanho e enquadramento (lógica pura, sem GPU).

/// Largura e altura em pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// Retângulo em pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

/// Tamanho de saída do encoder: a fonte reduzida para caber em `max`, mantendo a proporção,
/// sem nunca aumentar, e com largura e altura pares (exigência do NV12).
pub fn scaled_to_fit(source: Size, max: Size) -> Size {
    if source.width == 0 || source.height == 0 {
        return Size::new(0, 0);
    }
    let scale = f64::min(
        1.0,
        f64::min(
            max.width as f64 / source.width as f64,
            max.height as f64 / source.height as f64,
        ),
    );
    let even = |v: f64| ((v.round() as u32) & !1).max(2);
    Size::new(even(source.width as f64 * scale), even(source.height as f64 * scale))
}

/// Onde desenhar uma imagem `content` dentro de `area`, centralizada e sem distorcer
/// (barras pretas nas laterais ou em cima e embaixo).
pub fn fit_inside(content: Size, area: Size) -> Rect {
    if content.width == 0 || content.height == 0 || area.width == 0 || area.height == 0 {
        return Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
    }
    let scale = f64::min(
        area.width as f64 / content.width as f64,
        area.height as f64 / content.height as f64,
    );
    let w = ((content.width as f64 * scale).round() as u32).min(area.width);
    let h = ((content.height as f64 * scale).round() as u32).min(area.height);
    let left = (area.width - w) / 2;
    let top = (area.height - h) / 2;
    Rect {
        left,
        top,
        right: left + w,
        bottom: top + h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_4k_vira_1080p() {
        assert_eq!(
            scaled_to_fit(Size::new(3840, 2160), Size::new(1920, 1080)),
            Size::new(1920, 1080)
        );
    }

    #[test]
    fn nunca_aumenta_a_fonte() {
        assert_eq!(
            scaled_to_fit(Size::new(1280, 720), Size::new(1920, 1080)),
            Size::new(1280, 720)
        );
    }

    #[test]
    fn ultrawide_mantem_a_proporcao_e_fica_par() {
        // 3440x1440 cabendo em 1920x1080: limitado pela largura.
        let out = scaled_to_fit(Size::new(3440, 1440), Size::new(1920, 1080));
        assert_eq!(out.width, 1920);
        assert_eq!(out.height % 2, 0);
        assert!((out.height as i32 - 804).abs() <= 1);
    }

    #[test]
    fn janela_de_tamanho_impar_vira_par() {
        let out = scaled_to_fit(Size::new(1001, 701), Size::new(1920, 1080));
        assert_eq!((out.width % 2, out.height % 2), (0, 0));
    }

    #[test]
    fn fonte_vazia_nao_quebra() {
        assert_eq!(scaled_to_fit(Size::new(0, 720), Size::new(1920, 1080)), Size::new(0, 0));
        assert_eq!(
            fit_inside(Size::new(0, 0), Size::new(800, 600)),
            Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0
            }
        );
    }

    #[test]
    fn imagem_larga_ganha_barras_em_cima_e_embaixo() {
        let r = fit_inside(Size::new(1920, 1080), Size::new(1000, 1000));
        assert_eq!((r.left, r.right), (0, 1000));
        assert_eq!(r.bottom - r.top, 563);
        assert_eq!(r.top, (1000 - 563) / 2);
    }

    #[test]
    fn imagem_alta_ganha_barras_nas_laterais() {
        let r = fit_inside(Size::new(1080, 1920), Size::new(1920, 1080));
        assert_eq!((r.top, r.bottom), (0, 1080));
        assert_eq!(r.left, (1920 - r.right + r.left) / 2);
    }
}
