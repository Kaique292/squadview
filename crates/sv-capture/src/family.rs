//! "Família" do jogo: as janelas que pertencem ao mesmo jogo da fonte escolhida.
//!
//! Jogos com launcher abrem a partida em **outro processo** (o cliente do LoL,
//! `LeagueClientUx.exe`, abre `Game\League of Legends.exe`). Para seguir o jogo, o app precisa
//! saber que as duas janelas são do mesmo jogo sem nenhuma lista de jogos conhecidos: a regra é
//! "os executáveis estão na mesma pasta de instalação". Ver `docs/captura.md`.

use crate::Source;

/// Pastas que guardam **vários** programas. A pasta de instalação de um jogo é a pasta logo
/// abaixo da mais funda delas (`...\steamapps\common\<jogo>\`, `...\Riot Games\<jogo>\`).
const CONTAINERS: &[&str] = &[
    "program files",
    "program files (x86)",
    "common files",
    "windowsapps",
    "xboxgames",
    "riot games",
    "epic games",
    "gog games",
    "ea games",
    "games",
    "jogos",
    "appdata",
    "local",
    "locallow",
    "roaming",
    "programs",
    "desktop",
    "documents",
    "downloads",
    "users",
];

/// Pastas de sistema: nada embaixo delas forma família (ex.: `C:\Windows\explorer.exe`).
const SYSTEM: &[&str] = &["windows", "programdata"];

/// Pasta de instalação do programa (em minúsculas, terminando em `\`), ou `None` quando não dá para
/// dizer com segurança (executável solto numa pasta genérica, pasta do Windows, caminho estranho).
///
/// - `C:\Riot Games\League of Legends\Game\League of Legends.exe` → `c:\riot games\league of legends\`
/// - `D:\SteamLibrary\steamapps\common\Apex Legends\r5apex.exe` → `d:\steamlibrary\steamapps\common\apex legends\`
/// - `D:\LoL\Game\League of Legends.exe` (nada genérico no caminho) → `d:\lol\`
pub fn install_root(exe_path: &str) -> Option<String> {
    let path = exe_path.trim_start_matches(r"\\?\").to_lowercase();
    let segments: Vec<&str> = path.split(['\\', '/']).filter(|s| !s.is_empty()).collect();
    // Unidade + pelo menos uma pasta + o executável.
    if segments.len() < 3 || !segments[0].ends_with(':') {
        return None;
    }
    let dirs = &segments[..segments.len() - 1];
    let is_container = |i: usize| {
        let name = dirs[i];
        let parent = dirs.get(i.wrapping_sub(1)).copied();
        CONTAINERS.contains(&name)
            || (name == "common" && parent == Some("steamapps"))
            // `C:\Users\<nome>\` também guarda de tudo.
            || parent == Some("users")
    };
    // O contêiner mais fundo que tem logo abaixo uma pasta de programa (que não é outro contêiner).
    let root_end = (1..dirs.len() - 1)
        .rev()
        .find(|&i| is_container(i) && !is_container(i + 1))
        .map(|i| i + 1)
        .or_else(|| (!is_container(1) && !SYSTEM.contains(&dirs[1])).then_some(1))?;
    Some(format!("{}\\", dirs[..=root_end].join("\\")))
}

/// As duas fontes são do mesmo jogo? Mesmo processo, mesmo executável ou mesma pasta de
/// instalação. Sem caminho (processo de administrador que não deixa consultar), só o processo.
pub fn same_family(a: &Source, b: &Source) -> bool {
    if a.process_id.is_some() && a.process_id == b.process_id {
        return true;
    }
    let (Some(pa), Some(pb)) = (a.process_path.as_deref(), b.process_path.as_deref()) else {
        return false;
    };
    if pa.eq_ignore_ascii_case(pb) {
        return true;
    }
    matches!((install_root(pa), install_root(pb)), (Some(ra), Some(rb)) if ra == rb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CaptureSource;

    fn root(path: &str) -> Option<String> {
        install_root(path)
    }

    fn src(pid: u32, path: Option<&str>) -> Source {
        Source {
            id: CaptureSource::Window(pid as isize),
            name: "x".into(),
            process_id: Some(pid),
            process_name: None,
            process_path: path.map(str::to_owned),
        }
    }

    #[test]
    fn lol_cliente_e_partida_tem_a_mesma_raiz() {
        let client = root(r"C:\Riot Games\League of Legends\LeagueClientUx.exe");
        let game = root(r"C:\Riot Games\League of Legends\Game\League of Legends.exe");
        assert_eq!(client.as_deref(), Some(r"c:\riot games\league of legends\"));
        assert_eq!(client, game);
    }

    #[test]
    fn lol_em_pasta_propria_sem_nada_generico() {
        assert_eq!(
            root(r"D:\LoL\Game\League of Legends.exe"),
            root(r"D:\LoL\LeagueClientUx.exe")
        );
        assert_eq!(root(r"D:\LoL\LeagueClientUx.exe").as_deref(), Some(r"d:\lol\"));
    }

    #[test]
    fn valorant_e_o_riot_client_sao_jogos_diferentes() {
        let launcher = root(r"C:\Riot Games\VALORANT\live\VALORANT.exe");
        let game = root(r"C:\Riot Games\VALORANT\live\ShooterGame\Binaries\Win64\VALORANT-Win64-Shipping.exe");
        assert_eq!(launcher.as_deref(), Some(r"c:\riot games\valorant\"));
        assert_eq!(launcher, game);
        assert_ne!(root(r"C:\Riot Games\Riot Client\RiotClientServices.exe"), game);
        assert_ne!(root(r"C:\Riot Games\League of Legends\LeagueClientUx.exe"), game);
    }

    #[test]
    fn steam_separa_cada_jogo_e_o_proprio_steam() {
        let cs2 = root(r"D:\SteamLibrary\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe");
        assert_eq!(
            cs2.as_deref(),
            Some(r"d:\steamlibrary\steamapps\common\counter-strike global offensive\")
        );
        let pre = root(r"C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\REDprelauncher.exe");
        let game = root(r"C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\Cyberpunk2077.exe");
        assert_eq!(pre, game);
        assert_ne!(root(r"C:\Program Files (x86)\Steam\steam.exe"), game);
        assert_ne!(cs2, game);
    }

    #[test]
    fn epic_separa_jogos_mesmo_dentro_de_program_files() {
        let fortnite =
            root(r"C:\Program Files\Epic Games\Fortnite\FortniteGame\Binaries\Win64\FortniteClient-Win64-Shipping.exe");
        assert_eq!(fortnite.as_deref(), Some(r"c:\program files\epic games\fortnite\"));
        assert_eq!(
            fortnite,
            root(r"C:\Program Files\Epic Games\Fortnite\FortniteGame\Binaries\Win64\FortniteLauncher.exe")
        );
        assert_ne!(
            root(r"C:\Program Files\Epic Games\RocketLeague\Binaries\Win64\RocketLeague.exe"),
            fortnite
        );
        assert_ne!(
            root(r"C:\Program Files (x86)\Epic Games\Launcher\Portal\Binaries\Win64\EpicGamesLauncher.exe"),
            fortnite
        );
    }

    #[test]
    fn outras_lojas_e_programas_comuns() {
        assert_eq!(
            root(r"C:\Program Files (x86)\Ubisoft\Ubisoft Game Launcher\games\Rainbow Six Siege\RainbowSix.exe")
                .as_deref(),
            Some(r"c:\program files (x86)\ubisoft\ubisoft game launcher\games\rainbow six siege\")
        );
        assert_eq!(
            root(r"C:\XboxGames\Halo Infinite\Content\HaloInfinite.exe").as_deref(),
            Some(r"c:\xboxgames\halo infinite\")
        );
        assert_eq!(
            root(r"C:\Users\kaique\AppData\Local\Discord\app-1.0.9\Discord.exe").as_deref(),
            Some(r"c:\users\kaique\appdata\local\discord\")
        );
        assert_eq!(
            root(r"C:\Users\kaique\meujogo\jogo.exe").as_deref(),
            Some(r"c:\users\kaique\meujogo\")
        );
        assert_eq!(
            root(r"\\?\C:\Program Files\Google\Chrome\Application\chrome.exe").as_deref(),
            Some(r"c:\program files\google\")
        );
    }

    #[test]
    fn pastas_genericas_ou_do_sistema_nao_formam_familia() {
        assert_eq!(root(r"C:\Windows\explorer.exe"), None);
        assert_eq!(root(r"C:\Windows\System32\notepad.exe"), None);
        assert_eq!(root(r"C:\Games\solto.exe"), None);
        assert_eq!(root(r"C:\Program Files\solto.exe"), None);
        assert_eq!(root(r"C:\solto.exe"), None);
        assert_eq!(root("solto.exe"), None);
        assert_eq!(root(r"C:\Users\kaique\jogo.exe"), None);
    }

    #[test]
    fn familia_por_processo_executavel_ou_pasta() {
        let lobby = src(1, Some(r"C:\Riot Games\League of Legends\LeagueClientUx.exe"));
        let game = src(2, Some(r"C:\Riot Games\League of Legends\Game\League of Legends.exe"));
        let chrome = src(3, Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe"));
        assert!(same_family(&lobby, &game));
        assert!(!same_family(&lobby, &chrome));
        // Mesmo processo, mesmo sem caminho (jogo rodando como administrador).
        assert!(same_family(&src(7, None), &src(7, None)));
        assert!(!same_family(&src(7, None), &src(8, None)));
        // Mesmo executável solto numa pasta genérica (sem raiz): ainda é o mesmo programa.
        assert!(same_family(
            &src(4, Some(r"C:\Games\x.exe")),
            &src(5, Some(r"c:\games\X.EXE"))
        ));
        assert!(!same_family(
            &src(4, Some(r"C:\Games\x.exe")),
            &src(5, Some(r"C:\Games\y.exe"))
        ));
    }
}
