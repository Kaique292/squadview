//! Experimento da luz ambiente do redesign (ADR 0034): uma "live" de teste num swap chain de
//! composição e, atrás dela, o MESMO swap chain num segundo visual, ampliado e desfocado pelo
//! DirectComposition. Serve para ver o efeito e medir quanto o compositor do Windows gasta.
//!
//! `cargo run -p sv-media --release --example luz_ambiente -- <com|mip|sem> <segundos> [redução] [sem-desfoque]`
//!
//! O modo `app` mede o caminho de verdade do app: a live por baixo da interface com o `Renderer` do
//! `sv-media` e a luz ligada pela mesma API da sala (`app luz` liga, `app sem` não):
//! `cargo run -p sv-media --release --example luz_ambiente -- app <luz|sem> <segundos>`

use std::time::{Duration, Instant};

use sv_gpu::Gpu;
use windows::Win32::Foundation::RECT;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_RESOURCE_MISC_GENERATE_MIPS, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, ID3D11DeviceContext1, ID3D11RenderTargetView, ID3D11ShaderResourceView, ID3D11Texture2D,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice3, IDCompositionDesktopDevice, IDCompositionDevice3, IDCompositionVisual2,
    IDCompositionVisual3,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_EFFECT_FLIP_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory2,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, RegisterClassW, SW_SHOW,
    ShowWindow, TranslateMessage, WNDCLASSW, WS_EX_NOREDIRECTIONBITMAP, WS_OVERLAPPEDWINDOW,
};
use windows::core::{IUnknown, Interface, w};

const WIDTH: i32 = 1280;
const HEIGHT: i32 = 760;
/// A live de teste: 960 × 540 no meio da janela.
const LIVE: (f32, f32, u32, u32) = (160.0, 110.0, 960, 540);
/// No modo "mip", a luz usa a redução 2^MIP_LEVEL da live (1/32: 30 × 16).
const MIP_LEVEL: u32 = 5;

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: repassa para o padrão do Windows.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// O caminho do app: `VideoWindow` composta numa janela, `Renderer` a 60 fps e a luz da sala.
fn app(ambient: bool, seconds: u64) -> windows::core::Result<()> {
    use sv_gpu::{GpuFrame, Size};
    use sv_media::{Bounds, VideoWindow, WindowTarget};
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;

    let gpu = Gpu::new()?;
    let host = VideoWindow::open(
        WindowTarget::Standalone {
            title: "SquadView - luz ambiente (app)".into(),
        },
        Size::new(1280, 720),
    )?;
    let video = VideoWindow::open(
        WindowTarget::Composed {
            parent: host.control().hwnd(),
        },
        Size::new(1920, 1080),
    )?;
    let control = video.control();
    control.set_bounds(Bounds {
        x: 160,
        y: 110,
        width: 960,
        height: 540,
    });
    if ambient {
        control.set_ambient(true);
        sv_media::set_ambient_area(Some(Bounds {
            x: 0,
            y: 0,
            width: 1260,
            height: 700,
        }));
    }
    let frame = GpuFrame {
        texture: sv_gpu::testing::texture(&gpu, Size::new(1920, 1080), DXGI_FORMAT_NV12, None)?,
        slice: 0,
        size: Size::new(1920, 1080),
        timestamp_us: 0,
    };
    let mut renderer = video.renderer(&gpu)?;
    let started = Instant::now();
    let mut frames = 0u64;
    while started.elapsed() < Duration::from_secs(seconds) {
        renderer.present(&frame)?;
        frames += 1;
        std::thread::sleep(Duration::from_millis(16));
    }
    println!(
        "app, luz {}: {frames} quadros em {:.1} s",
        if ambient { "LIGADA" } else { "desligada" },
        started.elapsed().as_secs_f32()
    );
    sv_media::set_ambient_area(None);
    Ok(())
}

fn main() -> windows::core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "app") {
        let seconds = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(15);
        return app(args.get(2).is_some_and(|a| a == "luz"), seconds);
    }
    let ambient = args.get(1).is_some_and(|a| a == "com");
    // "mip": a luz vem de uma imagem minúscula da live, gerada na GPU (sem efeito do compositor).
    let mip = args.get(1).is_some_and(|a| a == "mip");
    // Os argumentos 1 e 2 já foram lidos; o 3 (a redução) é lido na montagem da luz.
    let seconds: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(15);
    let gpu = Gpu::new()?;

    // SAFETY: janela, swap chain e árvore de composição, todos deste processo e desta thread.
    unsafe {
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("SquadViewLuzAmbiente"),
            ..Default::default()
        };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP,
            w!("SquadViewLuzAmbiente"),
            w!("SquadView - luz ambiente (experimento)"),
            WS_OVERLAPPEDWINDOW,
            80,
            80,
            WIDTH,
            HEIGHT,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let _ = ShowWindow(hwnd, SW_SHOW);

        // A "live": um swap chain de composição, como o do app.
        let factory: IDXGIFactory2 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0))?;
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: LIVE.2,
            Height: LIVE.3,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            ..Default::default()
        };
        let swapchain = factory.CreateSwapChainForComposition(&gpu.device, &desc, None)?;

        // O dispositivo 3 (o do app é o 1): só ele tem o desfoque.
        let desktop: IDCompositionDesktopDevice = DCompositionCreateDevice3(None::<&IUnknown>)?;
        let device3: IDCompositionDevice3 = desktop.cast()?;
        let target = desktop.CreateTargetForHwnd(hwnd, true)?;
        let root: IDCompositionVisual2 = desktop.CreateVisual()?;
        target.SetRoot(&root)?;

        if ambient {
            // A luz: o MESMO swap chain num visual reduzido (1/REDUCE) e desfocado, dentro de outro
            // que amplia de volta, 1,6× o tamanho da live. Desfocar pequeno custa REDUCE² menos.
            let reduce: f32 = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(8.0);
            let inner: IDCompositionVisual2 = desktop.CreateVisual()?;
            inner.SetContent(&swapchain)?;
            let down = desktop.CreateScaleTransform()?;
            down.SetScaleX2(1.0 / reduce)?;
            down.SetScaleY2(1.0 / reduce)?;
            inner.SetTransform(&down)?;
            // Argumento 4: "sem-desfoque" mede só a cópia ampliada, sem o efeito.
            if std::env::args().nth(4).is_none_or(|a| a != "sem-desfoque") {
                let blur = device3.CreateGaussianBlurEffect()?;
                blur.SetStandardDeviation2(48.0 / reduce)?;
                inner.SetEffect(&blur)?;
            }
            let glow: IDCompositionVisual2 = desktop.CreateVisual()?;
            glow.AddVisual(&inner, false, None)?;
            let up = desktop.CreateScaleTransform()?;
            up.SetScaleX2(reduce * 1.6)?;
            up.SetScaleY2(reduce * 1.6)?;
            glow.SetTransform(&up)?;
            // Centraliza a luz ampliada na live.
            glow.SetOffsetX2(LIVE.0 - LIVE.2 as f32 * 0.3)?;
            glow.SetOffsetY2(LIVE.1 - LIVE.3 as f32 * 0.3)?;
            let glow3: IDCompositionVisual3 = glow.cast()?;
            glow3.SetOpacity2(0.65)?;
            root.AddVisual(&glow, false, None)?;
        }

        // Modo "mip": um swap chain minúsculo (a live reduzida 32×) que o compositor só amplia.
        let tiny = if mip {
            let (tw, th) = (LIVE.2 >> MIP_LEVEL, LIVE.3 >> MIP_LEVEL);
            let tiny = factory.CreateSwapChainForComposition(
                &gpu.device,
                &DXGI_SWAP_CHAIN_DESC1 {
                    Width: tw,
                    Height: th,
                    ..desc
                },
                None,
            )?;
            let mips = D3D11_TEXTURE2D_DESC {
                Width: LIVE.2,
                Height: LIVE.3,
                MipLevels: MIP_LEVEL + 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
                CPUAccessFlags: 0,
                MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
            };
            let mut texture: Option<ID3D11Texture2D> = None;
            gpu.device.CreateTexture2D(&mips, None, Some(&mut texture))?;
            let texture = texture.expect("textura das reduções");
            let mut srv: Option<ID3D11ShaderResourceView> = None;
            gpu.device.CreateShaderResourceView(&texture, None, Some(&mut srv))?;
            let glow: IDCompositionVisual2 = desktop.CreateVisual()?;
            glow.SetContent(&tiny)?;
            let up = desktop.CreateScaleTransform()?;
            up.SetScaleX2(LIVE.2 as f32 * 1.6 / tw as f32)?;
            up.SetScaleY2(LIVE.3 as f32 * 1.6 / th as f32)?;
            glow.SetTransform(&up)?;
            glow.SetOffsetX2(LIVE.0 - LIVE.2 as f32 * 0.3)?;
            glow.SetOffsetY2(LIVE.1 - LIVE.3 as f32 * 0.3)?;
            let glow3: IDCompositionVisual3 = glow.cast()?;
            glow3.SetOpacity2(0.65)?;
            root.AddVisual(&glow, false, None)?;
            Some((tiny, texture, srv.expect("srv das reduções")))
        } else {
            None
        };

        let live: IDCompositionVisual2 = desktop.CreateVisual()?;
        live.SetContent(&swapchain)?;
        live.SetOffsetX2(LIVE.0)?;
        live.SetOffsetY2(LIVE.1)?;
        let clip = desktop.CreateRectangleClip()?;
        clip.SetLeft2(0.0)?;
        clip.SetTop2(0.0)?;
        clip.SetRight2(LIVE.2 as f32)?;
        clip.SetBottom2(LIVE.3 as f32)?;
        for set in [
            windows::Win32::Graphics::DirectComposition::IDCompositionRectangleClip::SetTopLeftRadiusX2,
            windows::Win32::Graphics::DirectComposition::IDCompositionRectangleClip::SetTopLeftRadiusY2,
            windows::Win32::Graphics::DirectComposition::IDCompositionRectangleClip::SetTopRightRadiusX2,
            windows::Win32::Graphics::DirectComposition::IDCompositionRectangleClip::SetTopRightRadiusY2,
            windows::Win32::Graphics::DirectComposition::IDCompositionRectangleClip::SetBottomLeftRadiusX2,
            windows::Win32::Graphics::DirectComposition::IDCompositionRectangleClip::SetBottomLeftRadiusY2,
            windows::Win32::Graphics::DirectComposition::IDCompositionRectangleClip::SetBottomRightRadiusX2,
            windows::Win32::Graphics::DirectComposition::IDCompositionRectangleClip::SetBottomRightRadiusY2,
        ] {
            set(&clip, 18.0)?;
        }
        live.SetClip(&clip)?;
        root.AddVisual(&live, true, None)?;
        desktop.Commit()?;

        let context1: ID3D11DeviceContext1 = gpu.context.cast()?;
        let started = Instant::now();
        let mut frames = 0u64;
        let mut msg = MSG::default();
        while started.elapsed() < Duration::from_secs(seconds) {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            // Uma cena que se mexe: fundo escuro e três blocos de cor que andam e mudam de tom.
            let back: ID3D11Texture2D = swapchain.GetBuffer(0)?;
            let mut rtv: Option<ID3D11RenderTargetView> = None;
            gpu.device.CreateRenderTargetView(&back, None, Some(&mut rtv))?;
            let rtv = rtv.expect("render target");
            let t = started.elapsed().as_secs_f32();
            context1.ClearView(&rtv, &[0.05, 0.04, 0.12, 1.0], None);
            let blocks = [
                ([1.0, 0.48, 0.24, 1.0], 0.0f32),
                ([0.43, 0.31, 0.96, 1.0], 2.1),
                ([0.16, 0.82, 1.0, 1.0], 4.2),
            ];
            for (color, phase) in blocks {
                let x = ((t * 0.6 + phase).sin() * 0.5 + 0.5) * (LIVE.2 as f32 - 320.0);
                let y = ((t * 0.45 + phase * 1.3).cos() * 0.5 + 0.5) * (LIVE.3 as f32 - 220.0);
                let rect = RECT {
                    left: x as i32,
                    top: y as i32,
                    right: x as i32 + 320,
                    bottom: y as i32 + 220,
                };
                context1.ClearView(&rtv, &color, Some(&[rect]));
            }
            if let Some((tiny, texture, srv)) = &tiny {
                // Tudo na GPU: o quadro vai para a textura, as reduções são geradas, e a de 1/32
                // vai para o swap chain minúsculo. Nada volta para a CPU.
                gpu.context.CopySubresourceRegion(texture, 0, 0, 0, 0, &back, 0, None);
                gpu.context.GenerateMips(srv);
                let tiny_back: ID3D11Texture2D = tiny.GetBuffer(0)?;
                gpu.context
                    .CopySubresourceRegion(&tiny_back, 0, 0, 0, 0, texture, MIP_LEVEL, None);
                tiny.Present(0, DXGI_PRESENT(0)).ok()?;
            }
            swapchain.Present(1, DXGI_PRESENT(0)).ok()?;
            frames += 1;
        }
        let elapsed = started.elapsed().as_secs_f32();
        println!(
            "luz ambiente {}: {frames} quadros em {elapsed:.1} s ({:.1} fps)",
            if mip {
                "LIGADA (mip)"
            } else if ambient {
                "LIGADA (desfoque)"
            } else {
                "desligada"
            },
            frames as f32 / elapsed
        );
        drop(target);
    }
    Ok(())
}
