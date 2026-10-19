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
        let adapter_name = adapter_name(&device).unwrap_or_else(|_| "placa desconhecida".into());

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

fn adapter_name(device: &ID3D11Device) -> Result<String> {
    // SAFETY: chamadas COM de leitura em objetos válidos.
    unsafe {
        let desc = device.cast::<IDXGIDevice>()?.GetAdapter()?.GetDesc()?;
        let len = desc
            .Description
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(desc.Description.len());
        Ok(String::from_utf16_lossy(&desc.Description[..len]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "precisa de placa de vídeo"]
    fn cria_device_com_suporte_a_video() {
        let gpu = Gpu::new().expect("device");
        assert!(!gpu.adapter_name().is_empty());
    }
}
