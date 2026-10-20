//! Áudio só do jogo: WASAPI em modo "process loopback" (Windows 10 2004+ / 11).
//!
//! Captura o som que um processo (e os processos filhos dele) toca, e nada mais: o áudio da
//! call no Discord, do Spotify ou de outra aba não entra na transmissão.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS,
    AUDIOCLIENT_ACTIVATION_PARAMS_0, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler,
    IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, IAgileObject};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::core::{HRESULT, IUnknown, Interface, Ref, implement};

/// Formato entregue: 48 kHz, estéreo, f32 intercalado (o mesmo do Opus).
pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;

/// Uma captura de áudio em andamento. Para ao ser descartada.
#[derive(Debug)]
pub struct AudioCapture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioCapture {
    /// Começa a capturar o áudio do processo `pid`. `on_samples` recebe blocos de amostras
    /// intercaladas (L, R, L, R…), na thread de captura, a cada ~10 ms.
    pub fn start(pid: u32, on_samples: impl FnMut(&[f32]) + Send + 'static) -> windows::core::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread_stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("sv-audio-capture".into())
            .spawn(move || {
                let mut on_samples = on_samples;
                match open(pid) {
                    Ok(stream) => {
                        let _ = ready_tx.send(Ok(()));
                        stream.run(&thread_stop, &mut on_samples);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })
            .expect("não foi possível criar a thread de áudio");
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                stop,
                thread: Some(thread),
            }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(windows::core::Error::from_thread())
            }
        }
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Stream {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    event: HANDLE,
}

impl Stream {
    fn run(self, stop: &AtomicBool, on_samples: &mut impl FnMut(&[f32])) {
        let mut buffer: Vec<f32> = Vec::with_capacity(4800 * CHANNELS as usize);
        // SAFETY: client e capture pertencem a esta thread; o buffer do WASAPI só é lido entre
        // GetBuffer e ReleaseBuffer, com o tamanho informado por ele.
        unsafe {
            if self.client.Start().is_err() {
                return;
            }
            while !stop.load(Ordering::Relaxed) {
                if WaitForSingleObject(self.event, 100) != WAIT_OBJECT_0 {
                    continue;
                }
                while let Ok(frames) = self.capture.GetNextPacketSize()
                    && frames > 0
                {
                    let mut data = std::ptr::null_mut();
                    let mut count = 0u32;
                    let mut flags = 0u32;
                    if self
                        .capture
                        .GetBuffer(&mut data, &mut count, &mut flags, None, None)
                        .is_err()
                    {
                        break;
                    }
                    let len = count as usize * CHANNELS as usize;
                    let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null();
                    let samples = if silent {
                        None
                    } else {
                        Some(std::slice::from_raw_parts(data as *const f32, len))
                    };
                    copy_packet(&mut buffer, samples, len);
                    let _ = self.capture.ReleaseBuffer(count);
                    on_samples(&buffer);
                }
            }
            let _ = self.client.Stop();
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: o evento foi criado em open() e não é usado depois daqui.
        unsafe {
            let _ = CloseHandle(self.event);
        }
    }
}

fn open(pid: u32) -> windows::core::Result<Stream> {
    // SAFETY: inicialização do COM nesta thread; as estruturas passadas vivem até o fim das chamadas.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let client = activate(pid)?;
        let format = capture_format();
        // O loopback de processo não informa o formato do mixer: pedimos o nosso e o Windows converte.
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
            200_000, // 20 ms de buffer, em unidades de 100 ns
            0,
            &format,
            None,
        )?;
        let capture: IAudioCaptureClient = client.GetService()?;
        let event = CreateEventW(None, false, false, None)?;
        client.SetEventHandle(event)?;
        Ok(Stream { client, capture, event })
    }
}

/// Ativa o "dispositivo virtual" de loopback do processo. A ativação é assíncrona: o Windows
/// chama o handler, numa thread dele, quando termina.
///
/// # Safety
/// Chamar numa thread com o COM iniciado.
unsafe fn activate(pid: u32) -> windows::core::Result<IAudioClient> {
    let params = loopback_params(pid);
    let blob = BlobPropVariant::new(&params);
    let (tx, rx) = mpsc::channel();
    let handler: IActivateAudioInterfaceCompletionHandler = ActivationHandler { done: tx }.into();
    // SAFETY: `blob` aponta para `params`, que vive até a ativação terminar (esperamos no rx abaixo).
    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(blob.as_ptr()),
            &handler,
        )?
    };
    rx.recv_timeout(Duration::from_secs(5))
        .map_err(|_| windows::core::Error::from_thread())?;
    let mut result = HRESULT(0);
    let mut unknown: Option<IUnknown> = None;
    // SAFETY: a operação terminou (o handler avisou); ponteiros de saída válidos.
    unsafe { operation.GetActivateResult(&mut result, &mut unknown)? };
    result.ok()?;
    unknown.ok_or_else(windows::core::Error::from_thread)?.cast()
}

/// Formato pedido ao loopback: 48 kHz, estéreo, f32 (o Windows converte do formato do mixer).
fn capture_format() -> WAVEFORMATEX {
    let block_align = CHANNELS * 4;
    WAVEFORMATEX {
        wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
        nChannels: CHANNELS,
        nSamplesPerSec: SAMPLE_RATE,
        nAvgBytesPerSec: SAMPLE_RATE * u32::from(block_align),
        nBlockAlign: block_align,
        wBitsPerSample: 32,
        cbSize: 0,
    }
}

/// Parâmetros de ativação: o som do processo `pid` e dos filhos dele (jogos que abrem o
/// executável de verdade a partir de um launcher).
fn loopback_params(pid: u32) -> AUDIOCLIENT_ACTIVATION_PARAMS {
    AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                TargetProcessId: pid,
                ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE,
            },
        },
    }
}

/// Copia um pacote do WASAPI para `buffer` (reaproveitado: sem alocar depois que a capacidade
/// assenta). `None` é um pacote marcado como silêncio: vira `len` zeros.
fn copy_packet(buffer: &mut Vec<f32>, data: Option<&[f32]>, len: usize) {
    buffer.clear();
    match data {
        Some(samples) => buffer.extend_from_slice(samples),
        None => buffer.resize(len, 0.0),
    }
}

/// O handler de ativação precisa ser "agile" (o Windows chama de outra thread).
#[implement(IActivateAudioInterfaceCompletionHandler, IAgileObject)]
struct ActivationHandler {
    done: mpsc::Sender<()>,
}

// IAgileObject não tem métodos: só marca que o objeto pode ser chamado de qualquer thread.
impl windows::Win32::System::Com::IAgileObject_Impl for ActivationHandler_Impl {}

impl IActivateAudioInterfaceCompletionHandler_Impl for ActivationHandler_Impl {
    fn ActivateCompleted(&self, _: Ref<IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        let _ = self.done.send(());
        Ok(())
    }
}

/// PROPVARIANT do tipo VT_BLOB apontando para os parâmetros de ativação. Montado à mão porque
/// a união aninhada do windows-rs não expõe o campo `blob` de forma simples.
#[repr(C)]
struct BlobPropVariant {
    vt: u16,
    reserved: [u16; 3],
    size: u32,
    data: *const u8,
}

impl BlobPropVariant {
    const VT_BLOB: u16 = 65;

    fn new(params: &AUDIOCLIENT_ACTIVATION_PARAMS) -> Self {
        Self {
            vt: Self::VT_BLOB,
            reserved: [0; 3],
            size: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
            data: params as *const _ as *const u8,
        }
    }

    fn as_ptr(&self) -> *const PROPVARIANT {
        self as *const Self as *const PROPVARIANT
    }
}

// Mesmo layout do PROPVARIANT com VT_BLOB: 8 bytes de cabeçalho + BLOB { u32, ponteiro }.
const _: () = assert!(std::mem::size_of::<BlobPropVariant>() == std::mem::size_of::<PROPVARIANT>());

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;

    #[test]
    fn formato_pedido_e_48_khz_estereo_float() {
        // WAVEFORMATEX é packed: desmonta em variáveis antes de comparar.
        let WAVEFORMATEX {
            wFormatTag,
            nChannels,
            nSamplesPerSec,
            nAvgBytesPerSec,
            nBlockAlign,
            wBitsPerSample,
            cbSize,
        } = capture_format();
        assert_eq!(
            (
                wFormatTag,
                nChannels,
                nSamplesPerSec,
                nAvgBytesPerSec,
                nBlockAlign,
                wBitsPerSample,
                cbSize
            ),
            (3, 2, 48_000, 384_000, 8, 32, 0)
        );
    }

    #[test]
    fn ativacao_pede_o_processo_e_os_filhos_dele() {
        let params = loopback_params(4242);
        assert_eq!(params.ActivationType, AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK);
        // SAFETY: a união foi montada com ProcessLoopbackParams.
        let loopback = unsafe { params.Anonymous.ProcessLoopbackParams };
        assert_eq!(loopback.TargetProcessId, 4242);
        assert_eq!(
            loopback.ProcessLoopbackMode,
            PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE
        );
    }

    #[test]
    fn propvariant_blob_aponta_para_os_parametros() {
        let params = loopback_params(7);
        let blob = BlobPropVariant::new(&params);
        assert_eq!(blob.vt, BlobPropVariant::VT_BLOB);
        assert_eq!(blob.reserved, [0; 3]);
        assert_eq!(blob.size as usize, std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>());
        assert_eq!(blob.data, &params as *const _ as *const u8);
        assert_eq!(blob.as_ptr() as *const u8, &blob as *const _ as *const u8);
    }

    #[test]
    fn pacote_com_som_e_copiado_e_silencio_vira_zeros() {
        let mut buffer = Vec::with_capacity(8);
        copy_packet(&mut buffer, Some(&[0.1, 0.2]), 2);
        assert_eq!(buffer, [0.1, 0.2]);
        copy_packet(&mut buffer, None, 4);
        assert_eq!(buffer, [0.0; 4]);
    }

    #[test]
    #[ignore = "precisa do serviço de áudio do Windows"]
    fn processo_inexistente_nao_trava() {
        // O Windows aceita ou recusa o pid (depende da versão); o importante é responder logo.
        let start = std::time::Instant::now();
        let capture = AudioCapture::start(0x7fff_fff0, |_| {});
        assert!(start.elapsed() < Duration::from_secs(6));
        if let Ok(capture) = capture {
            assert!(format!("{capture:?}").contains("AudioCapture"));
        }
    }

    #[test]
    #[ignore = "precisa de saída de áudio; toca um bipe pelo PowerShell"]
    fn captura_so_o_audio_do_processo_escolhido() {
        // Um PowerShell que toca um tom de 1 s três vezes (o áudio sai pela placa de som).
        let mut child = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Add-Type -AssemblyName System.Windows.Forms; \
                 $p = New-Object System.Media.SoundPlayer \"$env:WINDIR\\Media\\Windows Notify System Generic.wav\"; \
                 1..6 | % { $p.PlaySync() }",
            ])
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(500));

        let (tx, rx) = mpsc::channel::<f32>();
        let capture = AudioCapture::start(child.id(), move |s| {
            let _ = tx.send(s.iter().fold(0.0f32, |m, x| m.max(x.abs())));
        })
        .expect("ativar o loopback do processo");
        let mut peak = 0.0f32;
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        while std::time::Instant::now() < deadline {
            if let Ok(p) = rx.recv_timeout(Duration::from_millis(200)) {
                peak = peak.max(p);
            }
        }
        drop(capture);
        let _ = child.kill();
        let _ = child.wait();
        assert!(peak > 0.01, "não capturou som do processo (pico {peak})");
    }

    #[test]
    #[ignore = "precisa do serviço de áudio do Windows"]
    fn processo_sem_som_entrega_silencio_sem_erro() {
        let capture = AudioCapture::start(std::process::id(), |_| {});
        assert!(capture.is_ok(), "{capture:?}");
    }
}
