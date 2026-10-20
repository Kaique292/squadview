//! Cache pequeno de objetos por chave (views e amostras por textura), sem alocação por frame.

use std::collections::HashMap;
use std::hash::Hash;

/// Guarda até `limit` objetos por chave. Ao encher, esvazia tudo: no pipeline as chaves são
/// texturas reaproveitadas (poucas), então encher significa que o conjunto de texturas mudou.
#[derive(Debug)]
pub struct BoundedCache<K, V> {
    map: HashMap<K, V>,
    limit: usize,
}

impl<K: Eq + Hash, V: Clone> BoundedCache<K, V> {
    pub fn new(limit: usize) -> Self {
        Self {
            map: HashMap::new(),
            limit: limit.max(1),
        }
    }

    /// Devolve o objeto de `key`, criando com `create` se ainda não existir. Se a criação
    /// falhar, nada é guardado.
    pub fn get_or_try_insert<E>(&mut self, key: K, create: impl FnOnce() -> Result<V, E>) -> Result<V, E> {
        if let Some(value) = self.map.get(&key) {
            return Ok(value.clone());
        }
        if self.map.len() >= self.limit {
            self.map.clear();
        }
        let value = create()?;
        self.map.insert(key, value.clone());
        Ok(value)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cria_uma_vez_e_reaproveita() {
        let mut cache = BoundedCache::new(4);
        let mut criados = 0;
        for _ in 0..3 {
            let v: Result<_, ()> = cache.get_or_try_insert(7, || {
                criados += 1;
                Ok("view")
            });
            assert_eq!(v, Ok("view"));
        }
        assert_eq!(criados, 1);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn ao_encher_esvazia_e_recomeca() {
        let mut cache = BoundedCache::new(2);
        for k in 0..2 {
            cache.get_or_try_insert(k, || Ok::<_, ()>(k)).unwrap();
        }
        assert_eq!(cache.len(), 2);
        cache.get_or_try_insert(9, || Ok::<_, ()>(9)).unwrap();
        assert_eq!(cache.len(), 1, "texturas novas: o cache recomeçou só com a nova");
        // A chave antiga precisa ser criada de novo.
        let mut recriou = false;
        cache
            .get_or_try_insert(0, || {
                recriou = true;
                Ok::<_, ()>(0)
            })
            .unwrap();
        assert!(recriou);
    }

    #[test]
    fn erro_na_criacao_nao_guarda_nada() {
        let mut cache: BoundedCache<u32, u32> = BoundedCache::new(4);
        assert_eq!(cache.get_or_try_insert(1, || Err("falhou")), Err("falhou"));
        assert!(cache.is_empty());
    }

    #[test]
    fn limite_zero_vira_um() {
        let mut cache = BoundedCache::new(0);
        cache.get_or_try_insert(1, || Ok::<_, ()>(1)).unwrap();
        cache.get_or_try_insert(2, || Ok::<_, ()>(2)).unwrap();
        assert_eq!(cache.len(), 1);
    }
}
