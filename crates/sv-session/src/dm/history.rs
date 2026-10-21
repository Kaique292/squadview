//! Histórico das conversas, só neste PC (ADR 0017): um arquivo JSON por amigo em `conversas/`.
//!
//! Conversas são pequenas: o arquivo é lido inteiro, mudado e regravado (num arquivo temporário
//! e depois trocado, para um app fechando no meio não deixar a conversa corrompida).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::surface::lock;

/// Mensagens guardadas por conversa (as mais antigas saem).
pub const MAX_MESSAGES: usize = 2000;

/// Em que pé está a mensagem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Nossa, esperando os dois estarem online.
    Pending,
    /// Nossa, chegou no PC do amigo.
    Delivered,
    /// Do amigo, recebida.
    Received,
}

/// Um anexo: o arquivo fica no disco, a conversa só aponta para ele.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub name: String,
    pub size: u64,
    pub mime: String,
    /// Onde o arquivo está neste PC: o original (enviado sem cópia), a cópia guardada pelo app, ou
    /// onde foi baixado. `None` enquanto quem recebe não baixou.
    pub path: Option<String>,
    /// Quanto já foi baixado (ou enviado), em bytes.
    pub done: u64,
    /// O arquivo não existe mais no PC de quem mandou (apagou ou moveu): não dá para baixar.
    #[serde(default)]
    pub gone: bool,
}

/// Uma mensagem da conversa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DmMessage {
    pub id: String,
    /// Nós mandamos (`false`: o amigo).
    pub mine: bool,
    /// Quando foi escrita (ms desde 1970, no relógio de quem mandou).
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<Attachment>,
    pub status: Status,
}

/// Resumo de uma conversa, para a lista da lateral.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub friend: String,
    pub last: Option<DmMessage>,
    /// Mensagens nossas esperando o amigo ficar online.
    pub pending: usize,
}

/// As conversas guardadas numa pasta, com as já lidas em memória.
#[derive(Debug)]
pub struct History {
    dir: PathBuf,
    cache: Mutex<HashMap<String, Vec<DmMessage>>>,
}

/// Um id de amigo vira nome de arquivo (só dígitos do Discord; o resto é recusado).
fn file_name(friend: &str) -> Option<String> {
    (!friend.is_empty() && friend.len() <= 32 && friend.bytes().all(|b| b.is_ascii_alphanumeric()))
        .then(|| format!("{friend}.json"))
}

impl History {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// As mensagens com o amigo, da mais antiga para a mais nova.
    pub fn messages(&self, friend: &str) -> Vec<DmMessage> {
        self.with(friend, |list| list.clone())
    }

    /// Guarda uma mensagem nova; repetida (o mesmo `id` chegando de novo) não entra. `true` se entrou.
    pub fn push(&self, friend: &str, message: DmMessage) -> bool {
        self.change(friend, |list| {
            if list.iter().any(|m| m.id == message.id) {
                return false;
            }
            list.push(message);
            let extra = list.len().saturating_sub(MAX_MESSAGES);
            list.drain(..extra);
            true
        })
    }

    /// Muda uma mensagem (status, progresso do anexo). Devolve como ficou, se ela existe.
    pub fn update(&self, friend: &str, id: &str, change: impl FnOnce(&mut DmMessage)) -> Option<DmMessage> {
        self.change(friend, |list| {
            let m = list.iter_mut().find(|m| m.id == id)?;
            change(m);
            Some(m.clone())
        })
    }

    /// Mensagens nossas que ainda não chegaram no amigo.
    pub fn pending(&self, friend: &str) -> Vec<DmMessage> {
        self.with(friend, |list| {
            list.iter()
                .filter(|m| m.mine && m.status == Status::Pending)
                .cloned()
                .collect()
        })
    }

    /// Todas as conversas com mensagens, a mais recente primeiro.
    pub fn conversations(&self) -> Vec<Conversation> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut list: Vec<Conversation> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str()?.strip_suffix(".json").map(str::to_owned))
            .filter(|friend| file_name(friend).is_some())
            .map(|friend| {
                let messages = self.messages(&friend);
                Conversation {
                    pending: messages
                        .iter()
                        .filter(|m| m.mine && m.status == Status::Pending)
                        .count(),
                    last: messages.last().cloned(),
                    friend,
                }
            })
            .filter(|c| c.last.is_some())
            .collect();
        list.sort_by_key(|c| std::cmp::Reverse(c.last.as_ref().map_or(0, |m| m.at)));
        list
    }

    /// Apaga o histórico de uma conversa (ou de todas, com `None`). Os anexos baixados ficam onde a
    /// pessoa salvou; quem apaga a pasta de anexos é a limpeza de cache.
    pub fn clear(&self, friend: Option<&str>) {
        let mut cache = lock(&self.cache);
        match friend {
            Some(f) => {
                cache.remove(f);
                if let Some(name) = file_name(f) {
                    let _ = std::fs::remove_file(self.dir.join(name));
                }
            }
            None => {
                cache.clear();
                let _ = std::fs::remove_dir_all(&self.dir);
            }
        }
    }

    fn with<T>(&self, friend: &str, read: impl FnOnce(&Vec<DmMessage>) -> T) -> T {
        let mut cache = lock(&self.cache);
        let list = cache.entry(friend.to_owned()).or_insert_with(|| self.load(friend));
        read(list)
    }

    fn change<T>(&self, friend: &str, change: impl FnOnce(&mut Vec<DmMessage>) -> T) -> T {
        let mut cache = lock(&self.cache);
        let list = cache.entry(friend.to_owned()).or_insert_with(|| self.load(friend));
        let out = change(list);
        self.save(friend, list);
        out
    }

    fn load(&self, friend: &str) -> Vec<DmMessage> {
        file_name(friend)
            .and_then(|name| std::fs::read(self.dir.join(name)).ok())
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default()
    }

    fn save(&self, friend: &str, list: &[DmMessage]) {
        let Some(name) = file_name(friend) else { return };
        let Ok(json) = serde_json::to_vec(list) else { return };
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let path = self.dir.join(&name);
        let temp = self.dir.join(format!("{name}.tmp"));
        if std::fs::write(&temp, json).is_ok() {
            let _ = std::fs::rename(&temp, &path);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("squadview-dm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn text(id: &str, mine: bool, at: u64, status: Status) -> DmMessage {
        DmMessage {
            id: id.into(),
            mine,
            at,
            text: Some(format!("msg {id}")),
            file: None,
            status,
        }
    }

    const ANA: &str = "111111111111111111";
    const BETO: &str = "222222222222222222";

    #[test]
    fn guarda_no_disco_sem_repetir_e_le_de_novo() {
        let dir = temp_dir("guarda");
        let h = History::new(&dir);
        assert!(h.push(ANA, text("1", true, 10, Status::Pending)));
        assert!(
            !h.push(ANA, text("1", true, 10, Status::Pending)),
            "mesmo id não duplica"
        );
        assert!(h.push(ANA, text("2", false, 20, Status::Received)));
        // Outro "app" lendo a mesma pasta vê o mesmo.
        let outra = History::new(&dir);
        assert_eq!(
            outra.messages(ANA).iter().map(|m| &m.id[..]).collect::<Vec<_>>(),
            ["1", "2"]
        );
        assert!(!dir.join(format!("{ANA}.json.tmp")).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn muda_status_e_lista_pendentes_e_conversas() {
        let dir = temp_dir("status");
        let h = History::new(&dir);
        h.push(ANA, text("1", true, 10, Status::Pending));
        h.push(ANA, text("2", true, 11, Status::Pending));
        h.push(BETO, text("3", false, 50, Status::Received));
        assert_eq!(h.pending(ANA).len(), 2);
        let entregue = h.update(ANA, "1", |m| m.status = Status::Delivered).unwrap();
        assert_eq!(entregue.status, Status::Delivered);
        assert!(h.update(ANA, "nao-existe", |_| {}).is_none());
        assert_eq!(h.pending(ANA).len(), 1);
        let conversas = h.conversations();
        assert_eq!(conversas.iter().map(|c| &c.friend[..]).collect::<Vec<_>>(), [BETO, ANA]);
        assert_eq!(conversas[1].pending, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn guarda_no_maximo_as_ultimas_mensagens() {
        let dir = temp_dir("limite");
        let h = History::new(&dir);
        for i in 0..MAX_MESSAGES + 5 {
            h.push(ANA, text(&i.to_string(), true, i as u64, Status::Delivered));
        }
        let list = h.messages(ANA);
        assert_eq!(list.len(), MAX_MESSAGES);
        assert_eq!(list[0].id, "5");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apaga_uma_conversa_ou_todas_e_recusa_nome_estranho() {
        let dir = temp_dir("apaga");
        let h = History::new(&dir);
        h.push(ANA, text("1", true, 1, Status::Pending));
        h.push(BETO, text("2", true, 2, Status::Pending));
        h.clear(Some(ANA));
        assert!(h.messages(ANA).is_empty());
        assert_eq!(h.conversations().len(), 1);
        h.clear(None);
        assert!(h.conversations().is_empty());
        assert!(!dir.exists());
        // Id com caminho não vira arquivo.
        assert!(h.push("../fora", text("x", true, 1, Status::Pending)));
        assert!(!dir.join("../fora.json").exists());
        assert_eq!(file_name(""), None);
        assert_eq!(History::new("nao-existe-mesmo").conversations(), vec![]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
