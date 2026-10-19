//! Buffer de jitter do áudio: segura alguns pacotes para absorver a variação de chegada,
//! devolve em ordem e decide quando um pacote está perdido (vira PLC no decoder).

use std::collections::BTreeMap;

/// O que tocar a seguir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    Packet(Vec<u8>),
    /// O pacote esperado não chegou a tempo: o decoder disfarça a falta.
    Lost,
}

#[derive(Debug)]
pub struct JitterBuffer {
    /// Quantos pacotes segurar antes de começar a tocar (cada um tem 20 ms).
    target: usize,
    /// Acima disso o buffer está atrasado demais: descarta os mais antigos para voltar ao alvo.
    max: usize,
    packets: BTreeMap<u32, Vec<u8>>,
    next_seq: Option<u32>,
    playing: bool,
}

impl JitterBuffer {
    /// `target_packets` × 20 ms é o atraso extra aceito (ex.: 3 = 60 ms).
    pub fn new(target_packets: usize) -> Self {
        let target = target_packets.max(1);
        Self {
            target,
            max: target * 4,
            packets: BTreeMap::new(),
            next_seq: None,
            playing: false,
        }
    }

    pub fn push(&mut self, seq: u32, packet: Vec<u8>) {
        // Pacote atrasado demais (já tocamos ou demos como perdido): não serve mais.
        if self.next_seq.is_some_and(|next| seq < next) {
            return;
        }
        self.packets.insert(seq, packet);
        if self.packets.len() > self.max
            && let Some(&newest) = self.packets.keys().next_back()
        {
            // Pula para perto do mais novo, mantendo `target` pacotes de folga.
            let keep_from = newest + 1 - self.target as u32;
            self.packets.retain(|&s, _| s >= keep_from);
            self.next_seq = Some(keep_from);
        }
    }

    /// Próximo pacote a tocar, ou `None` se ainda é cedo (enchendo o buffer ou sem dados).
    pub fn pop(&mut self) -> Option<Next> {
        if !self.playing {
            if self.packets.len() < self.target {
                return None;
            }
            self.playing = true;
            self.next_seq = self.packets.keys().next().copied();
        }
        let next = self.next_seq?;
        if let Some(packet) = self.packets.remove(&next) {
            self.next_seq = Some(next + 1);
            return Some(Next::Packet(packet));
        }
        if self.packets.is_empty() {
            // Acabou o que tinha (o outro lado parou ou a rede travou): volta a encher antes de tocar.
            self.playing = false;
            self.next_seq = None;
            return None;
        }
        // Tem pacotes depois do que falta: ele está perdido.
        self.next_seq = Some(next + 1);
        Some(Next::Lost)
    }

    pub fn len(&self) -> usize {
        self.packets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(seq: u32) -> Vec<u8> {
        vec![seq as u8]
    }

    #[test]
    fn espera_encher_antes_de_tocar() {
        let mut j = JitterBuffer::new(3);
        j.push(0, p(0));
        j.push(1, p(1));
        assert_eq!(j.pop(), None);
        j.push(2, p(2));
        assert_eq!(j.pop(), Some(Next::Packet(p(0))));
    }

    #[test]
    fn reordena_pacotes_que_chegam_fora_de_ordem() {
        let mut j = JitterBuffer::new(3);
        for s in [2, 0, 1] {
            j.push(s, p(s));
        }
        let out: Vec<_> = std::iter::from_fn(|| j.pop()).collect();
        assert_eq!(out, vec![Next::Packet(p(0)), Next::Packet(p(1)), Next::Packet(p(2))]);
    }

    #[test]
    fn pacote_que_nao_chegou_vira_perda_e_o_seguinte_toca() {
        let mut j = JitterBuffer::new(2);
        for s in [0, 2, 3] {
            j.push(s, p(s));
        }
        assert_eq!(j.pop(), Some(Next::Packet(p(0))));
        assert_eq!(j.pop(), Some(Next::Lost));
        assert_eq!(j.pop(), Some(Next::Packet(p(2))));
    }

    #[test]
    fn pacote_atrasado_que_chega_depois_da_vez_e_ignorado() {
        let mut j = JitterBuffer::new(1);
        j.push(0, p(0));
        j.push(2, p(2));
        j.pop();
        assert_eq!(j.pop(), Some(Next::Lost));
        j.push(1, p(1));
        assert_eq!(j.pop(), Some(Next::Packet(p(2))));
        assert!(j.is_empty());
    }

    #[test]
    fn quando_acaba_volta_a_encher() {
        let mut j = JitterBuffer::new(2);
        j.push(0, p(0));
        j.push(1, p(1));
        j.pop();
        j.pop();
        assert_eq!(j.pop(), None);
        j.push(2, p(2));
        assert_eq!(j.pop(), None, "precisa de 2 de novo");
        j.push(3, p(3));
        assert_eq!(j.pop(), Some(Next::Packet(p(2))));
    }

    #[test]
    fn atraso_acumulado_e_cortado_para_o_alvo() {
        let mut j = JitterBuffer::new(2);
        // 20 pacotes chegam de uma vez (a rede travou e soltou tudo).
        for s in 0..20 {
            j.push(s, p(s));
        }
        // Nunca segura mais que 4× o alvo (8 pacotes = 160 ms) e toca a partir de perto do mais novo.
        assert!(j.len() <= 8);
        match j.pop() {
            Some(Next::Packet(first)) => assert!(first[0] >= 12, "começou do {}", first[0]),
            other => panic!("{other:?}"),
        }
    }
}
