use crate::memory::MarkdownMemory;
use crate::traits::Memory;
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

pub fn create_memory(workspace_dir: &Path) -> Result<Arc<dyn Memory>> {
    Ok(Arc::new(MarkdownMemory::new(workspace_dir)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn factory_markdown() {
        let tmp = TempDir::new().unwrap();
        let mem = create_memory(tmp.path()).unwrap();
        assert_eq!(mem.name(), "markdown");
    }
}
