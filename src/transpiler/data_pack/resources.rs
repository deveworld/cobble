use super::DataPack;

impl DataPack {
    pub fn add_tag(&mut self, tag_name: String, functions: Vec<String>) {
        self.tags.insert(tag_name, functions);
    }

    pub fn add_advancement(&mut self, name: String, json: String) {
        self.advancements.insert(name, json);
    }

    pub fn add_loot_table(&mut self, name: String, json: String) {
        self.loot_tables.insert(name, json);
    }

    pub fn add_recipe(&mut self, name: String, json: String) {
        self.recipes.insert(name, json);
    }

    pub fn add_predicate(&mut self, name: String, json: String) {
        self.predicates.insert(name, json);
    }

    pub fn add_item_modifier(&mut self, name: String, json: String) {
        self.item_modifiers.insert(name, json);
    }
}
