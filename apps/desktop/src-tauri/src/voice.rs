//! Comandos do chat de voz (ADR 0010): mudo, surdo, dispositivos, volume e silenciar por pessoa.
//! O áudio e a rede da voz ficam no `sv_session::Room`; aqui só a ponte com a interface.

use serde::{Deserialize, Serialize};
use sv_media::{AudioDevice, Flow, list_devices};
use sv_session::{VoiceEvent, VoiceSettings};
use tauri::{AppHandle, Emitter, Runtime, State};

use crate::room::{RoomState, lock};

/// Quem começou ou parou de falar (`memberId` nulo = nós).
pub const SPEAKING_EVENT: &str = "voice-speaking";
/// Nível do nosso microfone (0 a 1), para o medidor das configurações.
pub const LEVEL_EVENT: &str = "voice-level";
/// Com quem a voz está ligada.
pub const PEERS_EVENT: &str = "voice-peers";
/// Falha de microfone ou de saída.
pub const ERROR_EVENT: &str = "voice-error";
/// Não deu para ligar a voz com alguém.
pub const FAILED_EVENT: &str = "voice-failed";

/// O que a interface guarda e manda (espelha `VoiceSettings`, com mudo e surdo).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoicePrefs {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub input_volume: f32,
    pub output_volume: f32,
    pub noise_suppression: bool,
    /// Controle automático de ganho (ligado se a interface antiga não mandar).
    #[serde(default = "enabled")]
    pub auto_gain: bool,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub deafened: bool,
}

impl VoicePrefs {
    pub fn settings(&self) -> VoiceSettings {
        VoiceSettings {
            input_device: self.input_device.clone(),
            output_device: self.output_device.clone(),
            input_gain: self.input_volume.clamp(0.0, 2.0),
            output_volume: self.output_volume.clamp(0.0, 2.0),
            noise_suppression: self.noise_suppression,
            auto_gain: self.auto_gain,
        }
    }
}

fn enabled() -> bool {
    true
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Speaking {
    member_id: Option<String>,
    speaking: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct VoiceFailed {
    pub member_id: String,
    pub reason: String,
}

/// Repassa um evento do motor de voz para a interface.
pub fn emit_event<R: Runtime>(app: &AppHandle<R>, event: VoiceEvent) {
    let _ = match event {
        VoiceEvent::Speaking { member, speaking } => app.emit(
            SPEAKING_EVENT,
            Speaking {
                member_id: member,
                speaking,
            },
        ),
        VoiceEvent::InputLevel(level) => app.emit(LEVEL_EVENT, level),
        VoiceEvent::MicFailed(e) => app.emit(ERROR_EVENT, format!("Não deu para abrir o microfone: {e}")),
        VoiceEvent::OutputFailed(e) => app.emit(ERROR_EVENT, format!("Não deu para abrir a saída de áudio: {e}")),
    };
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Devices {
    inputs: Vec<AudioDevice>,
    outputs: Vec<AudioDevice>,
}

/// Microfones e saídas do Windows, para escolher nas configurações.
#[tauri::command]
pub async fn voice_devices() -> Result<Devices, String> {
    tauri::async_runtime::spawn_blocking(|| Devices {
        inputs: list_devices(Flow::Input),
        outputs: list_devices(Flow::Output),
    })
    .await
    .map_err(|e| e.to_string())
}

/// Silencia o nosso microfone para todos.
#[tauri::command]
pub fn voice_set_muted(state: State<'_, RoomState>, muted: bool) {
    if let Some(room) = lock(&state).room.as_ref() {
        room.set_muted(muted);
    }
}

/// Ensurdece (não ouve ninguém e fica mudo).
#[tauri::command]
pub fn voice_set_deafened(state: State<'_, RoomState>, deafened: bool) {
    if let Some(room) = lock(&state).room.as_ref() {
        room.set_deafened(deafened);
    }
}

/// Troca dispositivos, volumes e a supressão de ruído (vale na hora).
#[tauri::command]
pub fn voice_set_settings(state: State<'_, RoomState>, prefs: VoicePrefs) {
    if let Some(voice) = lock(&state).room.as_ref().and_then(|r| r.voice()) {
        voice.set_settings(prefs.settings());
    }
}

/// Volume de um amigo só para nós, de 0 a 2.
#[tauri::command]
pub fn voice_set_peer_volume(state: State<'_, RoomState>, member_id: String, volume: f32) {
    if let Some(voice) = lock(&state).room.as_ref().and_then(|r| r.voice()) {
        voice.set_peer_volume(&member_id, volume);
    }
}

/// Silencia um amigo só para nós.
#[tauri::command]
pub fn voice_set_peer_muted(state: State<'_, RoomState>, member_id: String, muted: bool) {
    if let Some(voice) = lock(&state).room.as_ref().and_then(|r| r.voice()) {
        voice.set_peer_muted(&member_id, muted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{test_app, worker};
    use tauri::Manager;

    fn prefs(input: f32, output: f32) -> VoicePrefs {
        VoicePrefs {
            input_device: Some("mic".into()),
            output_device: None,
            input_volume: input,
            output_volume: output,
            noise_suppression: true,
            auto_gain: true,
            muted: false,
            deafened: false,
        }
    }

    #[test]
    fn preferencias_viram_ajustes_do_motor_com_volume_limitado() {
        let s = prefs(3.5, -1.0).settings();
        assert_eq!(s.input_gain, 2.0);
        assert_eq!(s.output_volume, 0.0);
        assert_eq!(s.input_device.as_deref(), Some("mic"));
        assert!(s.noise_suppression);
        // A interface manda em camelCase, e mudo/surdo são opcionais.
        let from_ui: VoicePrefs = serde_json::from_str(
            r#"{"inputDevice":null,"outputDevice":"fone","inputVolume":1,"outputVolume":0.5,"noiseSuppression":false}"#,
        )
        .unwrap();
        assert!(!from_ui.muted && !from_ui.deafened);
        // Interface antiga (sem o campo): o controle automático de ganho vem ligado.
        assert!(from_ui.auto_gain && from_ui.settings().auto_gain);
        assert_eq!(from_ui.settings().output_device.as_deref(), Some("fone"));
    }

    #[test]
    fn eventos_do_motor_de_voz_chegam_a_interface() {
        let t = test_app();
        let speaking = t.events(SPEAKING_EVENT);
        let level = t.events(LEVEL_EVENT);
        let errors = t.events(ERROR_EVENT);
        emit_event(
            t.handle(),
            VoiceEvent::Speaking {
                member: Some("aa".into()),
                speaking: true,
            },
        );
        speaking.wait("falando", |v| v["memberId"] == "aa" && v["speaking"] == true);
        emit_event(t.handle(), VoiceEvent::InputLevel(0.5));
        level.wait("nível", |v| v == 0.5);
        emit_event(t.handle(), VoiceEvent::MicFailed("sem microfone".into()));
        errors.wait("microfone", |v| {
            v.as_str().is_some_and(|s| s.contains("microfone: sem microfone"))
        });
        emit_event(t.handle(), VoiceEvent::OutputFailed("sem saída".into()));
        errors.wait("saída", |v| {
            v.as_str().is_some_and(|s| s.contains("saída de áudio: sem saída"))
        });
    }

    #[test]
    fn comandos_de_voz_fora_da_sala_nao_fazem_nada() {
        let t = test_app();
        let state = || t.app.state::<RoomState>();
        voice_set_muted(state(), true);
        voice_set_deafened(state(), true);
        voice_set_settings(state(), prefs(1.0, 1.0));
        voice_set_peer_volume(state(), "aa".into(), 1.5);
        voice_set_peer_muted(state(), "aa".into(), true);
    }

    #[test]
    fn lista_os_dispositivos_de_audio() {
        // Na esteira pode não haver nenhum; só não pode falhar.
        let devices = tauri::async_runtime::block_on(voice_devices()).unwrap();
        let json = serde_json::to_value(&devices).unwrap();
        assert!(json["inputs"].is_array() && json["outputs"].is_array());
    }

    #[test]
    fn mudo_e_surdo_na_sala_chegam_aos_outros() {
        if worker().is_none() {
            return;
        }
        let ana = test_app();
        let beto = test_app();
        let beto_members = beto.events(crate::room::MEMBERS_EVENT);
        let join = |t: &crate::test_support::TestApp, code: Option<String>, name: &str| {
            tauri::async_runtime::block_on(crate::room::room_join(
                t.handle().clone(),
                code,
                name.into(),
                None,
                None,
                None,
                None,
            ))
            .unwrap()
        };
        let info = join(&ana, None, "Ana");
        join(&beto, Some(info.code), "Beto");
        voice_set_muted(ana.app.state::<RoomState>(), true);
        beto_members.wait("Ana muda", |v| {
            v.as_array()
                .is_some_and(|a| a.iter().any(|m| m["name"] == "Ana" && m["muted"] == true))
        });
        voice_set_deafened(ana.app.state::<RoomState>(), true);
        beto_members.wait("Ana surda", |v| {
            v.as_array()
                .is_some_and(|a| a.iter().any(|m| m["name"] == "Ana" && m["deafened"] == true))
        });
        // Sem motor de voz (entrou sem `voice`), os ajustes por pessoa são ignorados.
        voice_set_settings(ana.app.state::<RoomState>(), prefs(1.0, 1.0));
        voice_set_peer_volume(ana.app.state::<RoomState>(), "x".into(), 0.5);
        voice_set_peer_muted(ana.app.state::<RoomState>(), "x".into(), true);
        tauri::async_runtime::block_on(crate::room::room_leave(ana.handle().clone())).unwrap();
        tauri::async_runtime::block_on(crate::room::room_leave(beto.handle().clone())).unwrap();
    }
}
