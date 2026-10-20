//! Comandos do chat de voz (ADR 0010): mudo, surdo, dispositivos, volume e silenciar por pessoa.
//! O áudio e a rede da voz ficam no `sv_session::Room`; aqui só a ponte com a interface.

use serde::{Deserialize, Serialize};
use sv_media::{AudioDevice, Flow, list_devices};
use sv_session::{VoiceEvent, VoiceSettings};
use tauri::{AppHandle, Emitter, State};

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
        }
    }
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
pub fn emit_event(app: &AppHandle, event: VoiceEvent) {
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
