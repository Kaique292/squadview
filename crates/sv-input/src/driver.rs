//! O ViGEmBus no PC: se está instalado e onde está o desinstalador (ADR 0023). O app não embute
//! o driver: leva quem hospeda ao instalador oficial e deixa remover quando quiser.

use vigem_rust::{BusError, Client, ClientError};
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY, REG_SAM_FLAGS, RRF_RT_REG_SZ, RegCloseKey,
    RegEnumKeyExW, RegGetValueW, RegOpenKeyExW,
};
use windows::core::{HSTRING, PWSTR};

/// Onde baixar o instalador oficial (o projeto foi arquivado, mas os instaladores continuam lá).
pub const INSTALLER_URL: &str = "https://github.com/nefarius/ViGEmBus/releases/latest";

/// Situação do driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverStatus {
    /// Instalado e respondendo: dá para abrir um fliperama.
    Installed,
    /// Não instalado (ou o serviço não está rodando).
    Missing,
    /// Instalado numa versão que o app não conhece.
    Incompatible(String),
    /// Instalado, mas falhou por outro motivo (ex.: acesso negado).
    Unavailable(String),
}

/// Conecta ao driver e solta na hora, só para saber a situação dele.
pub fn driver_status() -> DriverStatus {
    match Client::connect() {
        Ok(_) => DriverStatus::Installed,
        Err(e) => status_of(&e),
    }
}

fn status_of(error: &ClientError) -> DriverStatus {
    match error {
        ClientError::Bus(BusError::BusNotFound { .. }) => DriverStatus::Missing,
        ClientError::Bus(BusError::VersionMismatch { .. }) => DriverStatus::Incompatible(error.to_string()),
        other => DriverStatus::Unavailable(other.to_string()),
    }
}

/// O desinstalador do ViGEmBus registrado no Windows (o mesmo de "Aplicativos instalados").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uninstaller {
    pub name: String,
    pub version: Option<String>,
    /// Comando registrado (`UninstallString`), como o Windows roda.
    pub command: String,
}

/// Uma entrada de `...\CurrentVersion\Uninstall`, já lida do registro.
#[derive(Debug, Clone, Default)]
struct Entry {
    name: Option<String>,
    version: Option<String>,
    command: Option<String>,
}

const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

/// Procura o desinstalador do ViGEmBus nas duas vistas do registro (64 e 32 bits).
pub fn find_uninstaller() -> Option<Uninstaller> {
    [KEY_WOW64_64KEY, KEY_WOW64_32KEY]
        .into_iter()
        .find_map(|view| pick(read_entries(view)))
}

/// Escolhe a entrada do ViGEmBus. Só o nome conta: o instalador oficial se registra como
/// "ViGEm Bus Driver" (com ou sem o "Nefarius" na frente, conforme a versão).
fn pick(entries: impl IntoIterator<Item = Entry>) -> Option<Uninstaller> {
    entries.into_iter().find_map(|e| {
        let name = e.name?;
        let normalized = name.to_lowercase().replace(' ', "");
        if !normalized.contains("vigembus") {
            return None;
        }
        Some(Uninstaller {
            name,
            version: e.version,
            command: e.command.filter(|c| !c.trim().is_empty())?,
        })
    })
}

fn read_entries(view: REG_SAM_FLAGS) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut root = HKEY::default();
    // SAFETY: caminho e HKEY válidos; `root` é fechado no fim.
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(UNINSTALL_KEY),
            None,
            KEY_READ | view,
            &mut root,
        )
    };
    if opened != ERROR_SUCCESS {
        return entries;
    }
    let mut name = [0u16; 256];
    for index in 0.. {
        let mut len = name.len() as u32;
        // SAFETY: `name` tem `len` posições; o Windows escreve o nome da subchave nele.
        let found = unsafe {
            RegEnumKeyExW(
                root,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if found != ERROR_SUCCESS {
            break;
        }
        let sub_key = HSTRING::from_wide(&name[..len as usize]);
        entries.push(Entry {
            name: string_value(root, &sub_key, "DisplayName"),
            version: string_value(root, &sub_key, "DisplayVersion"),
            command: string_value(root, &sub_key, "UninstallString"),
        });
    }
    // SAFETY: `root` foi aberto acima.
    unsafe {
        let _ = RegCloseKey(root);
    }
    entries
}

/// Um valor de texto (`REG_SZ`) da subchave `sub` de `root`.
fn string_value(root: HKEY, sub: &HSTRING, value: &str) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut bytes = std::mem::size_of_val(&buf) as u32;
    // SAFETY: `buf` tem `bytes` bytes; o Windows escreve o texto terminado em zero.
    let result = unsafe {
        RegGetValueW(
            root,
            sub,
            &HSTRING::from(value),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    };
    if result != ERROR_SUCCESS {
        return None;
    }
    let chars = (bytes as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, command: &str) -> Entry {
        Entry {
            name: Some(name.into()),
            version: Some("1.22.0".into()),
            command: Some(command.into()),
        }
    }

    #[test]
    fn acha_o_vigembus_pelo_nome_entre_os_programas() {
        let found = pick([
            entry("Steam", r#""C:\Steam\uninstall.exe""#),
            entry("Nefarius Virtual Gamepad Emulation Bus Driver", "MsiExec.exe /X{A}"),
            entry("ViGEm Bus Driver", "MsiExec.exe /X{B}"),
        ])
        .expect("achou");
        assert_eq!(found.name, "ViGEm Bus Driver");
        assert_eq!(found.command, "MsiExec.exe /X{B}");
        assert_eq!(found.version.as_deref(), Some("1.22.0"));
        assert!(pick([entry("Nefarius ViGEmBus", "x")]).is_some(), "sem espaço no nome");
    }

    #[test]
    fn sem_vigembus_ou_sem_comando_nao_tem_desinstalador() {
        assert_eq!(pick([entry("Steam", "x"), entry("Discord", "y")]), None);
        assert_eq!(pick([entry("ViGEm Bus Driver", "  ")]), None);
        assert_eq!(pick([Entry::default()]), None);
    }

    #[test]
    fn le_o_registro_sem_falhar() {
        // Em qualquer PC a lista de programas existe; com ou sem o ViGEmBus, a busca termina.
        assert!(!read_entries(KEY_WOW64_64KEY).is_empty());
        let _ = find_uninstaller();
    }

    #[test]
    fn situacao_do_driver_sempre_responde() {
        // Na CI o driver não existe (`Missing`); num PC de teste, `Installed`.
        let status = driver_status();
        assert!(
            matches!(status, DriverStatus::Installed | DriverStatus::Missing),
            "{status:?}"
        );
    }
}
