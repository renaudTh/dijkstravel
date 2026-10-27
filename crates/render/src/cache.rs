//! Cache borné partagé entre threads : la valeur la moins récemment lue part en premier.

use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, Mutex, OnceLock};

pub struct Lru<K, V> {
    capacity: usize,
    state: Mutex<State<K, V>>,
}

struct State<K, V> {
    tick: u64,
    entries: HashMap<K, (Arc<OnceLock<V>>, u64)>,
}

impl<K: Hash + Eq + Clone, V: Clone> Lru<K, V> {
    pub fn new(capacity: usize) -> Self {
        Lru { capacity, state: Mutex::new(State { tick: 0, entries: HashMap::new() }) }
    }

    /// Valeur en cache, sinon calculée une seule fois hors du verrou : un thread qui demande la même
    /// clé pendant le calcul attend son résultat au lieu de le refaire.
    pub fn get_or_insert_with(&self, key: &K, compute: impl FnOnce() -> V) -> V {
        let cell = {
            let mut state = self.state.lock().unwrap();
            state.tick += 1;
            let tick = state.tick;
            let cell = match state.entries.get_mut(key) {
                Some((cell, at)) => {
                    *at = tick;
                    cell.clone()
                }
                None => {
                    let cell = Arc::new(OnceLock::new());
                    state.entries.insert(key.clone(), (cell.clone(), tick));
                    cell
                }
            };
            if state.entries.len() > self.capacity {
                // Éviction par lot : un parcours complet pour un dixième de la capacité libérée.
                let mut ticks: Vec<u64> = state.entries.values().map(|(_, at)| *at).collect();
                let cut = (ticks.len() - self.capacity + self.capacity / 10).min(ticks.len() - 1);
                let (_, &mut limit, _) = ticks.select_nth_unstable(cut);
                state.entries.retain(|_, (_, at)| *at > limit);
            }
            cell
        };
        cell.get_or_init(compute).clone()
    }

    pub fn clear(&self) {
        self.state.lock().unwrap().entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn keeps_recent_values_within_capacity() {
        let cache = Lru::new(10);
        for i in 0..100 {
            cache.get_or_insert_with(&i, || i * 2);
            assert_eq!(cache.get_or_insert_with(&0, || unreachable!()), 0, "la valeur relue reste");
        }
        assert_eq!(cache.get_or_insert_with(&99, || unreachable!()), 198);
        assert!(cache.state.lock().unwrap().entries.len() <= 10);
    }

    #[test]
    fn computes_each_key_once_across_threads() {
        let cache = Lru::new(10);
        let calls = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    cache.get_or_insert_with(&1, || {
                        calls.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        1
                    })
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
