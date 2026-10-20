//! Dispositivos de áudio do Windows (microfones e saídas), para o usuário escolher qual usar.

use serde::Serialize;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, EDataFlow, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eCommunications,
    eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, STGM_READ};
use windows::core::{HSTRING, PWSTR};

/// Entrada (microfone) ou saída (fone, caixa).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Input,
    Output,
}

impl Flow {
    fn data_flow(self) -> EDataFlow {
        match self {
            Self::Input => eCapture,
            Self::Output => eRender,
        }
    }
}

/// Um dispositivo de áudio ativo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    /// Identificador do Windows (estável entre aberturas do app).
    pub id: String,
    /// Nome que aparece no Windows (ex.: "Microfone (Logitech G933 Gaming Headset)").
    pub name: String,
    /// É o padrão do Windows para chamadas.
    pub is_default: bool,
}

/// Lista os dispositivos ativos de `flow`. Vazio se o Windows não responder.
pub fn list_devices(flow: Flow) -> Vec<AudioDevice> {
    // SAFETY: COM iniciado nesta thread; as strings do Windows são copiadas e liberadas logo.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(devices) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
            return Vec::new();
        };
        let default = devices
            .GetDefaultAudioEndpoint(flow.data_flow(), eCommunications)
            .ok()
            .and_then(|d| device_id(&d));
        let Ok(collection) = devices.EnumAudioEndpoints(flow.data_flow(), DEVICE_STATE_ACTIVE) else {
            return Vec::new();
        };
        let count = collection.GetCount().unwrap_or(0);
        (0..count)
            .filter_map(|i| collection.Item(i).ok())
            .filter_map(|device| {
                let id = device_id(&device)?;
                let name = friendly_name(&device).unwrap_or_else(|| "Dispositivo de áudio".into());
                Some(AudioDevice {
                    is_default: default.as_deref() == Some(id.as_str()),
                    id,
                    name,
                })
            })
            .collect()
    }
}

/// Abre o dispositivo `id` ou, sem id (ou se ele sumiu), o padrão do Windows para chamadas.
///
/// # Safety
/// Chamar numa thread com o COM iniciado.
pub(crate) unsafe fn open_device(flow: Flow, id: Option<&str>) -> windows::core::Result<IMMDevice> {
    // SAFETY: o chamador iniciou o COM.
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        if let Some(id) = id
            && let Ok(device) = devices.GetDevice(&HSTRING::from(id))
        {
            return Ok(device);
        }
        // Microfone: o padrão de comunicação (o mesmo que o Discord usa). Saída: o padrão de chamadas,
        // e se não houver, o padrão geral.
        devices
            .GetDefaultAudioEndpoint(flow.data_flow(), eCommunications)
            .or_else(|_| devices.GetDefaultAudioEndpoint(flow.data_flow(), eConsole))
    }
}

/// # Safety
/// `device` válido, COM iniciado.
unsafe fn device_id(device: &IMMDevice) -> Option<String> {
    // SAFETY: GetId devolve uma string alocada pelo COM, liberada logo depois de copiada.
    unsafe {
        let raw: PWSTR = device.GetId().ok()?;
        let id = raw.to_string().ok();
        windows::Win32::System::Com::CoTaskMemFree(Some(raw.0 as *const _));
        id
    }
}

/// # Safety
/// `device` válido, COM iniciado.
unsafe fn friendly_name(device: &IMMDevice) -> Option<String> {
    // SAFETY: o property store e o PROPVARIANT vivem até o fim desta função.
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let name = value.to_string();
        (!name.is_empty()).then_some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrada_e_microfone_e_saida_e_reproducao() {
        assert_eq!(Flow::Input.data_flow(), eCapture);
        assert_eq!(Flow::Output.data_flow(), eRender);
    }

    #[test]
    fn listar_nunca_falha_mesmo_sem_dispositivos() {
        // Na esteira não há placa de som: a lista vem vazia, sem erro nem pânico.
        for flow in [Flow::Input, Flow::Output] {
            let devices = list_devices(flow);
            assert!(devices.iter().filter(|d| d.is_default).count() <= 1);
            assert!(devices.iter().all(|d| !d.id.is_empty() && !d.name.is_empty()));
        }
    }

    #[test]
    #[ignore = "precisa de microfone"]
    fn id_desconhecido_abre_o_padrao_e_id_conhecido_abre_ele_mesmo() {
        let _ = init_com();
        let mics = list_devices(Flow::Input);
        let first = mics.first().expect("pelo menos um microfone");
        // SAFETY: COM iniciado acima nesta thread.
        unsafe {
            let device = open_device(Flow::Input, Some(&first.id)).unwrap();
            assert_eq!(device_id(&device).as_deref(), Some(first.id.as_str()));
            let fallback = open_device(Flow::Input, Some("{nao-existe}")).unwrap();
            assert!(device_id(&fallback).is_some());
            assert!(friendly_name(&fallback).is_some());
            assert!(open_device(Flow::Output, None).is_ok());
        }
    }

    fn init_com() -> windows::core::HRESULT {
        // SAFETY: inicialização do COM na thread do teste.
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
    }

    #[test]
    #[ignore = "depende dos dispositivos de áudio do PC"]
    fn lista_microfones_e_saidas_com_um_padrao() {
        for flow in [Flow::Input, Flow::Output] {
            let devices = list_devices(flow);
            println!("{flow:?}: {devices:#?}");
            assert!(devices.iter().filter(|d| d.is_default).count() <= 1);
        }
    }
}
