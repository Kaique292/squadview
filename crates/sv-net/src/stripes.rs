//! Faixas e paridade da malha (ADR 0012, ADR 0028): cada frame codificado é cortado em
//! [`DATA_STRIPES`] faixas de dados e uma de paridade (o XOR delas). Cada faixa desce por um caminho
//! (uma árvore da sala), e com quaisquer 4 das 5 o frame volta inteiro: a saída de quem repassava
//! uma faixa não congela ninguém.
//!
//! - [`Splitter`]: quem transmite corta o frame, reaproveitando os buffers.
//! - [`Reassembler`]: quem recebe junta as faixas que chegam de várias conexões e entrega os
//!   frames na ordem, pulando o que passou do prazo.

use serde::{Deserialize, Serialize};

use crate::protocol::FrameHeader;
use crate::simulcast::Layer;

/// Faixas de dados por frame.
pub const DATA_STRIPES: usize = 4;
/// Todas as faixas: as de dados e a de paridade (a última).
pub const STRIPES: usize = DATA_STRIPES + 1;
/// Índice da faixa de paridade.
pub const PARITY: u8 = DATA_STRIPES as u8;
/// Máscara (um bit por faixa) só das faixas de dados: quem recebe tudo de um caminho só não
/// precisa da paridade.
pub const DATA_MASK: u8 = (1 << DATA_STRIPES) - 1;
/// Maior frame aceito numa faixa (o mesmo limite dos frames inteiros).
pub const MAX_FRAME_BYTES: u32 = crate::protocol::MAX_FRAME_BYTES as u32;

/// Cabeçalho de cada faixa (seguido pelos bytes dela). Vai na frente de cada stream de faixa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StripeHeader {
    /// O frame inteiro: sequência (da camada), hora da captura, keyframe, tempo de encode.
    pub frame: FrameHeader,
    /// Camada do simulcast: 0 a principal, 1 a baixa.
    pub layer: u8,
    /// Qual faixa: 0 a 3 são dados, 4 é a paridade.
    pub index: u8,
    /// Tamanho do frame inteiro, para cortar o enchimento da última faixa.
    pub frame_len: u32,
}

impl StripeHeader {
    /// Tamanho de cada faixa deste frame (as 5 têm o mesmo; a última de dados é completada com
    /// zeros).
    pub const fn stripe_len(&self) -> usize {
        stripe_len(self.frame_len as usize)
    }

    /// A camada do simulcast.
    pub const fn layer(&self) -> Layer {
        Layer::from_u8(self.layer)
    }
}

/// Tamanho de cada faixa de um frame de `frame_len` bytes.
pub const fn stripe_len(frame_len: usize) -> usize {
    frame_len.div_ceil(DATA_STRIPES)
}

/// Corta frames em faixas, sempre nos mesmos buffers (sem alocar por frame depois do primeiro
/// keyframe grande).
#[derive(Debug, Default)]
pub struct Splitter {
    stripes: [Vec<u8>; STRIPES],
}

impl Splitter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Corta `data` e devolve as 5 faixas (as 4 de dados e a paridade), válidas até o próximo
    /// corte.
    pub fn split(&mut self, data: &[u8]) -> &[Vec<u8>; STRIPES] {
        let len = stripe_len(data.len());
        for (i, stripe) in self.stripes.iter_mut().enumerate() {
            stripe.clear();
            if i < DATA_STRIPES {
                let start = (i * len).min(data.len());
                let end = ((i + 1) * len).min(data.len());
                stripe.extend_from_slice(&data[start..end]);
            }
            stripe.resize(len, 0);
        }
        let (data_stripes, parity) = self.stripes.split_at_mut(DATA_STRIPES);
        for stripe in data_stripes.iter() {
            xor_into(&mut parity[0], stripe);
        }
        &self.stripes
    }
}

/// Guarda um buffer para reaproveitar, sem deixar o pool passar de uma janela inteira.
fn recycle(pool: &mut Vec<Vec<u8>>, buf: Vec<u8>) {
    if pool.len() < WINDOW * STRIPES {
        pool.push(buf);
    }
}

fn xor_into(acc: &mut [u8], other: &[u8]) {
    for (a, b) in acc.iter_mut().zip(other) {
        *a ^= b;
    }
}

/// Frames em montagem ao mesmo tempo. Uma faixa mais adiantada que isso empurra os mais velhos.
const WINDOW: usize = 16;
/// Um frame incompleto é pulado quando outro mais novo já está pronto e ele espera há mais que isto.
const DEADLINE_US: u64 = 150_000;

/// O que a remontagem entrega.
#[derive(Debug, PartialEq, Eq)]
pub enum Assembled {
    /// Um frame inteiro, na ordem.
    Frame(FrameHeader, Vec<u8>),
    /// Estes frames não chegaram a tempo (faltou mais de uma faixa): o decoder precisa de um
    /// keyframe para continuar.
    Lost { first_seq: u32, count: u32 },
}

#[derive(Debug, Default)]
struct Slot {
    in_use: bool,
    seq: u32,
    header: Option<StripeHeader>,
    stripes: [Option<Vec<u8>>; STRIPES],
    have: u8,
    first_us: u64,
}

impl Slot {
    fn complete(&self) -> bool {
        usize::from(self.have) >= DATA_STRIPES
    }
}

/// Junta as faixas de uma camada (de um stream de quem transmite) e entrega os frames na ordem.
#[derive(Debug)]
pub struct Reassembler {
    slots: Vec<Slot>,
    /// A sequência do próximo frame a entregar (`None` até a primeira faixa).
    next: Option<u32>,
    /// Buffers de faixa devolvidos, para a próxima leitura.
    pool: Vec<Vec<u8>>,
}

impl Default for Reassembler {
    fn default() -> Self {
        Self::new()
    }
}

impl Reassembler {
    pub fn new() -> Self {
        Self {
            slots: (0..WINDOW).map(|_| Slot::default()).collect(),
            next: None,
            pool: Vec::new(),
        }
    }

    /// Um buffer para ler a próxima faixa (de um frame que já passou, se houver).
    pub fn buffer(&mut self) -> Vec<u8> {
        let mut b = self.pool.pop().unwrap_or_default();
        b.clear();
        b
    }

    /// Chegou uma faixa (`payload`, do tamanho `header.stripe_len()`), às `now_us` no nosso relógio.
    /// Os frames que ficam prontos (ou perdidos) saem por `out`, na ordem.
    pub fn push(&mut self, header: StripeHeader, payload: Vec<u8>, now_us: u64, out: &mut impl FnMut(Assembled)) {
        let seq = header.frame.seq;
        if usize::from(header.index) >= STRIPES
            || header.frame_len > MAX_FRAME_BYTES
            || payload.len() != header.stripe_len()
        {
            recycle(&mut self.pool, payload);
            return;
        }
        let next = *self.next.get_or_insert(seq);
        // Atrasada demais (o frame já foi entregue ou pulado).
        if seq.wrapping_sub(next) > u32::MAX / 2 {
            recycle(&mut self.pool, payload);
            return;
        }
        // Adiantada demais: os mais velhos que não couberem na janela são pulados.
        let ahead = seq.wrapping_sub(next) as usize;
        if ahead >= WINDOW {
            let skip = (ahead - WINDOW + 1) as u32;
            self.skip(skip, out);
        }
        let slot = &mut self.slots[seq as usize % WINDOW];
        if !slot.in_use || slot.seq != seq {
            Self::reset(slot, &mut self.pool);
            slot.in_use = true;
            slot.seq = seq;
            slot.first_us = now_us;
        }
        let i = usize::from(header.index);
        if slot.stripes[i].is_some() {
            recycle(&mut self.pool, payload);
        } else {
            slot.stripes[i] = Some(payload);
            slot.have += 1;
            slot.header.get_or_insert(header);
        }
        self.deliver(now_us, out);
    }

    /// Entrega o que está pronto na ordem; um frame incompleto velho, com outro pronto depois dele,
    /// é dado como perdido.
    fn deliver(&mut self, now_us: u64, out: &mut impl FnMut(Assembled)) {
        // Frames pulados em seguida viram um aviso só.
        let mut lost: Option<(u32, u32)> = None;
        while let Some(next) = self.next {
            let idx = next as usize % WINDOW;
            let seen = self.slots[idx].in_use && self.slots[idx].seq == next;
            if seen && self.slots[idx].complete() {
                if let Some((first_seq, count)) = lost.take() {
                    out(Assembled::Lost { first_seq, count });
                }
                let frame = self.take(idx);
                out(frame);
                self.next = Some(next.wrapping_add(1));
                continue;
            }
            let newer_ready = (1..WINDOW as u32).any(|d| {
                let s = &self.slots[next.wrapping_add(d) as usize % WINDOW];
                s.in_use && s.seq == next.wrapping_add(d) && s.complete()
            });
            // O prazo conta da primeira faixa que está esperando: faixas de caminhos diferentes
            // chegam com atrasos diferentes, e um frame sem nenhuma faixa ainda pode estar a caminho.
            let oldest = self.slots.iter().filter(|s| s.in_use).map(|s| s.first_us).min();
            let expired = oldest.is_some_and(|t| now_us.saturating_sub(t) >= DEADLINE_US);
            if !(newer_ready && expired) {
                break;
            }
            if seen {
                Self::reset(&mut self.slots[idx], &mut self.pool);
            }
            lost = Some(lost.map_or((next, 1), |(first, count)| (first, count + 1)));
            self.next = Some(next.wrapping_add(1));
        }
        if let Some((first_seq, count)) = lost {
            out(Assembled::Lost { first_seq, count });
        }
    }

    /// Pula `count` frames a partir do próximo (avisando que se perderam).
    fn skip(&mut self, count: u32, out: &mut impl FnMut(Assembled)) {
        let Some(next) = self.next else { return };
        for d in 0..count.min(WINDOW as u32) {
            let seq = next.wrapping_add(d);
            let slot = &mut self.slots[seq as usize % WINDOW];
            if slot.in_use && slot.seq == seq {
                Self::reset(slot, &mut self.pool);
            }
        }
        out(Assembled::Lost { first_seq: next, count });
        self.next = Some(next.wrapping_add(count));
    }

    /// Monta o frame do slot (refazendo a faixa que faltou pela paridade) e libera o slot.
    fn take(&mut self, idx: usize) -> Assembled {
        let slot = &mut self.slots[idx];
        let header = slot.header.expect("slot completo tem cabeçalho");
        if let Some(missing) = (0..DATA_STRIPES).find(|i| slot.stripes[*i].is_none()) {
            let mut rebuilt = slot.stripes[usize::from(PARITY)]
                .take()
                .expect("com uma faixa de dados faltando, a paridade chegou");
            for (i, s) in slot.stripes.iter().enumerate().take(DATA_STRIPES) {
                if i != missing
                    && let Some(s) = s
                {
                    xor_into(&mut rebuilt, s);
                }
            }
            slot.stripes[missing] = Some(rebuilt);
        }
        let mut data = Vec::with_capacity(header.frame_len as usize);
        for s in slot.stripes.iter().take(DATA_STRIPES).flatten() {
            data.extend_from_slice(s);
        }
        data.truncate(header.frame_len as usize);
        Self::reset(slot, &mut self.pool);
        Assembled::Frame(header.frame, data)
    }

    fn reset(slot: &mut Slot, pool: &mut Vec<Vec<u8>>) {
        for s in &mut slot.stripes {
            if let Some(b) = s.take() {
                recycle(pool, b);
            }
        }
        slot.in_use = false;
        slot.header = None;
        slot.have = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(seq: u32, index: u8, frame_len: usize) -> StripeHeader {
        StripeHeader {
            frame: FrameHeader {
                seq,
                timestamp_us: u64::from(seq),
                keyframe: seq == 0,
                encode_us: 2_000,
            },
            layer: 0,
            index,
            frame_len: frame_len as u32,
        }
    }

    fn frame_bytes(seq: u32, len: usize) -> Vec<u8> {
        (0..len)
            .map(|i| (i as u32).wrapping_mul(31).wrapping_add(seq.wrapping_mul(7)) as u8)
            .collect()
    }

    /// Corta o frame `seq` e entrega as faixas `indexes` ao remontador.
    fn feed(r: &mut Reassembler, s: &mut Splitter, seq: u32, len: usize, indexes: &[u8], now: u64) -> Vec<Assembled> {
        let data = frame_bytes(seq, len);
        let stripes = s.split(&data).clone();
        let mut got = Vec::new();
        for &i in indexes {
            r.push(header(seq, i, len), stripes[usize::from(i)].clone(), now, &mut |a| {
                got.push(a)
            });
        }
        got
    }

    #[test]
    fn quaisquer_4_das_5_faixas_remontam_o_frame_byte_a_byte() {
        // Tamanhos que não dividem por 4, um vazio e um keyframe grande.
        for len in [0, 1, 3, 4, 5, 1_001, 300_003] {
            for missing in 0..STRIPES as u8 {
                let mut r = Reassembler::new();
                let mut s = Splitter::new();
                let indexes: Vec<u8> = (0..STRIPES as u8).filter(|i| *i != missing).collect();
                let got = feed(&mut r, &mut s, 0, len, &indexes, 0);
                assert_eq!(
                    got,
                    vec![Assembled::Frame(header(0, 0, len).frame, frame_bytes(0, len))],
                    "len {len}, sem a faixa {missing}"
                );
            }
        }
    }

    #[test]
    fn com_3_faixas_o_frame_espera_e_chega_quando_vem_a_quarta() {
        let mut r = Reassembler::new();
        let mut s = Splitter::new();
        assert!(feed(&mut r, &mut s, 0, 100, &[0, 1, 4], 0).is_empty());
        let got = feed(&mut r, &mut s, 0, 100, &[3], 10);
        assert_eq!(
            got,
            vec![Assembled::Frame(header(0, 0, 100).frame, frame_bytes(0, 100))]
        );
    }

    #[test]
    fn frames_saem_na_ordem_mesmo_com_faixas_fora_de_ordem() {
        let mut r = Reassembler::new();
        let mut s = Splitter::new();
        let mut got = feed(&mut r, &mut s, 0, 50, &[0, 1], 0);
        // O frame 1 fica pronto antes do 0: espera o 0.
        got.extend(feed(&mut r, &mut s, 1, 50, &[0, 1, 2, 3], 1));
        assert!(got.is_empty(), "{got:?}");
        got.extend(feed(&mut r, &mut s, 0, 50, &[4, 2], 2));
        let seqs: Vec<u32> = got
            .iter()
            .map(|a| match a {
                Assembled::Frame(h, _) => h.seq,
                Assembled::Lost { .. } => u32::MAX,
            })
            .collect();
        assert_eq!(seqs, vec![0, 1]);
    }

    #[test]
    fn frame_que_perdeu_duas_faixas_e_pulado_depois_do_prazo() {
        let mut r = Reassembler::new();
        let mut s = Splitter::new();
        let mut got = feed(&mut r, &mut s, 0, 80, &[0, 1, 2], 0);
        // O 1 está pronto, mas o 0 ainda está no prazo: espera.
        got.extend(feed(&mut r, &mut s, 1, 80, &[0, 1, 2, 3], 50_000));
        assert!(got.is_empty(), "{got:?}");
        // Passou do prazo: o 0 é dado como perdido e o 1 sai.
        got.extend(feed(&mut r, &mut s, 2, 80, &[0, 1, 2, 3], DEADLINE_US + 1));
        assert_eq!(got[0], Assembled::Lost { first_seq: 0, count: 1 });
        assert!(matches!(&got[1], Assembled::Frame(h, _) if h.seq == 1));
        assert!(matches!(&got[2], Assembled::Frame(h, _) if h.seq == 2));
        // Uma faixa atrasada do frame pulado é ignorada.
        assert!(feed(&mut r, &mut s, 0, 80, &[3], DEADLINE_US + 2).is_empty());
    }

    #[test]
    fn faixa_muito_adiantada_empurra_a_janela() {
        let mut r = Reassembler::new();
        let mut s = Splitter::new();
        assert!(feed(&mut r, &mut s, 0, 40, &[0], 0).is_empty());
        let far = WINDOW as u32 + 2;
        // Os 3 mais velhos saem da janela na hora.
        let got = feed(&mut r, &mut s, far, 40, &[0, 1, 2, 3], 1);
        assert_eq!(got, vec![Assembled::Lost { first_seq: 0, count: 3 }]);
        // Os do meio ainda podem chegar; passado o prazo, viram um aviso só e o adiantado sai.
        let got = feed(&mut r, &mut s, far, 40, &[4], DEADLINE_US + 1);
        assert_eq!(got.len(), 2, "{got:?}");
        assert_eq!(
            got[0],
            Assembled::Lost {
                first_seq: 3,
                count: far - 3
            }
        );
        assert!(matches!(&got[1], Assembled::Frame(h, _) if h.seq == far));
    }

    #[test]
    fn frame_sem_nenhuma_faixa_ainda_espera_o_prazo() {
        // Faixas de caminhos diferentes: o frame 1 chega inteiro antes de qualquer faixa do 0.
        let mut r = Reassembler::new();
        let mut s = Splitter::new();
        assert!(feed(&mut r, &mut s, 0, 60, &[0], 0).is_empty());
        assert!(feed(&mut r, &mut s, 2, 60, &[0, 1, 2, 3], 10).is_empty());
        let got = feed(&mut r, &mut s, 1, 60, &[0, 1, 2, 3], 20);
        assert!(got.is_empty(), "o 0 ainda está no prazo: {got:?}");
        let got = feed(&mut r, &mut s, 0, 60, &[1, 2, 3], 30);
        let seqs: Vec<u32> = got
            .iter()
            .filter_map(|a| match a {
                Assembled::Frame(h, _) => Some(h.seq),
                Assembled::Lost { .. } => None,
            })
            .collect();
        assert_eq!(seqs, vec![0, 1, 2], "nada se perdeu");
    }

    #[test]
    fn faixa_repetida_ou_com_tamanho_errado_e_ignorada() {
        let mut r = Reassembler::new();
        let mut s = Splitter::new();
        assert!(
            feed(&mut r, &mut s, 0, 100, &[0, 0, 0, 1], 0).is_empty(),
            "a repetida não conta duas vezes"
        );
        let mut got = Vec::new();
        r.push(header(0, 2, 100), vec![0; 7], 0, &mut |a| got.push(a));
        r.push(header(0, 9, 100), vec![0; 25], 0, &mut |a| got.push(a));
        let mut huge = header(0, 2, 100);
        huge.frame_len = MAX_FRAME_BYTES + 1;
        r.push(huge, vec![0; 25], 0, &mut |a| got.push(a));
        assert!(got.is_empty());
    }

    #[test]
    fn sequencia_vira_de_u32_sem_travar() {
        let mut r = Reassembler::new();
        let mut s = Splitter::new();
        let mut seqs = Vec::new();
        for seq in [u32::MAX - 1, u32::MAX, 0, 1] {
            for a in feed(&mut r, &mut s, seq, 30, &[0, 1, 2, 4], 0) {
                if let Assembled::Frame(h, _) = a {
                    seqs.push(h.seq);
                }
            }
        }
        assert_eq!(seqs, vec![u32::MAX - 1, u32::MAX, 0, 1]);
    }

    #[test]
    fn buffers_das_faixas_sao_reaproveitados() {
        let mut r = Reassembler::new();
        let mut s = Splitter::new();
        for seq in 0..100 {
            feed(&mut r, &mut s, seq, 1_000, &[0, 1, 2, 3, 4], 0);
        }
        assert!(r.pool.len() <= WINDOW * STRIPES);
        let b = r.buffer();
        assert!(
            b.is_empty() && b.capacity() >= 250,
            "o buffer devolvido vem limpo e com espaço"
        );
    }
}
