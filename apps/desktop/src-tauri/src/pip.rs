//! A janelinha do picture in picture (ADR 0021): uma janela do app, pequena, sem borda e sempre no
//! topo, com o vídeo por baixo da página dela (o mesmo visual do DirectComposition, levado para a
//! árvore desta janela) e, por cima, os controles em HTML: silenciar, volume e voltar.

use sv_session::Bounds;
use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use crate::room::PipInfo;

/// O rótulo da janelinha (uma por vez).
pub const LABEL: &str = "pip";
/// Largura com que ela abre (pixels da interface) e a menor.
const WIDTH: f64 = 480.0;
const MIN: (f64, f64) = (240.0, 135.0);
/// Distância do canto da tela.
const MARGIN: f64 = 24.0;

/// Texto seguro para ir na URL da página (letras, números e `-_.~` passam; o resto vira `%XX`).
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// O endereço da página da janelinha, com o que ela mostra.
pub fn page(member_id: &str, info: &PipInfo) -> String {
    format!(
        "index.html#pip={}&nome={}&som={}&volume={}",
        encode(member_id),
        encode(&info.name),
        u8::from(info.listening),
        info.volume.min(200)
    )
}

/// A altura da janelinha na proporção do vídeo (16:9 sem tamanho), no mínimo a menor.
pub fn height_for(width: f64, video: (u32, u32)) -> f64 {
    let (w, h) = video;
    let aspect = if w == 0 || h == 0 {
        16.0 / 9.0
    } else {
        f64::from(w) / f64::from(h)
    };
    (width / aspect).round().max(MIN.1)
}

/// Abre a janelinha da live de `member_id`, no canto de baixo à direita da tela do app.
pub fn open<R: Runtime>(app: &AppHandle<R>, member_id: &str, info: &PipInfo) -> Result<WebviewWindow<R>, String> {
    if let Some(old) = app.get_webview_window(LABEL) {
        let _ = old.destroy();
    }
    let height = height_for(WIDTH, (info.width, info.height));
    let mut builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(page(member_id, info).into()))
        .title("SquadView")
        .inner_size(WIDTH, height)
        .min_inner_size(MIN.0, MIN.1)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .shadow(true);
    // No canto da tela onde o app está.
    if let Some(monitor) = app
        .get_webview_window("main")
        .and_then(|w| w.current_monitor().ok().flatten())
    {
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        let x = f64::from(area.position.x) / scale + f64::from(area.size.width) / scale - WIDTH - MARGIN;
        let y = f64::from(area.position.y) / scale + f64::from(area.size.height) / scale - height - MARGIN;
        builder = builder.position(x, y);
    }
    let window = builder.build().map_err(|e| e.to_string())?;
    let (events_app, member) = (app.clone(), member_id.to_owned());
    let events_window = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Resized(size) => {
            if let Ok(hwnd) = events_window.hwnd() {
                let area = Bounds {
                    x: 0,
                    y: 0,
                    width: size.width,
                    height: size.height,
                };
                crate::room::pip_resized(&events_app, &member, hwnd.0 as isize, area);
            }
        }
        WindowEvent::Destroyed => crate::room::pip_closed(&events_app, &member),
        _ => {}
    });
    Ok(window)
}

/// Onde o vídeo fica na janelinha: a área inteira dela (pixels físicos).
pub fn placement<R: Runtime>(window: &WebviewWindow<R>) -> Result<(isize, Bounds), String> {
    let hwnd = window.hwnd().map_err(|e| e.to_string())?;
    let size = window.inner_size().map_err(|e| e.to_string())?;
    Ok((
        hwnd.0 as isize,
        Bounds {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endereco_da_pagina_leva_o_que_ela_mostra_sem_quebrar_a_url() {
        let info = PipInfo {
            name: "Ana Paula & cia".into(),
            listening: true,
            volume: 250,
            ..PipInfo::default()
        };
        assert_eq!(
            page("bbbbbbbbbbbbbbbb", &info),
            "index.html#pip=bbbbbbbbbbbbbbbb&nome=Ana%20Paula%20%26%20cia&som=1&volume=200"
        );
        assert_eq!(encode("Zé"), "Z%C3%A9");
    }

    #[test]
    fn a_janelinha_abre_na_proporcao_do_video() {
        assert_eq!(height_for(480.0, (1920, 1080)), 270.0);
        assert_eq!(height_for(480.0, (0, 0)), 270.0, "sem tamanho: 16:9");
        assert_eq!(height_for(480.0, (1000, 1000)), 480.0);
        assert_eq!(height_for(480.0, (4000, 100)), MIN.1, "no mínimo a menor altura");
    }
}
