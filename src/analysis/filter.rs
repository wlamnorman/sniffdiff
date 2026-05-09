use std::path::{Component, Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileFilter {
    exclude_tests: bool,
}

impl Default for FileFilter {
    fn default() -> Self {
        Self {
            exclude_tests: true,
        }
    }
}

impl FileFilter {
    pub(crate) fn should_skip(&self, path: &Path) -> bool {
        self.exclude_tests && is_test_path(path)
    }
}

pub(crate) fn is_test_path(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            Component::Normal(name) if name == "tests" || name == "test"
        )
    }) || path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.ends_with(".py") && (name.starts_with("test_") || name.ends_with("_test.py"))
        })
}
