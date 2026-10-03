use proc_macro2::TokenStream;
use quote::{format_ident, quote};

const LENGTH: &str = "auto | 0 | nonnegative ch | %";
const SPACING: &str = "0 | nonnegative ch | %";
const NUMBER: &str = "finite nonnegative number";
const COLOR: &str = "default | ANSI color name | #rrggbb";
const PROPERTY_HELP: &str = "use a property from hypercmd_build::profile::CSS_PROPERTIES";
/// The accepted property/value table used by validation and the profile reference.
pub const PROPERTIES: &[(&str, &str, &str)] = &[
    ("display", "Display", "flex | none"),
    ("flex-direction", "Row", "row | column"),
    ("flex-grow", "Grow", NUMBER),
    ("flex-shrink", "Shrink", NUMBER),
    ("flex-basis", "Basis", LENGTH),
    ("width", "Width", LENGTH),
    ("height", "Height", LENGTH),
    ("min-width", "MinWidth", LENGTH),
    ("min-height", "MinHeight", LENGTH),
    ("max-width", "MaxWidth", LENGTH),
    ("max-height", "MaxHeight", LENGTH),
    ("gap", "Gap", "one or two lengths: 0 | nonnegative ch | %"),
    ("row-gap", "RowGap", SPACING),
    ("column-gap", "ColumnGap", SPACING),
    (
        "padding",
        "Padding",
        "one to four lengths: 0 | nonnegative ch | %",
    ),
    ("padding-top", "PaddingTop", SPACING),
    ("padding-right", "PaddingRight", SPACING),
    ("padding-bottom", "PaddingBottom", SPACING),
    ("padding-left", "PaddingLeft", SPACING),
    (
        "align-items",
        "AlignItems",
        "flex-start | center | flex-end | stretch",
    ),
    (
        "align-self",
        "AlignSelf",
        "auto | flex-start | center | flex-end | stretch",
    ),
    (
        "justify-content",
        "Justify",
        "flex-start | center | flex-end | space-between | space-around | space-evenly",
    ),
    ("overflow", "Overflow", "visible | hidden | auto"),
    ("white-space", "WhiteSpace", "normal | pre | pre-wrap"),
    ("color", "Foreground", COLOR),
    ("background-color", "Background", COLOR),
    ("font-weight", "Bold", "bold | normal"),
    ("font-style", "Italic", "italic | normal"),
    ("text-decoration", "Underline", "underline | none"),
    ("border-style", "BorderStyle", "none | solid | rounded"),
    ("border-color", "BorderColor", COLOR),
];

pub(crate) fn parse(source: &str) -> Result<Vec<TokenStream>, (usize, String)> {
    let source = comments(source)?;
    let mut rules = Vec::new();
    let mut offset = 0;
    while !source[offset..].trim().is_empty() {
        let start = offset + source[offset..].len() - source[offset..].trim_start().len();
        let open = source[start..]
            .find('{')
            .map(|index| start + index)
            .ok_or((start, "expected selector { declarations }".into()))?;
        let close = source[open + 1..]
            .find('}')
            .map(|index| open + 1 + index)
            .ok_or((open, "unterminated declaration block".into()))?;
        let mut declarations = Vec::new();
        let mut declaration_offset = open + 1;
        for text in source[open + 1..close].split(';') {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                let (name, value) = trimmed
                    .split_once(':')
                    .ok_or((declaration_offset, "expected property: value".into()))?;
                declarations.extend(
                    declaration(name.trim(), value.trim())
                        .map_err(|message| (declaration_offset, message))?,
                );
            }
            declaration_offset += text.len() + 1;
        }
        for selector in source[start..open].split(',') {
            let parts = selector_tokens(selector.trim()).map_err(|message| (start, message))?;
            rules.push(quote! { ::hypercmd::style::Rule { selector: &[#(#parts),*], declarations: &[#(#declarations),*] } });
        }
        offset = close + 1;
    }
    Ok(rules)
}

fn comments(source: &str) -> Result<String, (usize, String)> {
    let mut text = source.to_owned();
    while let Some(start) = text.find("/*") {
        let end = text[start + 2..]
            .find("*/")
            .map(|end| start + end + 4)
            .ok_or((start, "unterminated CSS comment".into()))?;
        text.replace_range(start..end, &" ".repeat(end - start));
    }
    Ok(text)
}

fn selector_tokens(selector: &str) -> Result<Vec<TokenStream>, String> {
    let mut remaining = selector;
    let mut parts = Vec::new();
    while !remaining.is_empty() {
        let prefix = remaining.as_bytes()[0];
        let (variant, rest) = match prefix {
            b'.' => ("Class", &remaining[1..]),
            b'#' => ("Id", &remaining[1..]),
            b':' => ("Pseudo", &remaining[1..]),
            _ if parts.is_empty() => ("Tag", remaining),
            _ => return Err(SELECTOR_ERROR.into()),
        };
        let length = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            .count();
        if length == 0 {
            return Err(SELECTOR_ERROR.into());
        }
        let name = &rest[..length];
        let token = match (variant, name) {
            ("Pseudo", "focus") => quote!(::hypercmd::style::Selector::Focus),
            ("Pseudo", "disabled") => quote!(::hypercmd::style::Selector::Disabled),
            ("Pseudo", _) => return Err(SELECTOR_ERROR.into()),
            _ => {
                let variant = format_ident!("{variant}");
                quote!(::hypercmd::style::Selector::#variant(#name))
            }
        };
        parts.push(token);
        remaining = &rest[length..];
    }
    if parts.is_empty() {
        Err(SELECTOR_ERROR.into())
    } else {
        Ok(parts)
    }
}

const SELECTOR_ERROR: &str =
    "terminal-v1 selectors accept tag, .class, #id, :focus and :disabled without combinators";

fn declaration(name: &str, value: &str) -> Result<Vec<TokenStream>, String> {
    let (_, variant, accepted) = *PROPERTIES
        .iter()
        .find(|(property, _, _)| *property == name)
        .ok_or_else(|| format!("unsupported terminal-v1 property {name}; {PROPERTY_HELP}"))?;
    let invalid = || format!("unsupported {name}: {value}; accepted values: {accepted}");
    if matches!(variant, "Gap" | "Padding") {
        return shorthand(variant, value).ok_or_else(invalid);
    }
    let token = match accepted {
        LENGTH | SPACING => length(value, accepted == LENGTH),
        NUMBER => number(value).map(|number| quote!(#number)),
        COLOR => color(value),
        _ => keyword(variant, value, accepted),
    }
    .ok_or_else(invalid)?;
    Ok(vec![declaration_token(variant, &token)])
}

fn declaration_token(variant: &str, value: &TokenStream) -> TokenStream {
    let variant = format_ident!("{variant}");
    quote!(::hypercmd::style::Declaration::#variant(#value))
}

// Expands `gap` and `padding` the way CSS does for one to four values.
fn shorthand(variant: &str, value: &str) -> Option<Vec<TokenStream>> {
    let sides: &[&str] = if variant == "Gap" {
        &["RowGap", "ColumnGap"]
    } else {
        &["PaddingTop", "PaddingRight", "PaddingBottom", "PaddingLeft"]
    };
    let values: Vec<_> = value
        .split_whitespace()
        .map(|value| length(value, false))
        .collect::<Option<_>>()?;
    let indices = match (sides.len(), values.len()) {
        (_, 1) => [0, 0, 0, 0],
        (_, 2) => [0, 1, 0, 1],
        (4, 3) => [0, 1, 2, 1],
        (4, 4) => [0, 1, 2, 3],
        _ => return None,
    };
    Some(
        sides
            .iter()
            .zip(indices)
            .map(|(side, index)| declaration_token(side, &values[index]))
            .collect(),
    )
}

// Boolean properties list their true value first in the accepted list.
fn keyword(variant: &str, value: &str, accepted: &str) -> Option<TokenStream> {
    let index = accepted.split(" | ").position(|choice| choice == value)?;
    if matches!(variant, "Display" | "Row" | "Bold" | "Italic" | "Underline") {
        let enabled = index == 0;
        return Some(quote!(#enabled));
    }
    let kind = if matches!(variant, "AlignItems" | "AlignSelf" | "Justify") {
        "Alignment"
    } else {
        variant
    };
    let kind = format_ident!("{kind}");
    let keyword = format_ident!("{}", camel(value.trim_start_matches("flex-")));
    Some(quote!(::hypercmd::style::#kind::#keyword))
}

fn camel(value: &str) -> String {
    value
        .split('-')
        .flat_map(|word| {
            let mut chars = word.chars();
            let first = chars.next().map(|first| first.to_ascii_uppercase());
            first.into_iter().chain(chars)
        })
        .collect()
}

fn number(value: &str) -> Option<f32> {
    value
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
}
fn length(value: &str, auto: bool) -> Option<TokenStream> {
    if value == "auto" && auto {
        return Some(quote!(::hypercmd::style::Length::Auto));
    }
    if value == "0" {
        return Some(quote!(::hypercmd::style::Length::Cells(0.0)));
    }
    if let Some(value) = value.strip_suffix("ch").and_then(number) {
        return Some(quote!(::hypercmd::style::Length::Cells(#value)));
    }
    value.strip_suffix('%').and_then(number).map(|value| {
        let value = value / 100.0;
        quote!(::hypercmd::style::Length::Percent(#value))
    })
}
fn color(value: &str) -> Option<TokenStream> {
    if let Some(hex) = value
        .strip_prefix('#')
        .filter(|value| value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        let rgb = u32::from_str_radix(hex, 16).ok()?;
        let (red, green, blue) = ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
        return Some(quote!(::hypercmd::style::Color::Rgb(#red, #green, #blue)));
    }
    let name = match value {
        "default" => "Reset",
        "black" => "Black",
        "red" => "Red",
        "green" => "Green",
        "yellow" => "Yellow",
        "blue" => "Blue",
        "magenta" => "Magenta",
        "cyan" => "Cyan",
        "white" | "gray" => "Gray",
        "bright-black" => "DarkGray",
        "bright-red" => "LightRed",
        "bright-green" => "LightGreen",
        "bright-yellow" => "LightYellow",
        "bright-blue" => "LightBlue",
        "bright-magenta" => "LightMagenta",
        "bright-cyan" => "LightCyan",
        "bright-white" => "White",
        _ => return None,
    };
    let name = format_ident!("{name}");
    Some(quote!(::hypercmd::style::Color::#name))
}

#[cfg(test)]
mod tests {
    #[test]
    fn unsupported_css_is_rejected_at_the_authored_offset() {
        for css in [
            "p > span { color: red }",
            "p { width: 20px }",
            "p { display: grid }",
            "@import 'other.css';",
            "p { color: red !important }",
            "p { gap: auto }",
            "p { color: #+12345 }",
        ] {
            let (offset, message) = super::parse(css).expect_err("unsupported CSS must fail");
            assert!(css.is_char_boundary(offset));
            assert!(!message.is_empty());
        }
    }
}
