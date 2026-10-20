//! Lista do que pode ser transmitido: monitores e janelas de aplicativos.

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindow, IsWindowVisible, MONITORINFOF_PRIMARY, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

/// Identifica uma fonte de captura. Os handles do Windows viram números para poderem ir à interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "handle", rename_all = "camelCase")]
pub enum CaptureSource {
    Monitor(isize),
    Window(isize),
}

/// Uma fonte de captura com o que a interface precisa mostrar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: CaptureSource,
    /// "Monitor 1 (principal)" ou o título da janela.
    pub name: String,
    /// Processo dono da janela (para capturar só o áudio dele). `None` em monitores.
    pub process_id: Option<u32>,
    /// Nome do executável, ex.: "cs2.exe".
    pub process_name: Option<String>,
    /// Caminho completo do executável (para achar a janela nova quando o jogo troca de janela).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_path: Option<String>,
}

/// Dados crus de uma janela, usados para decidir se ela aparece na lista.
#[derive(Debug, Clone)]
pub(crate) struct WindowInfo {
    pub title: String,
    pub visible: bool,
    pub tool_window: bool,
    pub has_owner: bool,
    pub cloaked: bool,
    pub process_id: u32,
}

/// Uma janela aparece na lista se o usuário consegue vê-la e escolhê-la: visível, com título,
/// sem ser janela de ferramenta ou popup com dono, não escondida pelo Windows e não do próprio SquadView.
pub(crate) fn is_capturable(w: &WindowInfo, own_process_id: u32) -> bool {
    w.visible
        && !w.title.trim().is_empty()
        && !w.tool_window
        && !w.has_owner
        && !w.cloaked
        && w.process_id != own_process_id
        && !IGNORED_TITLES.contains(&w.title.as_str())
}

/// Janelas do sistema que passam nos filtros mas não fazem sentido transmitir.
const IGNORED_TITLES: &[&str] = &[
    "Program Manager",
    "Windows Input Experience",
    "Configurações",
    "Settings",
];

/// Monitores primeiro (o principal antes), depois as janelas em ordem de empilhamento (a da frente primeiro).
pub fn list_sources() -> Vec<Source> {
    let mut sources = list_monitors();
    sources.extend(list_windows());
    sources
}

fn list_monitors() -> Vec<Source> {
    let mut monitors: Vec<(HMONITOR, bool)> = Vec::new();
    // SAFETY: o callback só roda durante a chamada e recebe o ponteiro do Vec vivo acima.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(monitor_proc), LPARAM(&mut monitors as *mut _ as isize));
    }
    monitor_sources(
        monitors
            .into_iter()
            .map(|(h, primary)| (h.0 as isize, primary))
            .collect(),
    )
}

/// Monitores (handle, é o principal?) na ordem do Windows → fontes, com o principal primeiro.
pub(crate) fn monitor_sources(mut monitors: Vec<(isize, bool)>) -> Vec<Source> {
    monitors.sort_by_key(|&(_, primary)| !primary);
    monitors
        .into_iter()
        .enumerate()
        .map(|(i, (handle, primary))| Source {
            id: CaptureSource::Monitor(handle),
            name: monitor_name(i, primary),
            process_id: None,
            process_name: None,
            process_path: None,
        })
        .collect()
}

/// "Monitor 1 (principal)", "Monitor 2"… (`index` começa em 0).
fn monitor_name(index: usize, primary: bool) -> String {
    if primary {
        format!("Monitor {} (principal)", index + 1)
    } else {
        format!("Monitor {}", index + 1)
    }
}

unsafe extern "system" fn monitor_proc(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: `data` é o &mut Vec passado por list_monitors; `info` tem o tamanho certo.
    unsafe {
        let primary = GetMonitorInfoW(monitor, &mut info as *mut _ as *mut _).as_bool()
            && info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0;
        (*(data.0 as *mut Vec<(HMONITOR, bool)>)).push((monitor, primary));
    }
    TRUE
}

fn list_windows() -> Vec<Source> {
    let windows = top_level_windows().into_iter().map(|h| (h, window_info(h))).collect();
    // SAFETY: chamada simples sem argumentos.
    let own = unsafe { GetCurrentProcessId() };
    window_sources(windows, own, process_path)
}

/// Todas as janelas de primeiro nível, da frente para trás (ordem de empilhamento).
pub(crate) fn top_level_windows() -> Vec<isize> {
    let mut windows: Vec<isize> = Vec::new();
    // SAFETY: o callback só roda durante a chamada e recebe o ponteiro do Vec vivo acima.
    unsafe {
        let _ = EnumWindows(Some(window_proc), LPARAM(&mut windows as *mut _ as isize));
    }
    windows
}

/// Janelas (handle, dados) em ordem de empilhamento → as fontes que aparecem na lista, com o
/// executável de cada uma (`path_of` descobre o caminho pelo id do processo).
pub(crate) fn window_sources(
    windows: Vec<(isize, WindowInfo)>,
    own_process_id: u32,
    path_of: impl Fn(u32) -> Option<String>,
) -> Vec<Source> {
    windows
        .into_iter()
        .filter(|(_, info)| is_capturable(info, own_process_id))
        .map(|(hwnd, info)| {
            let path = path_of(info.process_id);
            Source {
                id: CaptureSource::Window(hwnd),
                name: info.title,
                process_id: Some(info.process_id),
                process_name: path.as_deref().and_then(exe_name).map(str::to_owned),
                process_path: path,
            }
        })
        .collect()
}

/// Nome do executável no fim do caminho (`C:\Jogos\cs2.exe` → `cs2.exe`).
pub(crate) fn exe_name(path: &str) -> Option<&str> {
    path.rsplit('\\').next()
}

unsafe extern "system" fn window_proc(hwnd: HWND, data: LPARAM) -> BOOL {
    // SAFETY: `data` é o &mut Vec passado por top_level_windows.
    unsafe { (*(data.0 as *mut Vec<isize>)).push(hwnd.0 as isize) };
    TRUE
}

/// Título, visibilidade, estilo e processo de uma janela. Para um handle que já não existe, vem
/// tudo vazio (o Windows só devolve falha).
pub(crate) fn window_info(handle: isize) -> WindowInfo {
    let hwnd = HWND(handle as *mut _);
    // SAFETY: consultas a um HWND que pode já não existir; o buffer tem o tamanho informado.
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        let mut buf = vec![0u16; len.max(0) as usize + 1];
        let read = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..read.max(0) as usize]);

        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4);
        let mut process_id = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut process_id));

        WindowInfo {
            title,
            visible: IsWindowVisible(hwnd).as_bool(),
            tool_window: GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0,
            has_owner: GetWindow(hwnd, GW_OWNER).is_ok_and(|o| !o.is_invalid()),
            cloaked: cloaked != 0,
            process_id,
        }
    }
}

/// A fonte ainda está transmitível? Monitores sempre; janelas, enquanto existirem e estiverem visíveis
/// (jogos que trocam de janela, como o LoL ao sair do lobby, escondem ou fecham a antiga).
pub fn is_alive(source: CaptureSource) -> bool {
    match source {
        CaptureSource::Monitor(_) => true,
        // SAFETY: consultas a um HWND que pode já não existir; o Windows só devolve falso nesse caso.
        CaptureSource::Window(h) => unsafe {
            let hwnd = HWND(h as *mut _);
            IsWindow(Some(hwnd)).as_bool() && IsWindowVisible(hwnd).as_bool()
        },
    }
}

/// Caminho completo do executável do processo, ou `None` se o Windows não deixar consultar.
pub(crate) fn process_path(pid: u32) -> Option<String> {
    // SAFETY: o handle é fechado antes de sair; o buffer tem o tamanho informado.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn janela(title: &str) -> WindowInfo {
        WindowInfo {
            title: title.into(),
            visible: true,
            tool_window: false,
            has_owner: false,
            cloaked: false,
            process_id: 100,
        }
    }

    #[test]
    fn janela_normal_aparece() {
        assert!(is_capturable(&janela("Counter-Strike 2"), 1));
    }

    #[test]
    fn janelas_invisiveis_sem_titulo_ou_de_ferramenta_ficam_de_fora() {
        assert!(!is_capturable(
            &WindowInfo {
                visible: false,
                ..janela("Jogo")
            },
            1
        ));
        assert!(!is_capturable(&janela("   "), 1));
        assert!(!is_capturable(
            &WindowInfo {
                tool_window: true,
                ..janela("Jogo")
            },
            1
        ));
        assert!(!is_capturable(
            &WindowInfo {
                has_owner: true,
                ..janela("Jogo")
            },
            1
        ));
        assert!(!is_capturable(
            &WindowInfo {
                cloaked: true,
                ..janela("Jogo")
            },
            1
        ));
    }

    #[test]
    fn o_proprio_squadview_nao_aparece() {
        assert!(!is_capturable(&janela("SquadView"), 100));
    }

    #[test]
    fn janelas_do_sistema_ficam_de_fora() {
        assert!(!is_capturable(&janela("Program Manager"), 1));
    }

    #[test]
    fn fonte_vira_json_legivel_para_a_interface() {
        let json = serde_json::to_string(&CaptureSource::Window(42)).unwrap();
        assert_eq!(json, r#"{"kind":"window","handle":42}"#);
        let back: CaptureSource = serde_json::from_str(r#"{"kind":"monitor","handle":7}"#).unwrap();
        assert_eq!(back, CaptureSource::Monitor(7));
    }

    #[test]
    fn monitor_esta_sempre_vivo_e_janela_inexistente_nao() {
        assert!(is_alive(CaptureSource::Monitor(0)));
        assert!(!is_alive(CaptureSource::Window(0x7fff_fff0)));
    }

    #[test]
    fn monitor_principal_vem_primeiro_e_com_nome() {
        let sources = monitor_sources(vec![(10, false), (20, true), (30, false)]);
        let ids: Vec<_> = sources.iter().map(|s| s.id).collect();
        assert_eq!(
            ids,
            vec![
                CaptureSource::Monitor(20),
                CaptureSource::Monitor(10),
                CaptureSource::Monitor(30)
            ]
        );
        assert_eq!(sources[0].name, "Monitor 1 (principal)");
        assert_eq!(sources[2].name, "Monitor 3");
        assert!(
            sources
                .iter()
                .all(|s| s.process_id.is_none() && s.process_path.is_none())
        );
        assert!(monitor_sources(Vec::new()).is_empty());
    }

    #[test]
    fn janelas_viram_fontes_com_o_executavel() {
        let mut escondida = janela("Escondida");
        escondida.visible = false;
        let path = |pid: u32| (pid == 100).then(|| r"D:\Steam\cs2\cs2.exe".to_string());
        let sources = window_sources(
            vec![
                (1, janela("Counter-Strike 2")),
                (2, escondida),
                (3, janela("Program Manager")),
            ],
            1,
            path,
        );
        assert_eq!(sources.len(), 1);
        let cs = &sources[0];
        assert_eq!(cs.id, CaptureSource::Window(1));
        assert_eq!(cs.name, "Counter-Strike 2");
        assert_eq!(cs.process_id, Some(100));
        assert_eq!(cs.process_name.as_deref(), Some("cs2.exe"));
        assert_eq!(cs.process_path.as_deref(), Some(r"D:\Steam\cs2\cs2.exe"));

        // Processo sem permissão de consulta (ex.: rodando como administrador): sem caminho.
        let sem = window_sources(vec![(1, janela("Jogo"))], 1, |_| None);
        assert_eq!((sem[0].process_name.clone(), sem[0].process_path.clone()), (None, None));
    }

    #[test]
    fn nome_do_executavel_e_o_fim_do_caminho() {
        assert_eq!(
            exe_name(r"C:\Riot Games\League of Legends\LeagueClientUx.exe"),
            Some("LeagueClientUx.exe")
        );
        assert_eq!(exe_name("solto.exe"), Some("solto.exe"));
    }

    #[test]
    fn caminho_do_proprio_processo() {
        // SAFETY: chamada simples sem argumentos.
        let own = unsafe { GetCurrentProcessId() };
        let path = process_path(own).expect("o próprio processo sempre pode ser consultado");
        assert!(path.to_lowercase().ends_with(".exe"), "{path}");
        assert!(process_path(0).is_none(), "processo ocioso do sistema não tem caminho");
    }

    #[test]
    fn listar_fontes_nunca_falha() {
        // Na esteira pode não haver monitor nem janelas: a lista só precisa ser coerente.
        let sources = list_sources();
        let first_window = sources
            .iter()
            .position(|s| matches!(s.id, CaptureSource::Window(_)))
            .unwrap_or(sources.len());
        assert!(
            sources[first_window..]
                .iter()
                .all(|s| matches!(s.id, CaptureSource::Window(_)) && s.process_id.is_some()),
            "monitores antes das janelas"
        );
    }

    #[test]
    fn janela_viva_visivel_e_escondida() {
        let window = crate::test_window::TestWindow::open("SquadView (fonte)", 320, 240);
        let id = CaptureSource::Window(window.hwnd);
        assert!(is_alive(id));
        window.hide();
        assert!(!is_alive(id), "janela escondida (jogo trocou de janela) não está viva");
        let hwnd = window.hwnd;
        drop(window);
        assert!(!is_alive(CaptureSource::Window(hwnd)), "janela fechada");
    }

    #[test]
    #[ignore = "precisa de uma sessão de desktop"]
    fn lista_pelo_menos_um_monitor() {
        let sources = list_sources();
        assert!(sources.iter().any(|s| matches!(s.id, CaptureSource::Monitor(_))));
        assert!(sources[0].name.contains("principal"));
    }
}
