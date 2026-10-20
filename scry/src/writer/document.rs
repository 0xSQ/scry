//! Language-neutral configuration values with comments and presentation hints.

use indexmap::IndexMap;

use crate::desc::{Desc, DescKind, VariantRepr};
use crate::key_path::{KeyPath, Segment, TryIntoKeyPath};
use crate::node::Kind;
use crate::{Describe, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

/// A configuration value with comments and presentation hints.
///
/// Documents generate fresh configuration text. They do not retain the source, comments, or
/// expressions of a previously parsed file. Values remain available through [`Self::node`] and
/// can be exported explicitly without annotations through [`Self::into_node`].
#[derive(Debug, Clone)]
pub struct Document {
    pub(super) node: Node,
    pub(super) annotations: Annotations,
    pub(super) header: Option<String>,
    pub(super) footer: Option<String>,
}

impl Document {
    /// Creates an unannotated document from an owned node.
    pub fn from_node(mut node: Node) -> Self {
        let path = node.path.clone();
        anchor(&mut node, &path);
        let annotations = Annotations::for_node(&node);
        Self {
            node,
            annotations,
            header: None,
            footer: None,
        }
    }

    /// Converts a value into an unannotated document.
    pub fn from_value(value: &(impl ToNode + ?Sized)) -> Result<Self, NodeError> {
        Ok(Self::from_node(value.to_node()?))
    }

    /// Converts actual settings into a document with their descriptions as comments.
    ///
    /// Present struct fields follow declaration order. Undescribed fields retain their supplied
    /// order. Missing fields and display descriptions of defaults never supply values.
    pub fn from_config<T: ToNode + Describe + ?Sized>(value: &T) -> Result<Self, NodeError> {
        Ok(Self::from_description(value.to_node()?, &T::describe()))
    }

    pub(super) fn from_description(mut node: Node, description: &Desc) -> Self {
        let path = node.path.clone();
        anchor(&mut node, &path);
        let header = (!description.doc.is_empty()).then(|| normalize_comment(&description.doc));
        let mut annotations = decorate(&mut node, description);
        // The root description belongs to the header rather than the root value's leading text.
        annotations.comment = None;
        Self {
            node,
            annotations,
            header,
            footer: None,
        }
    }

    /// Constructs an ordered map through a closure.
    pub fn map(
        build: impl FnOnce(&mut MapBuilder) -> Result<(), NodeError>,
    ) -> Result<Self, NodeError> {
        let mut map = MapBuilder::default();
        build(&mut map)?;
        Ok(map.finish())
    }

    /// Constructs a sequence through a closure.
    pub fn seq(
        build: impl FnOnce(&mut SeqBuilder) -> Result<(), NodeError>,
    ) -> Result<Self, NodeError> {
        let mut sequence = SeqBuilder::default();
        build(&mut sequence)?;
        Ok(sequence.finish())
    }

    /// Sets the document's introductory comment, replacing any type description.
    pub fn header(mut self, text: impl AsRef<str>) -> Self {
        self.header = Some(normalize_comment(text.as_ref()));
        self
    }

    /// Sets the comment following the document's root value.
    pub fn footer(mut self, text: impl AsRef<str>) -> Self {
        self.footer = Some(normalize_comment(text.as_ref()));
        self
    }

    /// Requests a compact presentation of the root container.
    pub fn compact(mut self) -> Self {
        self.annotations.layout = Layout::Compact;
        self
    }

    /// Requests an expanded presentation of the root container.
    pub fn expanded(mut self) -> Self {
        self.annotations.layout = Layout::Expanded;
        self
    }

    /// Sets a comment inside the root container after its final child.
    pub fn tail_comment(mut self, text: impl AsRef<str>) -> Result<Self, NodeError> {
        self.at_mut(KeyPath::new())?.tail_comment(text)?;
        Ok(self)
    }

    /// Returns the document's values without changing decoding visit state.
    pub fn node(&self) -> &Node {
        &self.node
    }

    /// Extracts the document's values, explicitly discarding comments and layout hints.
    pub fn into_node(self) -> Node {
        self.node
    }

    /// Returns an annotation handle at an existing configuration path.
    ///
    /// Paths use Scry's usual dotted and bracketed syntax. [`KeyPath::new`] selects the root.
    pub fn at_mut(&mut self, path: impl TryIntoKeyPath) -> Result<DocumentEntry<'_>, NodeError> {
        let path = path.try_into_key_path()?;
        let (node, annotations) = locate_mut(&self.node, &mut self.annotations, &path)?;
        Ok(DocumentEntry { node, annotations })
    }

    /// Replaces an existing value while retaining its own comments and layout hints.
    ///
    /// Descendant annotations and the replaced container's tail comment are discarded. No
    /// missing entries or intermediate containers are created.
    pub fn replace(
        &mut self,
        path: impl TryIntoKeyPath,
        value: impl ToNode,
    ) -> Result<(), NodeError> {
        let path = path.try_into_key_path()?;
        let target = self.node.req_node(&path)?.path.clone();
        let mut replacement = value.to_node().map_err(|error| error.prepend_path(&target))?;
        anchor(&mut replacement, &target);
        let children = Annotations::for_node(&replacement).children;
        let entry = self.at_mut(&path)?;
        entry.annotations.children = children;
        entry.annotations.tail_comment = None;
        self.node.set_node(path, replacement)
    }
}

/// Constructs ordered map entries and their annotations.
#[derive(Debug, Default)]
pub struct MapBuilder {
    entries: IndexMap<String, Node>,
    annotations: Vec<Annotations>,
}

impl MapBuilder {
    /// Adds a value under a literal key and returns its annotation handle.
    ///
    /// Dots and brackets in the key are ordinary key characters. Duplicate keys are rejected.
    pub fn entry(
        &mut self,
        key: impl Into<String>,
        value: impl ToNode,
    ) -> Result<DocumentEntry<'_>, NodeError> {
        let key = key.into();
        let path = KeyPath::from_keys([&key]);
        let node = value.to_node().map_err(|error| error.prepend_path(&path))?;
        self.entry_document(key, Document::from_node(node))
    }

    /// Adds a constructed document under a literal key.
    ///
    /// A nested document's header becomes its leading comment and its footer becomes a tail
    /// comment. A footer requires a container because it belongs inside the nested value.
    pub fn entry_document(
        &mut self,
        key: impl Into<String>,
        mut document: Document,
    ) -> Result<DocumentEntry<'_>, NodeError> {
        let key = key.into();
        let path = KeyPath::from_keys([&key]);
        if self.entries.contains_key(&key) {
            return Err(NodeError::invalid_value(&path, "duplicate document key"));
        }
        attach_nested_comments(&mut document).map_err(|error| error.prepend_path(&path))?;
        anchor(&mut document.node, &path);
        let (_, node) = self.entries.insert_full(key, document.node);
        debug_assert!(node.is_none());
        self.annotations.push(document.annotations);
        let node = self.entries.last().expect("the entry was just inserted").1;
        let annotations = self.annotations.last_mut().expect("the annotation was just inserted");
        Ok(DocumentEntry { node, annotations })
    }

    /// Constructs a nested value through a closure and adds it under a literal key.
    pub fn entry_with(
        &mut self,
        key: impl Into<String>,
        build: impl FnOnce(&mut DocumentBuilder) -> Result<(), NodeError>,
    ) -> Result<DocumentEntry<'_>, NodeError> {
        let key = key.into();
        let path = KeyPath::from_keys([&key]);
        let document = DocumentBuilder::build(build).map_err(|error| error.prepend_path(&path))?;
        self.entry_document(key, document)
    }

    fn finish(self) -> Document {
        Document {
            node: Node::new_map(KeyPath::new(), self.entries),
            annotations: Annotations {
                children: self.annotations,
                ..Default::default()
            },
            header: None,
            footer: None,
        }
    }
}

/// Constructs sequence elements and their annotations.
#[derive(Debug, Default)]
pub struct SeqBuilder {
    elements: Vec<Node>,
    annotations: Vec<Annotations>,
}

impl SeqBuilder {
    /// Adds a value and returns its annotation handle.
    pub fn elem(&mut self, value: impl ToNode) -> Result<DocumentEntry<'_>, NodeError> {
        let path = KeyPath::from_index(self.elements.len());
        let node = value.to_node().map_err(|error| error.prepend_path(&path))?;
        self.elem_document(Document::from_node(node))
    }

    /// Adds a constructed document as an element.
    pub fn elem_document(
        &mut self,
        mut document: Document,
    ) -> Result<DocumentEntry<'_>, NodeError> {
        let path = KeyPath::from_index(self.elements.len());
        attach_nested_comments(&mut document).map_err(|error| error.prepend_path(&path))?;
        anchor(&mut document.node, &path);
        self.elements.push(document.node);
        self.annotations.push(document.annotations);
        let node = self.elements.last().expect("the element was just inserted");
        let annotations = self.annotations.last_mut().expect("the annotation was just inserted");
        Ok(DocumentEntry { node, annotations })
    }

    /// Constructs a nested value through a closure and appends it.
    pub fn elem_with(
        &mut self,
        build: impl FnOnce(&mut DocumentBuilder) -> Result<(), NodeError>,
    ) -> Result<DocumentEntry<'_>, NodeError> {
        let path = KeyPath::from_index(self.elements.len());
        let document = DocumentBuilder::build(build).map_err(|error| error.prepend_path(&path))?;
        self.elem_document(document)
    }

    fn finish(self) -> Document {
        Document {
            node: Node::new_vec(KeyPath::new(), self.elements),
            annotations: Annotations {
                children: self.annotations,
                ..Default::default()
            },
            header: None,
            footer: None,
        }
    }
}

/// Constructs the single value supplied by a nested builder closure.
#[derive(Debug, Default)]
pub struct DocumentBuilder {
    document: Option<Document>,
}

impl DocumentBuilder {
    /// Supplies an ordinary value.
    pub fn value(&mut self, value: impl ToNode) -> Result<(), NodeError> {
        self.document(Document::from_value(&value)?)
    }

    /// Supplies a map built through a closure.
    pub fn map(
        &mut self,
        build: impl FnOnce(&mut MapBuilder) -> Result<(), NodeError>,
    ) -> Result<(), NodeError> {
        self.document(Document::map(build)?)
    }

    /// Supplies a sequence built through a closure.
    pub fn seq(
        &mut self,
        build: impl FnOnce(&mut SeqBuilder) -> Result<(), NodeError>,
    ) -> Result<(), NodeError> {
        self.document(Document::seq(build)?)
    }

    /// Supplies an already constructed document.
    pub fn document(&mut self, document: Document) -> Result<(), NodeError> {
        if self.document.is_some() {
            return Err(NodeError::new("a document builder must supply exactly one value"));
        }
        self.document = Some(document);
        Ok(())
    }

    fn build(
        build: impl FnOnce(&mut Self) -> Result<(), NodeError>,
    ) -> Result<Document, NodeError> {
        let mut builder = Self::default();
        build(&mut builder)?;
        builder
            .document
            .ok_or_else(|| NodeError::new("a document builder must supply exactly one value"))
    }
}

/// Changes the presentation of an entry, element, or root value.
#[derive(Debug)]
pub struct DocumentEntry<'a> {
    node: &'a Node,
    annotations: &'a mut Annotations,
}

impl DocumentEntry<'_> {
    /// Sets the comment preceding this value or its map key.
    ///
    /// Callers provide text without comment markers. Line endings are normalized for renderers.
    pub fn comment(&mut self, text: impl AsRef<str>) -> &mut Self {
        self.annotations.comment = Some(normalize_comment(text.as_ref()));
        self
    }

    /// Sets a single-line comment following this value and any required separator.
    ///
    /// Renderers reject line breaks in this text before writing the document.
    pub fn end_comment(&mut self, text: impl AsRef<str>) -> &mut Self {
        self.annotations.end_comment = Some(text.as_ref().to_owned());
        self
    }

    /// Sets the comment inside this container after its final child.
    pub fn tail_comment(&mut self, text: impl AsRef<str>) -> Result<&mut Self, NodeError> {
        if self.node.kind.is_leaf() {
            return Err(NodeError::invalid_value(
                &self.node.path,
                "a tail comment requires a map or sequence",
            ));
        }
        self.annotations.tail_comment = Some(normalize_comment(text.as_ref()));
        Ok(self)
    }

    /// Requests a blank line before this value and its leading comment.
    pub fn blank_before(&mut self) -> &mut Self {
        self.annotations.blank_before = true;
        self
    }

    /// Requests a compact presentation of this container.
    pub fn compact(&mut self) -> &mut Self {
        self.annotations.layout = Layout::Compact;
        self
    }

    /// Requests an expanded presentation of this container.
    pub fn expanded(&mut self) -> &mut Self {
        self.annotations.layout = Layout::Expanded;
        self
    }
}

// ---------------------------------------------------------------------------------------------- //

#[derive(Debug, Clone, Default)]
pub(super) struct Annotations {
    pub comment: Option<String>,
    pub end_comment: Option<String>,
    pub tail_comment: Option<String>,
    pub blank_before: bool,
    pub layout: Layout,
    pub children: Vec<Annotations>,
}

impl Annotations {
    pub(super) fn child(&self, index: usize) -> &Self {
        static EMPTY: Annotations = Annotations {
            comment: None,
            end_comment: None,
            tail_comment: None,
            blank_before: false,
            layout: Layout::Auto,
            children: Vec::new(),
        };
        self.children.get(index).unwrap_or(&EMPTY)
    }

    fn for_node(node: &Node) -> Self {
        let children = match &node.kind {
            Kind::Leaf(_) => Vec::new(),
            Kind::Vec(elements) => elements.iter().map(Self::for_node).collect(),
            Kind::Map(entries) => entries.values().map(Self::for_node).collect(),
        };
        Self {
            children,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Layout {
    #[default]
    Auto,
    Compact,
    Expanded,
}

/// Matches description structure without reading or marking configuration leaves.
fn decorate(node: &mut Node, description: &Desc) -> Annotations {
    let mut annotations = Annotations::default();
    if !description.doc.is_empty() {
        annotations.comment = Some(normalize_comment(&description.doc));
    }
    match (&mut node.kind, &description.kind) {
        (Kind::Map(entries), DescKind::Struct { fields }) => {
            let mut supplied = std::mem::take(entries);
            for field in fields {
                if let Some(mut child) = supplied.shift_remove(&field.name) {
                    let mut child_annotations = decorate(&mut child, &field.value);
                    if !field.doc.is_empty() {
                        child_annotations.comment = Some(normalize_comment(&field.doc));
                    }
                    entries.insert(field.name.clone(), child);
                    annotations.children.push(child_annotations);
                }
            }
            for (key, child) in supplied {
                annotations.children.push(Annotations::for_node(&child));
                entries.insert(key, child);
            }
        }
        (Kind::Vec(elements), DescKind::List { item }) => {
            annotations.children = elements.iter_mut().map(|child| decorate(child, item)).collect();
        }
        (Kind::Vec(elements), DescKind::Tuple { items }) if elements.len() == items.len() => {
            annotations.children =
                elements.iter_mut().zip(items).map(|(child, desc)| decorate(child, desc)).collect();
        }
        (Kind::Map(entries), DescKind::Enum { variants }) if entries.len() == 1 => {
            let (name, child) = entries.first_mut().expect("the map contains one entry");
            if let Some(variant) = variants.iter().find(|variant| &variant.name == name) {
                if let VariantRepr::Payload { payload, .. } = &variant.repr {
                    let mut child_annotations = decorate(child, payload);
                    if !variant.doc.is_empty() {
                        child_annotations.comment = Some(normalize_comment(&variant.doc));
                    }
                    annotations.children = vec![child_annotations];
                }
            }
        }
        _ => {}
    }
    if annotations.children.is_empty() {
        annotations.children = Annotations::for_node(node).children;
    }
    annotations
}

/// Finds matching value and annotation entries using the same path validation as Node.
fn locate_mut<'a>(
    root: &'a Node,
    annotations: &'a mut Annotations,
    path: &KeyPath,
) -> Result<(&'a Node, &'a mut Annotations), NodeError> {
    let target = root.req_node(path)?;
    let mut node = root;
    let mut current = annotations;
    for segment in path.iter() {
        let index = match (&node.kind, segment) {
            (Kind::Map(entries), Segment::Key(key)) => {
                let (index, _, child) = entries.get_full(key).expect("the path was validated");
                node = child;
                index
            }
            (Kind::Vec(elements), Segment::Index(index)) => {
                node = &elements[*index];
                *index
            }
            _ => unreachable!("the path was validated"),
        };
        current = &mut current.children[index];
    }
    Ok((target, current))
}

/// Anchors constructed children without changing their decoding visit state.
fn anchor(node: &mut Node, path: &KeyPath) {
    node.path = path.clone();
    match &mut node.kind {
        Kind::Leaf(_) => {}
        Kind::Map(entries) => {
            for (key, child) in entries {
                anchor(child, &path.push_key(key));
            }
        }
        Kind::Vec(elements) => {
            for (index, child) in elements.iter_mut().enumerate() {
                anchor(child, &path.push_index(index));
            }
        }
    }
}

fn attach_nested_comments(document: &mut Document) -> Result<(), NodeError> {
    if let Some(header) = document.header.take() {
        document.annotations.comment = Some(match document.annotations.comment.take() {
            Some(comment) => format!("{header}\n\n{comment}"),
            None => header,
        });
    }
    if let Some(footer) = document.footer.take() {
        if document.node.kind.is_leaf() {
            return Err(NodeError::invalid_value(
                &KeyPath::new(),
                "a nested document footer requires a map or sequence",
            ));
        }
        document.annotations.tail_comment = Some(match document.annotations.tail_comment.take() {
            Some(comment) => format!("{comment}\n\n{footer}"),
            None => footer,
        });
    }
    Ok(())
}

fn normalize_comment(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if is_line_break(character) {
            if character == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            normalized.push('\n');
        } else {
            normalized.push(character);
        }
    }
    normalized
}

fn is_line_break(character: char) -> bool {
    matches!(
        character,
        '\n' | '\r' | '\u{000b}' | '\u{000c}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
    )
}

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desc::FieldDesc;

    #[test]
    fn builders_anchor_nested_paths_and_keep_literal_keys() -> Result<(), NodeError> {
        let document = Document::map(|map| {
            map.entry("a.b", 1)?;
            map.entry_with("items", |value| {
                value.seq(|sequence| {
                    sequence.elem_with(|value| {
                        value.map(|map| {
                            map.entry("name", "first")?.comment("The first item.");
                            Ok(())
                        })
                    })?;
                    Ok(())
                })
            })?;
            Ok(())
        })?;
        assert_eq!(document.node().req_node("[\"a.b\"]")?.path.to_string(), "[\"a.b\"]");
        assert_eq!(document.node().req_node("items[0].name")?.path.to_string(), "items[0].name");
        assert_eq!(
            document.annotations.children[1].children[0].children[0].comment.as_deref(),
            Some("The first item.")
        );
        Ok(())
    }

    #[test]
    fn duplicate_keys_and_incomplete_nested_values_are_rejected() {
        let duplicate = Document::map(|map| {
            map.entry("same.key", 1)?;
            map.entry("same.key", 2)?;
            Ok(())
        })
        .unwrap_err();
        assert_eq!(duplicate.path().unwrap().to_string(), "[\"same.key\"]");
        let incomplete = Document::map(|map| {
            map.entry_with("missing", |_| Ok(()))?;
            Ok(())
        })
        .unwrap_err();
        assert_eq!(incomplete.path().unwrap().to_string(), "missing");
    }

    #[test]
    fn comments_normalize_line_breaks_and_retain_end_comment_text_for_validation(
    ) -> Result<(), NodeError> {
        let mut document = Document::map(|map| {
            map.entry("value", 42)?.comment("one\r\ntwo\rthree\u{0085}four\u{2028}five\u{2029}six");
            Ok(())
        })?;
        assert_eq!(
            document.annotations.children[0].comment.as_deref(),
            Some("one\ntwo\nthree\nfour\nfive\nsix")
        );
        document.at_mut("value")?.end_comment("before\nafter");
        assert_eq!(document.annotations.children[0].end_comment.as_deref(), Some("before\nafter"));
        assert!(document.at_mut("value")?.tail_comment("Inside a scalar.").is_err());
        Ok(())
    }

    #[test]
    fn replacement_retains_entry_annotations_but_discards_descendants() -> Result<(), NodeError> {
        let mut document = Document::map(|map| {
            map.entry_with("items", |value| {
                value.seq(|sequence| {
                    sequence.elem(1)?.comment("Old element.");
                    Ok(())
                })
            })?
            .comment("The sequence.")
            .blank_before()
            .expanded()
            .tail_comment("Old container explanation.")?;
            Ok(())
        })?;
        document.replace("items", [2, 3])?;
        let annotations = &document.annotations.children[0];
        assert_eq!(annotations.comment.as_deref(), Some("The sequence."));
        assert!(annotations.blank_before);
        assert_eq!(annotations.layout, Layout::Expanded);
        assert!(annotations.tail_comment.is_none());
        assert_eq!(annotations.children.len(), 2);
        assert!(annotations.children.iter().all(|child| child.comment.is_none()));
        assert_eq!(document.node().req_node("items[1]")?.path.to_string(), "items[1]");
        assert!(document.replace("absent", 42).is_err());
        Ok(())
    }

    #[test]
    fn conversion_anchors_nested_values_and_errors_without_mutating_documents(
    ) -> Result<(), NodeError> {
        struct Invalid;
        impl ToNode for Invalid {
            fn to_node(&self) -> Result<Node, NodeError> {
                Err(NodeError::invalid_value(&KeyPath::from_keys(["child"]), "cannot serialize"))
            }
        }
        let mut document = Document::from_value(&vec![vec![1, 2]])?;
        assert_eq!(document.node().req_node("[0][1]")?.path.to_string(), "[0][1]");
        document.at_mut("[0]")?.comment("Retained sequence.");
        let error = document.replace("[0]", Invalid).unwrap_err();
        assert_eq!(error.path().unwrap().to_string(), "[0].child");
        assert_eq!(document.annotations.children[0].comment.as_deref(), Some("Retained sequence."));
        assert_eq!(document.node().req_node("[0]")?.as_vec()?.len(), 2);
        Ok(())
    }

    #[test]
    fn decoration_orders_known_fields_without_inventing_values_or_visiting_leaves(
    ) -> Result<(), NodeError> {
        struct Settings(Node);
        impl ToNode for Settings {
            fn to_node(&self) -> Result<Node, NodeError> {
                self.0.to_node()
            }
        }
        impl Describe for Settings {
            fn describe() -> Desc {
                Desc::structure(vec![
                    FieldDesc::new(
                        "first",
                        Desc::plain("number").with_doc("Value type description."),
                    )
                    .with_doc("Field explanation."),
                    FieldDesc::new("second", Desc::plain("number")),
                    FieldDesc::new("absent", Desc::plain("number")),
                ])
                .with_doc("Settings explanation.")
            }
        }
        let settings = Settings(
            Document::map(|map| {
                map.entry("extra_b", 4)?;
                map.entry("second", 2)?;
                map.entry("extra_a", 3)?;
                map.entry("first", 1)?;
                Ok(())
            })?
            .into_node(),
        );
        let document = Document::from_config(&settings)?;
        let keys: Vec<_> = document.node().as_map()?.keys().map(String::as_str).collect();
        assert_eq!(keys, ["first", "second", "extra_b", "extra_a"]);
        assert_eq!(document.header.as_deref(), Some("Settings explanation."));
        assert_eq!(document.annotations.children[0].comment.as_deref(), Some("Field explanation."));
        assert!(settings.0.as_map()?.values().all(|node| !node.as_leaf().unwrap().is_visited()));
        assert!(document
            .node()
            .as_map()?
            .values()
            .all(|node| !node.as_leaf().unwrap().is_visited()));
        Ok(())
    }

    #[test]
    fn decoration_stops_at_opaque_or_mismatched_shapes() -> Result<(), NodeError> {
        let mut node = Document::map(|map| {
            map.entry("value", [1, 2])?;
            Ok(())
        })?
        .into_node();
        let description = Desc::structure(vec![FieldDesc::new(
            "value",
            Desc::structure(vec![FieldDesc::new("imaginary", Desc::plain("number"))]),
        )
        .with_doc("Outer explanation.")]);
        let annotations = decorate(&mut node, &description);
        assert_eq!(annotations.children[0].comment.as_deref(), Some("Outer explanation."));
        assert!(annotations.children[0].children.iter().all(|child| child.comment.is_none()));
        Ok(())
    }
}
