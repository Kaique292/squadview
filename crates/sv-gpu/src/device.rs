use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
    ID3D11Device, ID3D11DeviceContext, ID3D11Multithread, ID3D11VideoContext1, ID3D11VideoDevice,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::core::{Interface, Result};

/// O device D3D11 compartilhado por todo o pipeline de mídia.
///
/// O contexto imediato é protegido para uso por várias threads (exigência do Media Foundation),
/// então clones de [`Gpu`] podem ser usados pela thread de captura e pela do encoder.
#[derive(Debug, Clone)]
pub struct Gpu {
    pub device: ID3D11Device,
    pub context: ID3D11DeviceContext,
    pub video_device: ID3D11VideoDevice,
    pub video_context: ID3D11VideoContext1,
    adapter_name: String,
}

impl Gpu {
    /// Cria o device na placa de vídeo principal.
    pub fn new() -> Result<Self> {
        let levels: [D3D_FEATURE_LEVEL; 2] = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
        let mut device = None;
        let mut context = None;
        // SAFETY: ponteiros de saída válidos e que vivem até o fim da chamada.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )?;
        }
        let device = device.expect("D3D11CreateDevice devolveu sucesso sem device");
        let context = context.expect("D3D11CreateDevice devolveu sucesso sem contexto");

        // SAFETY: chamada COM simples num contexto válido.
        unsafe {
            let _ = context.cast::<ID3D11Multithread>()?.SetMultithreadProtected(true);
        }

        let video_device: ID3D11VideoDevice = device.cast()?;
        let video_context: ID3D11VideoContext1 = context.cast()?;
        let adapter_name = adapter_name(&device).unwrap_or_else(|_| UNKNOWN_ADAPTER.into());

        Ok(Self {
            device,
            context,
            video_device,
            video_context,
            adapter_name,
        })
    }

    /// Nome da placa de vídeo, para as estatísticas e para os relatórios de bug.
    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }
}

const UNKNOWN_ADAPTER: &str = "placa desconhecida";

fn adapter_name(device: &ID3D11Device) -> Result<String> {
    // SAFETY: chamadas COM de leitura em objetos válidos.
    let desc = unsafe { device.cast::<IDXGIDevice>()?.GetAdapter()?.GetDesc()? };
    Ok(utf16_until_nul(&desc.Description))
}

/// Texto de um buffer UTF-16 de tamanho fixo do Windows, até o primeiro zero (ou o buffer todo).
pub(crate) fn utf16_until_nul(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_WARP;

    use super::*;

    #[test]
    #[ignore = "precisa de placa de vídeo"]
    fn cria_device_com_suporte_a_video() {
        let gpu = Gpu::new().expect("device");
        assert!(!gpu.adapter_name().is_empty());
        assert!(!gpu.adapter_name().contains("Basic Render"), "{}", gpu.adapter_name());
    }

    /// O adaptador de software (WARP) existe em todo Windows, inclusive na esteira, mas não
    /// tem a API de vídeo do D3D11 (VideoDevice/VideoProcessor). Por isso a conversão e o
    /// encode/decode só são testados com placa de vídeo. Se um dia o WARP passar a ter, este
    /// teste avisa e os testes de conversão podem rodar na esteira.
    #[test]
    fn warp_nao_oferece_a_api_de_video_do_d3d11() {
        let mut device = None;
        // SAFETY: ponteiro de saída válido e vivo até o fim da chamada.
        let result = unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )
        };
        result.expect("o WARP existe em todo Windows");
        let device: ID3D11Device = device.unwrap();
        assert!(device.cast::<ID3D11VideoDevice>().is_err());
        assert_eq!(adapter_name(&device).unwrap(), "Microsoft Basic Render Driver");
    }

    #[test]
    fn nome_do_adaptador_para_no_primeiro_zero() {
        let mut buf = [0u16; 8];
        for (dst, c) in buf.iter_mut().zip("RTX".encode_utf16()) {
            *dst = c;
        }
        assert_eq!(utf16_until_nul(&buf), "RTX");
        let cheio: Vec<u16> = "ABCD".encode_utf16().collect();
        assert_eq!(utf16_until_nul(&cheio), "ABCD");
        assert_eq!(utf16_until_nul(&[]), "");
    }
}
