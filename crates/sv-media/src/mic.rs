//! Microfone: WASAPI compartilhado no dispositivo escolhido, entregue em 48 kHz mono f32.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioCaptureClient, IAudioClient,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoInitializeEx};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

use crate::devices::{Flow, open_device};
use crate::wasapi::{buffer_duration, copy_packet, float_format};

/// Taxa entregue (a mesma do Opus e do supressor de ruído).
pub const MIC_SAMPLE_RATE: u32 = 48_000;

/// O microfone aberto. Para ao ser descartado.
#[derive(Debug)]
pub struct MicCapture {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl MicCapture {
    /// Abre o microfone `device` (ou o padrão de chamadas do Windows). `on_samples` recebe blocos
    /// mono de ~10 ms na thread do microfone.
    pub fn start(
        device: Option<String>,
        on_samples: impl FnMut(&[f32]) + Send + 'static,
    ) -> windows::core::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread_stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("sv-mic".into())
            .spawn(move || {
                let mut on_samples = on_samples;
                match open(device.as_deref()) {
                    Ok(stream) => {
                        let _ = ready_tx.send(Ok(()));
                        stream.run(&thread_stop, &mut on_samples);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })
            .expect("não foi possível criar a thread do microfone");
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

impl Drop for MicCapture {
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
        // Reaproveitado a cada bloco: nada de alocação no caminho do áudio.
        let mut buffer: Vec<f32> = Vec::with_capacity(4800);
        // SAFETY: client e capture pertencem a esta thread; o buffer do WASAPI só é lido entre
        // GetBuffer e ReleaseBuffer, no tamanho informado por ele.
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
                    let len = count as usize;
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

fn open(device: Option<&str>) -> windows::core::Result<Stream> {
    // SAFETY: COM iniciado nesta thread; as estruturas passadas vivem até o fim das chamadas.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let device = open_device(Flow::Input, device)?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        // Pedimos 48 kHz mono e o Windows converte do formato do microfone.
        let format = float_format(1, MIC_SAMPLE_RATE);
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
            buffer_duration(20),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "precisa de microfone"]
    fn abre_o_microfone_padrao_e_recebe_amostras() {
        let (tx, rx) = mpsc::channel();
        let mic = MicCapture::start(None, move |s| {
            let _ = tx.send(s.len());
        })
        .expect("abrir o microfone");
        let got = rx.recv_timeout(std::time::Duration::from_secs(2)).expect("amostras");
        assert!(got > 0);
        drop(mic);
    }

    #[test]
    #[ignore = "precisa de microfone"]
    fn microfone_que_sumiu_cai_no_padrao_do_windows() {
        let (tx, rx) = mpsc::channel();
        let mic = MicCapture::start(Some("{id-que-nao-existe}".into()), move |s| {
            let _ = tx.send(s.len());
        })
        .expect("abrir o microfone padrão");
        assert!(rx.recv_timeout(std::time::Duration::from_secs(2)).is_ok());
        assert!(format!("{mic:?}").contains("MicCapture"));
    }

    #[test]
    #[ignore = "precisa de microfone"]
    fn abre_cada_microfone_da_lista() {
        for device in crate::list_devices(Flow::Input) {
            let mic = MicCapture::start(Some(device.id.clone()), |_| {});
            assert!(mic.is_ok(), "{}: {mic:?}", device.name);
        }
    }
}
