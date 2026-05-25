use std::io::Write;
use tempfile::NamedTempFile;

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Resource {
    String(String),
    File { content: String },
}

impl From<&str> for Resource {
    fn from(s: &str) -> Self {
        Resource::String(s.to_string())
    }
}

impl From<String> for Resource {
    fn from(s: String) -> Self {
        Resource::String(s)
    }
}

pub(crate) struct ResolvedResource {
    pub value: String,
    pub handle: Option<NamedTempFile>,
}

impl Resource {
    pub(crate) fn resolve(&self) -> Result<ResolvedResource, std::io::Error> {
        match self {
            Resource::String(s) => Ok(ResolvedResource {
                value: s.clone(),
                handle: None,
            }),
            Resource::File { content } => {
                let mut file = NamedTempFile::new()?;
                file.write_all(content.as_bytes())?;
                file.flush()?;
                let path = file.path().to_string_lossy().into_owned();
                Ok(ResolvedResource {
                    value: path,
                    handle: Some(file),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_creates_string_variant() {
        let r: Resource = "--verbose".into();
        assert_eq!(r, Resource::String("--verbose".into()));
    }

    #[test]
    fn from_owned_string_creates_string_variant() {
        let r: Resource = String::from("hello").into();
        assert_eq!(r, Resource::String("hello".into()));
    }

    #[test]
    fn string_resolves_to_itself() {
        let r = Resource::String("value".into());
        let resolved = r.resolve().unwrap();
        assert_eq!(resolved.value, "value");
        assert!(resolved.handle.is_none());
    }

    #[test]
    fn file_resolves_to_a_readable_path() {
        let r = Resource::File {
            content: "hello from file".into(),
        };
        let resolved = r.resolve().unwrap();
        assert!(
            std::path::Path::new(&resolved.value).exists(),
            "resolved path should exist on disk"
        );
        assert_eq!(
            std::fs::read_to_string(&resolved.value).unwrap(),
            "hello from file"
        );
        assert!(resolved.handle.is_some());
    }

    #[test]
    fn file_is_cleaned_up_when_handle_drops() {
        let r = Resource::File {
            content: "ephemeral".into(),
        };
        let resolved = r.resolve().unwrap();
        let path = resolved.value.clone();
        assert!(std::path::Path::new(&path).exists());
        drop(resolved.handle);
        assert!(
            !std::path::Path::new(&path).exists(),
            "file should be deleted after handle is dropped"
        );
    }
}
