//! Captura de tela e de áudio no Windows.
//!
//! Fase 1: Windows.Graphics.Capture com a textura mantida na GPU (zero cópia para a CPU)
//! e WASAPI process loopback para capturar só o áudio do jogo.

/// O que pode ser capturado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureSource {
    /// Um monitor inteiro, pelo índice.
    Monitor(u32),
    /// Uma janela, pelo título.
    Window(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fontes_diferentes_nao_sao_iguais() {
        assert_ne!(CaptureSource::Monitor(0), CaptureSource::Window("Jogo".into()));
    }
}
