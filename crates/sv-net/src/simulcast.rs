//! Simulcast (ADR 0026): quem transmite gera a camada principal e, quando alguém precisa, uma
//! camada baixa (720p30). Para cada espectador, [`LayerChooser`] decide qual camada ele recebe pelo
//! que se perde na fila dele: quem não dá conta da principal desce para a baixa sem mudar nada para
//! os outros, e de tempos em tempos tenta subir de novo.
//!
//! O relógio é o dos próprios frames (`timestamp_us` da captura): nada de chamada de sistema por
//! frame, e o teste controla o tempo.

/// Uma camada do simulcast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Layer {
    /// A qualidade escolhida por quem transmite.
    Main = 0,
    /// 720p30, para quem não aguenta a principal.
    Low = 1,
}

impl Layer {
    /// O inverso de `layer as u8` (para guardar num atômico).
    pub const fn from_u8(value: u8) -> Self {
        if value == Self::Low as u8 {
            Self::Low
        } else {
            Self::Main
        }
    }
}

/// Janela em que as perdas são contadas.
const WINDOW_US: u64 = 1_000_000;
/// Frames que não couberam na fila, numa janela, para descer para a camada baixa.
const DROPS_TO_DOWNGRADE: u32 = 2;
/// Tempo sem perda na camada baixa antes da primeira tentativa de subir.
const FIRST_PROBE_US: u64 = 10_000_000;
/// A espera entre tentativas dobra a cada uma que falha, até este teto.
const MAX_PROBE_US: u64 = 120_000_000;
/// Desceu de novo em menos que isto depois de subir: a tentativa falhou.
const PROBE_FAIL_US: u64 = 5_000_000;
/// Na principal sem descer por este tempo: a próxima espera volta ao começo.
const STABLE_US: u64 = 30_000_000;
/// A troca espera o keyframe da camada nova; se ele não vier (o encoder da baixa não abriu),
/// desiste depois deste tempo.
const SWITCH_TIMEOUT_US: u64 = 3_000_000;

/// Vídeo escrito e ainda não enviado acima disto conta como rede atrasada (cerca de 1/4 de
/// segundo de 1080p60 a 8 Mbps).
const BACKLOG_BYTES: u64 = 256 * 1024;
/// E precisa ficar acima por este tempo: um keyframe grande, numa rede boa, sai em poucos ms.
const BACKLOG_HOLD_US: u64 = 250_000;

/// Mede quanto vídeo está preso na conexão de um espectador: o QUIC guarda na fila de envio o que
/// escrevemos e a rede ainda não levou (até ~10 MB), então a fila do espectador quase nunca enche
/// e o atraso cresceria por segundos antes de algum frame ser descartado.
///
/// A conta é "escrito menos enviado" por amostra, sem deixar ficar negativa: o que sai a mais
/// (cabeçalhos, áudio, confirmações) não vira crédito para esconder um atraso depois.
#[derive(Debug, Clone, Default)]
pub struct BacklogMeter {
    last_tx: Option<u64>,
    debt: u64,
    over_since_us: Option<u64>,
}

impl BacklogMeter {
    pub const fn new() -> Self {
        Self {
            last_tx: None,
            debt: 0,
            over_since_us: None,
        }
    }

    /// Escrevemos `written` bytes; a conexão já mandou `udp_tx_bytes` no total. Diz se a rede está
    /// atrasada (o que está preso passou do limite e continua passando).
    pub fn sample(&mut self, written: u64, udp_tx_bytes: u64, now_us: u64) -> bool {
        let sent = self.last_tx.map_or(0, |last| udp_tx_bytes.saturating_sub(last));
        self.last_tx = Some(udp_tx_bytes);
        self.debt = (self.debt + written).saturating_sub(sent);
        if self.debt <= BACKLOG_BYTES {
            self.over_since_us = None;
            return false;
        }
        let since = *self.over_since_us.get_or_insert(now_us);
        now_us.saturating_sub(since) >= BACKLOG_HOLD_US
    }
}

/// Escolhe a camada de um espectador.
#[derive(Debug, Clone)]
pub struct LayerChooser {
    /// O espectador aceita trocar de camada (apps antigos não aceitam: ficam na principal).
    simulcast: bool,
    /// A camada que ele recebe agora.
    active: Layer,
    /// A camada para onde ele vai; a troca acontece no próximo keyframe dela.
    target: Layer,
    switch_started_us: u64,
    window_start_us: Option<u64>,
    drops: u32,
    /// Desde quando não se perde nada (vale na camada baixa, para tentar subir).
    clean_since_us: u64,
    probe_wait_us: u64,
    /// Quando subiu para a principal pela última vez (para saber se a tentativa deu certo).
    upgraded_at_us: Option<u64>,
}

impl LayerChooser {
    pub const fn new(simulcast: bool) -> Self {
        Self {
            simulcast,
            active: Layer::Main,
            target: Layer::Main,
            switch_started_us: 0,
            window_start_us: None,
            drops: 0,
            clean_since_us: 0,
            probe_wait_us: FIRST_PROBE_US,
            upgraded_at_us: None,
        }
    }

    /// A camada que o espectador recebe agora.
    pub const fn active(&self) -> Layer {
        self.active
    }

    /// Este espectador precisa da camada baixa (recebe ela ou está indo para ela).
    pub fn needs_low(&self) -> bool {
        self.active == Layer::Low || self.target == Layer::Low
    }

    /// Um frame da camada `layer` vai sair: diz se ele é para este espectador. A troca de camada
    /// acontece aqui, no keyframe da camada alvo, para o decoder recomeçar num quadro completo.
    pub fn accepts(&mut self, layer: Layer, keyframe: bool, now_us: u64) -> bool {
        if keyframe && layer == self.target && layer != self.active {
            self.active = layer;
            self.window_start_us = Some(now_us);
            self.drops = 0;
            self.clean_since_us = now_us;
            if layer == Layer::Main {
                self.upgraded_at_us = Some(now_us);
            }
        }
        layer == self.active
    }

    /// O resultado de entregar um frame da camada ativa (`delivered`: coube na fila do
    /// espectador). Quando começa uma troca, devolve a camada que precisa mandar um keyframe.
    pub fn record(&mut self, delivered: bool, now_us: u64) -> Option<Layer> {
        if !self.simulcast {
            return None;
        }
        if self.target != self.active && now_us.saturating_sub(self.switch_started_us) >= SWITCH_TIMEOUT_US {
            self.target = self.active;
        }
        let start = *self.window_start_us.get_or_insert(now_us);
        if now_us.saturating_sub(start) >= WINDOW_US {
            self.window_start_us = Some(now_us);
            self.drops = 0;
        }
        if !delivered {
            self.drops += 1;
            self.clean_since_us = now_us;
        }
        match self.active {
            Layer::Main if self.target == Layer::Main => {
                if self.drops >= DROPS_TO_DOWNGRADE {
                    if self
                        .upgraded_at_us
                        .is_some_and(|t| now_us.saturating_sub(t) < PROBE_FAIL_US)
                    {
                        self.probe_wait_us = (self.probe_wait_us * 2).min(MAX_PROBE_US);
                    }
                    self.upgraded_at_us = None;
                    return Some(self.start_switch(Layer::Low, now_us));
                }
                if self
                    .upgraded_at_us
                    .is_some_and(|t| now_us.saturating_sub(t) >= STABLE_US)
                {
                    self.probe_wait_us = FIRST_PROBE_US;
                    self.upgraded_at_us = None;
                }
            }
            Layer::Low
                if self.target == Layer::Low && now_us.saturating_sub(self.clean_since_us) >= self.probe_wait_us =>
            {
                return Some(self.start_switch(Layer::Main, now_us));
            }
            _ => {}
        }
        None
    }

    /// Prende o espectador na camada principal (na malha, os pais só repassam ela). Devolve
    /// `Some(Main)` quando começa a voltar, para pedir o keyframe.
    pub fn force_main(&mut self, now_us: u64) -> Option<Layer> {
        if self.target == Layer::Main {
            return None;
        }
        self.upgraded_at_us = None;
        Some(self.start_switch(Layer::Main, now_us))
    }

    fn start_switch(&mut self, to: Layer, now_us: u64) -> Layer {
        self.target = to;
        self.switch_started_us = now_us;
        self.drops = 0;
        self.window_start_us = Some(now_us);
        to
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: u64 = 16_667;

    /// Entrega frames da camada ativa a 60 fps de `from` até `to` (µs); `lost` diz quais não
    /// couberam. Devolve as trocas pedidas, com o instante.
    fn run(c: &mut LayerChooser, from: u64, to: u64, mut lost: impl FnMut(u64) -> bool) -> Vec<(u64, Layer)> {
        let mut switches = Vec::new();
        let mut t = from;
        while t < to {
            let layer = c.active();
            if c.accepts(layer, false, t)
                && let Some(s) = c.record(!lost(t), t)
            {
                switches.push((t, s));
            }
            t += FRAME;
        }
        switches
    }

    /// Chega o keyframe da camada alvo.
    fn keyframe(c: &mut LayerChooser, layer: Layer, t: u64) {
        assert!(c.accepts(layer, true, t));
    }

    #[test]
    fn sem_perda_fica_na_principal() {
        let mut c = LayerChooser::new(true);
        assert!(run(&mut c, 0, 60_000_000, |_| false).is_empty());
        assert_eq!(c.active(), Layer::Main);
        assert!(!c.needs_low());
    }

    #[test]
    fn perdas_na_fila_descem_para_a_baixa_no_keyframe_dela() {
        let mut c = LayerChooser::new(true);
        let switches = run(&mut c, 0, 2_000_000, |t| t > 500_000);
        assert_eq!(switches.len(), 1);
        assert_eq!(switches[0].1, Layer::Low);
        // Até o keyframe da baixa, continua recebendo a principal (nada congela).
        assert_eq!(c.active(), Layer::Main);
        assert!(c.needs_low());
        assert!(!c.accepts(Layer::Low, false, 2_000_000), "baixa sem keyframe não serve");
        keyframe(&mut c, Layer::Low, 2_000_000);
        assert_eq!(c.active(), Layer::Low);
        assert!(!c.accepts(Layer::Main, true, 2_010_000), "a principal deixa de ir");
    }

    #[test]
    fn uma_perda_solta_nao_derruba_a_qualidade() {
        let mut c = LayerChooser::new(true);
        // Um frame perdido a cada 2 s: nunca 2 na mesma janela.
        let switches = run(&mut c, 0, 20_000_000, |t| t % 2_000_000 < FRAME);
        assert!(switches.is_empty(), "{switches:?}");
    }

    #[test]
    fn na_baixa_sem_perda_tenta_subir_e_a_espera_dobra_se_falhar() {
        let mut c = LayerChooser::new(true);
        run(&mut c, 0, 1_000_000, |_| true);
        keyframe(&mut c, Layer::Low, 1_000_000);
        // 10 s limpos na baixa: tenta a principal.
        let up = run(&mut c, 1_000_000, 12_000_000, |_| false);
        assert_eq!(up.len(), 1);
        assert_eq!(up[0].1, Layer::Main);
        assert!((up[0].0 - 11_000_000) < 2 * FRAME, "{up:?}");
        keyframe(&mut c, Layer::Main, 12_000_000);
        // Não aguentou: volta a perder logo e desce.
        let down = run(&mut c, 12_000_000, 13_000_000, |_| true);
        assert_eq!(down.first().map(|s| s.1), Some(Layer::Low));
        keyframe(&mut c, Layer::Low, 13_000_000);
        // A próxima tentativa espera o dobro: 20 s.
        let early = run(&mut c, 13_000_000, 30_000_000, |_| false);
        assert!(early.is_empty(), "{early:?}");
        let later = run(&mut c, 30_000_000, 34_000_000, |_| false);
        assert_eq!(later.first().map(|s| s.1), Some(Layer::Main));
    }

    #[test]
    fn subiu_e_ficou_estavel_a_espera_volta_ao_comeco() {
        let mut c = LayerChooser::new(true);
        run(&mut c, 0, 1_000_000, |_| true);
        keyframe(&mut c, Layer::Low, 1_000_000);
        run(&mut c, 1_000_000, 12_000_000, |_| false);
        keyframe(&mut c, Layer::Main, 12_000_000);
        // Falha uma vez (espera vai a 20 s)...
        run(&mut c, 12_000_000, 12_500_000, |_| true);
        keyframe(&mut c, Layer::Low, 12_500_000);
        run(&mut c, 12_500_000, 33_000_000, |_| false);
        keyframe(&mut c, Layer::Main, 33_000_000);
        // ...depois fica 30 s bem na principal.
        assert!(run(&mut c, 33_000_000, 64_000_000, |_| false).is_empty());
        // A rede piora de novo: desce, e a próxima tentativa já é em 10 s.
        run(&mut c, 64_000_000, 65_000_000, |_| true);
        keyframe(&mut c, Layer::Low, 65_000_000);
        let up = run(&mut c, 65_000_000, 76_000_000, |_| false);
        assert_eq!(up.first().map(|s| s.1), Some(Layer::Main), "{up:?}");
    }

    #[test]
    fn se_a_baixa_nao_chega_desiste_da_troca() {
        let mut c = LayerChooser::new(true);
        run(&mut c, 0, 1_000_000, |_| true);
        assert!(c.needs_low());
        // O encoder da baixa não abriu: nenhum keyframe dela em 3 s.
        run(&mut c, 1_000_000, 4_500_000, |_| false);
        assert!(!c.needs_low(), "parou de pedir a camada baixa");
        assert_eq!(c.active(), Layer::Main);
    }

    #[test]
    fn preso_na_principal_volta_para_ela_no_keyframe() {
        let mut c = LayerChooser::new(true);
        run(&mut c, 0, 1_000_000, |_| true);
        keyframe(&mut c, Layer::Low, 1_000_000);
        assert_eq!(c.force_main(1_100_000), Some(Layer::Main));
        assert_eq!(c.force_main(1_200_000), None, "pede o keyframe uma vez só");
        assert!(
            c.accepts(Layer::Low, false, 1_300_000),
            "até o keyframe, a baixa continua"
        );
        keyframe(&mut c, Layer::Main, 1_400_000);
        assert_eq!(c.active(), Layer::Main);
        assert!(!c.needs_low());
    }

    #[test]
    fn app_antigo_nunca_troca_de_camada() {
        let mut c = LayerChooser::new(false);
        assert!(run(&mut c, 0, 10_000_000, |_| true).is_empty());
        assert_eq!(c.active(), Layer::Main);
        assert!(!c.accepts(Layer::Low, true, 0));
    }

    #[test]
    fn rede_que_leva_tudo_nunca_fica_atrasada() {
        let mut m = BacklogMeter::new();
        let mut tx = 1_000_000;
        for i in 0..600 {
            // Um keyframe de 300 KB a cada segundo e frames de 15 KB; sai tudo e mais um pouco.
            let frame = if i % 60 == 0 { 300_000 } else { 15_000 };
            assert!(!m.sample(frame, tx, i * 16_667), "frame {i}");
            tx += frame + 2_000;
        }
    }

    #[test]
    fn rede_que_leva_a_metade_fica_atrasada_depois_de_um_tempo() {
        let mut m = BacklogMeter::new();
        let mut tx = 0;
        let mut first = None;
        for i in 0..120u64 {
            // 8 Mbps escritos, 4 Mbps enviados.
            if m.sample(16_667, tx, i * 16_667) && first.is_none() {
                first = Some(i);
            }
            tx += 8_333;
        }
        let first = first.expect("devia acusar o atraso");
        // 256 KB acumulam em ~0,5 s e precisam durar mais 0,25 s.
        assert!((40..=60).contains(&first), "acusou no frame {first}");
    }

    #[test]
    fn o_que_sai_a_mais_nao_esconde_um_atraso_depois() {
        let mut m = BacklogMeter::new();
        // 10 minutos de áudio e cabeçalhos: sai muito mais do que o vídeo escrito.
        assert!(!m.sample(0, 0, 0));
        assert!(!m.sample(0, 50_000_000, 1));
        // Depois a rede trava: nada sai, e o atraso aparece mesmo assim.
        let mut late = false;
        for i in 0..60u64 {
            late |= m.sample(16_667, 50_000_000, 1 + i * 16_667);
        }
        assert!(late);
    }

    #[test]
    fn camada_vai_e_volta_do_atomico() {
        for layer in [Layer::Main, Layer::Low] {
            assert_eq!(Layer::from_u8(layer as u8), layer);
        }
    }
}
