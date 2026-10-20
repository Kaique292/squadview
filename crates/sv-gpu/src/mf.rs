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

/// Microssegundos do pipeline → tempo do MF (unidades de 100 ns).
pub const fn us_to_mf_time(us: u64) -> i64 {
    us as i64 * 10
}

/// Tempo do MF (unidades de 100 ns) → microssegundos; tempos negativos viram 0.
pub const fn mf_time_to_us(time: i64) -> u64 {
    let us = time / 10;
    if us < 0 { 0 } else { us as u64 }
}

/// Duração de um quadro a `fps`, em unidades de 100 ns (fps 0 conta como 1).
pub const fn frame_duration(fps: u32) -> i64 {
    10_000_000 / if fps == 0 { 1 } else { fps as i64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    use windows::Win32::Media::MediaFoundation::{
        MFT_CATEGORY_VIDEO_DECODER, MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
        MFT_ENUM_FLAG_SYNCMFT, MFVideoFormat_H264, MFVideoFormat_NV12,
    };

    #[test]
    fn empacota_tamanho_como_o_media_foundation() {
        assert_eq!(pack(1920, 1080), 0x0000_0780_0000_0438);
        assert_eq!(unpack(pack(1920, 1080)), (1920, 1080));
        assert_eq!(unpack(pack(u32::MAX, 0)), (u32::MAX, 0));
    }

    #[test]
    fn converte_tempos_entre_o_pipeline_e_o_media_foundation() {
        assert_eq!(us_to_mf_time(16_667), 166_670);
        assert_eq!(mf_time_to_us(166_670), 16_667);
        assert_eq!(mf_time_to_us(us_to_mf_time(123_456_789)), 123_456_789);
        assert_eq!(mf_time_to_us(-50), 0, "tempo negativo não vira um número gigante");
    }

    #[test]
    fn duracao_do_quadro_em_100_ns() {
        assert_eq!(frame_duration(60), 166_666);
        assert_eq!(frame_duration(30), 333_333);
        assert_eq!(frame_duration(0), 10_000_000);
    }

    #[test]
    fn inicia_o_media_foundation_mais_de_uma_vez() {
        init_thread().unwrap();
        init_thread().unwrap();
        // Outra thread também pode iniciar (cada thread de mídia chama ao começar).
        std::thread::spawn(|| init_thread().unwrap()).join().unwrap();
    }

    #[test]
    fn tipo_de_video_leva_tamanho_taxa_e_progressivo() {
        init_thread().unwrap();
        let t = video_type(MFVideoFormat_NV12, Size::new(1280, 720), 30).unwrap();
        // SAFETY: leituras de atributos de um tipo de mídia válido.
        unsafe {
            assert_eq!(t.GetGUID(&MF_MT_MAJOR_TYPE).unwrap(), MFMediaType_Video);
            assert_eq!(t.GetGUID(&MF_MT_SUBTYPE).unwrap(), MFVideoFormat_NV12);
            assert_eq!(unpack(t.GetUINT64(&MF_MT_FRAME_SIZE).unwrap()), (1280, 720));
            assert_eq!(unpack(t.GetUINT64(&MF_MT_FRAME_RATE).unwrap()), (30, 1));
            assert_eq!(
                t.GetUINT32(&MF_MT_INTERLACE_MODE).unwrap(),
                MFVideoInterlace_Progressive.0 as u32
            );
        }
    }

    #[test]
    fn categoria_sem_nenhum_transform_devolve_none() {
        init_thread().unwrap();
        let nada = GUID::from_u128(0x5356_0000_0000_0000_0000_0000_0000_0001);
        let found = find_transform(
            nada,
            MFT_ENUM_FLAG_SORTANDFILTER,
            MFVideoFormat_NV12,
            MFVideoFormat_H264,
        )
        .unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn procurar_encoder_de_hardware_nunca_falha() {
        // Na esteira não há placa de vídeo: a busca devolve None em vez de erro.
        init_thread().unwrap();
        let found = find_transform(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            MFVideoFormat_NV12,
            MFVideoFormat_H264,
        );
        assert!(found.is_ok(), "{found:?}");
    }

    #[test]
    #[ignore = "precisa de placa de vídeo com encoder H.264"]
    fn acha_o_encoder_de_hardware_com_nome() {
        init_thread().unwrap();
        let found = find_transform(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            MFVideoFormat_NV12,
            MFVideoFormat_H264,
        )
        .unwrap()
        .expect("encoder de hardware");
        eprintln!("encoder: {}", found.name);
        assert!(!found.name.is_empty());
    }

    #[test]
    #[ignore = "precisa do decoder H.264 do Windows (ausente em algumas edições de servidor)"]
    fn acha_o_decoder_h264_do_windows() {
        init_thread().unwrap();
        let found = find_transform(
            MFT_CATEGORY_VIDEO_DECODER,
            MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER,
            MFVideoFormat_H264,
            MFVideoFormat_NV12,
        )
        .unwrap()
        .expect("decoder H.264");
        assert!(format!("{found:?}").contains("FoundTransform"));
    }

    #[test]
    #[ignore = "precisa de placa de vídeo"]
    fn gerenciador_entrega_o_device_da_placa() {
        let gpu = Gpu::new().unwrap();
        init_thread().unwrap();
        let manager = device_manager(&gpu).unwrap();
        // SAFETY: abre e fecha o handle do device no gerenciador recém-criado.
        unsafe {
            let handle = manager.OpenDeviceHandle().unwrap();
            manager.CloseDeviceHandle(handle).unwrap();
        }
    }
}
