//! Leitura de H.264 em formato Annex B (NALs separados por `00 00 01` ou `00 00 00 01`).

/// Tipos de NAL que importam para o SquadView.
pub const NAL_IDR: u8 = 5;
pub const NAL_SPS: u8 = 7;
pub const NAL_PPS: u8 = 8;

/// Percorre os NALs de um frame Annex B, devolvendo o conteúdo de cada um (sem o start code).
pub fn nal_units(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    let starts = start_code_positions(data);
    let mut bounds: Vec<(usize, usize)> = Vec::with_capacity(starts.len());
    for (i, &(_, payload_at)) in starts.iter().enumerate() {
        let end = starts.get(i + 1).map(|&(next_code, _)| next_code).unwrap_or(data.len());
        if payload_at < end {
            bounds.push((payload_at, end));
        }
    }
    bounds.into_iter().map(move |(a, b)| &data[a..b])
}

/// Tipo do NAL (5 bits baixos do primeiro byte).
pub fn nal_type(nal: &[u8]) -> Option<u8> {
    nal.first().map(|b| b & 0x1f)
}

/// O frame começa um GOP novo (tem um slice IDR), ou seja, dá para começar a decodificar por ele.
pub fn is_keyframe(data: &[u8]) -> bool {
    nal_units(data).any(|n| nal_type(n) == Some(NAL_IDR))
}

/// O frame já traz SPS e PPS (necessários para quem entra no meio do stream).
pub fn has_parameter_sets(data: &[u8]) -> bool {
    let mut sps = false;
    let mut pps = false;
    for n in nal_units(data) {
        match nal_type(n) {
            Some(NAL_SPS) => sps = true,
            Some(NAL_PPS) => pps = true,
            _ => {}
        }
    }
    sps && pps
}

/// Posições (início do start code, início do NAL) de cada start code.
fn start_code_positions(data: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            let code_at = if i > 0 && data[i - 1] == 0 { i - 1 } else { i };
            out.push((code_at, i + 3));
            i += 3;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // SPS (0x67), PPS (0x68), IDR (0x65) com start codes de 4 e 3 bytes.
    const KEYFRAME: &[u8] = &[
        0, 0, 0, 1, 0x67, 0x42, 0x00, 0, 0, 0, 1, 0x68, 0xce, 0, 0, 1, 0x65, 0x88, 0x84,
    ];
    // Slice não-IDR (0x41).
    const DELTA: &[u8] = &[0, 0, 0, 1, 0x41, 0x9a, 0x02];

    #[test]
    fn separa_os_nals_com_start_codes_de_3_e_4_bytes() {
        let nals: Vec<&[u8]> = nal_units(KEYFRAME).collect();
        assert_eq!(
            nals,
            vec![&[0x67, 0x42, 0x00][..], &[0x68, 0xce][..], &[0x65, 0x88, 0x84][..]]
        );
    }

    #[test]
    fn reconhece_keyframe_e_frame_comum() {
        assert!(is_keyframe(KEYFRAME));
        assert!(!is_keyframe(DELTA));
    }

    #[test]
    fn sabe_se_o_frame_traz_sps_e_pps() {
        assert!(has_parameter_sets(KEYFRAME));
        assert!(!has_parameter_sets(DELTA));
    }

    #[test]
    fn dados_vazios_ou_sem_start_code_nao_quebram() {
        assert_eq!(nal_units(&[]).count(), 0);
        assert_eq!(nal_units(&[1, 2, 3, 4]).count(), 0);
        assert!(!is_keyframe(&[0, 0, 1]));
        assert_eq!(nal_type(&[]), None);
    }
}
