#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickItem {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickList {
    pub items: Vec<PickItem>,
    pub query: String,
    pub selected: usize,
}

impl PickList {
    pub fn new(items: Vec<PickItem>) -> Self {
        Self {
            items,
            query: String::new(),
            selected: 0,
        }
    }

    pub fn filtered(&self) -> Vec<&PickItem> {
        self.items
            .iter()
            .filter(|item| subsequence(&self.query, &item.label))
            .collect()
    }

    pub fn move_selection(&mut self, delta: isize) {
        let count = self.filtered().len();
        if count == 0 {
            self.selected = 0;
            return;
        }
        let next = self.selected as isize + delta;
        self.selected = next.clamp(0, count as isize - 1) as usize;
    }

    pub fn accepted(&self) -> Option<&PickItem> {
        self.filtered().get(self.selected).copied()
    }

    pub fn on_insert(&mut self, text: &str) {
        self.query.push_str(text);
        self.selected = 0;
    }

    pub fn on_backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    pub fn select_value(&mut self, value: &str) {
        if let Some(index) = self.filtered().iter().position(|item| item.value == value) {
            self.selected = index;
        }
    }
}

fn subsequence(query: &str, label: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut chars = label.chars().map(|ch| ch.to_ascii_lowercase());
    query
        .chars()
        .map(|ch| ch.to_ascii_lowercase())
        .all(|needle| chars.any(|hay| hay == needle))
}
