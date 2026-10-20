//! Captura de vídeo com Windows.Graphics.Capture (WGC).
//!
//! Cada frame chega como textura D3D11 no mesmo device do encoder e é entregue ao callback
//! enquanto ainda é válido. O callback deve usá-lo na hora (ex.: converter para NV12 numa
//! textura própria); depois que ele retorna, o Windows reaproveita a textura.

use std::sync::Mutex;

use sv_gpu::{Gpu, GpuFrame, Size, clock};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::{Interface, Result, factory};

use crate::CaptureSource;

const PIXEL_FORMAT: DirectXPixelFormat = DirectXPixelFormat::B8G8R8A8UIntNormalized;
/// Buffers no pool do WGC: 2 basta, porque cada frame é convertido na hora.
const POOL_BUFFERS: i32 = 2;

/// Uma captura em andamento. Para quando é descartada.
#[derive(Debug)]
pub struct VideoCapture {
    session: GraphicsCaptureSession,
    pool: Direct3D11CaptureFramePool,
}

impl VideoCapture {
    /// Começa a capturar `source`. `on_frame` roda numa thread do Windows a cada frame novo.
    pub fn start<F>(gpu: &Gpu, source: CaptureSource, on_frame: F) -> Result<Self>
    where
        F: FnMut(&GpuFrame) + Send + 'static,
    {
        let item = capture_item(source)?;
        let device = winrt_device(gpu)?;
        let initial = item.Size()?;
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&device, PIXEL_FORMAT, POOL_BUFFERS, initial)?;
        let session = pool.CreateCaptureSession(&item)?;

        // Opções que dependem da versão do Windows: se não existirem, segue sem elas.
        let _ = session.SetIsCursorCaptureEnabled(true);
        let _ = session.SetIsBorderRequired(false);
        // Sem isso o Windows entrega no máximo a cada ~2 vsyncs (medido: 37,5 fps num monitor de 75 Hz).
        // O limite de fps fica a cargo do FrameThrottle.
        let _ = session.SetMinUpdateInterval(windows::Foundation::TimeSpan { Duration: 10_000 });

        let state = Mutex::new(FrameState {
            on_frame,
            pool_size: initial,
            device: AgileDevice(device),
        });
        pool.FrameArrived(&TypedEventHandler::new(
            move |pool: windows::core::Ref<Direct3D11CaptureFramePool>, _| {
                let Ok(pool) = pool.ok() else { return Ok(()) };
                let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                state.handle(pool)
            },
        ))?;
        session.StartCapture()?;
        Ok(Self { session, pool })
    }
}

impl Drop for VideoCapture {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

struct FrameState<F> {
    on_frame: F,
    pool_size: SizeInt32,
    device: AgileDevice,
}

/// O IDirect3DDevice criado por CreateDirect3D11DeviceFromDXGIDevice é livre de threads (agile),
/// mas o windows-rs não marca a interface como Send.
struct AgileDevice(IDirect3DDevice);

// SAFETY: o device WinRT de D3D11 é agile e o device D3D11 por trás tem proteção multithread
// ligada (ver sv_gpu::Gpu::new), então pode ser usado a partir da thread do callback do WGC.
unsafe impl Send for AgileDevice {}

impl<F: FnMut(&GpuFrame)> FrameState<F> {
    fn handle(&mut self, pool: &Direct3D11CaptureFramePool) -> Result<()> {
        let frame = pool.TryGetNextFrame()?;
        let size = match frame_action(frame.ContentSize()?, self.pool_size) {
            FrameAction::Recreate(content) => {
                self.pool_size = content;
                pool.Recreate(&self.device.0, PIXEL_FORMAT, POOL_BUFFERS, content)?;
                return frame.Close();
            }
            FrameAction::Skip => return frame.Close(),
            FrameAction::Deliver(size) => size,
        };

        let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
        // SAFETY: a superfície do frame é uma textura D3D11 válida enquanto o frame não é fechado.
        let texture: ID3D11Texture2D = unsafe { access.GetInterface()? };
        // Usa a hora em que o frame chegou, e não o SystemRelativeTime: medido nesta máquina, ele fica
        // 4 a 10 ms no futuro (marca o vsync em que a imagem vai aparecer), o que esconderia latência.
        let timestamp_us = clock::now_us();

        (self.on_frame)(&GpuFrame {
            texture,
            slice: 0,
            size,
            timestamp_us,
        });
        frame.Close()
    }
}

/// O que fazer com um frame que chegou do WGC.
#[derive(Debug, Clone, Copy, PartialEq)]
enum FrameAction {
    /// A janela mudou de tamanho: recria o pool com o tamanho novo e descarta este frame
    /// (a textura dele tem o tamanho antigo).
    Recreate(SizeInt32),
    /// Janela minimizada (tamanho zero): nada para mostrar.
    Skip,
    /// Entrega o frame com este tamanho.
    Deliver(Size),
}

fn frame_action(content: SizeInt32, pool: SizeInt32) -> FrameAction {
    if content.Width != pool.Width || content.Height != pool.Height {
        FrameAction::Recreate(content)
    } else if content.Width <= 0 || content.Height <= 0 {
        FrameAction::Skip
    } else {
        FrameAction::Deliver(Size::new(content.Width as u32, content.Height as u32))
    }
}

fn capture_item(source: CaptureSource) -> Result<GraphicsCaptureItem> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    // SAFETY: os handles vieram de list_sources; se a janela fechou, o Windows devolve erro.
    unsafe {
        match source {
            CaptureSource::Monitor(h) => interop.CreateForMonitor(HMONITOR(h as *mut _)),
            CaptureSource::Window(h) => interop.CreateForWindow(HWND(h as *mut _)),
        }
    }
}

fn winrt_device(gpu: &Gpu) -> Result<IDirect3DDevice> {
    let dxgi: IDXGIDevice = gpu.device.cast()?;
    // SAFETY: device DXGI válido; a função devolve um IInspectable que embrulha o mesmo device.
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? };
    inspectable.cast()
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::list_sources;
    use crate::test_window::TestWindow;

    fn size(w: i32, h: i32) -> SizeInt32 {
        SizeInt32 { Width: w, Height: h }
    }

    #[test]
    fn frame_do_tamanho_do_pool_e_entregue() {
        assert_eq!(
            frame_action(size(1920, 1080), size(1920, 1080)),
            FrameAction::Deliver(Size::new(1920, 1080))
        );
    }

    #[test]
    fn janela_redimensionada_recria_o_pool_e_descarta_o_frame() {
        assert_eq!(
            frame_action(size(1280, 720), size(1920, 1080)),
            FrameAction::Recreate(size(1280, 720))
        );
        // Mudou só a altura.
        assert_eq!(
            frame_action(size(1920, 800), size(1920, 1080)),
            FrameAction::Recreate(size(1920, 800))
        );
    }

    #[test]
    fn janela_minimizada_nao_entrega_nada() {
        assert_eq!(frame_action(size(0, 0), size(0, 0)), FrameAction::Skip);
        assert_eq!(
            frame_action(size(0, 0), size(800, 600)),
            FrameAction::Recreate(size(0, 0))
        );
    }

    #[test]
    #[ignore = "precisa de placa de vídeo e de uma sessão de desktop"]
    fn captura_janela_e_acompanha_o_redimensionamento() {
        let gpu = Gpu::new().unwrap();
        let window = TestWindow::open("SquadView (captura)", 400, 300);
        std::thread::sleep(Duration::from_millis(200));
        let (tx, rx) = mpsc::channel();
        let capture = VideoCapture::start(&gpu, CaptureSource::Window(window.hwnd), move |frame| {
            let _ = tx.send(frame.size);
        })
        .expect("capturar a janela");
        assert!(format!("{capture:?}").contains("VideoCapture"));
        let first = rx.recv_timeout(Duration::from_secs(3)).expect("nenhum frame da janela");
        assert!(first.width > 0 && first.height > 0);

        window.resize(640, 480);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut resized = None;
        while std::time::Instant::now() < deadline {
            if let Ok(s) = rx.recv_timeout(Duration::from_millis(100))
                && s.width > first.width
            {
                resized = Some(s);
                break;
            }
        }
        let resized = resized.expect("nenhum frame com o tamanho novo");
        assert!(resized.height > first.height);
        drop(capture);
    }

    #[test]
    #[ignore = "precisa de placa de vídeo"]
    fn janela_inexistente_nao_pode_ser_capturada() {
        let gpu = Gpu::new().unwrap();
        let result = VideoCapture::start(&gpu, CaptureSource::Window(0x7fff_fff0), |_| {});
        assert!(result.is_err());
    }

    #[test]
    #[ignore = "precisa de placa de vídeo e de uma sessão de desktop"]
    fn captura_frames_do_monitor_principal() {
        let gpu = Gpu::new().unwrap();
        let monitor = list_sources()
            .into_iter()
            .find(|s| matches!(s.id, CaptureSource::Monitor(_)))
            .unwrap();
        let (tx, rx) = mpsc::channel();
        let _capture = VideoCapture::start(&gpu, monitor.id, move |frame| {
            let _ = tx.send((frame.size, frame.timestamp_us));
        })
        .unwrap();
        let (size, ts) = rx.recv_timeout(Duration::from_secs(3)).expect("nenhum frame em 3 s");
        assert!(size.width > 0 && size.height > 0);
        assert!(ts > 0);
    }
}
