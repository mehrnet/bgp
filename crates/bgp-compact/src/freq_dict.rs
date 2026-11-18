use std::collections::HashMap;

#[derive(Debug)]
pub struct FreqDict {
    ids: HashMap<String, u32>,
    items: Vec<String>,
    freq: Vec<usize>,
}

impl Default for FreqDict {
    fn default() -> Self {
        Self::new()
    }
}

impl FreqDict {
    #[must_use]
    pub fn new() -> Self {
        let mut ids = HashMap::new();
        ids.insert(String::new(), 0);
        Self {
            ids,
            items: vec![String::new()],
            freq: vec![0],
        }
    }

    pub fn id(&mut self, s: &str) -> u32 {
        if s.is_empty() {
            return 0;
        }
        if let Some(&idx) = self.ids.get(s) {
            if let Some(slot) = self.freq.get_mut(idx as usize) {
                *slot += 1;
            }
            return idx;
        }
        let idx = self.items.len() as u32;
        self.ids.insert(s.to_owned(), idx);
        self.items.push(s.to_owned());
        self.freq.push(1);
        idx
    }

    pub fn bump(&mut self, s: &str) {
        if !s.is_empty() {
            let _ = self.id(s);
        }
    }

    #[must_use]
    pub fn lookup_id(&self, s: &str) -> u32 {
        self.ids.get(s).copied().unwrap_or(0)
    }

    #[must_use]
    pub fn compact_order(&self) -> (Vec<String>, Vec<u32>) {
        let n = self.items.len().saturating_sub(1);
        let mut order: Vec<u32> = (1..=n as u32).collect();
        order.sort_by(|&a, &b| {
            let fa = self.freq.get(a as usize).copied().unwrap_or(0);
            let fb = self.freq.get(b as usize).copied().unwrap_or(0);
            fb.cmp(&fa).then_with(|| {
                let sa = self.items.get(a as usize).map_or("", String::as_str);
                let sb = self.items.get(b as usize).map_or("", String::as_str);
                sa.cmp(sb)
            })
        });
        let mut remap = vec![0u32; self.items.len()];
        let mut list = Vec::with_capacity(n);
        for (i, &old_id) in order.iter().enumerate() {
            if let Some(slot) = remap.get_mut(old_id as usize) {
                *slot = (i + 1) as u32;
            }
            if let Some(s) = self.items.get(old_id as usize) {
                list.push(s.clone());
            }
        }
        (list, remap)
    }
}
