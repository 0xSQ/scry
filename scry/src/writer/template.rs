//! Documented configuration scaffolds generated from type descriptions.

use crate::desc::{DescKind, FieldDesc, VariantRepr};
use crate::node::{Format, Kind, Value};
use crate::{Desc, Describe, KeyPath, Node, NodeError, ToNode};

use super::document::{Annotations, Document};
use super::literal::template_key;
use super::RhaiWriter;

// ---------------------------------------------------------------------------------------------- //

/// Controls the coverage and formatting of a configuration template.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TemplateConfig {
    /// The fields to include and whether their defaults are active.
    pub mode: TemplateMode,
    /// The spacing between documented fields.
    pub spacing: TemplateSpacing,
    /// The placement of field type hints.
    pub type_hints: TemplateTypeHints,
}

/// Selects the coverage and default presentation of a configuration template.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TemplateMode {
    /// Shows required fields and their enclosing objects.
    #[default]
    Min,
    /// Shows the interface with commented defaults and optional structures.
    Ref,
    /// Writes known defaults as active values without enabling absent features.
    Explicit,
}

/// Selects the spacing between configuration fields.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TemplateSpacing {
    /// Separates fields and their documentation with a blank line.
    #[default]
    Blank,
    /// Places fields and their documentation on consecutive lines.
    Compact,
}

/// Selects where configuration field type hints appear.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TemplateTypeHints {
    /// Appends the type to the documentation above each field.
    #[default]
    Above,
    /// Appends the type to the entry unless its placeholder already names the type.
    Inline,
    /// Omits type annotations while retaining typed placeholders.
    None,
}

/// Generates a documented configuration scaffold without constructing a complete value.
///
/// Unknown values remain commented placeholders. The output has valid language syntax but may
/// require editing before it decodes or runs. Only described root objects are supported.
/// Fields are separated by blank lines, with type hints appended to their documentation.
pub fn format_template<T: Describe>(
    format: Format,
    mode: TemplateMode,
) -> Result<String, NodeError> {
    format_template_with_config::<T>(
        format,
        &TemplateConfig {
            mode,
            ..Default::default()
        },
    )
}

/// Generates a configuration scaffold with explicit coverage and formatting options.
///
/// Like [`format_template`], this requires a described root object and leaves unknown values
/// as commented placeholders. Formatting options do not change the generated values.
pub fn format_template_with_config<T: Describe>(
    format: Format,
    config: &TemplateConfig,
) -> Result<String, NodeError> {
    // Validate the format even when the description contains no literal values or keys.
    template_key("", format.clone())?;
    let description = T::describe();
    let DescKind::Struct { fields } = &description.kind else {
        return Err(NodeError::new(
            "Configuration templates currently require a described root object.",
        ));
    };
    let generator = Generator { format, config };
    let mut lines = comments(&description.doc);
    lines.extend(comments("Fill required placeholders and uncomment their entries."));
    if config.mode == TemplateMode::Ref {
        lines.extend(comments("Commented optional entries may be enabled when needed."));
    }
    lines.extend(generator.object(fields, 0, &KeyPath::new())?);
    Ok(format!("{}\n", lines.join("\n")))
}

struct Generator<'a> {
    format: Format,
    config: &'a TemplateConfig,
}

impl Generator<'_> {
    const MAX_DEPTH: usize = 8;

    fn object(
        &self,
        fields: &[FieldDesc],
        depth: usize,
        path: &KeyPath,
    ) -> Result<Vec<String>, NodeError> {
        let opening = if self.format == Format::Rhai {
            "#{"
        } else {
            "{"
        };
        let mut lines = vec![opening.to_owned()];
        for field in fields {
            let block = self.field(field, depth + 1, &path.push_key(&field.name))?;
            if block.is_empty() {
                continue;
            }
            if self.config.spacing == TemplateSpacing::Blank && lines.len() > 1 {
                lines.push(String::new());
            }
            lines.extend(indent(block));
        }
        if lines.len() == 1 {
            lines[0].push('}');
        } else {
            lines.push("}".to_owned());
        }
        Ok(lines)
    }

    fn field(
        &self,
        field: &FieldDesc,
        depth: usize,
        path: &KeyPath,
    ) -> Result<Vec<String>, NodeError> {
        if self.config.mode == TemplateMode::Min && field.is_optional() {
            return Ok(Vec::new());
        }
        let default = if field.is_optional() {
            field.template_default().map_err(|error| error.prepend_path(path))?
        } else {
            None
        };
        let mut note = None;
        let (value, active) = match default {
            Some(node) => {
                let absent =
                    matches!(&node.kind, Kind::Leaf(leaf) if matches!(leaf.value, Value::Null));
                if absent && self.config.mode == TemplateMode::Ref {
                    note = Some("Omitted by default.");
                    (self.shape(&field.value, depth, path)?, false)
                } else {
                    let value = if matches!(&node.kind, Kind::Vec(items) if items.is_empty()) {
                        match &field.value.kind {
                            DescKind::List { item } => self.sequence_shape(item, depth, path)?,
                            _ => self
                                .literal(node, &field.value)
                                .map_err(|error| error.prepend_path(path))?,
                        }
                    } else {
                        self.literal(node, &field.value)
                            .map_err(|error| error.prepend_path(path))?
                    };
                    (value, self.config.mode == TemplateMode::Explicit)
                }
            }
            None => {
                if field.is_optional() {
                    note = Some("Default value unavailable to the template generator.");
                }
                let object =
                    matches!(field.value.kind, DescKind::Struct { .. }) && depth < Self::MAX_DEPTH;
                (self.shape(&field.value, depth, path)?, !field.is_optional() && object)
            }
        };

        let mut lines = comments(&self.field_documentation(field, note)?);
        if depth >= Self::MAX_DEPTH && value[0] == "<value>" {
            lines.extend(comments("Shape expansion stopped at the depth limit."));
        }
        let mut entry = value;
        let key = template_key(&field.name, self.format.clone())?;
        let hint = if self.config.type_hints == TemplateTypeHints::Inline {
            inline_type(&field.value, &entry[0])
        } else {
            None
        };
        entry[0] = format!("{key}: {}", entry[0]);
        entry.last_mut().expect("a value contains at least one line").push(',');
        if let Some(hint) = hint {
            entry[0].push_str(&format!(" // type: {hint}"));
        }
        lines.extend(if active { entry } else { comment_out(entry) });
        Ok(lines)
    }

    fn shape(&self, desc: &Desc, depth: usize, path: &KeyPath) -> Result<Vec<String>, NodeError> {
        // Description construction is eager. This bounds expansion of an already built shape.
        if depth >= Self::MAX_DEPTH {
            return Ok(vec!["<value>".to_owned()]);
        }
        match &desc.kind {
            DescKind::Struct { fields } => self.object(fields, depth, path),
            DescKind::List { item } => self.sequence_shape(item, depth, path),
            DescKind::Tuple { items } => {
                let mut lines = vec!["[".to_owned()];
                for (index, item) in items.iter().enumerate() {
                    if depth + 1 >= Self::MAX_DEPTH {
                        lines.extend(indent(comments(
                            "Shape expansion stopped at the depth limit.",
                        )));
                    }
                    let mut item_lines = self.shape(item, depth + 1, &path.push_index(index))?;
                    item_lines.last_mut().expect("a shape is nonempty").push(',');
                    lines.extend(indent(item_lines));
                }
                lines.push("]".to_owned());
                Ok(lines)
            }
            DescKind::Plain { hint } => {
                let hint = hint.as_deref().unwrap_or("value");
                let placeholder = if matches!(
                    hint,
                    "string"
                        | "path"
                        | "bool"
                        | "f32"
                        | "f64"
                        | "i8"
                        | "i16"
                        | "i32"
                        | "i64"
                        | "isize"
                        | "u8"
                        | "u16"
                        | "u32"
                        | "u64"
                        | "usize"
                ) {
                    format!("<{hint}>")
                } else {
                    "<value>".to_owned()
                };
                Ok(vec![placeholder])
            }
            DescKind::Enum { variants } => {
                let hint =
                    if variants.iter().all(|variant| matches!(variant.repr, VariantRepr::Unit)) {
                        "<string>"
                    } else {
                        "<value>"
                    };
                Ok(vec![hint.to_owned()])
            }
        }
    }

    fn sequence_shape(
        &self,
        item: &Desc,
        depth: usize,
        path: &KeyPath,
    ) -> Result<Vec<String>, NodeError> {
        let mut element = self.shape(item, depth + 1, &path.push_index(0))?;
        element.last_mut().expect("a shape is nonempty").push(',');
        let mut lines = vec!["[".to_owned()];
        lines.extend(indent(comments("Element shape. Fill values before enabling:")));
        if depth + 1 >= Self::MAX_DEPTH {
            lines.extend(indent(comments("Shape expansion stopped at the depth limit.")));
        }
        lines.extend(indent(comment_out(element)));
        lines.push("]".to_owned());
        Ok(lines)
    }

    fn literal(&self, node: Node, desc: &Desc) -> Result<Vec<String>, NodeError> {
        let mut document = Document::from_description(node, desc);
        self.annotate_literal(&document.node, &mut document.annotations, desc)?;
        document.header = None;
        let text = match self.format {
            Format::Rhai => {
                let mut writer = RhaiWriter::to_string_writer();
                writer.write(&document)?;
                writer.into_string()?
            }
            #[cfg(feature = "format-json5")]
            Format::Json5 => {
                let mut writer = super::Json5Writer::to_string_writer();
                writer.write(&document)?;
                writer.into_string()?
            }
            _ => unreachable!("the template format was validated"),
        };
        Ok(text.lines().map(str::to_owned).collect())
    }

    fn annotate_literal(
        &self,
        node: &Node,
        annotations: &mut Annotations,
        desc: &Desc,
    ) -> Result<(), NodeError> {
        match (&node.kind, &desc.kind) {
            (Kind::Map(entries), DescKind::Struct { fields }) => {
                for (index, (key, child)) in entries.iter().enumerate() {
                    let child_annotations = &mut annotations.children[index];
                    child_annotations.blank_before =
                        self.config.spacing == TemplateSpacing::Blank && index > 0;
                    if let Some(field) = fields.iter().find(|field| &field.name == key) {
                        child_annotations.comment = Some(self.field_documentation(field, None)?);
                        if self.config.type_hints == TemplateTypeHints::Inline {
                            child_annotations.end_comment =
                                type_hint(&field.value).map(|hint| format!("type: {hint}"));
                        }
                        self.annotate_literal(child, child_annotations, &field.value)?;
                    }
                }
            }
            (Kind::Vec(elements), DescKind::List { item }) => {
                for (child, child_annotations) in elements.iter().zip(&mut annotations.children) {
                    self.annotate_literal(child, child_annotations, item)?;
                }
            }
            (Kind::Vec(elements), DescKind::Tuple { items }) => {
                for ((child, child_annotations), desc) in
                    elements.iter().zip(&mut annotations.children).zip(items)
                {
                    self.annotate_literal(child, child_annotations, desc)?;
                }
            }
            (Kind::Map(entries), DescKind::Enum { variants }) if entries.len() == 1 => {
                let (name, child) = entries.first().expect("the map contains one entry");
                if let Some(variant) = variants.iter().find(|variant| &variant.name == name) {
                    if let VariantRepr::Payload { payload, .. } = &variant.repr {
                        self.annotate_literal(child, &mut annotations.children[0], payload)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn field_documentation(
        &self,
        field: &FieldDesc,
        note: Option<&str>,
    ) -> Result<String, NodeError> {
        let documentation = if field.doc.is_empty() {
            &field.value.doc
        } else {
            &field.doc
        };
        let mut lines = normalized_lines(documentation);
        while lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        let policy = if field.is_optional() {
            "Optional."
        } else {
            "Required."
        };
        let mut metadata = String::new();
        if self.config.type_hints == TemplateTypeHints::Above {
            if let Some(hint) = type_hint(&field.value) {
                metadata.push_str(&format!("Type: {hint}. "));
            }
        }
        metadata.push_str(policy);
        if let Some(last) = lines.last_mut() {
            *last = last.trim_end().to_owned();
            if !last.ends_with(['.', '!', '?']) {
                last.push('.');
            }
            last.push(' ');
            last.push_str(&metadata);
        } else {
            lines.push(metadata);
        }
        if let Some(note) = note {
            let last = lines.last_mut().expect("the field has an omission policy");
            last.push(' ');
            last.push_str(note);
        }
        if let Some(choices) = self.choices(&field.value)? {
            lines.push(choices);
        }
        Ok(lines.join("\n"))
    }

    fn choices(&self, desc: &Desc) -> Result<Option<String>, NodeError> {
        match &desc.kind {
            DescKind::Enum { variants } => {
                let labels = variants
                    .iter()
                    .map(|variant| variant.name.to_node()?.to_string_as(self.format.clone()))
                    .collect::<Result<Vec<_>, _>>()?;
                let label =
                    if variants.iter().all(|variant| matches!(variant.repr, VariantRepr::Unit)) {
                        "Choices"
                    } else {
                        "Alternatives"
                    };
                Ok(Some(format!("{label}: {}.", labels.join(", "))))
            }
            _ => Ok(None),
        }
    }
}

fn inline_type(desc: &Desc, first_line: &str) -> Option<String> {
    let hint = type_hint(desc)?;
    if first_line == format!("<{hint}>") {
        None
    } else {
        Some(hint)
    }
}

fn type_hint(desc: &Desc) -> Option<String> {
    match &desc.kind {
        DescKind::Plain { hint } => hint
            .as_ref()
            .map(|hint| hint.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|hint| !hint.is_empty()),
        DescKind::List { item } => type_hint(item).map(|hint| format!("list<{hint}>")),
        DescKind::Enum { .. } if desc.unit_enum_variants().is_some() => Some("string".to_owned()),
        _ => None,
    }
}

fn comments(text: &str) -> Vec<String> {
    normalized_lines(text)
        .into_iter()
        .map(|line| {
            if line.is_empty() {
                "//".to_owned()
            } else {
                format!("// {line}")
            }
        })
        .collect()
}

fn normalized_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.replace("\r\n", "\n")
        .split([
            '\n', '\r', '\u{000b}', '\u{000c}', '\u{0085}', '\u{2028}', '\u{2029}',
        ])
        .map(str::to_owned)
        .collect()
}

fn comment_out(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            let content = line.trim_start();
            if content.is_empty() || content.starts_with("//") {
                line
            } else {
                let indentation = &line[..line.len() - content.len()];
                format!("{indentation}// {content}")
            }
        })
        .collect()
}

fn indent(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            if line.is_empty() {
                line
            } else {
                format!("    {line}")
            }
        })
        .collect()
}
