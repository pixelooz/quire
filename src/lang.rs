use std::path::Path;

#[derive(Debug)]
pub struct LanguageMeta {
    pub extensions: &'static [&'static str],
    pub name: &'static str,
    pub indent: Indent,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    #[default]
    PlainText,
    Rust,
    Go,
    Clang,
    Java,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indent {
    FourSpaces(&'static str),
    Tab,
}

impl Language {
    /// Returns the language's name, extensions, and indentation settings.
    pub fn metadata(self) -> LanguageMeta {
        LanguageMeta {
            extensions: self.extensions(),
            name: self.name(),
            indent: self.indent(),
        }
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Language::PlainText => &[],
            Language::Clang => &["c", "h"],
            Language::Rust => &["rs"],
            Language::Java => &["java"],
            Language::Go => &["go"],
        }
    }

    pub fn indent(self) -> Indent {
        match self {
            Language::Go => Indent::Tab,
            _ => Indent::FourSpaces("    "),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Language::PlainText => "plain",
            Language::Rust => "rust",
            Language::Go => "go",
            Language::Clang => "clang",
            Language::Java => "java",
        }
    }

    /// Detects a language from the file extension.
    ///
    /// Returns [`Language::PlainText`] when no matching extension is found.
    pub fn detect<P: AsRef<Path>>(path: P) -> Self {
        use Language::*;

        if let Some(ext) = path.as_ref().extension().and_then(|x| x.to_str()) {
            for lang in [PlainText, Clang, Rust, Java, Go] {
                if lang.extensions().contains(&ext) {
                    return lang;
                }
            }
        }
        PlainText
    }
}
