//! Emulated system clipboard owns transferred guest global-memory blocks.
//! These are ordinary guest allocations, not host clipboard pointers.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Clipboard {
    pub opened_by: Option<usize>,
    pub owner: u32,
    pub formats: std::collections::HashMap<u32, u32>,
}
impl Clipboard {
    pub fn validate(&self, heap: &crate::HeapSnapshot) -> Result<(), String> {
        let mut owned = std::collections::HashSet::new();
        for (&format, &pointer) in &self.formats {
            if format == 0
                || pointer == 0
                || !owned.insert(pointer)
                || !heap.owns_global_block(pointer)
            {
                return Err("checkpoint clipboard has an invalid global-memory owner".into());
            }
        }
        Ok(())
    }
}
