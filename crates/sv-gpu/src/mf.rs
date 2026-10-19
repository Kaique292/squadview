//! Ajudantes do Media Foundation compartilhados pelo encoder e pelo decoder.

use std::sync::Once;

use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFDXGIDeviceManager, IMFMediaType, IMFTransform, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_VERSION, MFCreateDXGIDeviceManager, MFCreateMediaType,
    MFMediaType_Video, MFSTARTUP_LITE, MFStartup, MFT_ENUM_FLAG, MFT_FRIENDLY_NAME_Attribute, MFT_REGISTER_TYPE_INFO,
    MFTEnumEx, MFVideoInterlace_Progressive,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoTaskMemFree};
use windows::core::{GUID, Result};

use crate::{Gpu, Size};

/// Inicia o COM (MTA) na thread atual e o Media Foundation no processo.
/// Pode ser chamado em toda thread de mídia; as chamadas repetidas são baratas.
pub fn init_thread() -> Result<()> {
    // SAFETY: inicialização do COM na thread atual; S_FALSE (já iniciado) também é sucesso.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    static STARTUP: Once = Once::new();
    let mut result = Ok(());
    STARTUP.call_once(|| {
        // SAFETY: MFStartup é seguro de chamar uma vez por processo; nunca chamamos MFShutdown.
        result = unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) };
    });
    result
}

/// Gerenciador que entrega o device D3D11 do [`Gpu`] a um MFT.
pub fn device_manager(gpu: &Gpu) -> Result<IMFDXGIDeviceManager> {
    let mut token = 0u32;
    let mut manager = None;
    // SAFETY: ponteiros de saída válidos; o device pertence ao Gpu e vive enquanto o manager existir.
    unsafe {
        MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
        let manager = manager.expect("MFCreateDXGIDeviceManager sem manager");
        manager.ResetDevice(&gpu.device, token)?;
        Ok(manager)
    }
}

/// Um MFT encontrado no sistema, já ativado.
#[derive(Debug)]
pub struct FoundTransform {
    pub transform: IMFTransform,
    /// Ex.: "NVIDIA H.264 Encoder MFT".
    pub name: String,
}

/// Procura e ativa o primeiro MFT da categoria que converte `input` em `output`.
pub fn find_transform(
    category: GUID,
    flags: MFT_ENUM_FLAG,
    input: GUID,
    output: GUID,
) -> Result<Option<FoundTransform>> {
    let input = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: input,
    };
    let output = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: output,
    };
    let mut list: *mut Option<IMFActivate> = std::ptr::null_mut();
    let mut count = 0u32;
    // SAFETY: o Windows aloca `list` com CoTaskMemAlloc; cada item é liberado ao sair do slice
    // (drop do Option<IMFActivate>) e o array é liberado com CoTaskMemFree.
    unsafe {
        MFTEnumEx(category, flags, Some(&input), Some(&output), &mut list, &mut count)?;
        if list.is_null() {
            return Ok(None);
        }
        let activates: Vec<Option<IMFActivate>> = (0..count as usize).map(|i| list.add(i).read()).collect();
        CoTaskMemFree(Some(list as *const _));

        for activate in activates.into_iter().flatten() {
            let name = friendly_name(&activate).unwrap_or_default();
            if let Ok(transform) = activate.ActivateObject::<IMFTransform>() {
                return Ok(Some(FoundTransform { transform, name }));
            }
        }
        Ok(None)
    }
}

fn friendly_name(activate: &IMFActivate) -> Result<String> {
    let mut buf = [0u16; 256];
    let mut len = 0u32;
    // SAFETY: buffer com tamanho informado; `len` recebe o número de caracteres escritos.
    unsafe { activate.GetString(&MFT_FRIENDLY_NAME_Attribute, &mut buf, Some(&mut len))? };
    Ok(String::from_utf16_lossy(&buf[..len as usize]))
}

/// Tipo de mídia de vídeo progressivo com tamanho e taxa de quadros.
pub fn video_type(subtype: GUID, size: Size, fps: u32) -> Result<IMFMediaType> {
    // SAFETY: chamadas de atributo num objeto recém-criado.
    unsafe {
        let t = MFCreateMediaType()?;
        t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        t.SetGUID(&MF_MT_SUBTYPE, &subtype)?;
        t.SetUINT64(&MF_MT_FRAME_SIZE, pack(size.width, size.height))?;
        t.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
        t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        Ok(t)
    }
}

/// Os atributos de tamanho e taxa do MF guardam dois u32 num u64 (alto, baixo).
pub const fn pack(high: u32, low: u32) -> u64 {
    ((high as u64) << 32) | low as u64
}

pub const fn unpack(value: u64) -> (u32, u32) {
    ((value >> 32) as u32, value as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empacota_tamanho_como_o_media_foundation() {
        assert_eq!(pack(1920, 1080), 0x0000_0780_0000_0438);
        assert_eq!(unpack(pack(1920, 1080)), (1920, 1080));
    }
}
