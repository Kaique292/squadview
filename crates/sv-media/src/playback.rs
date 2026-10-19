//! Toca áudio no dispositivo de saída padrão do Windows (WASAPI compartilhado).
//!
//! Quem decodifica empurra amostras com [`AudioPlayer::push`]; a thread de áudio puxa conforme a
//! placa pede. Se faltar, toca silêncio; se sobrar mais que [`MAX_BUFFERED_MS`], corta o excesso
//! para a latência não crescer.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    WAVEFORMATEX, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

use crate::opus::{CHANNELS, SAMPLE_RATE};

/// Maior atraso aceito na fila de reprodução.
pub const MAX_BUFFERED_MS: usize = 120;
const MAX_BUFFERED: usize = SAMPLE_RATE as usize * CHANNELS * MAX_BUFFERED_MS / 1000;

#[derive(Debug, Default)]
struct Queue {
    samples: VecDeque<f32>,
    volume: f32,
}

/// Saída de áudio em andamento. Para ao ser descartada.
#[derive(Debug)]
pub struct AudioPlayer {
    queue: Arc<Mutex<Queue>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioPlayer {
    pub fn start() -> windows::core::Result<Self> {
        let queue = Arc::new(Mutex::new(Queue {
            samples: VecDeque::new(),
            volume: 1.0,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (q, s) = (queue.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("sv-audio-playback".into())
            .spawn(move || match open() {
                Ok(out) => {
                    let _ = ready_tx.send(Ok(()));
                    out.run(&q, &s);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .expect("não foi possível criar a thread de reprodução");
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                queue,
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

    /// Enfileira amostras intercaladas a 48 kHz estéreo.
    pub fn push(&self, samples: &[f32]) {
        let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        push_bounded(&mut q.samples, samples, MAX_BUFFERED);
    }

    /// Volume de 0 (mudo) a 1.
    pub fn set_volume(&self, volume: f32) {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).volume = volume.clamp(0.0, 1.0);
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Acrescenta amostras, descartando as mais antigas acima de `max` (mantendo pares L/R alinhados).
pub(crate) fn push_bounded(queue: &mut VecDeque<f32>, samples: &[f32], max: usize) {
    queue.extend(samples.iter().copied());
    if queue.len() > max {
        let excess = (queue.len() - max).next_multiple_of(CHANNELS);
        queue.drain(..excess.min(queue.len()));
    }
}

/// Preenche `out` com o que tem na fila (aplicando o volume) e completa com silêncio.
fn fill(queue: &mut Queue, out: &mut [f32]) {
    let n = out.len().min(queue.samples.len());
    for (dst, src) in out.iter_mut().zip(queue.samples.drain(..n)) {
        *dst = src * queue.volume;
    }
    out[n..].fill(0.0);
}

struct Output {
    client: IAudioClient,
    render: IAudioRenderClient,
    event: windows::Win32::Foundation::HANDLE,
    buffer_frames: u32,
}

impl Output {
    fn run(self, queue: &Mutex<Queue>, stop: &AtomicBool) {
        // SAFETY: client e render pertencem a esta thread; o buffer do WASAPI só é escrito entre
        // GetBuffer e ReleaseBuffer, no tamanho pedido.
        unsafe {
            if self.client.Start().is_err() {
                return;
            }
            while !stop.load(Ordering::Relaxed) {
                if WaitForSingleObject(self.event, 100) != WAIT_OBJECT_0 {
                    continue;
                }
                let Ok(padding) = self.client.GetCurrentPadding() else {
                    break;
                };
                let frames = self.buffer_frames.saturating_sub(padding);
                if frames == 0 {
                    continue;
                }
                let Ok(ptr) = self.render.GetBuffer(frames) else { break };
                let out = std::slice::from_raw_parts_mut(ptr as *mut f32, frames as usize * CHANNELS);
                fill(&mut queue.lock().unwrap_or_else(|e| e.into_inner()), out);
                let _ = self.render.ReleaseBuffer(frames, 0);
            }
            let _ = self.client.Stop();
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        // SAFETY: evento criado em open() e não usado depois daqui.
        unsafe {
            let _ = CloseHandle(self.event);
        }
    }
}

fn open() -> windows::core::Result<Output> {
    // SAFETY: COM iniciado nesta thread; estruturas vivem até o fim das chamadas.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = devices.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        let block_align = CHANNELS as u16 * 4;
        let format = WAVEFORMATEX {
            wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
            nChannels: CHANNELS as u16,
            nSamplesPerSec: SAMPLE_RATE,
            nAvgBytesPerSec: SAMPLE_RATE * u32::from(block_align),
            nBlockAlign: block_align,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            300_000, // 30 ms
            0,
            &format,
            None,
        )?;
        let buffer_frames = client.GetBufferSize()?;
        let render: IAudioRenderClient = client.GetService()?;
        let event = CreateEventW(None, false, false, None)?;
        client.SetEventHandle(event)?;
        Ok(Output {
            client,
            render,
            event,
            buffer_frames,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fila_corta_o_excesso_mantendo_os_mais_novos() {
        let mut q = VecDeque::new();
        push_bounded(&mut q, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 4);
        assert_eq!(q.iter().copied().collect::<Vec<_>>(), vec![3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn falta_de_amostras_vira_silencio_e_volume_e_aplicado() {
        let mut q = Queue {
            samples: VecDeque::from(vec![1.0, -1.0]),
            volume: 0.5,
        };
        let mut out = [9.0; 4];
        fill(&mut q, &mut out);
        assert_eq!(out, [0.5, -0.5, 0.0, 0.0]);
        assert!(q.samples.is_empty());
    }

    #[test]
    #[ignore = "precisa de saída de áudio (toca 0,3 s de um tom baixo)"]
    fn toca_no_dispositivo_padrao() {
        let player = AudioPlayer::start().expect("abrir a saída de áudio");
        player.set_volume(0.2);
        let tone: Vec<f32> = (0..SAMPLE_RATE as usize * 3 / 10)
            .flat_map(|i| {
                let v = 0.2 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / SAMPLE_RATE as f32).sin();
                [v, v]
            })
            .collect();
        for chunk in tone.chunks(960 * 2) {
            player.push(chunk);
            std::thread::sleep(std::time::Duration::from_millis(18));
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}
