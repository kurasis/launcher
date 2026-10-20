//! Maven coordinates (`group:artifact:version[:classifier][@extension]`) and the
//! repository paths they name (D-039).
//!
//! Loader profiles name libraries by coordinate, and Forge's launch arguments
//! and processors find files by their place in a Maven tree. Each part is checked
//! strictly, so a coordinate can only name a plain relative path. An extension may
//! have several dotted parts (`tsrg.lzma`, NeoForge 1.21.11's mappings).

use crate::error::LauncherResult;
use crate::install::descriptor::{unsupported, validate_relative_path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coordinate {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub extension: String,
}

fn valid_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 128
        && part != "."
        && part != ".."
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'))
}

impl Coordinate {
    pub fn parse(text: &str) -> LauncherResult<Self> {
        let bad = || unsupported("library coordinate");
        let (main, extension) = match text.split_once('@') {
            Some((main, extension)) => (main, extension),
            None => (text, "jar"),
        };
        let parts: Vec<&str> = main.split(':').collect();
        let (group, artifact, version, classifier) = match parts.as_slice() {
            [g, a, v] => (*g, *a, *v, None),
            [g, a, v, c] => (*g, *a, *v, Some(*c)),
            _ => return Err(bad()),
        };
        let parts_ok = group.split('.').all(valid_part)
            && valid_part(artifact)
            && valid_part(version)
            && classifier.is_none_or(valid_part)
            && extension.split('.').all(valid_part);
        if !parts_ok {
            return Err(bad());
        }
        let coordinate = Self {
            group: group.to_owned(),
            artifact: artifact.to_owned(),
            version: version.to_owned(),
            classifier: classifier.map(str::to_owned),
            extension: extension.to_owned(),
        };
        validate_relative_path(&coordinate.path())?;
        Ok(coordinate)
    }

    /// The `/`-separated path inside a Maven repository.
    pub fn path(&self) -> String {
        let classifier = self
            .classifier
            .as_ref()
            .map(|c| format!("-{c}"))
            .unwrap_or_default();
        format!(
            "{}/{}/{}/{}-{}{classifier}.{}",
            self.group.replace('.', "/"),
            self.artifact,
            self.version,
            self.artifact,
            self.version,
            self.extension
        )
    }

    /// `group:artifact[:classifier]`: two libraries with the same key are the same
    /// library in different versions; the loader's copy wins.
    pub fn key(&self) -> String {
        match &self.classifier {
            Some(classifier) => format!("{}:{}:{classifier}", self.group, self.artifact),
            None => format!("{}:{}", self.group, self.artifact),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinates_map_to_maven_paths() {
        let c = Coordinate::parse("net.fabricmc:sponge-mixin:0.15.5+mixin.0.8.7").unwrap();
        assert_eq!(
            c.path(),
            "net/fabricmc/sponge-mixin/0.15.5+mixin.0.8.7/sponge-mixin-0.15.5+mixin.0.8.7.jar"
        );
        assert_eq!(c.key(), "net.fabricmc:sponge-mixin");
        let c =
            Coordinate::parse("net.minecraft:client:1.21.1-20240808.144430:mappings@txt").unwrap();
        assert_eq!(
            c.path(),
            "net/minecraft/client/1.21.1-20240808.144430/client-1.21.1-20240808.144430-mappings.txt"
        );
        assert_eq!(c.key(), "net.minecraft:client:mappings");
        let c =
            Coordinate::parse("de.oceanlabs.mcp:mcp_config:1.12.2-20200226.224830@zip").unwrap();
        assert!(c.path().ends_with("mcp_config-1.12.2-20200226.224830.zip"));
        let c =
            Coordinate::parse("net.neoforged:neoform:1.21.11-20251209.172050:mappings@tsrg.lzma")
                .unwrap();
        assert!(
            c.path()
                .ends_with("neoform-1.21.11-20251209.172050-mappings.tsrg.lzma")
        );
    }

    #[test]
    fn coordinates_cannot_escape() {
        for bad in [
            "",
            "a:b",
            "a:b:c:d:e",
            "..:b:c",
            "a:..:c",
            "a:b:..",
            "a/b:c:d",
            "a:b:c@../x",
            "a:b:c@.gz",
            "a:b:c@tar.",
            "a:b:c@tar..gz",
            "a:b:c@tar/gz",
            "a..b:c:d",
            "a:b c:d",
            "a:b:c:\\x",
        ] {
            assert!(Coordinate::parse(bad).is_err(), "{bad}");
        }
    }
}
