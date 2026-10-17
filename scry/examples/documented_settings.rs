//! Generates a documented Rhai preferences file from editable application settings.
//!
//! Run with: `cargo run --example documented_settings`.
//! Keep relative paths in this state and resolve them separately when applying preferences.

use std::path::PathBuf;

use scry::writer::{Document, RhaiWriter};
use scry::{Config, Desc, Node, NodeError, ToNode};

// ---------------------------------------------------------------------------------------------- //

/// Holds the preferences a viewer can expose in its settings dialog.
#[derive(Debug, Clone, PartialEq, Config, ToNode)]
struct Settings {
    /// The opaque canvas color behind the image, written as #RRGGBB.
    #[scry(default = Color([28, 28, 30]))]
    #[scry(
        from_node_with(read_color),
        to_node_with(write_color),
        describe_with(describe_color)
    )]
    background: Color,
    /// The multiplier for each zoom step, greater than 1 and at most 4.
    #[scry(default = 1.2, from_node_with(read_zoom_factor))]
    zoom_factor: f32,
    /// Limits retained image data in MiB. Use 0 to disable caching.
    #[scry(default = 512)]
    cache_size_mib: usize,
    /// Receives copied images. Relative paths start beside the preferences file.
    target_dir: Option<PathBuf>,
    /// Names copied files by their origin timestamp instead of their original filename.
    #[scry(default = false)]
    stamp: bool,
    /// Selects an editable metadata inspector. Omission or unit disables inspection.
    metadata: Option<MetadataSettings>,
}

/// Selects an optional script independently of ordinary viewer preferences.
#[derive(Debug, Clone, PartialEq, Config, ToNode)]
struct MetadataSettings {
    /// Names a Rhai script relative to the preferences file, or by absolute path.
    script: PathBuf,
}

fn main() -> Result<(), NodeError> {
    let mut settings: Settings = scry::from_defaults()?;
    // An application can offer a useful starter file that includes optional features.
    settings.metadata = Some(MetadataSettings {
        script: PathBuf::from("metadata.rhai"),
    });

    print!("{}", render_settings(&settings)?);
    Ok(())
}

fn render_settings(settings: &Settings) -> Result<String, NodeError> {
    let document = Document::from_config(settings)?
        .header("Viewer preferences.\nMissing settings use the built-in defaults.");
    let mut writer = RhaiWriter::to_string_writer();
    writer.write(&document)?;
    writer.into_string()
}

/// Stands in for a GUI framework's color type without adding a GUI dependency.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Color([u8; 3]);

fn read_color(node: &Node) -> Result<Color, NodeError> {
    let text: String = node.as_type()?;
    let invalid = || NodeError::invalid_value(&node.path, "Expected a color in #RRGGBB format.");
    if text.len() != 7 || !text.starts_with('#') || !text[1..].is_ascii() {
        return Err(invalid());
    }
    let mut channels = [0; 3];
    for (channel, start) in channels.iter_mut().zip([1, 3, 5]) {
        *channel = u8::from_str_radix(&text[start..start + 2], 16).map_err(|_| invalid())?;
    }
    Ok(Color(channels))
}

fn write_color(color: &Color) -> Result<Node, NodeError> {
    let [red, green, blue] = color.0;
    format!("#{red:02x}{green:02x}{blue:02x}").to_node()
}

fn describe_color() -> Desc {
    Desc::plain("opaque color string in #RRGGBB format")
}

fn read_zoom_factor(node: &Node) -> Result<f32, NodeError> {
    let value: f32 = node.as_type()?;
    if !value.is_finite() || value <= 1.0 || value > 4.0 {
        return Err(NodeError::invalid_value(
            &node.path,
            "Expected a finite zoom factor greater than 1 and at most 4.",
        ));
    }
    Ok(value)
}

// ---------------------------------------------------------------------------------------------- //

#[cfg(test)]
mod tests {
    use scry::node::Format;

    use super::*;

    #[test]
    fn generated_defaults_decode_with_custom_representations() {
        let settings: Settings = scry::from_defaults().unwrap();
        let output = render_settings(&settings).unwrap();
        assert!(output.contains("// The opaque canvas color behind the image"));
        assert!(output.contains("background: \"#1c1c1e\""));
        assert!(output.contains("target_dir: ()"));
        let node = Node::parse_str(&output, Format::Rhai).unwrap();
        assert_eq!(node.as_type::<Settings>().unwrap(), settings);
        node.ensure_no_unknown_keys().unwrap();
    }

    #[test]
    fn generated_edits_preserve_relative_paths_and_document_nested_settings() {
        let settings = Settings {
            background: Color([0, 128, 255]),
            zoom_factor: 1.1,
            cache_size_mib: 128,
            target_dir: Some(PathBuf::from("selected")),
            stamp: true,
            metadata: Some(MetadataSettings {
                script: PathBuf::from("metadata.rhai"),
            }),
        };
        let output = render_settings(&settings).unwrap();
        assert!(output.contains("// Names a Rhai script relative to the preferences file"));
        let node = Node::parse_str(&output, Format::Rhai).unwrap();
        assert_eq!(node.as_type::<Settings>().unwrap(), settings);
        node.ensure_no_unknown_keys().unwrap();
    }

    #[cfg(feature = "format-json5")]
    #[test]
    fn the_same_document_also_decodes_as_json5_settings() -> Result<(), NodeError> {
        let mut settings: Settings = scry::from_defaults()?;
        settings.target_dir = Some(PathBuf::from("selected"));
        settings.metadata = Some(MetadataSettings {
            script: PathBuf::from("metadata.rhai"),
        });
        let document = Document::from_config(&settings)?;
        let mut writer = scry::writer::Json5Writer::to_string_writer();
        writer.write(&document)?;
        let node = Node::parse_str(writer.as_str()?, Format::Json5)?;
        assert_eq!(node.as_type::<Settings>()?, settings);
        node.ensure_no_unknown_keys()?;
        let exported = document.into_node().to_string_as(Format::Json5)?;
        assert_eq!(Node::parse_str(&exported, Format::Json5)?.as_type::<Settings>()?, settings);
        Ok(())
    }
}
