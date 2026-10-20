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
    eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

use crate::devices::{Flow, open_device};
use crate::opus::{CHANNELS, SAMPLE_RATE};
use crate::wasapi::{buffer_duration, float_format};

/// Maior atraso aceito na fila de reprodução.
pub const MAX_BUFFERED_MS: usize = 120;
const MAX_BUFFERED: usize = SAMPLE_RATE as usize * CHANNELS * MAX_BUFFERED_MS / 1000;

/// Volume máximo (200%).
pub const MAX_VOLUME: f32 = 2.0;

#[derive(Debug, Default)]
struct Queue {
    samples: VecDeque<f32>,
    volume: f32,
}

impl Queue {
    fn new() -> Self {
        Self {
            samples: VecDeque::new(),
            volume: 1.0,
        }
    }

    fn push(&mut self, samples: &[f32]) {
        push_bounded(&mut self.samples, samples, MAX_BUFFERED);
    }

    fn buffered_ms(&self) -> usize {
        self.samples.len() * 1000 / (SAMPLE_RATE as usize * CHANNELS)
    }

    fn set_volume(&mut self, volume: f32) {
        // Curva perceptiva: 200% soa bem mais alto, como no Discord.
        self.volume = crate::level::volume_gain(volume);
    }
}

/// Saída de áudio em andamento. Para ao ser descartada.
#[derive(Debug)]
pub struct AudioPlayer {
    queue: Arc<Mutex<Queue>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioPlayer {
    /// Toca na saída padrão do Windows.
    pub fn start() -> windows::core::Result<Self> {
        Self::start_on(None)
    }

    /// Toca na saída `device` (ou na padrão, se `None` ou se ela sumiu).
    pub fn start_on(device: Option<String>) -> windows::core::Result<Self> {
        let queue = Arc::new(Mutex::new(Queue::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (q, s) = (queue.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("sv-audio-playback".into())
            .spawn(move || match open(device.as_deref()) {
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
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).push(samples);
    }

    /// Quanto áudio está na fila esperando a placa, em ms (quem mistura usa para saber a hora
    /// de mandar mais).
    pub fn buffered_ms(&self) -> usize {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).buffered_ms()
    }

    /// Volume de 0 (mudo) a 2 (200%, como no Discord).
    pub fn set_volume(&self, volume: f32) {
        self.queue.lock().unwrap_or_else(|e| e.into_inner()).set_volume(volume);
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

/// Quantos quadros cabem no buffer da placa agora (o resto ainda está esperando tocar).
fn writable_frames(buffer_frames: u32, padding: u32) -> u32 {
    buffer_frames.saturating_sub(padding)
}

/// Preenche `out` com o que tem na fila (aplicando o volume) e completa com silêncio.
fn fill(queue: &mut Queue, out: &mut [f32]) {
    let n = out.len().min(queue.samples.len());
    for (dst, src) in out.iter_mut().zip(queue.samples.drain(..n)) {
        // Acima de 100% o som pode passar do limite: corta em vez de distorcer na placa.
        *dst = crate::level::soft_clip(src * queue.volume);
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
                let frames = writable_frames(self.buffer_frames, padding);
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

fn open(device: Option<&str>) -> windows::core::Result<Output> {
    // SAFETY: COM iniciado nesta thread; estruturas vivem até o fim das chamadas.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let device = match device {
            Some(id) => open_device(Flow::Output, Some(id))?,
            // Sem escolha: a saída padrão geral (a mesma de antes, para o som do jogo).
            None => {
                let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                devices.GetDefaultAudioEndpoint(eRender, eConsole)?
            }
        };
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        let format = float_format(CHANNELS as u16, SAMPLE_RATE);
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            buffer_duration(30),
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
    fn volume_acima_de_100_e_limitado_sem_corte_seco() {
        let mut q = Queue {
            samples: VecDeque::from(vec![0.3, -0.8]),
            volume: 2.0,
        };
        let mut out = [0.0; 2];
        fill(&mut q, &mut out);
        assert!((out[0] - 0.6).abs() < 1e-6);
        // -1,6 passa pelo limitador: fica perto de -1, sem passar.
        assert!(out[1] >= -1.0 && out[1] < -0.99, "{}", out[1]);
    }

    #[test]
    fn fila_mede_o_atraso_e_limita_o_volume() {
        let mut q = Queue::new();
        assert_eq!(q.volume, 1.0);
        // 20 ms de estéreo a 48 kHz.
        q.push(&[0.0; 960 * 2]);
        assert_eq!(q.buffered_ms(), 20);
        // Muito mais que o limite: fica no máximo MAX_BUFFERED_MS.
        q.push(&vec![0.0; SAMPLE_RATE as usize * CHANNELS]);
        assert_eq!(q.buffered_ms(), MAX_BUFFERED_MS);
        // Acima de 200% fica em 200%, que pela curva perceptiva é ×4.
        q.set_volume(5.0);
        assert_eq!(q.volume, MAX_VOLUME * MAX_VOLUME);
        q.set_volume(-1.0);
        assert_eq!(q.volume, 0.0);
    }

    #[test]
    fn excesso_impar_nao_desalinha_esquerda_e_direita() {
        let mut q = VecDeque::new();
        // 5 amostras num limite de 4: corta 2 (um par L/R inteiro), não 1.
        push_bounded(&mut q, &[1.0, 2.0, 3.0, 4.0, 5.0], 4);
        assert_eq!(q.iter().copied().collect::<Vec<_>>(), vec![3.0, 4.0, 5.0]);
    }

    #[test]
    fn so_escreve_o_que_cabe_no_buffer_da_placa() {
        assert_eq!(writable_frames(1440, 480), 960);
        assert_eq!(writable_frames(1440, 1440), 0);
        assert_eq!(writable_frames(1440, 2000), 0, "padding maior que o buffer não estoura");
    }

    #[test]
    #[ignore = "precisa de saída de áudio"]
    fn mede_o_atraso_e_esvazia_tocando() {
        let player = AudioPlayer::start_on(Some("{saida-que-nao-existe}".into())).expect("cai na saída padrão");
        player.set_volume(0.0);
        player.push(&[0.0; 960 * 2 * 5]);
        assert!(player.buffered_ms() >= 50);
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(player.buffered_ms() < 50, "a placa não consumiu a fila");
        assert!(format!("{player:?}").contains("AudioPlayer"));
    }

    #[test]
    #[ignore = "precisa de saída de áudio"]
    fn toca_em_cada_saida_da_lista() {
        for device in crate::list_devices(Flow::Output) {
            let player = AudioPlayer::start_on(Some(device.id.clone()));
            assert!(player.is_ok(), "{}: {player:?}", device.name);
        }
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
